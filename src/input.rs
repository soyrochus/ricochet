use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Space,
    Pause,
    Restart,
    Quit,
}

#[derive(Default, Debug)]
pub struct Input {
    held: HashSet<KeyCode>,
}
impl Input {
    pub fn clear(&mut self) {
        self.held.clear();
    }
    pub fn direction(&self) -> f64 {
        let left = self.held.contains(&KeyCode::Left) || self.held.contains(&KeyCode::Char('a'));
        let right = self.held.contains(&KeyCode::Right) || self.held.contains(&KeyCode::Char('d'));
        f64::from(i8::from(right) - i8::from(left))
    }
    pub fn key(&mut self, key: KeyEvent, movement: bool) -> Option<Action> {
        let code = match key.code {
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            c => c,
        };
        if key.kind == KeyEventKind::Release {
            self.held.remove(&code);
            return None;
        }
        if matches!(
            code,
            KeyCode::Left | KeyCode::Right | KeyCode::Char('a' | 'd')
        ) {
            // Repeat never re-arms a key cleared by pause/focus/resize.
            if movement && key.kind == KeyEventKind::Press {
                self.held.insert(code);
            }
            return None;
        }
        if key.kind != KeyEventKind::Press {
            return None;
        }
        match code {
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Quit)
            }
            KeyCode::Char(' ') => Some(Action::Space),
            KeyCode::Esc | KeyCode::Char('p') => Some(Action::Pause),
            KeyCode::Char('r') => Some(Action::Restart),
            _ => None,
        }
    }
}

/// Replay startup keystrokes after raw response probing, before Crossterm owns
/// stdin. All game bindings, UTF-8 text, arrows and focus reports are retained.
/// Unbound function/control sequences are intentionally inert, as at runtime.
pub fn decode_startup_input(bytes: &[u8]) -> VecDeque<Event> {
    let mut events = VecDeque::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"\x1b[") {
            let Some(end) = bytes[i + 2..]
                .iter()
                .position(|b| (0x40..=0x7e).contains(b))
                .map(|n| i + 2 + n)
            else {
                break;
            };
            let final_byte = bytes[end];
            let body = String::from_utf8_lossy(&bytes[i + 2..end]);
            if final_byte == b'I' {
                events.push_back(Event::FocusGained);
            } else if final_byte == b'O' {
                events.push_back(Event::FocusLost);
            } else {
                let mut fields = body.split(';');
                let number = fields
                    .next()
                    .unwrap_or("")
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .parse::<u32>()
                    .unwrap_or(0);
                let mut modifier = fields.next().unwrap_or("1").split(':');
                let bits = modifier
                    .next()
                    .unwrap_or("1")
                    .parse::<u8>()
                    .unwrap_or(1)
                    .saturating_sub(1);
                let kind = match modifier.next() {
                    Some("2") => KeyEventKind::Repeat,
                    Some("3") => KeyEventKind::Release,
                    _ => KeyEventKind::Press,
                };
                let code = match final_byte {
                    b'D' => Some(KeyCode::Left),
                    b'C' => Some(KeyCode::Right),
                    b'u' => match number {
                        27 => Some(KeyCode::Esc),
                        13 => Some(KeyCode::Enter),
                        9 => Some(KeyCode::Tab),
                        57350 => Some(KeyCode::Left),
                        57351 => Some(KeyCode::Right),
                        n => char::from_u32(n).map(KeyCode::Char),
                    },
                    _ => None,
                };
                if let Some(code) = code {
                    events.push_back(Event::Key(KeyEvent::new_with_kind(
                        code,
                        kitty_modifiers(bits),
                        kind,
                    )));
                }
            }
            i = end + 1;
        } else if bytes[i..].starts_with(b"\x1bOD") || bytes[i..].starts_with(b"\x1bOC") {
            events.push_back(Event::Key(
                if bytes[i + 2] == b'D' {
                    KeyCode::Left
                } else {
                    KeyCode::Right
                }
                .into(),
            ));
            i += 3;
        } else {
            let b = bytes[i];
            let (code, modifiers, length) = if b == 27 {
                (KeyCode::Esc, KeyModifiers::NONE, 1)
            } else if (1..=26).contains(&b) {
                (
                    KeyCode::Char(char::from(b - 1 + b'a')),
                    KeyModifiers::CONTROL,
                    1,
                )
            } else {
                let text = String::from_utf8_lossy(&bytes[i..]);
                let c = text.chars().next().unwrap();
                (
                    KeyCode::Char(c),
                    KeyModifiers::NONE,
                    c.len_utf8().min(bytes.len() - i),
                )
            };
            events.push_back(Event::Key(KeyEvent::new(code, modifiers)));
            i += length;
        }
    }
    events
}

fn kitty_modifiers(bits: u8) -> KeyModifiers {
    let mut modifiers = KeyModifiers::NONE;
    for (bit, flag) in [
        (1, KeyModifiers::SHIFT),
        (2, KeyModifiers::ALT),
        (4, KeyModifiers::CONTROL),
        (8, KeyModifiers::SUPER),
        (16, KeyModifiers::HYPER),
        (32, KeyModifiers::META),
    ] {
        if bits & bit != 0 {
            modifiers |= flag;
        }
    }
    modifiers
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(c: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(c, KeyModifiers::NONE, kind)
    }
    #[test]
    fn physical_keys_release_independently_and_opposites_cancel() {
        let mut input = Input::default();
        input.key(key(KeyCode::Left, KeyEventKind::Press), true);
        input.key(key(KeyCode::Char('a'), KeyEventKind::Press), true);
        input.key(key(KeyCode::Char('a'), KeyEventKind::Release), true);
        assert_eq!(input.direction(), -1.0);
        input.key(key(KeyCode::Right, KeyEventKind::Press), true);
        assert_eq!(input.direction(), 0.0);
        input.clear();
        input.key(key(KeyCode::Right, KeyEventKind::Repeat), true);
        assert_eq!(input.direction(), 0.0);
    }
    #[test]
    fn one_shots_ignore_repeat_and_release() {
        for c in [' ', 'p', 'r', 'q'] {
            let mut i = Input::default();
            assert!(
                i.key(key(KeyCode::Char(c), KeyEventKind::Press), true)
                    .is_some()
            );
            assert!(
                i.key(key(KeyCode::Char(c), KeyEventKind::Repeat), true)
                    .is_none()
            );
            assert!(
                i.key(key(KeyCode::Char(c), KeyEventKind::Release), true)
                    .is_none()
            );
        }
    }
    #[test]
    fn startup_replay_handles_control_c_utf8_and_release_events() {
        let mut input = Input::default();
        let mut events =
            decode_startup_input("\x1b[99;5u\x1b[97;1:1u\x1b[97;1:3uñ\x1b[I\x1b[O".as_bytes());
        let Some(Event::Key(ctrl_c)) = events.pop_front() else {
            panic!("expected key")
        };
        assert_eq!(input.key(ctrl_c, true), Some(Action::Quit));
        for _ in 0..2 {
            let Some(Event::Key(key)) = events.pop_front() else {
                panic!("expected key")
            };
            input.key(key, true);
        }
        assert_eq!(input.direction(), 0.0);
        assert!(matches!(
            events.pop_front(),
            Some(Event::Key(KeyEvent {
                code: KeyCode::Char('ñ'),
                ..
            }))
        ));
        assert_eq!(events.pop_front(), Some(Event::FocusGained));
        assert_eq!(events.pop_front(), Some(Event::FocusLost));
    }
}
