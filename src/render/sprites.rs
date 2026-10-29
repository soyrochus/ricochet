//! Procedural straight-RGBA art and padded, immutable frame atlases.
use super::{font, kitty::SourceRect};
use crate::{constants::*, physics::Vec2};
pub const NAVY: [u8; 4] = [0x10, 0x18, 0x27, 255];
pub const PALE: [u8; 4] = [0xf3, 0xf6, 0xff, 255];
pub const CYAN: [u8; 4] = [0x65, 0xd6, 0xe8, 255];
pub const COLOURS: [[u8; 4]; 3] = [
    [66, 185, 167, 255],
    [245, 190, 91, 255],
    [241, 119, 113, 255],
];
pub const BRICK_PAD: f64 = 2.0;
pub const SHADOW_EXTENT: f64 = 6.0;
pub const BALL_EXTENT: f64 = 14.0;
pub const VARIANTS: [(u8, u8); 6] = [(1, 1), (2, 2), (2, 1), (3, 3), (3, 2), (3, 1)];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Banner {
    Ready,
    Level2,
    Level3,
    Paused,
    GameOver,
    Won,
}
impl Banner {
    pub fn text(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Level2 => "LEVEL 2",
            Self::Level3 => "LEVEL 3",
            Self::Paused => "PAUSED",
            Self::GameOver => "GAME OVER",
            Self::Won => "YOU WON",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Frame {
    Background,
    Ball,
    Trail(u8),
    Paddle(u8),
    Brick { initial: u8, hp: u8, flash: bool },
    Shatter { colour: u8, frame: u8 },
    Particle { colour: u8, size: u8, faded: bool },
    Logo(u8),
    Banner { kind: Banner, alpha: u8 },
    Overlay,
}
#[derive(Clone, Debug)]
pub struct Sprite {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Sprite {
    pub fn alpha(&self, alpha: f64) -> Self {
        let mut result = self.clone();
        for p in result.rgba.as_chunks_mut::<4>().0.iter_mut() {
            p[3] = (f64::from(p[3]) * alpha).round() as u8;
        }
        result
    }
}
/// `region` labels membership, including interior cuts and highlights. Smooth
/// colour/alpha fields need just one sample; boundaries use 4x4 coverage.
#[derive(Clone, Copy)]
pub(crate) struct Sample {
    pub colour: [f64; 4],
    pub region: u8,
}
pub(crate) fn sample(colour: [u8; 4], region: u8) -> Sample {
    Sample {
        colour: colour.map(f64::from),
        region,
    }
}
pub(crate) fn over(top: [f64; 4], bottom: [f64; 4]) -> [f64; 4] {
    let a = top[3] / 255.0;
    let b = bottom[3] / 255.0 * (1.0 - a);
    let total = a + b;
    if total == 0.0 {
        return [0.0; 4];
    }
    [
        (top[0] * a + bottom[0] * b) / total,
        (top[1] * a + bottom[1] * b) / total,
        (top[2] * a + bottom[2] * b) / total,
        total * 255.0,
    ]
}
pub(crate) fn raster(
    width: f64,
    height: f64,
    scale: f64,
    pixel: impl Fn(f64, f64) -> Sample,
) -> Sprite {
    let w = (width * scale).round().max(1.0) as u32;
    let h = (height * scale).round().max(1.0) as u32;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let x = f64::from(x) / scale;
            let y = f64::from(y) / scale;
            let centre = pixel(x + 0.5 / scale, y + 0.5 / scale);
            let edges = [
                pixel(x, y),
                pixel(x + 1.0 / scale, y),
                pixel(x, y + 1.0 / scale),
                pixel(x + 1.0 / scale, y + 1.0 / scale),
            ];
            let c = if edges.iter().all(|p| p.region == centre.region) {
                centre.colour
            } else {
                let mut sum = [0.0; 4];
                for sy in 0..4 {
                    for sx in 0..4 {
                        let p = pixel(
                            x + (f64::from(sx) + 0.5) / (4.0 * scale),
                            y + (f64::from(sy) + 0.5) / (4.0 * scale),
                        )
                        .colour;
                        for k in 0..3 {
                            sum[k] += p[k] * p[3] / 255.0;
                        }
                        sum[3] += p[3];
                    }
                }
                if sum[3] > 0.0 {
                    [
                        sum[0] * 255.0 / sum[3],
                        sum[1] * 255.0 / sum[3],
                        sum[2] * 255.0 / sum[3],
                        sum[3] / 16.0,
                    ]
                } else {
                    [0.0; 4]
                }
            };
            rgba.extend(c.map(|c| c.round().clamp(0.0, 255.0) as u8));
        }
    }
    Sprite {
        width: w,
        height: h,
        rgba,
    }
}
pub(crate) fn rounded(x: f64, y: f64, w: f64, h: f64, r: f64) -> f64 {
    let dx = (x - w / 2.0).abs() - (w / 2.0 - r);
    let dy = (y - h / 2.0).abs() - (h / 2.0 - r);
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r
}
fn shade(base: [u8; 4], factor: f64) -> [f64; 4] {
    [
        f64::from(base[0]) * factor,
        f64::from(base[1]) * factor,
        f64::from(base[2]) * factor,
        255.0,
    ]
}
fn line_distance(x: f64, y: f64, points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|p| {
            let (dx, dy) = (p[1].0 - p[0].0, p[1].1 - p[0].1);
            let t = ((x - p[0].0) * dx + (y - p[0].1) * dy) / (dx * dx + dy * dy);
            (x - p[0].0 - dx * t.clamp(0.0, 1.0)).hypot(y - p[0].1 - dy * t.clamp(0.0, 1.0))
        })
        .fold(f64::INFINITY, f64::min)
}
const CRACK1: [(f64, f64); 6] = [
    (27.0, 0.0),
    (30.0, 4.0),
    (26.0, 8.0),
    (31.0, 12.0),
    (28.0, 16.0),
    (30.0, 19.0),
];
const CRACK2: [(f64, f64); 5] = [
    (47.0, 6.0),
    (44.0, 10.0),
    (48.0, 15.0),
    (43.0, 20.0),
    (46.0, 24.0),
];
fn brick_pixel(x: f64, y: f64, initial: u8, hp: u8, flash: bool) -> Sample {
    let distance = rounded(x, y, BRICK_WIDTH, BRICK_HEIGHT, 1.5);
    let shadow = rounded(x - 3.0, y - 4.0, BRICK_WIDTH, BRICK_HEIGHT, 1.5);
    let shadow_alpha = 0.35 * (1.0 - shadow.max(0.0) / 2.0).clamp(0.0, 1.0).powi(2);
    if distance > 0.0 {
        return Sample {
            colour: [0.0, 0.0, 0.0, shadow_alpha * 255.0],
            region: 0,
        };
    }
    let damage = initial.saturating_sub(hp);
    let crack = (damage > 0 && line_distance(x, y, &CRACK1) < 0.9)
        || (damage > 1 && line_distance(x, y, &CRACK2) < 0.9);
    if crack {
        return sample(NAVY, 2);
    }
    let crack_edge = (damage > 0 && line_distance(x - 1.0, y, &CRACK1) < 0.9)
        || (damage > 1 && line_distance(x - 1.0, y, &CRACK2) < 0.9);
    let seed = ((x / 2.0).floor() as u32 * 1973)
        ^ ((y / 2.0).floor() as u32 * 9277)
        ^ (u32::from(hp) * 26699);
    let noise = f64::from(seed.wrapping_mul(0x45d9f3b).rotate_left(13) % 101) / 100.0 * 0.08 - 0.04;
    let mut factor = 1.12 - 0.24 * y / BRICK_HEIGHT + noise;
    let missing =
        damage > 0 && (23.0..35.0).contains(&x) || damage > 1 && (41.0..51.0).contains(&x);
    let region = if crack_edge {
        factor += 0.25;
        3
    } else if (y < 2.0 || x < 2.0) && !missing {
        factor += 0.18;
        4
    } else if y > BRICK_HEIGHT - 2.0 || x > BRICK_WIDTH - 2.0 {
        factor *= 0.68;
        5
    } else {
        1
    };
    if initial == 3
        && rounded(x - 4.0, y - 4.0, BRICK_WIDTH - 8.0, BRICK_HEIGHT - 8.0, 0.5).abs() < 0.5
    {
        factor *= 0.72;
    }
    let mut colour = shade(COLOURS[usize::from(hp.clamp(1, 3) - 1)], factor);
    if flash {
        for c in &mut colour[..3] {
            *c = *c * 0.4 + 255.0 * 0.6;
        }
    }
    Sample { colour, region }
}
pub fn brick(scale: f64, initial: u8, hp: u8) -> Sprite {
    brick_frame(scale, initial, hp, false)
}
fn brick_frame(scale: f64, initial: u8, hp: u8, flash: bool) -> Sprite {
    raster(BRICK_WIDTH + 8.0, BRICK_HEIGHT + 8.0, scale, |x, y| {
        brick_pixel(x - BRICK_PAD, y - BRICK_PAD, initial, hp, flash)
    })
}
pub fn ball(scale: f64) -> Sprite {
    raster(28.0, 28.0, scale, |x, y| {
        let (dx, dy) = (x - 14.0, y - 14.0);
        let d = dx.hypot(dy);
        let halo = [
            101.0,
            214.0,
            232.0,
            255.0
                * 0.25
                * (1.0 - (d - BALL_RADIUS).max(0.0) / (14.0 - BALL_RADIUS))
                    .max(0.0)
                    .powi(2),
        ];
        if d > BALL_RADIUS {
            return Sample {
                colour: halo,
                region: 0,
            };
        }
        let t = (d / BALL_RADIUS).powi(2);
        let mut c = [
            243.0 + (184.0 - 243.0) * t,
            246.0 + (208.0 - 246.0) * t,
            255.0 + (230.0 - 255.0) * t,
            255.0,
        ];
        let dot = (dx + 2.0).hypot(dy + 2.5) < 1.5;
        if dot {
            c = [255.0; 4];
        }
        Sample {
            colour: c,
            region: if dot { 2 } else { 1 },
        }
    })
}
pub fn paddle(scale: f64) -> Sprite {
    paddle_frame(scale, 0)
}
fn paddle_frame(scale: f64, frame: u8) -> Sprite {
    let (dw, dh) = match frame {
        1 => (4.0, -3.0),
        2 => (2.0, -1.0),
        _ => (0.0, 0.0),
    };
    let (w, h) = (PADDLE_WIDTH + dw, PADDLE_HEIGHT + dh);
    // Common canvas keeps bottom and horizontal centre fixed in every frame.
    raster(PADDLE_WIDTH + 8.0, PADDLE_HEIGHT + 10.0, scale, |x, y| {
        let (x, y) = (x - 4.0 + dw / 2.0, y - 2.0 + dh);
        let d = rounded(x, y, w, h, 3.0);
        let s = rounded(x, y - 4.0, w, h, 3.0);
        if d > 0.0 {
            return Sample {
                colour: [
                    0.0,
                    0.0,
                    0.0,
                    0.3 * 255.0 * (1.0 - s.max(0.0) / 2.0).clamp(0.0, 1.0).powi(2),
                ],
                region: 0,
            };
        }
        if y < 2.0 {
            sample([178, 241, 249, 255], 2)
        } else if y > h - 2.0 {
            sample([38, 132, 154, 255], 3)
        } else {
            Sample {
                colour: shade(CYAN, 1.12 - 0.24 * y / h),
                region: 1,
            }
        }
    })
}
pub fn background(scale: f64) -> Sprite {
    let w = (WIDTH * scale).round() as u32;
    let h = (HEIGHT * scale).round() as u32;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    // Smooth interior: analytic pixel coverage along the three border edges.
    for py in 0..h {
        let y = (f64::from(py) + 0.5) / scale;
        let t = y / HEIGHT;
        let row = [20.0 - 8.0 * t, 30.0 - 12.0 * t, 49.0 - 17.0 * t, 255.0];
        for px in 0..w {
            let x = (f64::from(px) + 0.5) / scale;
            let distance = x.min(WIDTH - x).min(y);
            let coverage = ((2.0 - distance) * scale + 0.5).clamp(0.0, 1.0);
            let glow = (1.0 - (distance - 2.0).max(0.0) / 4.0).max(0.0) * 0.20;
            let grid = if x % 40.0 < 1.0 || y % 40.0 < 1.0 {
                0.06
            } else {
                0.0
            };
            let c = over(
                [42.0, 63.0, 85.0, coverage * 255.0],
                over(
                    [42.0, 63.0, 85.0, glow * 255.0],
                    over([255.0, 255.0, 255.0, grid * 255.0], row),
                ),
            );
            rgba.extend(c.map(|v| v.round() as u8));
        }
    }
    Sprite {
        width: w,
        height: h,
        rgba,
    }
}
pub fn overlay(scale: f64) -> Sprite {
    let w = (WIDTH * scale).round() as u32;
    let h = (HEIGHT * scale).round() as u32;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let r = ((f64::from(x) / f64::from(w) * 2.0 - 1.0)
                .hypot(f64::from(y) / f64::from(h) * 2.0 - 1.0))
                / 2.0_f64.sqrt();
            let vignette = ((r - 0.6) / 0.4).clamp(0.0, 1.0) * 0.35;
            let scan = if y % 3 == 0 { 0.10 } else { 0.0 };
            rgba.extend([
                0,
                0,
                0,
                ((1.0 - (1.0 - vignette) * (1.0 - scan)) * 255.0).round() as u8,
            ]);
        }
    }
    Sprite {
        width: w,
        height: h,
        rgba,
    }
}
fn crack_x(points: &[(f64, f64)], y: f64) -> f64 {
    for pair in points.windows(2) {
        if y <= pair[1].1 {
            let t = ((y - pair[0].1) / (pair[1].1 - pair[0].1)).clamp(0.0, 1.0);
            return pair[0].0 + (pair[1].0 - pair[0].0) * t;
        }
    }
    points.last().unwrap().0
}
fn shatter(scale: f64, colour: u8, frame: u8) -> Sprite {
    let t = f64::from(frame) / 4.0;
    raster(BRICK_WIDTH + 16.0, BRICK_HEIGHT + 16.0, scale, |x, y| {
        // Six polygons share jagged seams derived from the damage crack path.
        for row in 0..2 {
            for col in 0..3 {
                let drift = Vec2::new(f64::from(col) - 1.0, f64::from(row) * 2.0 - 1.0)
                    .normalized()
                    * (6.0 * t);
                let (dx, dy) = (drift.x, drift.y);
                let (bx, by) = (x - 8.0 - dx, y - 8.0 - dy);
                let split1 = crack_x(&CRACK1, by);
                let split2 = crack_x(&CRACK2, by);
                let c = if bx < split1 {
                    0
                } else if bx < split2 {
                    1
                } else {
                    2
                };
                if c == col
                    && (by >= BRICK_HEIGHT / 2.0) == (row == 1)
                    && (0.0..BRICK_WIDTH).contains(&bx)
                    && (0.0..BRICK_HEIGHT).contains(&by)
                {
                    let mut p = brick_pixel(bx, by, colour, 1, false);
                    // Keep the original colour through destruction.
                    p.colour = shade(
                        COLOURS[usize::from(colour - 1)],
                        1.12 - 0.24 * by / BRICK_HEIGHT,
                    );
                    p.colour[3] *= match frame {
                        3 => 0.65,
                        4 => 0.25,
                        _ => 1.0,
                    };
                    p.region = 1 + row * 3 + col;
                    return p;
                }
            }
        }
        sample([0; 4], 0)
    })
}
#[derive(Clone, Debug)]
pub struct AtlasFrame {
    pub frame: Frame,
    pub source: SourceRect,
    pub origin: Vec2,
}
pub struct Atlas {
    pub sprite: Sprite,
    pub frames: Vec<AtlasFrame>,
}
/// Fixed grid, with a full transparent physical pixel on all four sides.
fn atlas(items: Vec<(Frame, Sprite, Vec2)>, columns: u32) -> Atlas {
    let cell_w = items.iter().map(|i| i.1.width).max().unwrap() + 2;
    let cell_h = items.iter().map(|i| i.1.height).max().unwrap() + 2;
    let width = cell_w * columns;
    let height = cell_h * (items.len() as u32).div_ceil(columns);
    let mut sprite = Sprite {
        width,
        height,
        rgba: vec![0; width as usize * height as usize * 4],
    };
    let mut frames = Vec::new();
    for (i, (frame, image, origin)) in items.into_iter().enumerate() {
        let x = i as u32 % columns * cell_w + 1;
        let y = i as u32 / columns * cell_h + 1;
        for row in 0..image.height {
            let dst = ((y + row) * width + x) as usize * 4;
            let src = (row * image.width) as usize * 4;
            sprite.rgba[dst..dst + image.width as usize * 4]
                .copy_from_slice(&image.rgba[src..src + image.width as usize * 4]);
        }
        frames.push(AtlasFrame {
            frame,
            source: SourceRect {
                x,
                y,
                w: image.width,
                h: image.height,
            },
            origin,
        });
    }
    Atlas { sprite, frames }
}
fn single(frame: Frame, sprite: Sprite) -> Atlas {
    let source = SourceRect {
        x: 0,
        y: 0,
        w: sprite.width,
        h: sprite.height,
    };
    Atlas {
        sprite,
        frames: vec![AtlasFrame {
            frame,
            source,
            origin: Vec2::default(),
        }],
    }
}
// Stack related row grids without wasting a large frame's slot on small art.
// Both grids retain their transparent gutters.
fn stack(top: Atlas, bottom: Atlas) -> Atlas {
    let width = top.sprite.width.max(bottom.sprite.width);
    let top_height = top.sprite.height;
    let height = top_height + bottom.sprite.height;
    let mut sprite = Sprite {
        width,
        height,
        rgba: vec![0; width as usize * height as usize * 4],
    };
    let mut frames = Vec::new();
    for (atlas, offset) in [(top, 0), (bottom, top_height)] {
        for row in 0..atlas.sprite.height {
            let dst = ((row + offset) * width) as usize * 4;
            let src = (row * atlas.sprite.width) as usize * 4;
            sprite.rgba[dst..dst + atlas.sprite.width as usize * 4]
                .copy_from_slice(&atlas.sprite.rgba[src..src + atlas.sprite.width as usize * 4]);
        }
        frames.extend(atlas.frames.into_iter().map(|mut f| {
            f.source.y += offset;
            f
        }));
    }
    Atlas { sprite, frames }
}
pub fn generate(scale: f64, full: bool) -> Vec<Atlas> {
    let mut result = vec![single(Frame::Background, background(scale))];
    let b = ball(scale);
    let mut items = vec![(Frame::Ball, b.clone(), Vec2::new(-14.0, -14.0))];
    for (i, a) in [0.40, 0.25, 0.14, 0.06].into_iter().enumerate() {
        items.push((Frame::Trail(i as u8), b.alpha(a), Vec2::new(-14.0, -14.0)));
    }
    result.push(atlas(items, 5));
    result.push(atlas(
        (0..3)
            .map(|i| {
                (
                    Frame::Paddle(i),
                    paddle_frame(scale, i),
                    Vec2::new(-4.0, -2.0),
                )
            })
            .collect(),
        3,
    ));
    let mut items = Vec::new();
    for (initial, hp) in VARIANTS {
        for flash in [false, true] {
            items.push((
                Frame::Brick { initial, hp, flash },
                brick_frame(scale, initial, hp, flash),
                Vec2::new(-BRICK_PAD, -BRICK_PAD),
            ));
        }
    }
    let brick_atlas = atlas(items, 6);
    let mut items = Vec::new();
    for colour in 1..=3 {
        for frame in 0..5 {
            items.push((
                Frame::Shatter { colour, frame },
                shatter(scale, colour, frame),
                Vec2::new(-8.0, -8.0),
            ));
        }
    }
    // Exactly one horizontal, five-frame shatter strip per colour.
    result.push(stack(brick_atlas, atlas(items, 5)));
    let mut items = Vec::new();
    for colour in 1..=3 {
        for size in 0..4 {
            for faded in [false, true] {
                let s = f64::from(size) + 3.0;
                let image = raster(s + 2.0, s + 2.0, scale, |x, y| {
                    if rounded(x - 1.0, y - 1.0, s, s, 0.6) <= 0.0 {
                        let mut p = sample(COLOURS[usize::from(colour - 1)], 1);
                        p.colour[3] = if faded { 90.0 } else { 230.0 };
                        p
                    } else {
                        sample([0; 4], 0)
                    }
                });
                items.push((
                    Frame::Particle {
                        colour,
                        size,
                        faded,
                    },
                    image,
                    Vec2::new(-(s + 2.0) / 2.0, -(s + 2.0) / 2.0),
                ));
            }
        }
    }
    result.push(atlas(items, 8));
    let logo = font::text("RICOCHET", 10.0, scale);
    let mut items = Vec::new();
    for frame in 0..8 {
        let mut image = logo.clone();
        if frame > 0 {
            for (i, p) in image.rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = (i % image.width as usize) as f64 / scale;
                let sweep =
                    (x - (f64::from(frame) - 0.5) * logo.width as f64 / (7.0 * scale)).abs();
                if sweep < 32.0 && p[3] > 0 {
                    for c in &mut p[..3] {
                        *c = (f64::from(*c) + (255.0 - f64::from(*c)) * (1.0 - sweep / 32.0) * 0.65)
                            as u8;
                    }
                }
            }
        }
        items.push((Frame::Logo(frame), image, Vec2::new(-4.0, -4.0)));
    }
    let logo_atlas = atlas(items, 2);
    let mut items = Vec::new();
    for kind in [
        Banner::Ready,
        Banner::Level2,
        Banner::Level3,
        Banner::Paused,
        Banner::GameOver,
        Banner::Won,
    ] {
        let image = font::text(kind.text(), 6.0, scale);
        for alpha in 0..4 {
            items.push((
                Frame::Banner { kind, alpha },
                image.alpha(f64::from(alpha + 1) / 4.0),
                Vec2::new(-4.0, -4.0),
            ));
        }
    }
    result.push(stack(logo_atlas, atlas(items, 3)));
    if full {
        result.push(single(Frame::Overlay, overlay(scale)));
    }
    result
}
