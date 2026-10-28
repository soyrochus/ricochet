use crate::{
    constants::*,
    input::Action,
    levels::{self, Brick},
    physics::{self, Ball, Paddle, Vec2},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Active {
    Ready,
    Playing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Title,
    Ready,
    Playing,
    Paused(Active),
    Won,
    GameOver,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub state: State,
    pub ball: Ball,
    pub paddle: Paddle,
    pub bricks: Vec<Brick>,
    pub score: u32,
    pub lives: u8,
    pub level: usize,
    pub viewport_small: bool,
    pub resume_required: bool,
    next_launch: f64,
    pub launch_sign: f64,
}
impl Default for Game {
    fn default() -> Self {
        let mut game = Self {
            state: State::Title,
            ball: Ball {
                position: Vec2::default(),
                velocity: Vec2::default(),
                speed: START_SPEEDS[0],
            },
            paddle: Paddle::default(),
            bricks: levels::generate(1),
            score: 0,
            lives: LIVES,
            level: 1,
            viewport_small: false,
            resume_required: false,
            next_launch: 1.0,
            launch_sign: 1.0,
        };
        game.attach();
        game
    }
}
impl Game {
    fn attach(&mut self) {
        self.ball.position = Vec2::new(self.paddle.centre(), PADDLE_Y - BALL_RADIUS - SEPARATION);
        self.ball.velocity = Vec2::default();
    }
    pub fn moving(&self) -> bool {
        !self.viewport_small
            && !self.resume_required
            && matches!(self.state, State::Ready | State::Playing)
    }
    pub fn ticking(&self) -> bool {
        self.moving()
    }
    pub fn action(&mut self, action: Action) {
        if self.viewport_small {
            return;
        }
        if self.resume_required {
            if action == Action::Pause {
                self.resume_required = false;
            }
            return;
        }
        match (self.state, action) {
            (State::Title, Action::Space) => self.start(),
            (State::Ready, Action::Space) => {
                self.launch_sign = self.next_launch;
                self.next_launch = -self.next_launch;
                let angle = 20.0_f64.to_radians();
                self.ball.velocity = Vec2::new(
                    self.launch_sign * self.ball.speed * angle.sin(),
                    -self.ball.speed * angle.cos(),
                );
                self.state = State::Playing;
            }
            (State::Ready, Action::Pause) => self.state = State::Paused(Active::Ready),
            (State::Playing, Action::Pause) => self.state = State::Paused(Active::Playing),
            (State::Paused(active), Action::Pause) => {
                self.state = match active {
                    Active::Ready => State::Ready,
                    Active::Playing => State::Playing,
                }
            }
            (State::Paused(_) | State::Won | State::GameOver, Action::Restart) => self.start(),
            _ => {}
        }
    }
    pub fn start(&mut self) {
        *self = Self::default();
        self.state = State::Ready;
    }
    pub fn focus_lost(&mut self) {
        match self.state {
            State::Ready => self.state = State::Paused(Active::Ready),
            State::Playing => self.state = State::Paused(Active::Playing),
            _ => {}
        }
    }
    pub fn viewport(&mut self, playable: bool) {
        if !playable {
            self.viewport_small = true;
            self.resume_required = true;
        } else {
            self.viewport_small = false;
        }
    }
    pub fn tick(&mut self, direction: f64) {
        if !self.ticking() {
            return;
        }
        if self.state == State::Ready {
            self.paddle.x = (self.paddle.x + direction.clamp(-1.0, 1.0) * PADDLE_SPEED * DT)
                .clamp(0.0, WIDTH - PADDLE_WIDTH);
            self.attach();
            return;
        }
        let outcome = physics::step(
            &mut self.ball,
            &mut self.paddle,
            &mut self.bricks,
            direction,
            self.launch_sign,
            DT,
        );
        self.score += outcome.score;
        if outcome.cleared {
            if self.level == 3 {
                self.state = State::Won;
            } else {
                self.level += 1;
                self.bricks = levels::generate(self.level);
                self.ball.speed = START_SPEEDS[self.level - 1];
                self.paddle = Paddle::default();
                self.attach();
                self.state = State::Ready;
            }
        } else if outcome.lost {
            self.lives -= 1;
            if self.lives == 0 {
                self.state = State::GameOver;
            } else {
                self.paddle = Paddle::default();
                self.attach();
                self.state = State::Ready;
            }
        }
    }
}
