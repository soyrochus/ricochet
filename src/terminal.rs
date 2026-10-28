use crate::{input::decode_startup_input, render::kitty::Registry};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableFocusChange, EnableFocusChange, KeyboardEnhancementFlags as Flags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    queue,
    style::ResetColor,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    collections::VecDeque,
    io::{self, IsTerminal, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const REQUIRED_KEYS: u8 = 1 | 2 | 8;
pub const PROBE_ID: u32 = 0x524943;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub columns: u16,
    pub rows: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
}
impl Geometry {
    pub fn read() -> io::Result<Self> {
        let size = terminal::window_size()?;
        Ok(Self {
            columns: size.columns,
            rows: size.rows,
            pixel_width: size.width.into(),
            pixel_height: size.height.into(),
        })
    }
    pub fn cell(self) -> Option<(u32, u32)> {
        let w = self.pixel_width.checked_div(self.columns.into())?;
        let h = self.pixel_height.checked_div(self.rows.into())?;
        (w > 0 && h > 0).then_some((w, h))
    }
}

#[derive(Default, Debug)]
pub struct Modes {
    pub raw: bool,
    pub alternate: bool,
    pub hidden: bool,
    pub keyboard: bool,
    pub focus: bool,
}

pub struct Session<W: Write> {
    pub out: W,
    pub graphics: Registry,
    pub modes: Modes,
    restore_raw: fn() -> io::Result<()>,
}
impl<W: Write> Session<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            graphics: Registry::default(),
            modes: Modes::default(),
            restore_raw: terminal::disable_raw_mode,
        }
    }
    pub fn fullscreen(&mut self) -> io::Result<()> {
        // Mark before output: a failed write may have partially enabled a mode.
        self.modes.alternate = true;
        queue!(self.out, EnterAlternateScreen)?;
        self.modes.hidden = true;
        queue!(self.out, Hide)?;
        self.modes.keyboard = true;
        queue!(
            self.out,
            PushKeyboardEnhancementFlags(Flags::from_bits_retain(REQUIRED_KEYS))
        )?;
        self.modes.focus = true;
        queue!(
            self.out,
            EnableFocusChange,
            ResetColor,
            Clear(ClearType::All)
        )?;
        self.out.flush()
    }
    /// Best-effort independent restorations: output failure must not skip raw
    /// mode restoration. Safe to call explicitly and again from Drop.
    pub fn cleanup(&mut self) -> io::Result<()> {
        // Cancel any escape string interrupted by a partial sprite write.
        let mut error = if self.modes.alternate || self.graphics.image_count() > 0 {
            self.out.write_all(b"\x18\x1b\\").err()
        } else {
            None
        };
        if let Err(e) = self.graphics.cleanup(&mut self.out) {
            error.get_or_insert(e);
        }
        macro_rules! attempt {
            ($expr:expr) => {
                if let Err(e) = $expr {
                    error.get_or_insert(e);
                }
            };
        }
        if self.modes.alternate {
            attempt!(self.out.write_all(b"\x1b[?2026l"));
        }
        if self.modes.focus {
            self.modes.focus = false;
            attempt!(queue!(self.out, DisableFocusChange));
        }
        if self.modes.keyboard {
            self.modes.keyboard = false;
            attempt!(queue!(self.out, PopKeyboardEnhancementFlags));
        }
        if self.modes.hidden {
            self.modes.hidden = false;
            attempt!(queue!(self.out, Show));
        }
        if self.modes.alternate {
            self.modes.alternate = false;
            attempt!(queue!(self.out, ResetColor));
            attempt!(queue!(self.out, LeaveAlternateScreen));
        }
        attempt!(self.out.flush());
        if self.modes.raw {
            match (self.restore_raw)() {
                Ok(()) => self.modes.raw = false,
                Err(e) => {
                    error.get_or_insert(e);
                }
            }
        }
        error.map_or(Ok(()), Err)
    }
}
impl<W: Write> Drop for Session<W> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

impl Session<io::Stdout> {
    pub fn start() -> io::Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(io::Error::other(
                "Ricochet requires interactive stdin and stdout. Run it directly in Ghostty 1.4+ (try --help).",
            ));
        }
        let mut session = Self::new(io::stdout());
        terminal::enable_raw_mode()?;
        session.modes.raw = true;
        session.fullscreen()?;
        Ok(session)
    }
}

#[derive(Default, Debug)]
pub struct Probe {
    pub graphics: bool,
    pub keyboard: Option<u8>,
    pub synchronized: bool,
    pub barrier: bool,
    sequence: Vec<u8>,
    pub unrelated: Vec<u8>,
}
impl Probe {
    /// Startup is the sole raw stdin reader. Extract only our replies, retaining
    /// complete intervening keyboard/focus sequences for the event loop.
    pub fn push(&mut self, byte: u8) -> io::Result<()> {
        if self.sequence == [0x1b] && byte == 0x1b {
            self.unrelated.push(0x1b);
            return Ok(());
        }
        if self.sequence.is_empty() && byte != 0x1b {
            self.unrelated.push(byte);
        } else {
            self.sequence.push(byte);
            let s = &self.sequence;
            let complete = if s.starts_with(b"\x1b_G") {
                s.ends_with(b"\x1b\\")
            } else if s.starts_with(b"\x1b[") {
                s.len() > 2 && (0x40..=0x7e).contains(&byte)
            } else {
                s.len() >= 3 || (s.len() == 2 && byte != b'_' && byte != b'O')
            };
            if complete {
                let text = String::from_utf8_lossy(s);
                if text == format!("\x1b_Gi={PROBE_ID};OK\x1b\\") {
                    self.graphics = true;
                } else if text.starts_with("\x1b[?") && text.ends_with('u') {
                    self.keyboard = text[3..text.len() - 1].parse().ok();
                } else if text.starts_with("\x1b[?2026;") && text.ends_with("$y") {
                    self.synchronized = matches!(&text[8..text.len() - 2], "1" | "2" | "3" | "4");
                } else if text.starts_with("\x1b[?") && text.ends_with('c') {
                    self.barrier = true;
                } else {
                    self.unrelated.extend_from_slice(s);
                }
                self.sequence.clear();
            }
        }
        if self.unrelated.len() + self.sequence.len() > 16384 {
            return Err(io::Error::other(
                "too much input during capability negotiation",
            ));
        }
        Ok(())
    }
    pub fn supported(&self) -> bool {
        self.graphics
            && self
                .keyboard
                .is_some_and(|v| v & REQUIRED_KEYS == REQUIRED_KEYS)
    }
}

pub struct Capabilities {
    pub geometry: Geometry,
    pub synchronized: bool,
    pub pending: VecDeque<crossterm::event::Event>,
}

pub fn negotiate(out: &mut impl Write, shutdown: &AtomicBool) -> io::Result<Capabilities> {
    let deadline = Instant::now() + Duration::from_secs(2);
    // No Crossterm event::poll/read/support queries are used until this phase
    // finishes. Crossterm 0.29 doesn't expose Kitty APC replies publicly.
    write!(
        out,
        "\x1b_Ga=q,i={PROBE_ID},s=1,v=1,t=d,f=24;AAAA\x1b\\\x1b[?u\x1b[?2026$p\x1b[c"
    )?;
    out.flush()?;
    let mut probe = Probe::default();
    while Instant::now() < deadline && !probe.barrier {
        if shutdown.load(Ordering::Relaxed) {
            return Err(io::Error::other("terminated during startup"));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining < Duration::from_millis(1) {
            break;
        }
        if let Some(byte) = read_probe_byte(remaining.min(Duration::from_millis(20)))? {
            probe.push(byte)?;
        }
    }
    if !probe.barrier || !probe.supported() {
        return Err(io::Error::other(
            "Required terminal capabilities unavailable: Kitty RGBA graphics and enhanced keyboard flags 1+2+8 (press/repeat/release for ordinary keys). Use Ghostty 1.4+ directly. Probe timed out or was rejected within 2 seconds.",
        ));
    }
    let geometry = Geometry::read()?;
    if geometry.cell().is_none() {
        return Err(io::Error::other(
            "Terminal OS pixel geometry unavailable (TIOCGWINSZ). Ricochet cannot position pixels accurately; use Ghostty 1.4+ directly.",
        ));
    }
    Ok(Capabilities {
        geometry,
        synchronized: probe.synchronized,
        pending: decode_startup_input(&probe.unrelated),
    })
}

fn read_probe_byte(wait: Duration) -> io::Result<Option<u8>> {
    let mut descriptor = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd; read points to a live one-byte buffer. This is
    // the only input owner before handing the descriptor to Crossterm.
    let result = unsafe { libc::poll(&mut descriptor, 1, wait.as_millis() as i32) };
    if result < 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::Interrupted {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if result == 0 {
        return Ok(None);
    }
    if descriptor.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
        return Err(io::Error::other("terminal disconnected"));
    }
    let mut byte = 0;
    let count = unsafe {
        libc::read(
            libc::STDIN_FILENO,
            &mut byte as *mut u8 as *mut libc::c_void,
            1,
        )
    };
    if count == 1 {
        Ok(Some(byte))
    } else {
        Err(io::Error::other("terminal input closed during negotiation"))
    }
}

pub struct Signals {
    pub shutdown: Arc<AtomicBool>,
    ids: Vec<signal_hook::SigId>,
}
impl Signals {
    pub fn install() -> io::Result<Self> {
        let mut signals = Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            ids: Vec::new(),
        };
        // Suspend is explicitly unsupported; SIGTSTP requests a clean exit.
        for signal in [
            libc::SIGINT,
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGQUIT,
            libc::SIGTSTP,
        ] {
            signals.ids.push(signal_hook::flag::register(
                signal,
                signals.shutdown.clone(),
            )?);
        }
        Ok(signals)
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_preserves_input_and_verifies_flags() {
        let mut p = Probe::default();
        let stream =
            format!(" \x1b[97;1:1u\x1b_Gi={PROBE_ID};OK\x1b\\\x1b[?11u\x1b[?2026;2$y\x1b[?62;4c");
        for b in stream.bytes() {
            p.push(b).unwrap();
        }
        assert!(p.supported() && p.synchronized && p.barrier);
        assert_eq!(p.unrelated, b" \x1b[97;1:1u");
        p.keyboard = Some(3);
        assert!(!p.supported());
    }
    #[test]
    fn cleanup_after_partial_initialization_is_idempotent() {
        let mut s = Session::new(Vec::new());
        s.modes.alternate = true;
        s.modes.keyboard = true;
        s.modes.hidden = true;
        s.graphics.upload(&mut s.out, 1, 1, 1, &[0; 4]).unwrap();
        s.cleanup().unwrap();
        let count = s.out.len();
        s.cleanup().unwrap();
        assert_eq!(s.out.len(), count);
        let output = String::from_utf8_lossy(&s.out);
        assert!(output.contains("d=N,I=1"));
        assert!(output.find("d=N").unwrap() < output.find("?1049l").unwrap());
        assert!(output.contains("<1u") && output.contains("?25h"));
    }
    #[test]
    fn output_failure_does_not_skip_raw_restoration_or_remaining_modes() {
        static RESTORED: AtomicBool = AtomicBool::new(false);
        fn raw() -> io::Result<()> {
            RESTORED.store(true, Ordering::Relaxed);
            Ok(())
        }
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("injected output failure"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("injected flush failure"))
            }
        }
        let mut s = Session::new(Broken);
        s.restore_raw = raw;
        s.modes = Modes {
            raw: true,
            alternate: true,
            hidden: true,
            keyboard: true,
            focus: true,
        };
        assert!(s.cleanup().is_err());
        assert!(RESTORED.load(Ordering::Relaxed));
        assert!(
            !s.modes.raw
                && !s.modes.alternate
                && !s.modes.hidden
                && !s.modes.keyboard
                && !s.modes.focus
        );
    }
    #[test]
    fn unwinding_drops_session_and_restores_owned_modes() {
        use std::{cell::RefCell, rc::Rc};
        #[derive(Clone)]
        struct Shared(Rc<RefCell<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.borrow_mut().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let bytes = Rc::new(RefCell::new(Vec::new()));
        let capture = bytes.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut s = Session::new(Shared(capture));
            s.fullscreen().unwrap();
            s.graphics
                .upload(&mut Vec::new(), 17, 1, 1, &[0; 4])
                .unwrap();
            panic!("injected unwind");
        }));
        assert!(result.is_err());
        let bytes = bytes.borrow();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("d=N,I=17")
                && text.contains("?25h")
                && text.contains("<1u")
                && text.contains("?1049l")
        );
    }
}
