use crate::{constants::*, physics::Rect};
#[derive(Clone, Debug, PartialEq)]
pub struct Brick {
    pub index: usize,
    pub rect: Rect,
    pub hp: u8,
    pub initial_hp: u8,
}

pub fn generate(level: usize) -> Vec<Brick> {
    let rows = match level {
        1 => [2, 2, 1, 1, 1],
        2 => [3, 3, 2, 2, 1],
        3 => [3, 3, 3, 2, 2],
        _ => panic!("level must be 1..=3"),
    };
    let mut bricks = Vec::new();
    for (row, hp) in rows.into_iter().enumerate() {
        for column in 0..10 {
            if level == 3 && (row + column) % 4 == 0 {
                continue;
            }
            bricks.push(Brick {
                index: row * 10 + column,
                rect: Rect {
                    x: GRID_X + column as f64 * (BRICK_WIDTH + GAP),
                    y: GRID_Y + row as f64 * (BRICK_HEIGHT + GAP),
                    w: BRICK_WIDTH,
                    h: BRICK_HEIGHT,
                },
                hp,
                initial_hp: hp,
            });
        }
    }
    bricks
}
