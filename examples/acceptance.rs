//! Explicit development fixture; absent from the normal gameplay CLI.
use std::{io, process::ExitCode};
fn execute() -> io::Result<()> {
    let name = std::env::args().nth(1).unwrap_or_default();
    if name == "effects" {
        return effects();
    }
    if matches!(name.as_str(), "error" | "panic") {
        use ricochet::{
            game::Game,
            render::Renderer,
            terminal::{self, Session, Signals},
        };
        let signals = Signals::install()?;
        let mut session = Session::start()?;
        let capabilities = terminal::negotiate(&mut session.out, &signals.shutdown)?;
        session.graphics.compression = capabilities.compression;
        Renderer::default().draw(
            &mut session,
            capabilities.geometry,
            &Game::default(),
            &[],
            std::time::Duration::ZERO,
            capabilities.synchronized,
        )?;
        if name == "panic" {
            panic!("development injected panic");
        }
        return Err(io::Error::other("development injected runtime error"));
    }
    let game = ricochet::app::fixture(&name).ok_or_else(|| {
        io::Error::other(
            "usage: acceptance won|game-over|damaged|effects [--fx full|reduced|off]|error|panic",
        )
    })?;
    ricochet::app::run_game(60, game)
}
fn main() -> ExitCode {
    match ricochet::app::restored_errors(|| execute().map_err(|e| e.to_string())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ricochet acceptance: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Deterministic presentation scene: synthetic notifications, no gameplay cheats.
fn effects() -> io::Result<()> {
    use crossterm::event::{self, Event};
    use ricochet::{
        app::{self, Cli, Controller},
        game::{Active, Event as GameEvent, State},
        physics::{ContactEvent, ContactKind, Vec2},
        render::{Renderer, fx::Level},
        terminal::{self, Geometry, Session, Signals},
    };
    use std::{
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    let level = match app::parse(std::env::args().skip(2)).map_err(io::Error::other)? {
        Cli::Run { fx, .. } => fx,
        _ => Level::Full,
    };
    let signals = Signals::install()?;
    let mut session = Session::start()?;
    let mut caps = terminal::negotiate(&mut session.out, &signals.shutdown)?;
    session.graphics.compression = caps.compression;
    let mut geometry = caps.geometry;
    let mut control = Controller::default();
    control.geometry(geometry);
    let mut renderer = Renderer::new(level);
    let mut last = Instant::now();
    let mut elapsed = 0.0;
    let mut previous_step = usize::MAX;
    let mut automatic_pause = false;
    loop {
        if signals.shutdown.load(Ordering::Relaxed) {
            break;
        }
        for _ in 0..128 {
            let e = if let Some(e) = caps.pending.pop_front() {
                Some(e)
            } else if event::poll(Duration::ZERO)? {
                Some(event::read()?)
            } else {
                None
            };
            let Some(e) = e else {
                break;
            };
            if matches!(e, Event::Resize(..)) {
                geometry = Geometry::read()?;
                control.geometry(geometry);
            }
            if control.event(e) {
                return session.cleanup();
            }
        }
        let updated = Geometry::read()?;
        if geometry != updated {
            geometry = updated;
            control.geometry(geometry);
        }
        let now = Instant::now();
        let dt = now - last;
        last = now;
        if !control.game.viewport_small
            && !control.game.resume_required
            && (!matches!(control.game.state, State::Paused(_)) || automatic_pause)
        {
            elapsed += dt.as_secs_f64().min(0.05);
        }
        let step = (elapsed * 10.0) as usize % 110;
        let mut events = control.drain_events();
        if step != previous_step {
            previous_step = step;
            let game = &mut control.game;
            match step {
                0 => {
                    let suspension = (game.viewport_small, game.resume_required);
                    *game = ricochet::game::Game::default();
                    (game.viewport_small, game.resume_required) = suspension;
                    events.push(GameEvent::Restarted);
                }
                25 => {
                    game.start();
                    events.push(GameEvent::Restarted);
                    game.bricks[0].initial_hp = 3;
                    game.bricks[0].hp = 3;
                }
                32 => {
                    game.state = State::Playing;
                    events.push(GameEvent::Launched);
                }
                38 | 40 | 42 => {
                    let hp = if step == 38 {
                        2
                    } else if step == 40 {
                        1
                    } else {
                        0
                    };
                    game.bricks[0].hp = hp;
                    let rect = game.bricks[0].rect;
                    events.push(GameEvent::Contact(ContactEvent {
                        kind: ContactKind::Brick {
                            index: 0,
                            hp_after: hp,
                        },
                        point: Vec2::new(rect.x + 34.0, rect.y + 24.0),
                        normal: Vec2::new(0.0, 1.0),
                    }));
                }
                43 => {
                    game.state = State::Paused(Active::Playing);
                    automatic_pause = true;
                }
                50 => {
                    game.state = State::Playing;
                    automatic_pause = false;
                }
                55 => events.push(GameEvent::Contact(ContactEvent {
                    kind: ContactKind::Paddle { offset: 0.0 },
                    point: Vec2::new(game.paddle.centre(), 556.0),
                    normal: Vec2::new(0.0, -1.0),
                })),
                60 => {
                    game.state = State::Ready;
                    events.push(GameEvent::LifeLost);
                }
                70 | 80 => {
                    game.level = if step == 70 { 2 } else { 3 };
                    game.state = State::Ready;
                    events.push(GameEvent::LevelCleared { next: game.level });
                }
                90 => {
                    game.state = State::GameOver;
                    events.push(GameEvent::GameOver);
                }
                100 => {
                    game.state = State::Won;
                    events.push(GameEvent::Won);
                }
                _ => {}
            }
        }
        if control.game.state == State::Playing {
            control.game.ball.position = Vec2::new(
                400.0 + (elapsed * 2.0).sin() * 180.0,
                380.0 + (elapsed * 3.0).cos() * 100.0,
            );
        }
        renderer.draw(
            &mut session,
            geometry,
            &control.game,
            &events,
            dt,
            caps.synchronized,
        )?;
        event::poll(Duration::from_millis(16))?;
    }
    session.cleanup()
}
