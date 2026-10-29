pub mod font;
pub mod fx;
pub mod kitty;
pub mod sprites;
use crate::{
    constants::*,
    game::{Event, Game, State},
    physics::Vec2,
    terminal::{Geometry, Session},
};
use kitty::{PixelPlacement, SourceRect};
use sprites::{AtlasFrame, Frame};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub geometry: Geometry,
    pub scale: f64,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
}
const BACKGROUND: u32 = 0x524900;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Placement {
    image: u32,
    at: PixelPlacement,
    source: SourceRect,
    z: i32,
}
pub struct Renderer {
    geometry: Option<(Geometry, bool)>,
    pub view: Option<Viewport>,
    pub effects: fx::Effects,
    frames: BTreeMap<Frame, (u32, AtlasFrame)>,
    placed: BTreeMap<u32, Placement>,
    hud: Vec<String>,
    buffer: Vec<u8>,
}
impl Default for Renderer {
    fn default() -> Self {
        Self::new(fx::Level::Full)
    }
}
impl Renderer {
    pub fn new(level: fx::Level) -> Self {
        Self {
            geometry: None,
            view: None,
            effects: fx::Effects::new(level),
            frames: BTreeMap::new(),
            placed: BTreeMap::new(),
            hud: Vec::new(),
            buffer: Vec::new(),
        }
    }
    pub fn draw(
        &mut self,
        session: &mut Session<impl Write>,
        geometry: Geometry,
        game: &Game,
        events: &[Event],
        elapsed: Duration,
        synchronized: bool,
    ) -> io::Result<()> {
        self.buffer.clear();
        self.effects.advance(elapsed, game);
        self.effects.ingest(events, game);
        self.effects.observe(game);
        let compression = session.graphics.compression;
        if self.geometry != Some((geometry, compression)) {
            session.graphics.cleanup(&mut self.buffer)?;
            self.geometry = Some((geometry, compression));
            self.view = Viewport::with_compression(geometry, compression);
            self.placed.clear();
            self.frames.clear();
            self.hud.clear();
            self.buffer
                .extend_from_slice(b"\x1b[0m\x1b[48;2;10;15;25m\x1b[2J");
            if let Some(view) = self.view {
                for (index, atlas) in
                    sprites::generate(view.scale, self.effects.level == fx::Level::Full)
                        .into_iter()
                        .enumerate()
                {
                    let number = BACKGROUND + index as u32;
                    session.graphics.upload(
                        &mut self.buffer,
                        number,
                        atlas.sprite.width,
                        atlas.sprite.height,
                        &atlas.sprite.rgba,
                    )?;
                    for frame in atlas.frames {
                        self.frames.insert(frame.frame, (number, frame));
                    }
                }
            }
        }
        let mut desired = BTreeMap::new();
        if let Some(view) = self.view {
            let shake = view.clamp_shake(self.effects.shake_offset());
            let mut draws = vec![fx::Draw {
                id: 1,
                position: Vec2::default(),
                frame: Frame::Background,
                z: 0,
            }];
            for brick in game.bricks.iter().filter(|b| b.hp > 0) {
                draws.push(fx::Draw {
                    id: 2 + brick.index as u32,
                    position: Vec2::new(brick.rect.x, brick.rect.y),
                    frame: Frame::Brick {
                        initial: brick.initial_hp,
                        hp: brick.hp,
                        flash: self.effects.flashing(brick.index),
                    },
                    z: 1,
                });
            }
            draws.push(fx::Draw {
                id: 52,
                position: Vec2::new(game.paddle.x, PADDLE_Y),
                frame: Frame::Paddle(self.effects.paddle_frame()),
                z: 4,
            });
            draws.push(fx::Draw {
                id: 53,
                position: game.ball.position,
                frame: Frame::Ball,
                z: 6,
            });
            draws.extend(self.effects.placements(game));
            if self.effects.level == fx::Level::Full {
                draws.push(fx::Draw {
                    id: 168,
                    position: Vec2::default(),
                    frame: Frame::Overlay,
                    z: 8,
                });
            }
            for draw in draws {
                let (image, frame) = &self.frames[&draw.frame];
                if let Some((at, source)) =
                    view.clipped(draw.position + frame.origin, frame.source, shake)
                {
                    desired.insert(
                        draw.id,
                        Placement {
                            image: *image,
                            at,
                            source,
                            z: draw.z,
                        },
                    );
                }
            }
        }
        for (&id, old) in &self.placed {
            if desired.get(&id).is_none_or(|new| new.image != old.image) {
                session.graphics.remove(&mut self.buffer, old.image, id)?;
            }
        }
        for (&id, new) in &desired {
            if self.placed.get(&id) != Some(new) {
                session.graphics.place_clipped(
                    &mut self.buffer,
                    new.image,
                    id,
                    new.at,
                    new.z,
                    Some(new.source),
                )?;
            }
        }
        self.placed = desired;
        self.text(geometry, game)?;
        if !self.buffer.is_empty() {
            kitty::frame(&mut session.out, &self.buffer, synchronized)?;
        }
        Ok(())
    }
    fn text(&mut self, geometry: Geometry, game: &Game) -> io::Result<()> {
        let state = if self.view.is_none() {
            "RESIZE / progress saved"
        } else if game.resume_required {
            "VIEWPORT RESTORED / P to continue"
        } else {
            match game.state {
                State::Title => "THREE LEVELS. THREE LIVES. EVERY ANGLE COUNTS.",
                State::Ready => "READY / Space to launch",
                State::Playing => "IN PLAY",
                State::Paused(_) => "PAUSED / P or Esc to resume / R to restart",
                State::Won => "YOU WON / Three levels cleared / R to play again",
                State::GameOver => "GAME OVER / R to play again",
            }
        };
        let prompt = if self.view.is_none() {
            format!(
                "Need 80x30 and 400x300 drawable pixels. Now {}x{}. Q quits.",
                geometry.columns, geometry.rows
            )
        } else if game.state == State::Title {
            "Space to start / Q to quit".to_owned()
        } else {
            "Left/Right or A/D  Move   Space  Launch   P/Esc  Pause   Q  Quit".to_owned()
        };
        let lines = vec![
            format!(
                "RICOCHET   /   SCORE {:05}   LIVES {}   LEVEL {}/3",
                game.score, game.lives, game.level
            ),
            "CORAL 3 HP   AMBER 2 HP   TEAL 1 HP   /   Cracks mark damage".to_owned(),
            prompt,
            state.to_owned(),
        ];
        let rows = [
            0,
            1,
            geometry.rows.saturating_sub(2),
            geometry.rows.saturating_sub(1),
        ];
        for (i, line) in lines.iter().enumerate() {
            if self.hud.get(i) == Some(line) {
                continue;
            }
            // ASCII-only HUD: character count is exactly the display width.
            let clipped = &line[..line
                .len()
                .min(usize::from(geometry.columns.saturating_sub(1)))];
            let color = if i == 0 {
                "\x1b[1;38;2;101;214;232m"
            } else if i == 3 {
                "\x1b[0;38;2;243;246;255m"
            } else {
                "\x1b[0;38;2;153;172;194m"
            };
            write!(
                self.buffer,
                "\x1b[{};1H\x1b[2K{color}{clipped}",
                rows[i] + 1
            )?;
        }
        self.hud = lines;
        Ok(())
    }
}
impl Viewport {
    pub fn new(geometry: Geometry) -> Option<Self> {
        Self::with_compression(geometry, false)
    }
    pub fn with_compression(geometry: Geometry, compression: bool) -> Option<Self> {
        if geometry.columns < 80 || geometry.rows < 30 {
            return None;
        }
        let (cell_width, cell_height) = geometry.cell()?;
        let available_w = cell_width * u32::from(geometry.columns);
        let available_h = cell_height * u32::from(geometry.rows - 4);
        // Bound large-window sprite memory; centre and letterbox at all sizes.
        let scale = (f64::from(available_w) / WIDTH)
            .min(f64::from(available_h) / HEIGHT)
            .min(if compression { 3.0 } else { 2.0 });
        let width = (WIDTH * scale).floor() as u32;
        let height = (HEIGHT * scale).floor() as u32;
        if width < 400 || height < 300 {
            return None;
        }
        Some(Self {
            geometry,
            scale,
            left: (available_w - width) / 2,
            top: 2 * cell_height + (available_h - height) / 2,
            width,
            height,
            cell_width,
            cell_height,
        })
    }
    pub fn clamp_shake(self, offset: Vec2) -> Vec2 {
        let right = self.cell_width * u32::from(self.geometry.columns) - self.left - self.width;
        let above = self.top - 2 * self.cell_height;
        let below = self.cell_height * u32::from(self.geometry.rows - 2) - self.top - self.height;
        Vec2::new(
            offset.x.clamp(
                -f64::from(self.left) / self.scale,
                f64::from(right) / self.scale,
            ),
            offset.y.clamp(
                -f64::from(above) / self.scale,
                f64::from(below) / self.scale,
            ),
        )
    }
    fn clipped(
        self,
        position: Vec2,
        mut source: SourceRect,
        shake: Vec2,
    ) -> Option<(PixelPlacement, SourceRect)> {
        let sx = (shake.x * self.scale).round() as i64;
        let sy = (shake.y * self.scale).round() as i64;
        let left = i64::from(self.left) + sx;
        let top = i64::from(self.top) + sy;
        let x = i64::from(self.left) + (position.x * self.scale).round() as i64 + sx;
        let y = i64::from(self.top) + (position.y * self.scale).round() as i64 + sy;
        let x0 = x.max(left).max(0);
        let y0 = y.max(top).max(i64::from(2 * self.cell_height));
        let x1 = (x + i64::from(source.w))
            .min(left + i64::from(self.width))
            .min(i64::from(self.geometry.columns) * i64::from(self.cell_width));
        let y1 = (y + i64::from(source.h))
            .min(top + i64::from(self.height))
            .min(i64::from(self.geometry.rows - 2) * i64::from(self.cell_height));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        source.x += (x0 - x) as u32;
        source.y += (y0 - y) as u32;
        source.w = (x1 - x0) as u32;
        source.h = (y1 - y0) as u32;
        let (px, py) = (x0 as u32, y0 as u32);
        Some((
            PixelPlacement {
                column: px / self.cell_width,
                row: py / self.cell_height,
                x_offset: px % self.cell_width,
                y_offset: py % self.cell_height,
            },
            source,
        ))
    }
    pub fn pixels(self, x: f64, y: f64) -> (u32, u32) {
        (
            (f64::from(self.left) + x * self.scale).round().max(0.0) as u32,
            (f64::from(self.top) + y * self.scale).round().max(0.0) as u32,
        )
    }
    pub fn at(self, x: f64, y: f64) -> PixelPlacement {
        let (px, py) = self.pixels(x, y);
        PixelPlacement {
            column: px / self.cell_width,
            row: py / self.cell_height,
            x_offset: px % self.cell_width,
            y_offset: py % self.cell_height,
        }
    }
}
