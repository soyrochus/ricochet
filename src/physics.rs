//! Exact swept circle versus rectangle faces and circular corner arcs. Paddle
//! motion is piecewise linear up to its clamp time, then stationary. The solver
//! consumes only the time before the earliest contact, with stable brick ties.
use crate::{constants::*, levels::Brick};
use std::ops::{Add, Mul, Sub};
const TIME_EPS: f64 = 1e-9;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}
impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Self {
        self * (1.0 / self.length())
    }
}
impl Add for Vec2 {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y)
    }
}
impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y)
    }
}
impl Mul<f64> for Vec2 {
    type Output = Self;
    fn mul(self, b: f64) -> Self {
        Self::new(self.x * b, self.y * b)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub position: Vec2,
    pub velocity: Vec2,
    pub speed: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paddle {
    pub x: f64,
}
impl Default for Paddle {
    fn default() -> Self {
        Self {
            x: (WIDTH - PADDLE_WIDTH) / 2.0,
        }
    }
}
impl Paddle {
    pub fn centre(self) -> f64 {
        self.x + PADDLE_WIDTH / 2.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub time: f64,
    pub normal: Vec2,
}

/// Faces are tested only over their real span. Corner hits use quadratic roots
/// on the corner circle, avoiding the square-corner expanded-AABB approximation.
pub fn sweep_circle(p: Vec2, v: Vec2, radius: f64, rect: Rect, limit: f64) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    let mut accept = |time: f64, normal: Vec2| {
        if time >= -TIME_EPS
            && time <= limit + TIME_EPS
            && v.dot(normal) < -TIME_EPS
            && best.is_none_or(|h| time < h.time - TIME_EPS)
        {
            best = Some(Hit {
                time: time.max(0.0),
                normal,
            });
        }
    };
    if v.x.abs() > TIME_EPS {
        for (x, n) in [
            (rect.x - radius, Vec2::new(-1.0, 0.0)),
            (rect.x + rect.w + radius, Vec2::new(1.0, 0.0)),
        ] {
            let t = (x - p.x) / v.x;
            let y = p.y + v.y * t;
            if y >= rect.y - TIME_EPS && y <= rect.y + rect.h + TIME_EPS {
                accept(t, n);
            }
        }
    }
    if v.y.abs() > TIME_EPS {
        for (y, n) in [
            (rect.y - radius, Vec2::new(0.0, -1.0)),
            (rect.y + rect.h + radius, Vec2::new(0.0, 1.0)),
        ] {
            let t = (y - p.y) / v.y;
            let x = p.x + v.x * t;
            if x >= rect.x - TIME_EPS && x <= rect.x + rect.w + TIME_EPS {
                accept(t, n);
            }
        }
    }
    let a = v.dot(v);
    if a > TIME_EPS {
        for (cx, sx) in [(rect.x, -1.0), (rect.x + rect.w, 1.0)] {
            for (cy, sy) in [(rect.y, -1.0), (rect.y + rect.h, 1.0)] {
                let corner = Vec2::new(cx, cy);
                let delta = p - corner;
                let b = 2.0 * delta.dot(v);
                let c = delta.dot(delta) - radius * radius;
                let discriminant = b * b - 4.0 * a * c;
                if discriminant >= 0.0 {
                    let t = (-b - discriminant.sqrt()) / (2.0 * a);
                    let relative = p + v * t - corner;
                    if relative.x * sx >= -TIME_EPS && relative.y * sy >= -TIME_EPS {
                        accept(t, relative * (1.0 / radius));
                    }
                }
            }
        }
    }
    best
}

pub fn paddle_bounce(ball: &mut Ball, paddle: Paddle, launch_sign: f64) {
    let offset = ((ball.position.x - paddle.centre()) / (PADDLE_WIDTH / 2.0)).clamp(-1.0, 1.0);
    let angle = offset * 65.0_f64.to_radians();
    let incoming_sign = if ball.velocity.x.abs() > TIME_EPS {
        ball.velocity.x.signum()
    } else {
        launch_sign
    };
    let mut vx = ball.speed * angle.sin();
    if vx.abs() < 0.12 * ball.speed {
        vx = 0.12 * ball.speed * incoming_sign;
    }
    ball.velocity = Vec2::new(vx, -(ball.speed * ball.speed - vx * vx).max(0.0).sqrt());
}

#[derive(Default, Debug, PartialEq)]
pub struct Outcome {
    pub score: u32,
    pub lost: bool,
    pub cleared: bool,
    pub contacts: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Wall,
    Brick(usize),
    Paddle,
    Loss,
}
#[derive(Clone, Copy)]
struct Contact {
    hit: Hit,
    kind: Kind,
}

/// No wall clock or terminal state. `dt` is bounded by the caller to one tick.
pub fn step(
    ball: &mut Ball,
    paddle: &mut Paddle,
    bricks: &mut [Brick],
    direction: f64,
    launch_sign: f64,
    dt: f64,
) -> Outcome {
    let mut result = Outcome::default();
    let mut remaining = dt;
    let requested = direction.clamp(-1.0, 1.0) * PADDLE_SPEED;
    let mut iterations = 0;
    while remaining > TIME_EPS && iterations < MAX_COLLISIONS {
        if bricks.iter().all(|b| b.hp == 0) {
            result.cleared = true;
            break;
        }
        let pv = if (requested < 0.0 && paddle.x > TIME_EPS)
            || (requested > 0.0 && paddle.x < WIDTH - PADDLE_WIDTH - TIME_EPS)
        {
            requested
        } else {
            0.0
        };
        let clamp_time = if pv > 0.0 {
            (WIDTH - PADDLE_WIDTH - paddle.x) / pv
        } else if pv < 0.0 {
            -paddle.x / pv
        } else {
            f64::INFINITY
        };
        let horizon = remaining.min(clamp_time);
        let mut contacts = Vec::with_capacity(8);
        let mut earliest = horizon + 1.0;
        let mut candidate = |time: f64, normal: Vec2, kind: Kind| {
            if time < -TIME_EPS || time > horizon + TIME_EPS {
                return;
            }
            if time < earliest - TIME_EPS {
                earliest = time.max(0.0);
                contacts.clear();
            }
            if (time - earliest).abs() <= TIME_EPS {
                contacts.push(Contact {
                    hit: Hit {
                        time: time.max(0.0),
                        normal,
                    },
                    kind,
                });
            }
        };
        let p = ball.position;
        let v = ball.velocity;
        if v.x < -TIME_EPS {
            candidate((BALL_RADIUS - p.x) / v.x, Vec2::new(1.0, 0.0), Kind::Wall);
        }
        if v.x > TIME_EPS {
            candidate(
                (WIDTH - BALL_RADIUS - p.x) / v.x,
                Vec2::new(-1.0, 0.0),
                Kind::Wall,
            );
        }
        if v.y < -TIME_EPS {
            candidate((BALL_RADIUS - p.y) / v.y, Vec2::new(0.0, 1.0), Kind::Wall);
        }
        if v.y > TIME_EPS {
            candidate(
                ((HEIGHT + BALL_RADIUS - p.y) / v.y).max(0.0),
                Vec2::default(),
                Kind::Loss,
            );
        }
        for (index, brick) in bricks.iter().enumerate().filter(|(_, b)| b.hp > 0) {
            if let Some(hit) = sweep_circle(p, v, BALL_RADIUS, brick.rect, horizon) {
                candidate(hit.time, hit.normal, Kind::Brick(index));
            }
        }
        // Top surface only. Evaluate horizontal paddle position at contact, not
        // at the end of the tick. Below/side contacts cannot rescue a miss.
        if v.y > TIME_EPS && p.y <= PADDLE_Y + TIME_EPS {
            let relative = Vec2::new(v.x - pv, v.y);
            let rect = Rect {
                x: paddle.x,
                y: PADDLE_Y,
                w: PADDLE_WIDTH,
                h: PADDLE_HEIGHT,
            };
            if let Some(hit) = sweep_circle(p, relative, BALL_RADIUS, rect, horizon)
                && hit.normal.y < -TIME_EPS
            {
                candidate(hit.time, hit.normal, Kind::Paddle);
            }
        }
        if contacts.is_empty() {
            ball.position = ball.position + ball.velocity * horizon;
            paddle.x = (paddle.x + pv * horizon).clamp(0.0, WIDTH - PADDLE_WIDTH);
            remaining -= horizon;
            // The clamp is a segment boundary, not a collision event.
            continue;
        }
        iterations += 1;
        result.contacts += 1;
        ball.position = ball.position + ball.velocity * earliest;
        paddle.x = (paddle.x + pv * earliest).clamp(0.0, WIDTH - PADDLE_WIDTH);
        remaining = (remaining - earliest).max(0.0);
        let mut normals = Vec::new();
        let mut paddle_hit = false;
        let mut loss = false;
        let mut destroyed = 0;
        // Candidates were inserted in stable brick vector order. Deduplicate
        // both brick damage and parallel normals at a simultaneous contact.
        let mut damaged = Vec::new();
        for contact in contacts {
            match contact.kind {
                Kind::Brick(i) if !damaged.contains(&i) => {
                    damaged.push(i);
                    bricks[i].hp -= 1;
                    result.score += 10;
                    if bricks[i].hp == 0 {
                        result.score += 25;
                        destroyed += 1;
                    }
                }
                Kind::Paddle => paddle_hit = true,
                Kind::Loss => loss = true,
                _ => {}
            }
            if contact.kind != Kind::Loss
                && !normals
                    .iter()
                    .any(|n: &Vec2| n.dot(contact.hit.normal) > 1.0 - 1e-8)
            {
                normals.push(contact.hit.normal);
            }
        }
        ball.speed = (ball.speed + f64::from(destroyed) * SPEED_STEP).min(MAX_SPEED);
        if bricks.iter().all(|b| b.hp == 0) {
            ball.velocity = ball.velocity.normalized() * ball.speed;
            result.cleared = true;
            break;
        }
        if loss {
            result.lost = true;
            break;
        }
        if paddle_hit {
            paddle_bounce(ball, *paddle, launch_sign);
        } else {
            // Independent simultaneous normals constrain both axes: reverse
            // each once. This also keeps symmetric rounded-corner seam hits
            // symmetric, instead of depending on sequential reflection order.
            let n = normals[0];
            let independent = normals
                .iter()
                .any(|other| (n.x * other.y - n.y * other.x).abs() > 1e-8);
            if independent {
                ball.velocity = ball.velocity * -1.0;
            } else {
                ball.velocity = ball.velocity - n * (2.0 * ball.velocity.dot(n));
            }
            ball.velocity = ball.velocity.normalized() * ball.speed;
        }
        for normal in normals {
            ball.position = ball.position + normal * SEPARATION;
        }
    }
    // If iteration cap is hit, discard remaining simulation time safely.
    result
}
