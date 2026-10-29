use base64::{Engine, engine::general_purpose::STANDARD};
use ricochet::{
    constants::*,
    game::Game,
    render::{
        Renderer, Viewport,
        kitty::{self, MAX_IMAGES, MAX_PLACEMENTS, PixelPlacement, Registry},
        sprites,
    },
    terminal::{Geometry, Session},
};
use std::{
    io::{self, Write},
    time::Duration,
};
fn geometry(columns: u16, rows: u16, cw: u32, ch: u32) -> Geometry {
    Geometry {
        columns,
        rows,
        pixel_width: u32::from(columns) * cw,
        pixel_height: u32::from(rows) * ch,
    }
}
#[test]
fn pixels_map_through_cells_without_snapping_and_stay_inside_hud() {
    for g in [
        geometry(80, 30, 8, 16),
        geometry(120, 40, 10, 20),
        geometry(160, 50, 14, 28),
        geometry(80, 30, 5, 12),
    ] {
        let v = Viewport::new(g).unwrap();
        assert!(v.top >= v.cell_height * 2);
        assert!(v.top + v.height <= u32::from(g.rows - 2) * v.cell_height);
        for (x, y) in [(0.0, 0.0), (24.25, 70.5), (400.5, 300.25), (793.0, 593.0)] {
            let at = v.at(x, y);
            let (px, py) = v.pixels(x, y);
            assert_eq!(at.column * v.cell_width + at.x_offset, px);
            assert_eq!(at.row * v.cell_height + at.y_offset, py);
            assert!(at.x_offset < v.cell_width && at.y_offset < v.cell_height);
            assert!((f64::from(px - v.left) - x * v.scale).abs() <= 0.5);
        }
        let a = v.at(400.0, 300.0);
        let b = v.at(400.0 + 1.0 / v.scale, 300.0);
        assert_eq!(a.column, b.column);
        assert_eq!(b.x_offset, a.x_offset + 1);
        assert!(v.scale <= 2.0);
    }
    assert!(Viewport::new(geometry(79, 40, 10, 20)).is_none());
    assert!(Viewport::new(geometry(120, 29, 10, 20)).is_none());
    assert!(Viewport::new(geometry(80, 30, 4, 10)).is_none());
    assert!(
        Viewport::new(Geometry {
            columns: 80,
            rows: 30,
            pixel_width: 0,
            pixel_height: 0
        })
        .is_none()
    );
}
#[test]
fn payload_chunk_boundaries_roundtrip_exact_rgba() {
    for width in [1, 768, 769, 1536, 1537, 2048] {
        let rgba = vec![171; width * 4];
        let mut reg = Registry::default();
        let mut out = Vec::new();
        reg.upload(&mut out, 17, width as u32, 1, &rgba).unwrap();
        let text = String::from_utf8(out).unwrap();
        let chunks: Vec<_> = text.split("\x1b_G").skip(1).collect();
        let mut combined = String::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let (header, payload) = chunk.split_once(';').unwrap();
            let payload = payload.strip_suffix("\x1b\\").unwrap();
            assert!(payload.len() <= 4096);
            assert!(header.contains(if i + 1 == chunks.len() { "m=0" } else { "m=1" }));
            if i > 0 {
                assert!(!header.contains("s=") && !header.contains("I="));
            }
            if i + 1 < chunks.len() {
                assert_eq!(payload.len() % 4, 0);
            }
            combined.push_str(payload);
        }
        assert_eq!(STANDARD.decode(combined).unwrap(), rgba);
    }
}
#[test]
fn image_numbers_and_placements_are_bounded_and_cleanup_is_owned() {
    let mut reg = Registry::default();
    let mut out = Vec::new();
    for id in 1..=MAX_IMAGES as u32 {
        reg.upload(&mut out, id, 1, 1, &[0; 4]).unwrap();
    }
    assert!(reg.upload(&mut out, 1, 1, 1, &[0; 4]).is_err());
    assert!(reg.upload(&mut out, 100, 1, 1, &[0; 4]).is_err());
    let at = PixelPlacement {
        column: 2,
        row: 3,
        x_offset: 7,
        y_offset: 15,
    };
    for p in 1..=MAX_PLACEMENTS as u32 {
        reg.place(&mut out, 1, p, at, 3).unwrap();
    }
    for _ in 0..500 {
        reg.place(&mut out, 1, 1, at, 3).unwrap();
    }
    assert!(
        reg.place(&mut out, 1, MAX_PLACEMENTS as u32 + 1, at, 3)
            .is_err()
    );
    assert_eq!(reg.placement_count(), MAX_PLACEMENTS);
    out.clear();
    reg.cleanup(&mut out).unwrap();
    assert_eq!(reg.image_count(), 0);
    assert_eq!(reg.placement_count(), 0);
    let text = String::from_utf8_lossy(&out);
    assert_eq!(text.matches("d=N").count(), MAX_IMAGES);
    assert!(!text.contains("d=A"));
    out.clear();
    reg.cleanup(&mut out).unwrap();
    assert!(out.is_empty());
}
#[test]
fn frames_cache_static_scene_and_only_update_changed_bricks_and_motion() {
    let g = geometry(80, 30, 8, 16);
    let mut game = Game::default();
    game.start();
    let mut session = Session::new(Vec::new());
    let mut renderer = Renderer::default();
    renderer
        .draw(&mut session, g, &game, &[], Duration::ZERO, true)
        .unwrap();
    assert_eq!(session.graphics.image_count(), 7);
    assert_eq!(session.graphics.placement_count(), 55);
    session.out.clear();
    renderer
        .draw(&mut session, g, &game, &[], Duration::ZERO, true)
        .unwrap();
    assert!(session.out.is_empty());
    game.tick(1.0);
    renderer
        .draw(&mut session, g, &game, &[], Duration::ZERO, true)
        .unwrap();
    let output = String::from_utf8_lossy(&session.out);
    assert!(!output.contains("a=t"));
    assert!(!output.contains("2K"));
    assert_eq!(output.matches("a=p").count(), 2);
    session.out.clear();
    game.bricks[0].hp -= 1;
    renderer
        .draw(&mut session, g, &game, &[], Duration::ZERO, false)
        .unwrap();
    let output = String::from_utf8_lossy(&session.out);
    assert_eq!(output.matches("a=d,d=n").count(), 0);
    assert_eq!(output.matches("a=p").count(), 1);
    assert!(!output.contains("a=t"));
    session.out.clear();
    game.bricks[0].hp = 0;
    renderer
        .draw(&mut session, g, &game, &[], Duration::ZERO, false)
        .unwrap();
    assert_eq!(session.graphics.placement_count(), 54);
    for i in 0..40 {
        game.start();
        game.tick(if i % 2 == 0 { 1.0 } else { -1.0 });
        session.out.clear();
        renderer
            .draw(&mut session, g, &game, &[], Duration::ZERO, false)
            .unwrap();
        assert_eq!(session.graphics.image_count(), 7);
        assert_eq!(session.graphics.placement_count(), 55);
        assert!(!String::from_utf8_lossy(&session.out).contains("a=t"));
    }
    // Font-size changes with the same character grid invalidate scale/pixels.
    session.out.clear();
    renderer
        .draw(
            &mut session,
            geometry(80, 30, 10, 20),
            &game,
            &[],
            Duration::ZERO,
            false,
        )
        .unwrap();
    assert!(String::from_utf8_lossy(&session.out).contains("a=t"));
    assert_eq!(session.graphics.image_count(), 7);
    assert_eq!(session.graphics.placement_count(), 55);
    session.out.clear();
    renderer
        .draw(
            &mut session,
            geometry(60, 20, 10, 20),
            &game,
            &[],
            Duration::ZERO,
            false,
        )
        .unwrap();
    assert_eq!(session.graphics.image_count(), 0);
    assert_eq!(session.graphics.placement_count(), 0);
    assert!(String::from_utf8_lossy(&session.out).contains("RESIZE"));
}
#[test]
fn ball_is_cropped_at_open_bottom_without_overlapping_footer() {
    let mut game = Game::default();
    game.ball.position.y = HEIGHT + 2.0;
    let mut session = Session::new(Vec::new());
    let mut renderer = Renderer::default();
    let g = geometry(80, 30, 10, 20);
    renderer
        .draw(&mut session, g, &game, &[], Duration::ZERO, false)
        .unwrap();
    let v = renderer.view.unwrap();
    let (_, py) = v.pixels(game.ball.position.x - 14.0, game.ball.position.y - 14.0);
    let expected = (v.top + v.height).saturating_sub(py);
    assert!(String::from_utf8_lossy(&session.out).contains(&format!(",h={expected};")));
    game.ball.position.y = HEIGHT + 15.0;
    session.out.clear();
    renderer
        .draw(&mut session, g, &game, &[], Duration::ZERO, false)
        .unwrap();
    assert!(String::from_utf8_lossy(&session.out).contains("a=d,d=n"));
}
#[test]
fn sprite_damage_is_visible_in_geometry_and_ball_is_round() {
    let intact = sprites::brick(1.0, 3, 3);
    let damaged = sprites::brick(1.0, 3, 2);
    let broken = sprites::brick(1.0, 3, 1);
    let count = |s: &sprites::Sprite| {
        s.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| **p == sprites::NAVY)
            .count()
    };
    assert_eq!(count(&intact), 0);
    assert!(count(&damaged) > 0);
    assert!(count(&broken) > count(&damaged));
    let ball = sprites::ball(1.0);
    assert_eq!(ball.rgba[3], 0);
    let centre = (14 * 28 + 14) * 4;
    assert_eq!(ball.rgba[centre + 3], 255);
}
struct FailOnce {
    bytes: Vec<u8>,
    call: usize,
    fail_at: usize,
    flushes: usize,
}
impl Write for FailOnce {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.call += 1;
        if self.call == self.fail_at {
            return Err(io::Error::other("injected write failure"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}
#[test]
fn synchronized_frame_ends_after_body_write_error_and_flushes_once_normally() {
    let mut writer = FailOnce {
        bytes: Vec::new(),
        call: 0,
        fail_at: 2,
        flushes: 0,
    };
    assert!(kitty::frame(&mut writer, b"body", true).is_err());
    assert!(writer.bytes.ends_with(b"\x1b[?2026l"));
    let mut writer = FailOnce {
        bytes: Vec::new(),
        call: 0,
        fail_at: 99,
        flushes: 0,
    };
    kitty::frame(&mut writer, b"body", true).unwrap();
    assert_eq!(writer.flushes, 1);
}
