pub mod kitty;
pub mod sprites;
use crate::{
    constants::*,
    game::{Game, State},
    terminal::Session,
};
use crate::{
    constants::{HEIGHT, WIDTH},
    terminal::Geometry,
};
use kitty::PixelPlacement;
use std::io::{self, Write};

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

const VARIANTS: [(u8, u8); 6] = [(1, 1), (2, 2), (2, 1), (3, 3), (3, 2), (3, 1)];
// These are aliases (image numbers), not pre-existing terminal image IDs.
const BACKGROUND: u32 = 0x524900;
const BALL: u32 = BACKGROUND + 1;
const PADDLE: u32 = BACKGROUND + 2;
fn brick_number(initial: u8, hp: u8) -> u32 {
    BACKGROUND
        + 3
        + VARIANTS
            .iter()
            .position(|v| *v == (initial, hp))
            .expect("valid brick HP") as u32
}

pub struct Renderer {
    geometry: Option<Geometry>,
    pub view: Option<Viewport>,
    bricks: [Option<u32>; 50],
    paddle: Option<PixelPlacement>,
    ball: Option<(PixelPlacement, u32)>,
    hud: Vec<String>,
    buffer: Vec<u8>,
}
impl Default for Renderer {
    fn default() -> Self {
        Self {
            geometry: None,
            view: None,
            bricks: [None; 50],
            paddle: None,
            ball: None,
            hud: Vec::new(),
            buffer: Vec::new(),
        }
    }
}
impl Renderer {
    pub fn draw(
        &mut self,
        session: &mut Session<impl Write>,
        geometry: Geometry,
        game: &Game,
        synchronized: bool,
    ) -> io::Result<()> {
        self.buffer.clear();
        if self.geometry != Some(geometry) {
            session.graphics.cleanup(&mut self.buffer)?;
            self.geometry = Some(geometry);
            self.view = Viewport::new(geometry);
            self.bricks = [None; 50];
            self.paddle = None;
            self.ball = None;
            self.hud.clear();
            self.buffer
                .extend_from_slice(b"\x1b[0m\x1b[48;2;10;15;25m\x1b[2J");
            if let Some(view) = self.view {
                self.upload(session, BACKGROUND, sprites::background(view.scale))?;
                self.upload(session, BALL, sprites::ball(view.scale))?;
                self.upload(session, PADDLE, sprites::paddle(view.scale))?;
                for (initial, hp) in VARIANTS {
                    self.upload(
                        session,
                        brick_number(initial, hp),
                        sprites::brick(view.scale, initial, hp),
                    )?;
                }
                session
                    .graphics
                    .place(&mut self.buffer, BACKGROUND, 1, view.at(0.0, 0.0), 0)?;
            }
        }
        if let Some(view) = self.view {
            let mut desired = [None; 50];
            for brick in game.bricks.iter().filter(|b| b.hp > 0) {
                desired[brick.index] = Some(brick_number(brick.initial_hp, brick.hp));
            }
            for (index, &number) in desired.iter().enumerate() {
                if self.bricks[index] == number {
                    continue;
                }
                let pid = index as u32 + 1;
                if let Some(old) = self.bricks[index] {
                    session.graphics.remove(&mut self.buffer, old, pid)?;
                }
                if let Some(new) = number {
                    let x = GRID_X + (index % 10) as f64 * (BRICK_WIDTH + GAP);
                    let y = GRID_Y + (index / 10) as f64 * (BRICK_HEIGHT + GAP);
                    session
                        .graphics
                        .place(&mut self.buffer, new, pid, view.at(x, y), 1)?;
                }
                self.bricks[index] = number;
            }
            let paddle = view.at(game.paddle.x, PADDLE_Y);
            if self.paddle != Some(paddle) {
                session
                    .graphics
                    .place(&mut self.buffer, PADDLE, 1, paddle, 2)?;
                self.paddle = Some(paddle);
            }
            let x = game.ball.position.x - BALL_RADIUS;
            let y = game.ball.position.y - BALL_RADIUS;
            let at = view.at(x, y);
            let (_, py) = view.pixels(x, y);
            // The bottom is open physically, but graphics are clipped before the
            // footer rows while the ball crosses the life-loss boundary.
            let h = (view.top + view.height)
                .saturating_sub(py)
                .min((2.0 * BALL_RADIUS * view.scale).round() as u32);
            if h == 0 {
                if self.ball.take().is_some() {
                    session.graphics.remove(&mut self.buffer, BALL, 1)?;
                }
            } else if self.ball != Some((at, h)) {
                session
                    .graphics
                    .place_clipped(&mut self.buffer, BALL, 1, at, 3, Some(kitty::SourceRect { x: 0, y: 0, w: (2.0 * BALL_RADIUS * view.scale).round() as u32, h }))?;
                self.ball = Some((at, h));
            }
        }
        self.text(geometry, game)?;
        if !self.buffer.is_empty() {
            kitty::frame(&mut session.out, &self.buffer, synchronized)?;
        }
        Ok(())
    }
    fn upload(
        &mut self,
        session: &mut Session<impl Write>,
        number: u32,
        sprite: sprites::Sprite,
    ) -> io::Result<()> {
        session.graphics.upload(
            &mut self.buffer,
            number,
            sprite.width,
            sprite.height,
            &sprite.rgba,
        )
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
        if geometry.columns < 80 || geometry.rows < 30 {
            return None;
        }
        let (cell_width, cell_height) = geometry.cell()?;
        let available_w = cell_width * u32::from(geometry.columns);
        let available_h = cell_height * u32::from(geometry.rows - 4);
        // Bound large-window sprite memory; centre and letterbox at all sizes.
        let scale = (f64::from(available_w) / WIDTH)
            .min(f64::from(available_h) / HEIGHT)
            .min(2.0);
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
