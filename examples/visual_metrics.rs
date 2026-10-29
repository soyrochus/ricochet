//! Repeatable CPU/transport measurements; never requires a terminal.
use ricochet::{
    constants::*,
    game::{Event, Game, State},
    physics::{ContactEvent, ContactKind, Vec2},
    render::{Renderer, fx::Level, kitty, sprites},
    terminal::{Geometry, Session},
};
use std::{
    io,
    time::{Duration, Instant},
};
fn main() -> io::Result<()> {
    let began = Instant::now();
    let atlases = sprites::generate(3.0, true);
    let regeneration = began.elapsed();
    if std::env::args().any(|a| a == "--profile") {
        for (i, a) in atlases.iter().enumerate() {
            eprintln!(
                "atlas {i}: {} bytes -> {} zlib",
                a.sprite.rgba.len(),
                kitty::compress(&a.sprite.rgba)?.len()
            );
        }
        let t = Instant::now();
        sprites::background(3.0);
        eprintln!("background {:?}", t.elapsed());
        let t = Instant::now();
        sprites::overlay(3.0);
        eprintln!("overlay {:?}", t.elapsed());
        let t = Instant::now();
        for (i, h) in sprites::VARIANTS {
            sprites::brick(3.0, i, h);
        }
        eprintln!("bricks {:?}", t.elapsed());
        let t = Instant::now();
        ricochet::render::font::text("RICOCHET", 10.0, 3.0);
        eprintln!("logo {:?}", t.elapsed());
    }
    let decoded: usize = atlases.iter().map(|a| a.sprite.rgba.len()).sum();
    let mut registry = kitty::Registry::default();
    registry.compression = true;
    let mut upload = Vec::new();
    for (i, a) in atlases.iter().enumerate() {
        registry.upload(
            &mut upload,
            i as u32 + 1,
            a.sprite.width,
            a.sprite.height,
            &a.sprite.rgba,
        )?;
    }
    if std::env::args().any(|a| a == "--art") {
        // Raw atlases for the standard-library PNG review helper.
        for (i, a) in atlases.iter().enumerate() {
            std::fs::write(
                format!(
                    "/tmp/ricochet-atlas-{i}-{}-{}.rgba",
                    a.sprite.width, a.sprite.height
                ),
                &a.sprite.rgba,
            )?;
        }
    }
    let geometry = Geometry {
        columns: 140,
        rows: 48,
        pixel_width: 2800,
        pixel_height: 2112,
    };
    let mut game = Game::default();
    game.start();
    game.state = State::Playing;
    game.ball.speed = MAX_SPEED;
    let mut session = Session::new(Vec::new());
    session.graphics.compression = true;
    let mut renderer = Renderer::new(Level::Full);
    renderer.draw(&mut session, geometry, &game, &[], Duration::ZERO, true)?;
    let mut times = Vec::new();
    let mut steady = 0;
    let mut shaking = 0;
    let mut peak = 0;
    for frame in 0..600 {
        let events: Vec<_> = (0..12)
            .map(|i| {
                Event::Contact(ContactEvent {
                    kind: ContactKind::Brick {
                        index: i,
                        hp_after: 0,
                    },
                    point: Vec2::new(200.0 + f64::from(i as u32) * 20.0, 100.0),
                    normal: Vec2::new(0.0, 1.0),
                })
            })
            .collect();
        let history = if frame % 15 == 0 {
            events.as_slice()
        } else {
            &[]
        };
        game.ball.position = Vec2::new(400.0 + (f64::from(frame) * 0.1).sin() * 200.0, 400.0);
        session.out.clear();
        let began = Instant::now();
        renderer.draw(
            &mut session,
            geometry,
            &game,
            history,
            Duration::from_secs_f64(1.0 / 60.0),
            true,
        )?;
        times.push(began.elapsed().as_nanos());
        if renderer.effects.shake_offset() != Vec2::default() {
            shaking = shaking.max(session.out.len());
        } else {
            steady = steady.max(session.out.len());
        }
        peak = peak.max(session.graphics.placement_count());
    }
    times.sort_unstable();
    println!(
        "{{\"scale\":3,\"images\":{},\"decoded_bytes\":{decoded},\"compressed_protocol_bytes\":{},\"regeneration_ms\":{:.3},\"render_median_ms\":{:.4},\"steady_frame_peak_bytes\":{steady},\"shake_frame_peak_bytes\":{shaking},\"peak_placements\":{peak}}}",
        atlases.len(),
        upload.len(),
        regeneration.as_secs_f64() * 1000.0,
        times[times.len() / 2] as f64 / 1e6
    );
    Ok(())
}
