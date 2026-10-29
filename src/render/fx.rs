//! Render-clock state in logical units. No terminal I/O or protocol commands.
use super::{
    font,
    sprites::{Banner, Frame},
};
use crate::{
    constants::*,
    game::{Event, Game, State},
    physics::{ContactKind, Vec2},
};
use std::{collections::VecDeque, time::Duration};

pub const TRAIL_START: u32 = 54;
pub const SHATTER_START: u32 = 62;
pub const PARTICLE_START: u32 = 70;
pub const BANNER_START: u32 = 166;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Full,
    Reduced,
    Off,
}
impl Level {
    pub fn particle_limit(self) -> usize {
        match self {
            Self::Full => 96,
            Self::Reduced => 32,
            Self::Off => 0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub id: u32,
    pub position: Vec2,
    pub frame: Frame,
    pub z: i32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    pub slot: usize,
    pub position: Vec2,
    pub velocity: Vec2,
    pub age: f64,
    pub lifetime: f64,
    pub colour: u8,
    pub size: u8,
}
#[derive(Clone, Debug, PartialEq)]
struct Shatter {
    slot: usize,
    position: Vec2,
    colour: u8,
    age: f64,
}
#[derive(Clone, Debug, PartialEq)]
struct Shake {
    amplitude: f64,
    duration: f64,
    born: f64,
    seed: u64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Effects {
    pub level: Level,
    pub trail: VecDeque<Vec2>,
    pub particles: VecDeque<Particle>,
    shatters: VecDeque<Shatter>,
    flashes: [f64; 50],
    squash: f64,
    shakes: VecDeque<Shake>,
    pub clock: f64,
    remainder: f64,
    event_counter: u64,
    level_banner: Option<Banner>,
    banner: Option<Banner>,
    banner_age: f64,
    was_frozen: bool,
}
impl Default for Effects {
    fn default() -> Self {
        Self::new(Level::Full)
    }
}
impl Effects {
    pub fn new(level: Level) -> Self {
        Self {
            level,
            trail: VecDeque::with_capacity(8),
            particles: VecDeque::with_capacity(level.particle_limit()),
            shatters: VecDeque::with_capacity(8),
            flashes: [0.0; 50],
            squash: 0.0,
            shakes: VecDeque::with_capacity(2),
            clock: 0.0,
            remainder: 0.0,
            event_counter: 0,
            level_banner: None,
            banner: None,
            banner_age: 0.0,
            was_frozen: false,
        }
    }
    fn clear(&mut self) {
        self.trail.clear();
        self.particles.clear();
        self.shatters.clear();
        self.flashes = [0.0; 50];
        self.squash = 0.0;
        self.shakes.clear();
        self.level_banner = None;
        self.banner = None;
        self.banner_age = 0.0;
        self.remainder = 0.0;
    }
    pub fn frozen(game: &Game) -> bool {
        matches!(game.state, State::Paused(_)) || game.viewport_small || game.resume_required
    }
    pub fn ingest(&mut self, events: &[Event], game: &Game) {
        for event in events {
            self.event_counter = self.event_counter.wrapping_add(1);
            match *event {
                Event::Restarted => self.clear(),
                Event::LevelCleared { next } => {
                    self.clear();
                    self.level_banner = Some(if next == 2 {
                        Banner::Level2
                    } else {
                        Banner::Level3
                    });
                }
                Event::Launched => {
                    self.trail.clear();
                    self.level_banner = None;
                }
                Event::LifeLost => {
                    self.trail.clear();
                    self.level_banner = None;
                    if self.level == Level::Full {
                        self.shake(6.0, 0.3, self.event_counter);
                    }
                }
                Event::Contact(c) if self.level != Level::Off => match c.kind {
                    ContactKind::Paddle { .. } => self.squash = 0.120,
                    ContactKind::Brick { index, hp_after } => {
                        if hp_after > 0 {
                            if let Some(f) = self.flashes.get_mut(index) {
                                *f = 0.070;
                            }
                        } else {
                            if self.shatters.len() == 8 {
                                self.shatters.pop_front();
                            }
                            let slot = free_slot(self.shatters.iter().map(|s| s.slot), 8);
                            let position = game.bricks.iter().find(|b| b.index == index).map_or(
                                Vec2::new(
                                    GRID_X + (index % 10) as f64 * (BRICK_WIDTH + GAP),
                                    GRID_Y + (index / 10) as f64 * (BRICK_HEIGHT + GAP),
                                ),
                                |b| Vec2::new(b.rect.x, b.rect.y),
                            );
                            self.shatters.push_back(Shatter {
                                slot,
                                position,
                                colour: 1,
                                age: 0.0,
                            });
                            if self.level == Level::Full {
                                self.shake(1.5, 0.080, self.event_counter ^ index as u64);
                            }
                        }
                        let mut rng = Random(
                            self.event_counter
                                ^ (game.level as u64).wrapping_mul(0x9e3779b97f4a7c15)
                                ^ index as u64
                                ^ c.point.x.to_bits()
                                ^ c.point.y.to_bits().rotate_left(17),
                        );
                        let count = match (self.level, hp_after == 0) {
                            (Level::Full, true) => 10,
                            (Level::Full, false) => 4,
                            (Level::Reduced, true) => 4,
                            _ => 2,
                        };
                        for _ in 0..count {
                            if self.particles.len() == self.level.particle_limit() {
                                self.particles.pop_front();
                            }
                            let slot = free_slot(
                                self.particles.iter().map(|p| p.slot),
                                self.level.particle_limit(),
                            );
                            let angle = (rng.next() * 2.0 - 1.0) * std::f64::consts::PI / 3.0;
                            let speed = 120.0 + rng.next() * 140.0;
                            let (s, cosa) = angle.sin_cos();
                            let velocity = Vec2::new(
                                c.normal.x * cosa - c.normal.y * s,
                                c.normal.x * s + c.normal.y * cosa,
                            ) * speed;
                            self.particles.push_back(Particle {
                                slot,
                                position: c.point,
                                velocity,
                                age: 0.0,
                                lifetime: 0.45 + rng.next() * 0.25,
                                colour: (hp_after + 1).min(3),
                                size: (rng.next() * 4.0) as u8,
                            });
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    fn shake(&mut self, amplitude: f64, duration: f64, seed: u64) {
        // A newer envelope of the same kind dominates every older one. Keep
        // the latest life-loss and destruction envelopes, then take their max;
        // even a burst of brick hits cannot retire an active stronger shake.
        self.shakes.retain(|s| s.amplitude != amplitude);
        self.shakes.push_back(Shake {
            amplitude,
            duration,
            born: self.clock,
            seed,
        });
    }
    pub fn advance(&mut self, dt: Duration, game: &Game) {
        let frozen = Self::frozen(game);
        let skip = frozen || self.was_frozen;
        self.was_frozen = frozen;
        if skip {
            return;
        }
        let dt = dt.as_secs_f64().min(0.050);
        self.clock += dt;
        self.banner_age += dt;
        for flash in &mut self.flashes {
            *flash = (*flash - dt).max(0.0);
        }
        self.squash = (self.squash - dt).max(0.0);
        for s in &mut self.shatters {
            s.age += dt;
        }
        self.shatters.retain(|s| s.age < 0.200);
        self.shakes.retain(|s| self.clock - s.born < s.duration);
        self.remainder += dt;
        while self.remainder + 1e-9 >= DT {
            self.remainder = (self.remainder - DT).max(0.0);
            for p in &mut self.particles {
                p.position = p.position + p.velocity * DT + Vec2::new(0.0, 0.5 * 900.0 * DT * DT);
                p.velocity.y += 900.0 * DT;
                p.age += DT;
            }
            self.particles.retain(|p| {
                p.age < p.lifetime
                    && (0.0..=WIDTH).contains(&p.position.x)
                    && (0.0..=HEIGHT).contains(&p.position.y)
            });
        }
    }
    /// Called once per rendered frame, after discontinuity events were consumed.
    pub fn observe(&mut self, game: &Game) {
        if game.state == State::Playing && !Self::frozen(game) && self.level != Level::Off {
            if self.trail.len() == 8 {
                self.trail.pop_front();
            }
            self.trail.push_back(game.ball.position);
        }
        let banner = if game.resume_required || game.viewport_small {
            Some(Banner::Paused)
        } else {
            match game.state {
                State::Ready => Some(self.level_banner.unwrap_or(Banner::Ready)),
                State::Paused(_) => Some(Banner::Paused),
                State::Won => Some(Banner::Won),
                State::GameOver => Some(Banner::GameOver),
                _ => None,
            }
        };
        if self.banner != banner {
            self.banner = banner;
            self.banner_age = 0.0;
        }
    }
    pub fn flashing(&self, index: usize) -> bool {
        self.flashes.get(index).is_some_and(|f| *f > 0.0)
    }
    pub fn paddle_frame(&self) -> u8 {
        if self.squash > 0.080 {
            0
        } else if self.squash > 0.040 {
            1
        } else if self.squash > 0.0 {
            2
        } else {
            0
        }
    }
    pub fn shake_offset(&self) -> Vec2 {
        let Some(s) = self.shakes.iter().max_by(|a, b| {
            let value = |s: &Shake| s.amplitude * (1.0 - (self.clock - s.born) / s.duration);
            value(a).total_cmp(&value(b))
        }) else {
            return Vec2::default();
        };
        let age = self.clock - s.born;
        let amplitude = s.amplitude * (1.0 - age / s.duration).clamp(0.0, 1.0);
        let mut rng = Random(s.seed ^ (age * 120.0).floor() as u64);
        Vec2::new(
            (rng.next() * 2.0 - 1.0) * amplitude,
            (rng.next() * 2.0 - 1.0) * amplitude,
        )
    }
    pub fn placements(&self, game: &Game) -> Vec<Draw> {
        let mut draws = Vec::with_capacity(115);
        // Retain the frozen trail and transients under the PAUSED banner.
        if matches!(
            game.state,
            State::Playing | State::Paused(crate::game::Active::Playing)
        ) || game.resume_required
        {
            let limit = if self.level == Level::Reduced {
                3
            } else {
                ((game.ball.speed / MAX_SPEED * 8.0).round() as usize).clamp(3, 8)
            };
            for (i, &position) in self.trail.iter().rev().take(limit).enumerate() {
                draws.push(Draw {
                    id: TRAIL_START + i as u32,
                    position,
                    frame: Frame::Trail((i / 2).min(3) as u8),
                    z: 5,
                });
            }
        }
        for s in &self.shatters {
            draws.push(Draw {
                id: SHATTER_START + s.slot as u32,
                position: s.position,
                frame: Frame::Shatter {
                    colour: s.colour,
                    frame: ((s.age / 0.040).floor() as u8).min(4),
                },
                z: 2,
            });
        }
        for p in &self.particles {
            draws.push(Draw {
                id: PARTICLE_START + p.slot as u32,
                position: p.position,
                frame: Frame::Particle {
                    colour: p.colour,
                    size: p.size,
                    faded: p.age >= p.lifetime * 0.7,
                },
                z: 3,
            });
        }
        if game.state == State::Title {
            let phase = self.clock % 2.5;
            let frame = if self.level == Level::Off || phase >= 0.4 {
                0
            } else {
                (phase / 0.05).floor() as u8
            };
            draws.push(Draw {
                id: BANNER_START,
                position: Vec2::new(
                    (WIDTH - font::width("RICOCHET", 10.0)) / 2.0,
                    HEIGHT / 3.0 - 35.0,
                ),
                frame: Frame::Logo(frame.min(7)),
                z: 7,
            });
        } else if let Some(kind) = self.banner {
            let alpha = if self.level == Level::Off || Self::frozen(game) {
                3
            } else {
                ((self.banner_age / 0.040).floor() as u8).min(3)
            };
            draws.push(Draw {
                id: BANNER_START,
                position: Vec2::new(
                    (WIDTH - font::width(kind.text(), 6.0)) / 2.0,
                    HEIGHT / 2.0 - 21.0,
                ),
                frame: Frame::Banner { kind, alpha },
                z: 7,
            });
        }
        draws
    }
    pub fn shatter_count(&self) -> usize {
        self.shatters.len()
    }
}
fn free_slot(slots: impl Iterator<Item = usize>, limit: usize) -> usize {
    let mut used = [false; 96];
    for slot in slots {
        used[slot] = true;
    }
    (0..limit)
        .find(|&i| !used[i])
        .expect("pool retires oldest before allocating")
}
struct Random(u64);
impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1_u64 << 53) as f64
    }
}
