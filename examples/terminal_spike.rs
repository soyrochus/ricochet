//! Kept as a real-terminal diagnostic. It doesn't bypass capability checks.
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ricochet::{
    input::{Action, Input},
    render::{Viewport, kitty, sprites},
    terminal::{self, Session, Signals},
};
use std::{
    io::{self, Write},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn main() -> io::Result<()> {
    let signals = Signals::install()?;
    let mut session = Session::start()?;
    let mut capabilities = terminal::negotiate(&mut session.out, &signals.shutdown)?;
    session.graphics.compression = capabilities.compression;
    let view = Viewport::with_compression(capabilities.geometry, capabilities.compression)
        .ok_or_else(|| io::Error::other("resize to 80x30 with at least 400x300 drawable pixels"))?;
    let mut bytes = Vec::new();
    for (number, sprite) in [
        (1, sprites::background(view.scale)),
        (2, sprites::ball(view.scale)),
    ] {
        session.graphics.upload(
            &mut bytes,
            number,
            sprite.width,
            sprite.height,
            &sprite.rgba,
        )?;
    }
    session
        .graphics
        .place(&mut bytes, 1, 1, view.at(0.0, 0.0), 0)?;
    write!(
        bytes,
        "\x1b[1;1HRICOCHET / terminal diagnostic\x1b[2;1HBall moves in sub-cell steps. Hold A/D or arrows; release; Q quits."
    )?;
    kitty::frame(&mut session.out, &bytes, capabilities.synchronized)?;
    let mut input = Input::default();
    let mut x = 400.0;
    let mut last = Instant::now();
    let mut releases = 0;
    loop {
        if signals.shutdown.load(Ordering::Relaxed) {
            break;
        }
        let now = Instant::now();
        let elapsed = (now - last).as_secs_f64().min(0.1);
        last = now;
        x = (x + input.direction() * 150.0 * elapsed).clamp(10.0, 780.0);
        let mut bytes = Vec::new();
        session
            .graphics
            .place(&mut bytes, 2, 1, view.at(x, 360.0), 3)?;
        write!(
            bytes,
            "\x1b[{};1H\x1b[2KRelease events: {releases}  x={x:.2}  (Paddle input clears on focus loss)",
            capabilities.geometry.rows - 1
        )?;
        kitty::frame(&mut session.out, &bytes, capabilities.synchronized)?;
        let event = if let Some(event) = capabilities.pending.pop_front() {
            Some(event)
        } else if event::poll(Duration::from_millis(16))? {
            Some(event::read()?)
        } else {
            None
        };
        match event {
            Some(Event::Key(key)) => {
                if key.kind == KeyEventKind::Release
                    && matches!(
                        key.code,
                        KeyCode::Left | KeyCode::Right | KeyCode::Char('a' | 'd')
                    )
                {
                    releases += 1;
                }
                if input.key(key, true) == Some(Action::Quit) {
                    break;
                }
            }
            Some(Event::FocusLost) => input.clear(),
            Some(Event::Resize(..)) => break, // Re-run at the new geometry.
            _ => {}
        }
    }
    session.cleanup()
}
