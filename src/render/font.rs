//! Embedded 5x7 alphabet; no font files or terminal glyph dependencies.
use super::sprites::{Sample, Sprite, raster, rounded};

pub fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '!' => [4, 4, 4, 4, 4, 0, 4],
        ' ' => [0; 7],
        _ => [0; 7],
    }
}
pub fn width(text: &str, unit: f64) -> f64 {
    text.len() as f64 * 6.0 * unit
}
pub fn text(text: &str, unit: f64, scale: f64) -> Sprite {
    // Nine small coverage tiles replace repeated SDF work across a whole word.
    // Composite all shadows, then outlines, then faces so neighbouring glyph
    // pixels cannot cast shadows over one another's fill.
    let mut result = Sprite {
        width: ((width(text, unit) + 8.0) * scale).round() as u32,
        height: ((7.0 * unit + 8.0) * scale).round() as u32,
        rgba: Vec::new(),
    };
    result
        .rgba
        .resize(result.width as usize * result.height as usize * 4, 0);
    let shadow = raster(unit + 8.0, unit + 8.0, scale, |x, y| {
        let d = rounded(x - 4.0, y - 5.5, unit * 0.94, unit * 0.94, unit * 0.16);
        Sample {
            colour: [
                0.0,
                0.0,
                0.0,
                (1.0 - d.max(0.0) / 2.0).clamp(0.0, 1.0) * 100.0,
            ],
            region: 0,
        }
    });
    let outline = raster(unit + 8.0, unit + 8.0, scale, |x, y| {
        let inside = rounded(x - 4.0, y - 4.0, unit * 0.94, unit * 0.94, unit * 0.16) <= 1.0;
        Sample {
            colour: if inside {
                [16.0, 24.0, 39.0, 255.0]
            } else {
                [0.0; 4]
            },
            region: u8::from(inside),
        }
    });
    let faces: Vec<_> = (0..7)
        .map(|row| {
            raster(unit + 8.0, unit + 8.0, scale, |x, y| {
                let inside =
                    rounded(x - 4.0, y - 4.0, unit * 0.94, unit * 0.94, unit * 0.16) <= 0.0;
                let t = ((f64::from(row) * unit + y - 4.0) / (7.0 * unit)).clamp(0.0, 1.0);
                Sample {
                    colour: if inside {
                        [
                            243.0 + (101.0 - 243.0) * t,
                            246.0 + (214.0 - 246.0) * t,
                            255.0 + (232.0 - 255.0) * t,
                            255.0,
                        ]
                    } else {
                        [0.0; 4]
                    },
                    region: u8::from(inside),
                }
            })
        })
        .collect();
    for pass in 0..3 {
        for (letter, c) in text.chars().enumerate() {
            for (row, bits) in glyph(c).into_iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) == 0 {
                        continue;
                    }
                    let tile = match pass {
                        0 => &shadow,
                        1 => &outline,
                        _ => &faces[row],
                    };
                    let x = (((letter * 6 + col) as f64) * unit * scale).round() as u32;
                    let y = (row as f64 * unit * scale).round() as u32;
                    blit(&mut result, tile, x, y);
                }
            }
        }
    }
    result
}
fn blit(dst: &mut Sprite, src: &Sprite, x: u32, y: u32) {
    for row in 0..src.height.min(dst.height - y) {
        for col in 0..src.width.min(dst.width - x) {
            let i = (row * src.width + col) as usize * 4;
            let j = ((row + y) * dst.width + x + col) as usize * 4;
            let a = u32::from(src.rgba[i + 3]);
            if a == 0 {
                continue;
            }
            if a == 255 || dst.rgba[j + 3] == 0 {
                dst.rgba[j..j + 4].copy_from_slice(&src.rgba[i..i + 4]);
                continue;
            }
            let b = u32::from(dst.rgba[j + 3]) * (255 - a);
            let total = a * 255 + b;
            for k in 0..3 {
                dst.rgba[j + k] = ((u32::from(src.rgba[i + k]) * a * 255
                    + u32::from(dst.rgba[j + k]) * b
                    + total / 2)
                    / total) as u8;
            }
            dst.rgba[j + 3] = ((total + 127) / 255) as u8;
        }
    }
}
