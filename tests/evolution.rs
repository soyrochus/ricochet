use base64::{Engine, engine::general_purpose::STANDARD};
use ricochet::{
    app::{Cli, Controller, parse},
    constants::*,
    game::{Active, Event, Game, State},
    input::Action,
    levels,
    physics::{self, Ball, ContactEvent, ContactKind, Paddle, Rect, Vec2},
    render::{
        Renderer, Viewport, font,
        fx::{Effects, Level},
        kitty::{self, PixelPlacement, Registry, SourceRect},
        sprites::{self, Frame},
    },
    terminal::{COMPRESSION_PROBE_ID, Geometry, PROBE_ID, Probe, Session},
};
use std::{io::Read, time::Duration};
fn contact(index: usize, hp_after: u8) -> Event {
    Event::Contact(ContactEvent {
        kind: ContactKind::Brick { index, hp_after },
        point: Vec2::new(400.0, 200.0),
        normal: Vec2::new(0.0, -1.0),
    })
}
fn game() -> Game {
    let mut g = Game::default();
    g.start();
    g.action(Action::Space);
    g
}
fn geometry(columns: u16, rows: u16, cw: u32, ch: u32) -> Geometry {
    Geometry {
        columns,
        rows,
        pixel_width: u32::from(columns) * cw,
        pixel_height: u32::from(rows) * ch,
    }
}
#[test]
fn compression_roundtrips_exact_dimensions_and_chunk_boundaries() {
    let mut seed = 7_u32;
    for (w, h) in [(1, 1), (769, 1), (100, 100)] {
        let bytes: Vec<u8> = (0..w * h * 4)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let mut r = Registry::default();
        r.compression = true;
        let mut out = Vec::new();
        r.upload(&mut out, 19, w, h, &bytes).unwrap();
        let text = String::from_utf8(out).unwrap();
        let mut combined = String::new();
        let chunks: Vec<_> = text.split("\x1b_G").skip(1).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let (header, payload) = chunk.split_once(';').unwrap();
            let payload = payload.strip_suffix("\x1b\\").unwrap();
            assert!(payload.len() <= kitty::CHUNK);
            if i == 0 {
                assert!(header.contains(&format!("s={w},v={h},o=z")));
            } else {
                assert!(!header.contains("o=z") && !header.contains("s="));
            }
            assert!(header.contains(if i + 1 == chunks.len() { "m=0" } else { "m=1" }));
            if i + 1 < chunks.len() {
                assert_eq!(payload.len(), 4096);
            }
            combined.push_str(payload);
        }
        let compressed = STANDARD.decode(combined).unwrap();
        let mut decoded = Vec::new();
        flate2::read::ZlibDecoder::new(&compressed[..])
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded.len(), (w * h * 4) as usize);
        assert_eq!(decoded, bytes);
    }
}
#[test]
fn optional_probe_failures_do_not_reject_terminal_and_use_scale_two() {
    for reply in [None, Some("EINVAL:unsupported"), Some("OK")] {
        let mut p = Probe::default();
        let mut stream = format!("\x1b_Gi={PROBE_ID};OK\x1b\\\x1b[?11u");
        if let Some(reply) = reply {
            stream += &format!("\x1b_Gi={COMPRESSION_PROBE_ID};{reply}\x1b\\");
        }
        stream += "\x1b[?62c";
        for b in stream.bytes() {
            p.push(b).unwrap();
        }
        assert!(p.supported() && p.barrier);
        assert_eq!(p.compression, reply == Some("OK"));
        assert!(p.unrelated.is_empty());
        let view = Viewport::with_compression(geometry(160, 70, 20, 40), p.compression).unwrap();
        assert_eq!(view.scale, if p.compression { 3.0 } else { 2.0 });
        let mut r = Registry::default();
        r.compression = p.compression;
        let mut out = Vec::new();
        r.upload(&mut out, 1, 1, 1, &[0; 4]).unwrap();
        assert_eq!(String::from_utf8_lossy(&out).contains("o=z"), p.compression);
    }
}
#[test]
fn placements_include_all_source_rectangle_keys() {
    let mut r = Registry::default();
    let mut out = Vec::new();
    r.upload(&mut out, 1, 100, 80, &vec![0; 100 * 80 * 4])
        .unwrap();
    out.clear();
    r.place_clipped(
        &mut out,
        1,
        7,
        PixelPlacement {
            column: 1,
            row: 2,
            x_offset: 3,
            y_offset: 4,
        },
        6,
        Some(SourceRect {
            x: 10,
            y: 20,
            w: 28,
            h: 14,
        }),
    )
    .unwrap();
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("a=p,I=1,p=7,X=3,Y=4,z=6,C=1,q=2,x=10,y=20,w=28,h=14;")
    );
}
#[test]
fn contacts_report_resolution_order_points_and_stable_brick_indices() {
    let mut b = Ball {
        position: Vec2::new(8.0, 8.0),
        velocity: Vec2::new(-200.0, -200.0),
        speed: 200.0_f64.hypot(200.0),
    };
    let out = physics::step(
        &mut b,
        &mut Paddle::default(),
        &mut levels::generate(1),
        0.0,
        1.0,
        DT,
    );
    assert_eq!(out.events.len(), 2);
    assert_eq!(
        out.events[0].kind,
        ContactKind::Wall {
            normal: Vec2::new(1.0, 0.0)
        }
    );
    assert_eq!(out.events[0].point.x, 0.0);
    assert_eq!(
        out.events[1].kind,
        ContactKind::Wall {
            normal: Vec2::new(0.0, 1.0)
        }
    );
    assert_eq!(out.events[1].point.y, 0.0);
    let mut bricks = levels::generate(1);
    bricks.truncate(2);
    bricks[0].rect = Rect {
        x: 100.0,
        y: 100.0,
        w: 68.0,
        h: 24.0,
    };
    bricks[0].index = 8;
    bricks[1].rect = Rect {
        x: 176.0,
        y: 100.0,
        w: 68.0,
        h: 24.0,
    };
    bricks[1].index = 9;
    let mut b = Ball {
        position: Vec2::new(172.0, 150.0),
        velocity: Vec2::new(0.0, -300.0),
        speed: 300.0,
    };
    let out = physics::step(&mut b, &mut Paddle::default(), &mut bricks, 0.0, 1.0, 0.1);
    assert_eq!(
        out.events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec![
            ContactKind::Brick {
                index: 8,
                hp_after: 1
            },
            ContactKind::Brick {
                index: 9,
                hp_after: 1
            }
        ]
    );
    let mut b = Ball {
        position: Vec2::new(400.0, 548.0),
        velocity: Vec2::new(0.0, 300.0),
        speed: 300.0,
    };
    let out = physics::step(
        &mut b,
        &mut Paddle::default(),
        &mut levels::generate(1),
        0.0,
        1.0,
        DT,
    );
    assert_eq!(out.events[0].kind, ContactKind::Paddle { offset: 0.0 });
    assert_eq!(out.events[0].point.y, PADDLE_Y);
}
#[test]
fn transition_events_fire_once_and_do_not_live_in_game() {
    let mut g = Game::default();
    assert_eq!(g.action(Action::Space), vec![Event::Restarted]);
    assert_eq!(g.action(Action::Space), vec![Event::Launched]);
    assert!(g.action(Action::Space).is_empty());
    for lives in (0..3).rev() {
        g.state = State::Playing;
        g.ball.position = Vec2::new(10.0, 606.9);
        g.ball.velocity = Vec2::new(0.0, 300.0);
        let events = g.tick(0.0);
        assert_eq!(
            events,
            if lives == 0 {
                vec![Event::LifeLost, Event::GameOver]
            } else {
                vec![Event::LifeLost]
            }
        );
        assert!(g.tick(0.0).is_empty());
    }
    for level in 1..=3 {
        g.start();
        g.level = level;
        g.state = State::Playing;
        for b in &mut g.bricks {
            b.hp = 0;
        }
        g.bricks[0].hp = 1;
        let r = g.bricks[0].rect;
        g.ball.position = Vec2::new(r.x + 20.0, r.y + r.h + 7.1);
        g.ball.velocity = Vec2::new(0.0, -300.0);
        let events = g.tick(0.0);
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[1],
            if level == 3 {
                Event::Won
            } else {
                Event::LevelCleared { next: level + 1 }
            }
        );
        assert!(g.tick(0.0).is_empty());
    }
}
#[test]
fn controller_queue_drops_oldest_and_drains_once() {
    let mut c = Controller::default();
    c.queue_events((0..300).map(|i| contact(i, 1)));
    let events = c.drain_events();
    assert_eq!(events.len(), 256);
    assert_eq!(events[0], contact(44, 1));
    assert_eq!(events[255], contact(299, 1));
    assert!(c.drain_events().is_empty());
}
#[test]
fn seeded_effects_and_fixed_particle_steps_are_repeatable_at_30_and_60() {
    let g = game();
    let mut a = Effects::default();
    let mut b = Effects::default();
    let events = [contact(3, 0), contact(5, 1), Event::LifeLost];
    a.ingest(&events, &g);
    b.ingest(&events, &g);
    assert_eq!(a, b);
    for _ in 0..4 {
        a.advance(Duration::from_secs_f64(1.0 / 30.0), &g);
        b.advance(Duration::from_secs_f64(1.0 / 30.0), &g);
        assert_eq!(a.shake_offset(), b.shake_offset());
    }
    let mut a = Effects::default();
    let mut b = Effects::default();
    a.ingest(&[contact(3, 0)], &g);
    b.ingest(&[contact(3, 0)], &g);
    for _ in 0..12 {
        a.advance(Duration::from_secs_f64(1.0 / 30.0), &g);
    }
    for _ in 0..24 {
        b.advance(Duration::from_secs_f64(1.0 / 60.0), &g);
    }
    assert_eq!(a.particles, b.particles);
}
#[test]
fn effects_freeze_clamp_time_clear_trails_and_retire_outside_particles() {
    let mut g = game();
    let mut fx = Effects::default();
    fx.ingest(&[contact(1, 0), Event::LifeLost], &g);
    fx.observe(&g);
    for freeze in 0..3 {
        g.state = if freeze == 0 {
            State::Paused(Active::Playing)
        } else {
            State::Playing
        };
        g.viewport_small = freeze == 1;
        g.resume_required = freeze == 2;
        let particles = fx.particles.clone();
        let shake = fx.shake_offset();
        let clock = fx.clock;
        let trail = fx.trail.clone();
        fx.advance(Duration::from_secs(5), &g);
        fx.observe(&g);
        assert_eq!(fx.particles, particles);
        assert_eq!(fx.shake_offset(), shake);
        assert_eq!(fx.clock, clock);
        assert_eq!(fx.trail, trail);
    }
    g.state = State::Playing;
    g.viewport_small = false;
    g.resume_required = false;
    fx.advance(Duration::ZERO, &g);
    let mut clone = fx.clone();
    fx.advance(Duration::from_secs(10), &g);
    clone.advance(Duration::from_millis(50), &g);
    assert_eq!(fx, clone);
    for e in [
        Event::Launched,
        Event::LifeLost,
        Event::LevelCleared { next: 2 },
    ] {
        fx.observe(&g);
        assert!(!fx.trail.is_empty());
        fx.ingest(&[e], &g);
        assert!(fx.trail.is_empty());
    }
    fx.ingest(&[contact(1, 0)], &g);
    assert!(!fx.particles.is_empty());
    for p in &mut fx.particles {
        p.position = Vec2::new(-10.0, 200.0);
    }
    fx.advance(Duration::from_millis(10), &g);
    assert!(fx.particles.is_empty());
}
#[test]
fn effect_levels_pools_and_restart_are_bounded() {
    let g = game();
    for level in [Level::Full, Level::Reduced, Level::Off] {
        let mut fx = Effects::new(level);
        fx.ingest(
            &(0..200).map(|i| contact(i % 50, 0)).collect::<Vec<_>>(),
            &g,
        );
        assert_eq!(fx.particles.len(), level.particle_limit());
        assert!(fx.shatter_count() <= 8);
        let mut slots = std::collections::BTreeSet::new();
        for p in &fx.particles {
            assert!(slots.insert(p.slot));
        }
        for _ in 0..20 {
            fx.observe(&g);
        }
        let draws = fx.placements(&g);
        let ghosts = draws
            .iter()
            .filter(|d| matches!(d.frame, Frame::Trail(_)))
            .count();
        assert!(
            ghosts
                <= match level {
                    Level::Full => 8,
                    Level::Reduced => 3,
                    Level::Off => 0,
                }
        );
        if level != Level::Full {
            assert_eq!(fx.shake_offset(), Vec2::default());
        }
        fx.ingest(&[Event::Restarted], &g);
        assert!(fx.particles.is_empty() && fx.trail.is_empty() && fx.shatter_count() == 0);
    }
}
#[test]
fn full_script_has_identical_state_in_all_modes_at_both_frame_rates() {
    let mut expected = None;
    for level in [Level::Full, Level::Reduced, Level::Off] {
        for fps in [30, 60] {
            let mut g = Game::default();
            let mut control = Controller::default();
            let mut fx = Effects::new(level);
            for tick in 0..72_000 {
                if g.state == State::Title {
                    let events = g.action(Action::Space);
                    control.queue_events(events);
                }
                if matches!(g.state, State::Won | State::GameOver) {
                    let events = g.action(Action::Restart);
                    control.queue_events(events);
                }
                if g.state == State::Ready {
                    let events = g.action(Action::Space);
                    control.queue_events(events);
                }
                let direction = (g.ball.position.x - g.paddle.centre()).signum();
                let events = g.tick(direction);
                control.queue_events(events);
                if tick % (120 / fps) == 0 {
                    let before = g.clone();
                    fx.advance(Duration::from_secs_f64(1.0 / f64::from(fps)), &g);
                    fx.ingest(&control.drain_events(), &g);
                    fx.observe(&g);
                    let _ = fx.placements(&g);
                    assert_eq!(g, before);
                }
            }
            if let Some(expected) = &expected {
                assert_eq!(&g, expected);
            } else {
                expected = Some(g);
            }
        }
    }
}
#[test]
fn art_has_coverage_greyscale_damage_padding_and_bounded_scale_three_memory() {
    for sprite in [
        sprites::ball(1.0),
        sprites::paddle(1.0),
        sprites::brick(1.0, 3, 3),
        font::text("A/Z 09!", 6.0, 1.0),
    ] {
        assert!(
            sprite
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 0 && p[3] < 255)
        );
    }
    const {
        assert!(sprites::SHADOW_EXTENT < GAP);
    }
    for (initial, hp) in [(2, 2), (3, 3), (3, 2)] {
        let a = sprites::brick(2.0, initial, hp);
        let b = sprites::brick(2.0, initial, hp - 1);
        let dark = b
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, p)| **p == sprites::NAVY)
            .count();
        assert!(dark > 0);
        let lum = |p: &[u8]| {
            0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
        };
        assert!(
            b.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, p)| **p == sprites::NAVY)
                .filter(|(i, p)| lum(&a.rgba[i * 4..i * 4 + 4]) > lum(*p) + 20.0)
                .count()
                >= 20
        );
    }
    let atlases = sprites::generate(3.0, true);
    let decoded: usize = atlases.iter().map(|a| a.sprite.rgba.len()).sum();
    assert!(decoded <= 64 * 1024 * 1024, "{decoded}");
    assert!(atlases.len() <= kitty::MAX_IMAGES);
    for colour in 1..=3 {
        let strip: Vec<_> = atlases
            .iter()
            .flat_map(|a| &a.frames)
            .filter(|f| matches!(f.frame,Frame::Shatter{colour:c,..} if c==colour))
            .collect();
        assert_eq!(strip.len(), 5);
        assert!(strip.iter().all(|f| f.source.y == strip[0].source.y));
    }
    for atlas in atlases.iter().filter(|a| a.frames.len() > 1) {
        for (i, a) in atlas.frames.iter().enumerate() {
            let r = a.source;
            assert!(
                r.x > 0
                    && r.y > 0
                    && r.x + r.w < atlas.sprite.width
                    && r.y + r.h < atlas.sprite.height
            );
            for x in r.x - 1..=r.x + r.w {
                for y in [r.y - 1, r.y + r.h] {
                    assert_eq!(
                        atlas.sprite.rgba[((y * atlas.sprite.width + x) * 4 + 3) as usize],
                        0
                    );
                }
            }
            for y in r.y..r.y + r.h {
                for x in [r.x - 1, r.x + r.w] {
                    assert_eq!(
                        atlas.sprite.rgba[((y * atlas.sprite.width + x) * 4 + 3) as usize],
                        0
                    );
                }
            }
            for b in &atlas.frames[i + 1..] {
                let s = b.source;
                assert!(r.x + r.w < s.x || s.x + s.w < r.x || r.y + r.h < s.y || s.y + s.h < r.y);
            }
        }
    }
    for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/!".chars() {
        assert_ne!(font::glyph(c), [0; 7]);
    }
}
#[test]
fn cli_accepts_every_effect_level_and_rejects_invalid_values() {
    for (value, fx) in [
        ("full", Level::Full),
        ("reduced", Level::Reduced),
        ("off", Level::Off),
    ] {
        assert_eq!(
            parse(["--fx".into(), value.into(), "--fps".into(), "30".into()]).unwrap(),
            Cli::Run { fps: 30, fx }
        );
    }
    for args in [vec!["--fx"], vec!["--fx", "quiet"]] {
        assert!(parse(args.into_iter().map(str::to_owned)).is_err());
    }
}
struct Placed {
    column: u32,
    row: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    z: i32,
}
fn placements(out: &[u8]) -> Vec<Placed> {
    // Parse cursor+placement commands independently of the renderer's mapping.
    let text = String::from_utf8_lossy(out);
    let mut row = 0;
    let mut column = 0;
    let mut result = Vec::new();
    for part in text.split('\x1b').skip(1) {
        if let Some(cursor) = part.strip_prefix('[').and_then(|p| p.split_once('H'))
            && let Some((r, c)) = cursor.0.split_once(';')
            && let (Ok(r), Ok(c)) = (r.parse::<u32>(), c.parse::<u32>())
        {
            row = r - 1;
            column = c - 1;
        }
        if part.starts_with("_Ga=p,") {
            let fields: std::collections::BTreeMap<_, _> = part[3..]
                .split(';')
                .next()
                .unwrap()
                .split(',')
                .filter_map(|p| p.split_once('='))
                .collect();
            let n = |k: &str| fields[k].parse::<u32>().unwrap();
            result.push(Placed {
                column,
                row,
                x: n("X"),
                y: n("Y"),
                width: n("w"),
                height: n("h"),
                z: fields["z"].parse().unwrap(),
            });
        }
    }
    result
}
#[test]
fn shake_is_clamped_and_every_placement_stays_inside_terminal_and_hud() {
    for geometry in [
        geometry(80, 34, 30, 60),
        geometry(80, 34, 30, 61),
        geometry(160, 70, 20, 40),
    ] {
        let mut session = Session::new(Vec::new());
        session.graphics.compression = true;
        let mut renderer = Renderer::default();
        let g = game();
        renderer
            .draw(
                &mut session,
                geometry,
                &g,
                &[Event::LifeLost],
                Duration::ZERO,
                true,
            )
            .unwrap();
        let v = renderer.view.unwrap();
        if v.left == 0 {
            assert_eq!(v.clamp_shake(Vec2::new(-6.0, -6.0)).x, 0.0);
        }
        for _ in 0..20 {
            session.out.clear();
            renderer
                .draw(
                    &mut session,
                    geometry,
                    &g,
                    &[],
                    Duration::from_secs_f64(1.0 / 60.0),
                    true,
                )
                .unwrap();
            for p in placements(&session.out) {
                assert!(p.column < u32::from(geometry.columns));
                assert!(p.row >= 2 && p.row < u32::from(geometry.rows - 2));
                assert!(p.x < v.cell_width && p.y < v.cell_height);
                assert!((0..=8).contains(&p.z));
                assert!(
                    p.column * v.cell_width + p.x + p.width
                        <= u32::from(geometry.columns) * v.cell_width
                );
                assert!(
                    p.row * v.cell_height + p.y + p.height
                        <= u32::from(geometry.rows - 2) * v.cell_height
                );
            }
        }
        assert_eq!(renderer.effects.shake_offset(), Vec2::default());
    }
}
#[test]
fn maximum_load_frames_stay_within_output_and_resource_budgets() {
    let mut session = Session::new(Vec::new());
    session.graphics.compression = true;
    let mut renderer = Renderer::default();
    let mut g = game();
    g.ball.speed = MAX_SPEED;
    let geometry = geometry(160, 70, 20, 40);
    renderer
        .draw(&mut session, geometry, &g, &[], Duration::ZERO, true)
        .unwrap();
    let mut peak = 0;
    for frame in 0..120 {
        let events: Vec<_> = (0..12).map(|i| contact(i, 0)).collect();
        let events = if frame % 15 == 0 { &events[..] } else { &[] };
        g.ball.position.x = 300.0 + f64::from(frame);
        session.out.clear();
        renderer
            .draw(
                &mut session,
                geometry,
                &g,
                events,
                Duration::from_secs_f64(1.0 / 60.0),
                true,
            )
            .unwrap();
        let limit = if renderer.effects.shake_offset() == Vec2::default() {
            16 * 1024
        } else {
            24 * 1024
        };
        assert!(
            session.out.len() <= limit,
            "frame {frame}: {}",
            session.out.len()
        );
        assert!(!String::from_utf8_lossy(&session.out).contains("a=t"));
        peak = peak.max(session.graphics.placement_count());
        assert!(session.graphics.placement_count() <= 168);
    }
    assert!(peak >= 160);
    assert_eq!(session.graphics.image_count(), 7);
}
#[test]
fn one_hundred_resize_restart_cycles_leave_only_owned_images_and_placements() {
    let mut session = Session::new(Vec::new());
    session.graphics.compression = true;
    let mut renderer = Renderer::default();
    let mut g = game();
    for cycle in 0..100 {
        let geo = geometry(80, 30, if cycle % 2 == 0 { 5 } else { 6 }, 12);
        g.start();
        session.out.clear();
        renderer
            .draw(
                &mut session,
                geo,
                &g,
                &[Event::Restarted, contact(1, 0)],
                Duration::from_millis(16),
                true,
            )
            .unwrap();
        assert!(session.graphics.image_count() <= kitty::MAX_IMAGES);
        assert!(session.graphics.placement_count() <= 168);
    }
    session.out.clear();
    session.cleanup().unwrap();
    let out = String::from_utf8_lossy(&session.out);
    assert_eq!(out.matches("d=N").count(), 7);
    assert!(!out.contains("d=A"));
    assert_eq!(session.graphics.image_count(), 0);
    assert_eq!(session.graphics.placement_count(), 0);
}

#[test]
fn flash_shatter_squash_and_banner_frames_follow_the_effect_clock() {
    let mut g = game();
    let mut fx = Effects::default();
    fx.ingest(
        &[
            contact(1, 2),
            Event::Contact(ContactEvent {
                kind: ContactKind::Paddle { offset: 0.0 },
                point: Vec2::new(400.0, 556.0),
                normal: Vec2::new(0.0, -1.0),
            }),
        ],
        &g,
    );
    assert!(fx.flashing(1));
    assert_eq!(fx.paddle_frame(), 0);
    fx.advance(Duration::from_millis(41), &g);
    assert_eq!(fx.paddle_frame(), 1);
    assert!(fx.flashing(1));
    fx.advance(Duration::from_millis(40), &g);
    assert_eq!(fx.paddle_frame(), 2);
    assert!(!fx.flashing(1));
    fx.advance(Duration::from_millis(40), &g);
    assert_eq!(fx.paddle_frame(), 0);
    fx.ingest(&[contact(2, 0)], &g);
    assert_eq!(fx.shatter_count(), 1);
    for expected in 0..5 {
        assert!(
            fx.placements(&g)
                .iter()
                .any(|d| matches!(d.frame,Frame::Shatter{frame,..} if frame==expected))
        );
        fx.advance(Duration::from_millis(40), &g);
    }
    assert_eq!(fx.shatter_count(), 0);
    g.state = State::Ready;
    fx.ingest(&[Event::LevelCleared { next: 2 }], &g);
    fx.observe(&g);
    for alpha in 0..4 {
        assert!(fx.placements(&g).iter().any(|d| d.frame
            == Frame::Banner {
                kind: sprites::Banner::Level2,
                alpha
            }));
        fx.advance(Duration::from_millis(40), &g);
    }
    g.state = State::Playing;
    fx.ingest(&[Event::Launched], &g);
    fx.observe(&g);
    assert!(
        !fx.placements(&g)
            .iter()
            .any(|d| matches!(d.frame, Frame::Banner { .. }))
    );
    fx.ingest(&[contact(1, 0)], &g);
    assert!(
        fx.particles
            .iter()
            .all(|p| (120.0..=260.0).contains(&p.velocity.length())
                && -p.velocity.y / p.velocity.length() >= 0.5
                && (0.45..=0.7).contains(&p.lifetime))
    );
    for _ in 0..20 {
        fx.advance(Duration::from_millis(50), &g);
    }
    assert!(fx.particles.is_empty());
}

#[test]
fn destruction_deletes_static_brick_in_the_shatter_frame_and_resizes_preserve_effects() {
    let mut g = game();
    let mut s = Session::new(Vec::new());
    s.graphics.compression = true;
    let mut r = Renderer::default();
    r.draw(
        &mut s,
        geometry(80, 30, 8, 16),
        &g,
        &[],
        Duration::ZERO,
        true,
    )
    .unwrap();
    s.out.clear();
    g.bricks[0].hp = 0;
    let before = g.clone();
    r.draw(
        &mut s,
        geometry(80, 30, 8, 16),
        &g,
        &[contact(0, 0)],
        Duration::ZERO,
        true,
    )
    .unwrap();
    assert_eq!(g, before);
    let out = String::from_utf8_lossy(&s.out);
    assert!(out.contains(",p=2,q=2;") && out.contains(",p=62,"));
    assert!(!out.contains("a=t"));
    let particles = r.effects.particles.clone();
    s.out.clear();
    r.draw(
        &mut s,
        geometry(80, 30, 10, 20),
        &g,
        &[],
        Duration::ZERO,
        true,
    )
    .unwrap();
    assert_eq!(r.effects.particles, particles);
    assert_eq!(r.effects.shatter_count(), 1);
    assert!(String::from_utf8_lossy(&s.out).contains("a=t"));
}

#[test]
fn overlapping_shakes_keep_the_maximum_envelope_through_a_burst() {
    let g = game();
    let mut single = Effects::default();
    single.ingest(&[Event::LifeLost], &g);
    let mut burst = single.clone();
    burst.ingest(&(0..50).map(|i| contact(i, 0)).collect::<Vec<_>>(), &g);
    assert_eq!(single.shake_offset(), burst.shake_offset());
    single.advance(Duration::from_millis(50), &g);
    burst.advance(Duration::from_millis(50), &g);
    assert_eq!(single.shake_offset(), burst.shake_offset());
}
