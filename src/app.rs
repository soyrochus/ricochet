use crate::{
    audio::{Audio, Level as SoundLevel},
    game::{Event as GameEvent, Game},
    input::{Action, Input},
    render::{Renderer, Viewport, fx::Level as FxLevel},
    terminal::{self, Geometry, Session, Signals},
    timing::Clock,
};
use crossterm::event::{self, Event};
use std::{
    collections::VecDeque,
    io,
    panic::{self, UnwindSafe},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

/// Report only after stack unwinding has dropped the terminal session. This is
/// called once at each executable entry point, before any worker/event loop.
pub fn restored_errors(
    operation: impl FnOnce() -> Result<(), String> + UnwindSafe,
) -> Result<(), String> {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(operation);
    panic::set_hook(hook);
    match result {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            Err(format!("unexpected error (terminal restored): {message}"))
        }
    }
}

pub const HELP: &str = "Ricochet - pixel Breakout for Ghostty 1.4+ on macOS/Linux\n\nUsage: ricochet [--fps 30|60] [--fx full|reduced|off] [--sound on|off] [--help] [--version]\n\n--fps 30|60          Render rate (default: 60)\n--fx full|reduced|off Effects level (default: full); reduced omits shake/overlay\n                     and limits particles/trail; off keeps static art/banners\n--sound on|off        Bump sound effects (default: on); wall, paddle and brick\n                     contacts only, no music and no continuous sound\n\nLeft/Right or A/D: move while held (both directions cancel)\nSpace: start / launch   P or Escape: pause / resume\nR: restart from pause or end screen   Q / Ctrl-C: quit\n\nRequires interactive stdin/stdout, Kitty graphics and enhanced key releases,\nand at least 80x30 cells with 400x300 drawable pixels.\nSIGTSTP exits cleanly; Ctrl-Z is ignored; suspend/resume is unsupported.\n";

#[derive(Debug, PartialEq, Eq)]
pub enum Cli {
    Help,
    Version,
    Run {
        fps: u32,
        fx: FxLevel,
        sound: SoundLevel,
    },
}
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let mut fps = 60;
    let mut fx = FxLevel::Full;
    let mut sound = SoundLevel::On;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(Cli::Help),
            "--version" | "-V" => return Ok(Cli::Version),
            "--fps" => {
                fps = match args.next().as_deref() {
                    Some("30") => 30,
                    Some("60") => 60,
                    _ => return Err("--fps requires 30 or 60".to_owned()),
                }
            }
            "--fx" => {
                fx = match args.next().as_deref() {
                    Some("full") => FxLevel::Full,
                    Some("reduced") => FxLevel::Reduced,
                    Some("off") => FxLevel::Off,
                    _ => return Err("--fx requires full, reduced or off".to_owned()),
                }
            }
            "--sound" => {
                sound = match args.next().as_deref() {
                    Some("on") => SoundLevel::On,
                    Some("off") => SoundLevel::Off,
                    _ => return Err("--sound requires on or off".to_owned()),
                }
            }
            _ => return Err(format!("unknown argument: {arg} (try --help)")),
        }
    }
    Ok(Cli::Run { fps, fx, sound })
}

/// Pure controller shared by the real event loop and acceptance tests.
pub struct Controller {
    pub game: Game,
    pub input: Input,
    pub clock: Clock,
    events: VecDeque<GameEvent>,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            game: Game::default(),
            input: Input::default(),
            clock: Clock::default(),
            events: VecDeque::with_capacity(256),
        }
    }
}
impl Controller {
    pub fn queue_events(&mut self, events: impl IntoIterator<Item = GameEvent>) {
        for event in events {
            if self.events.len() == 256 {
                self.events.pop_front();
            }
            self.events.push_back(event);
        }
    }
    pub fn drain_events(&mut self) -> Vec<GameEvent> {
        self.events.drain(..).collect()
    }
    /// Returns true when quitting. Any discontinuity invalidates accumulated
    /// time and held keys; focus gain never resumes.
    pub fn event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(key) => {
                if let Some(action) = self.input.key(key, self.game.moving()) {
                    if action == Action::Quit {
                        return true;
                    }
                    let before = (self.game.state, self.game.resume_required);
                    let events = self.game.action(action);
                    self.queue_events(events);
                    if before != (self.game.state, self.game.resume_required) {
                        self.input.clear();
                        self.clock.reset();
                    }
                }
            }
            Event::FocusLost => {
                self.input.clear();
                self.clock.reset();
                self.game.focus_lost();
            }
            _ => {}
        }
        false
    }
    pub fn geometry(&mut self, geometry: Geometry) {
        self.input.clear();
        self.clock.reset();
        self.game.viewport(Viewport::new(geometry).is_some());
    }
    pub fn elapsed(&mut self, elapsed: Duration) {
        let count = self.clock.advance(elapsed, self.game.ticking());
        for _ in 0..count {
            let previous = self.game.state;
            let events = self.game.tick(self.input.direction());
            self.queue_events(events);
            if self.game.state != previous {
                self.input.clear();
                self.clock.reset();
                break;
            }
        }
    }
}

pub fn run(fps: u32, fx: FxLevel, sound: SoundLevel) -> io::Result<()> {
    run_game_with_fx(fps, Game::default(), fx, sound)
}
pub fn run_game(fps: u32, game: Game) -> io::Result<()> {
    run_game_with_fx(fps, game, FxLevel::Full, SoundLevel::On)
}
pub fn run_game_with_fx(fps: u32, game: Game, fx: FxLevel, sound: SoundLevel) -> io::Result<()> {
    if !matches!(fps, 30 | 60) {
        return Err(io::Error::other("FPS must be 30 or 60"));
    }
    let signals = Signals::install()?;
    let mut session = Session::start()?;
    let mut capabilities = terminal::negotiate(&mut session.out, &signals.shutdown)?;
    session.graphics.compression = capabilities.compression;
    let mut geometry = capabilities.geometry;
    let mut control = Controller {
        game,
        ..Controller::default()
    };
    control.geometry(geometry);
    let mut renderer = Renderer::new(fx);
    let audio = Audio::new(sound);
    let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
    let mut previous = Instant::now();
    let mut render_at = previous;
    let mut rendered_at = previous;
    let mut geometry_at = previous;
    loop {
        if signals.shutdown.load(Ordering::Relaxed) {
            break;
        }
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(previous);
        previous = now;
        // Bounded input batches keep a flood of events from starving physics.
        let mut discontinuity = false;
        for _ in 0..128 {
            let incoming = if let Some(event) = capabilities.pending.pop_front() {
                Some(event)
            } else if event::poll(Duration::ZERO)? {
                Some(event::read()?)
            } else {
                None
            };
            let Some(event) = incoming else {
                break;
            };
            let before = (control.game.state, control.game.resume_required);
            if matches!(event, Event::Resize(..)) {
                geometry_at = now;
                discontinuity = true;
            }
            if matches!(event, Event::FocusLost) {
                discontinuity = true;
            }
            if control.event(event) {
                return session.cleanup();
            }
            discontinuity |= before != (control.game.state, control.game.resume_required);
        }
        if now >= geometry_at {
            let updated = Geometry::read()?;
            if geometry != updated {
                geometry = updated;
                control.geometry(geometry);
                discontinuity = true;
            }
            geometry_at = now + Duration::from_millis(250);
        }
        if discontinuity {
            control.clock.reset();
            previous = Instant::now();
        } else {
            control.elapsed(elapsed);
        }
        if now >= render_at {
            let events = control.drain_events();
            audio.ingest(&events);
            renderer.draw(
                &mut session,
                geometry,
                &control.game,
                &events,
                now.saturating_duration_since(rendered_at),
                capabilities.synchronized,
            )?;
            rendered_at = now;
            render_at += interval;
            if render_at <= Instant::now() {
                render_at = Instant::now() + interval;
            }
        }
        let tick_wait = if control.game.ticking() {
            control.clock.until_tick()
        } else {
            Duration::from_millis(50)
        };
        let wait = tick_wait
            .min(render_at.saturating_duration_since(Instant::now()))
            .min(geometry_at.saturating_duration_since(Instant::now()))
            .min(Duration::from_millis(50));
        // poll waits for input or the next deadline; no busy-spin or second
        // reader. The next iteration consumes the event it reports as ready.
        event::poll(wait)?;
    }
    session.cleanup()
}

#[cfg(feature = "dev-harness")]
pub fn fixture(name: &str) -> Option<Game> {
    use crate::{constants::*, game::State, physics::Vec2};
    let mut game = Game::default();
    game.start();
    match name {
        "won" => {
            game.level = 3;
            game.bricks = crate::levels::generate(3);
            for b in &mut game.bricks {
                b.hp = 0;
            }
            game.bricks[0].hp = 1;
            let b = &game.bricks[0];
            game.ball.position =
                Vec2::new(b.rect.x + 20.0, b.rect.y + b.rect.h + BALL_RADIUS + 0.1);
            game.ball.velocity = Vec2::new(0.0, -game.ball.speed);
            game.state = State::Playing;
            game.tick(0.0);
        }
        "game-over" => {
            game.lives = 1;
            game.state = State::Playing;
            game.ball.position = Vec2::new(10.0, HEIGHT + BALL_RADIUS - 0.1);
            game.ball.velocity = Vec2::new(0.0, game.ball.speed);
            game.tick(0.0);
        }
        "damaged" => {
            game.bricks[0].hp = 1;
        }
        _ => return None,
    }
    Some(game)
}
