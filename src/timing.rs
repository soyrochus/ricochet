use crate::constants::DT;
use std::time::Duration;
/// Simulation time is accumulated independently of rendering deadlines.
#[derive(Default)]
pub struct Clock {
    accumulator: f64,
}
impl Clock {
    pub fn reset(&mut self) {
        self.accumulator = 0.0;
    }
    pub fn advance(&mut self, elapsed: Duration, active: bool) -> usize {
        if !active {
            self.reset();
            return 0;
        }
        self.accumulator += elapsed.as_secs_f64().min(0.100);
        let due = ((self.accumulator + 1e-10) / DT).floor() as usize;
        let ticks = due.min(12);
        self.accumulator -= ticks as f64 * DT;
        if due > 12 {
            self.accumulator = 0.0;
        }
        self.accumulator = self.accumulator.max(0.0);
        ticks
    }
    pub fn until_tick(&self) -> Duration {
        Duration::from_secs_f64((DT - self.accumulator).max(0.0))
    }
}
