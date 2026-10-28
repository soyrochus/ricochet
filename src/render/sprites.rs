use crate::constants::*;
pub const NAVY: [u8; 4] = [0x10, 0x18, 0x27, 255];
pub const PALE: [u8; 4] = [0xf3, 0xf6, 0xff, 255];
pub const CYAN: [u8; 4] = [0x65, 0xd6, 0xe8, 255];

pub struct Sprite {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
fn raster(width: f64, height: f64, scale: f64, pixel: impl Fn(f64, f64) -> [u8; 4]) -> Sprite {
    let w = (width * scale).round().max(1.0) as u32;
    let h = (height * scale).round().max(1.0) as u32;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&pixel(
                (f64::from(x) + 0.5) / scale,
                (f64::from(y) + 0.5) / scale,
            ));
        }
    }
    Sprite {
        width: w,
        height: h,
        rgba,
    }
}
pub fn ball(scale: f64) -> Sprite {
    raster(BALL_RADIUS * 2.0, BALL_RADIUS * 2.0, scale, |x, y| {
        let d = (x - BALL_RADIUS).powi(2) + (y - BALL_RADIUS).powi(2);
        if d > BALL_RADIUS.powi(2) {
            [0, 0, 0, 0]
        } else if y > 10.0 {
            [184, 208, 230, 255]
        } else {
            PALE
        }
    })
}
pub fn paddle(scale: f64) -> Sprite {
    raster(PADDLE_WIDTH, PADDLE_HEIGHT, scale, |x, y| {
        if (x < 2.0 || x > PADDLE_WIDTH - 2.0) && !(2.0..PADDLE_HEIGHT - 2.0).contains(&y) {
            [0, 0, 0, 0]
        } else if y < 3.0 {
            [178, 241, 249, 255]
        } else if y > PADDLE_HEIGHT - 3.0 {
            [38, 132, 154, 255]
        } else {
            CYAN
        }
    })
}
pub fn brick(scale: f64, initial: u8, hp: u8) -> Sprite {
    raster(BRICK_WIDTH, BRICK_HEIGHT, scale, |x, y| {
        let color: [u8; 4] = match hp {
            3 => [241, 119, 113, 255],
            2 => [245, 190, 91, 255],
            _ => [66, 185, 167, 255],
        };
        let damage = initial.saturating_sub(hp);
        // Jagged navy cuts plus broken highlights remain visible in greyscale.
        let crack =
            damage > 0 && (x - (27.0 + ((y / 4.0).floor() % 2.0) * 4.0)).abs() < 1.6 && y < 19.0;
        let crack2 =
            damage > 1 && (x - (47.0 - ((y / 3.0).floor() % 3.0) * 3.0)).abs() < 1.6 && y > 5.0;
        if crack || crack2 {
            NAVY
        } else if y < 3.0 && (damage == 0 || x < 20.0 || x > 45.0) {
            [
                color[0].saturating_add(35),
                color[1].saturating_add(35),
                color[2].saturating_add(35),
                255,
            ]
        } else if y > BRICK_HEIGHT - 3.0 {
            [color[0] / 2, color[1] / 2, color[2] / 2, 255]
        } else {
            color
        }
    })
}
pub fn background(scale: f64) -> Sprite {
    raster(WIDTH, HEIGHT, scale, |x, y| {
        if x < 2.0 || x > WIDTH - 2.0 || y < 2.0 {
            [42, 63, 85, 255]
        } else if (x as u32).is_multiple_of(40) && (y as u32).is_multiple_of(40) {
            [27, 39, 57, 255]
        } else {
            NAVY
        }
    })
}
