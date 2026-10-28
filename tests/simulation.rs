use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ricochet::{
    app::Controller,
    constants::*,
    game::{Active, Game, State},
    input::Action,
    levels::{self, Brick},
    physics::{self, Ball, Paddle, Rect, Vec2},
    terminal::Geometry,
    timing::Clock,
};
use std::time::Duration;
fn ball(x: f64, y: f64, vx: f64, vy: f64) -> Ball {
    Ball {
        position: Vec2::new(x, y),
        velocity: Vec2::new(vx, vy),
        speed: vx.hypot(vy),
    }
}
fn brick(index: usize, x: f64, y: f64, hp: u8) -> Brick {
    Brick {
        index,
        rect: Rect {
            x,
            y,
            w: BRICK_WIDTH,
            h: BRICK_HEIGHT,
        },
        hp,
        initial_hp: hp,
    }
}
fn spare() -> Brick {
    brick(49, 700.0, 70.0, 3)
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}
fn key(code: KeyCode, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind))
}

#[test]
fn levels_have_exact_hp_geometry_and_omissions() {
    let first = levels::generate(1);
    assert_eq!(first.len(), 50);
    assert_eq!(first.iter().map(|b| u32::from(b.hp)).sum::<u32>(), 70);
    near(first[0].rect.x, 24.0);
    near(first[9].rect.x + BRICK_WIDTH, 776.0);
    assert_eq!(
        levels::generate(2)
            .iter()
            .map(|b| u32::from(b.hp))
            .sum::<u32>(),
        110
    );
    let third = levels::generate(3);
    assert_eq!(third.len(), 37);
    assert!(
        third
            .iter()
            .all(|b| ((b.index / 10) + (b.index % 10)) % 4 != 0)
    );
}
#[test]
fn wall_faces_and_simultaneous_wall_corner_reflect_once() {
    for (mut b, expected) in [
        (ball(8.0, 400.0, -300.0, 0.0), Vec2::new(300.0, 0.0)),
        (ball(792.0, 400.0, 300.0, 0.0), Vec2::new(-300.0, 0.0)),
        (ball(400.0, 8.0, 0.0, -300.0), Vec2::new(0.0, 300.0)),
        (ball(8.0, 8.0, -200.0, -200.0), Vec2::new(200.0, 200.0)),
    ] {
        let speed = b.speed;
        physics::step(&mut b, &mut Paddle::default(), &mut [spare()], 0.0, 1.0, DT);
        near(b.velocity.x, expected.x);
        near(b.velocity.y, expected.y);
        near(b.velocity.length(), speed);
    }
}
#[test]
fn exact_corner_arc_does_not_hit_empty_expanded_square() {
    let rect = Rect {
        x: 100.0,
        y: 100.0,
        w: 68.0,
        h: 24.0,
    };
    // Pass through the expanded AABB's empty top-left corner, above the arc.
    assert!(
        physics::sweep_circle(
            Vec2::new(92.0, 93.1),
            Vec2::new(10.0, -10.0),
            7.0,
            rect,
            1.0
        )
        .is_none()
    );
    let hit = physics::sweep_circle(
        Vec2::new(80.0, 80.0),
        Vec2::new(100.0, 100.0),
        7.0,
        rect,
        1.0,
    )
    .unwrap();
    near(hit.time, (20.0 - 7.0 / 2.0_f64.sqrt()) / 100.0);
    near(hit.normal.x, -1.0 / 2.0_f64.sqrt());
    near(hit.normal.y, -1.0 / 2.0_f64.sqrt());
}
#[test]
fn max_speed_sweep_cannot_tunnel_and_no_duplicate_contact_damage() {
    let mut b = ball(130.0, 250.0, 0.0, -MAX_SPEED);
    let mut bricks = [brick(0, 100.0, 100.0, 3), spare()];
    let out = physics::step(&mut b, &mut Paddle::default(), &mut bricks, 0.0, 1.0, 0.3);
    assert_eq!(out.score, 10);
    assert_eq!(bricks[0].hp, 2);
    assert!(b.velocity.y > 0.0);
    for _ in 0..10 {
        physics::step(&mut b, &mut Paddle::default(), &mut bricks, 0.0, 1.0, DT);
    }
    assert_eq!(bricks[0].hp, 2);
}
#[test]
fn brick_corner_reflects_geometric_normal_and_preserves_magnitude() {
    let mut b = ball(80.0, 80.0, 200.0, 200.0);
    let speed = b.speed;
    let mut bricks = [brick(0, 100.0, 100.0, 2), spare()];
    let out = physics::step(&mut b, &mut Paddle::default(), &mut bricks, 0.0, 1.0, 0.1);
    assert_eq!(out.score, 10);
    assert!(b.velocity.x < 0.0 && b.velocity.y < 0.0);
    near(b.velocity.length(), speed);
}
#[test]
fn simultaneous_distinct_bricks_are_damaged_once_in_stable_order() {
    let mut b = ball(172.0, 150.0, 0.0, -300.0);
    let mut bricks = [
        brick(0, 100.0, 100.0, 2),
        brick(1, 176.0, 100.0, 2),
        spare(),
    ];
    let out = physics::step(&mut b, &mut Paddle::default(), &mut bricks, 0.0, 1.0, 0.1);
    assert_eq!(out.score, 20);
    assert_eq!(bricks[0].hp, 1);
    assert_eq!(bricks[1].hp, 1);
    near(b.velocity.x, 0.0);
    assert!(b.velocity.y > 0.0);
    near(b.velocity.length(), 300.0);
}
#[test]
fn destruction_score_and_speed_increment_are_capped() {
    for (speed, want) in [(300.0, 304.0), (519.0, 520.0)] {
        let mut b = ball(130.0, 132.0, 0.0, -speed);
        let mut bricks = [brick(0, 100.0, 100.0, 1), spare()];
        let out = physics::step(&mut b, &mut Paddle::default(), &mut bricks, 0.0, 1.0, DT);
        assert_eq!(out.score, 35);
        assert_eq!(bricks[0].hp, 0);
        near(b.speed, want);
        near(b.velocity.length(), want);
    }
}
#[test]
fn paddle_angles_minimum_horizontal_and_incoming_sign() {
    let p = Paddle::default();
    for (offset, sign) in [(-1.0, -1.0), (0.0, 1.0), (1.0, 1.0)] {
        let mut b = ball(p.centre() + offset * 56.0, 549.0, 0.0, 300.0);
        physics::paddle_bounce(&mut b, p, sign);
        assert!(b.velocity.y < 0.0);
        assert!(b.velocity.x.abs() >= 36.0 - 1e-6);
        near(b.velocity.length(), 300.0);
        if offset != 0.0 {
            near(b.velocity.x, offset * 300.0 * 65.0_f64.to_radians().sin());
        }
    }
    let mut b = ball(p.centre(), 549.0, -30.0, 298.496231131986);
    b.speed = 300.0;
    physics::paddle_bounce(&mut b, p, 1.0);
    near(b.velocity.x, -36.0);
}
#[test]
fn moving_paddle_is_evaluated_at_contact_and_cannot_teleport() {
    // A final-position paddle would rescue this ball, but at its actual
    // contact time (.010s) the paddle's right edge is still only 218.
    let mut p = Paddle { x: 100.0 };
    let mut b = ball(260.0, 544.0, 0.0, 500.0);
    physics::step(&mut b, &mut p, &mut [spare()], 1.0, 1.0, 0.1);
    assert!(b.velocity.y > 0.0);
    near(p.x, 160.0);
    // A paddle initially out of range arrives before impact.
    let mut p = Paddle { x: 100.0 };
    let mut b = ball(215.0, 546.0, 0.0, 500.0);
    physics::step(&mut b, &mut p, &mut [spare()], 1.0, 1.0, DT);
    assert!(b.velocity.y < 0.0);
    near(p.x, 105.0);
    // A departing paddle must not catch a ball outside its swept corner.
    let mut p = Paddle { x: 100.0 };
    let mut b = ball(94.0, 548.0, 0.0, 500.0);
    physics::step(&mut b, &mut p, &mut [spare()], 1.0, 1.0, DT);
    assert!(b.velocity.y > 0.0);
}
#[test]
fn paddle_clamps_mid_tick_and_contacts_from_below_do_not_rescue() {
    let mut p = Paddle { x: 687.0 };
    let mut b = ball(744.0, 547.0, 0.0, 300.0);
    physics::step(&mut b, &mut p, &mut [spare()], 1.0, 1.0, DT);
    near(p.x, 688.0);
    assert!(b.velocity.y < 0.0);
    let mut b = ball(400.0, 575.0, 0.0, 300.0);
    physics::step(
        &mut b,
        &mut Paddle::default(),
        &mut [spare()],
        0.0,
        1.0,
        0.12,
    );
    assert!(b.velocity.y > 0.0);
    let mut g = Game::default();
    g.start();
    for _ in 0..400 {
        g.tick(-1.0);
    }
    near(g.paddle.x, 0.0);
    for _ in 0..400 {
        g.tick(1.0);
    }
    near(g.paddle.x, 688.0);
    near(g.ball.position.x, g.paddle.centre());
}
#[test]
fn miss_preserves_damage_score_and_speed_and_launch_alternates() {
    let mut g = Game::default();
    g.start();
    g.action(Action::Space);
    assert!(g.ball.velocity.x > 0.0);
    g.bricks[0].hp = 1;
    g.score = 10;
    g.ball.speed = 420.0;
    g.ball.position = Vec2::new(400.0, 606.0);
    g.ball.velocity = Vec2::new(0.0, 420.0);
    g.tick(0.0);
    assert_eq!(g.lives, 2);
    assert_eq!(g.score, 10);
    assert_eq!(g.bricks[0].hp, 1);
    near(g.ball.speed, 420.0);
    assert_eq!(g.state, State::Ready);
    g.action(Action::Space);
    assert!(g.ball.velocity.x < 0.0);
}
fn clear_level(g: &mut Game) {
    for b in &mut g.bricks {
        b.hp = 0;
    }
    g.bricks[0].hp = 1;
    let rect = g.bricks[0].rect;
    g.state = State::Playing;
    g.ball.position = Vec2::new(rect.x + 20.0, rect.y + rect.h + 7.1);
    g.ball.velocity = Vec2::new(0.0, -g.ball.speed);
    g.tick(0.0);
}
#[test]
fn three_levels_preserve_lives_and_score_and_end_in_won() {
    let mut g = Game::default();
    g.start();
    g.lives = 2;
    for level in 1..=3 {
        clear_level(&mut g);
        assert_eq!(g.score, level * 35);
        assert_eq!(g.lives, 2);
        if level < 3 {
            assert_eq!(g.level, level as usize + 1);
            assert_eq!(g.state, State::Ready);
            near(g.ball.speed, START_SPEEDS[level as usize]);
        }
    }
    assert_eq!(g.state, State::Won);
    g.action(Action::Restart);
    assert_eq!(g.state, State::Ready);
    assert_eq!(g.score, 0);
    assert_eq!(g.lives, 3);
    assert_eq!(g.level, 1);
}
#[test]
fn final_brick_takes_precedence_over_simultaneous_life_loss() {
    let mut g = Game::default();
    g.start();
    g.level = 3;
    g.lives = 1;
    g.state = State::Playing;
    g.bricks = vec![brick(0, 100.0, HEIGHT + 14.0, 1)];
    g.ball = ball(130.0, 606.0, 0.0, 300.0);
    g.tick(0.0);
    assert_eq!(g.state, State::Won);
    assert_eq!(g.lives, 1);
    assert_eq!(g.score, 35);
}
#[test]
fn game_over_ready_pause_and_restart_transitions() {
    let mut g = Game::default();
    g.action(Action::Restart);
    assert_eq!(g.state, State::Title);
    g.action(Action::Space);
    g.action(Action::Pause);
    assert_eq!(g.state, State::Paused(Active::Ready));
    g.action(Action::Pause);
    g.action(Action::Space);
    g.action(Action::Pause);
    assert_eq!(g.state, State::Paused(Active::Playing));
    g.action(Action::Restart);
    assert_eq!(g.state, State::Ready);
    for lives in (0..3).rev() {
        g.state = State::Playing;
        g.ball = ball(10.0, 606.0, 0.0, 300.0);
        g.tick(0.0);
        assert_eq!(g.lives, lives);
    }
    assert_eq!(g.state, State::GameOver);
    g.action(Action::Space);
    assert_eq!(g.state, State::GameOver);
    g.action(Action::Restart);
    assert_eq!(g.state, State::Ready);
    assert_eq!(g.lives, 3);
}
#[test]
fn focus_and_resize_preserve_progress_clear_keys_and_require_resume() {
    let mut c = Controller::default();
    c.game.start();
    c.game.action(Action::Space);
    c.event(key(KeyCode::Left, KeyEventKind::Press));
    assert_eq!(c.input.direction(), -1.0);
    c.event(Event::FocusLost);
    let paused = c.game.clone();
    c.elapsed(Duration::from_secs(5));
    assert_eq!(c.game, paused);
    assert_eq!(c.input.direction(), 0.0);
    c.event(Event::FocusGained);
    assert_eq!(c.game, paused);
    c.event(key(KeyCode::Char('p'), KeyEventKind::Press));
    assert_eq!(c.game.state, State::Playing);
    c.geometry(Geometry {
        columns: 60,
        rows: 20,
        pixel_width: 600,
        pixel_height: 400,
    });
    let small = c.game.clone();
    c.elapsed(Duration::from_secs(5));
    assert_eq!(c.game, small);
    c.geometry(Geometry {
        columns: 100,
        rows: 40,
        pixel_width: 1000,
        pixel_height: 800,
    });
    assert!(c.game.resume_required);
    let p = c.game.ball.position;
    c.elapsed(Duration::from_secs(5));
    assert_eq!(c.game.ball.position, p);
    c.event(key(KeyCode::Char('p'), KeyEventKind::Press));
    c.elapsed(Duration::from_secs_f64(2.0 * DT));
    assert_ne!(c.game.ball.position, p);
}
#[test]
fn stalls_are_bounded_and_paused_time_is_discarded() {
    let mut c = Clock::default();
    assert_eq!(c.advance(Duration::from_secs(4), true), 12);
    assert_eq!(c.advance(Duration::ZERO, true), 0);
    assert_eq!(c.advance(Duration::from_secs(4), false), 0);
    assert_eq!(c.advance(Duration::ZERO, true), 0);
}
fn history(fps: u32) -> Game {
    let mut c = Controller::default();
    c.game.start();
    c.game.action(Action::Space);
    let mut previous = Duration::ZERO;
    for frame in 0..fps * 3 {
        if frame == 0 {
            c.event(key(KeyCode::Right, KeyEventKind::Press));
        }
        if frame == fps / 2 {
            c.event(key(KeyCode::Right, KeyEventKind::Release));
            c.event(key(KeyCode::Left, KeyEventKind::Press));
        }
        if frame == fps {
            c.event(key(KeyCode::Left, KeyEventKind::Release));
        }
        let now = Duration::from_secs_f64(f64::from(frame + 1) / f64::from(fps));
        c.elapsed(now - previous);
        previous = now;
    }
    c.game
}
#[test]
fn rendering_at_30_and_60_has_identical_simulation() {
    assert_eq!(history(30), history(60));
}
#[test]
fn long_deterministic_run_is_finite_bounded_and_never_sticks() {
    fn run() -> (Game, usize) {
        let mut g = Game::default();
        g.start();
        let mut moving = 0;
        let mut stationary = 0;
        for _ in 0..72_000 {
            if matches!(g.state, State::Won | State::GameOver) {
                g.action(Action::Restart);
            }
            if g.state == State::Ready {
                g.action(Action::Space);
            }
            let previous = g.ball.position;
            let direction = (g.ball.position.x - g.paddle.centre()).signum();
            g.tick(direction);
            assert!(g.ball.position.x.is_finite() && g.ball.position.y.is_finite());
            assert!(g.ball.velocity.x.is_finite() && g.ball.velocity.y.is_finite());
            assert!((0.0..=688.0).contains(&g.paddle.x));
            if g.state == State::Playing {
                near(g.ball.velocity.length(), g.ball.speed);
                if previous == g.ball.position {
                    stationary += 1;
                } else {
                    moving += 1;
                    stationary = 0;
                }
                assert!(stationary < 3);
            }
        }
        (g, moving)
    }
    let a = run();
    let b = run();
    assert_eq!(a, b);
    assert!(a.1 > 60_000);
}
