//! Fixed-tick clock with render interpolation, time scale, and hitstop.
//! The sim advances in exact 1/60s ticks; rendering interpolates between the
//! previous and current tick using `alpha`. Hitstop and time scale affect how
//! fast real time feeds the accumulator — never the tick length itself, so
//! determinism is preserved.

pub const TICK_RATE: f64 = 60.0;
pub const FIXED_DT: f32 = (1.0 / TICK_RATE) as f32;

pub struct GameClock {
    pub tick: u64,
    accumulator: f64,
    /// Interpolation factor in [0,1) for rendering.
    pub alpha: f32,
    /// Slow-mo / speed-up multiplier applied to incoming real time.
    pub time_scale: f64,
    /// Seconds of real time remaining during which sim time is frozen.
    pub hitstop: f64,
    /// Clamp to avoid spiral-of-death after a long stall (alt-tab, debugger).
    pub max_ticks_per_frame: u32,
}

impl Default for GameClock {
    fn default() -> Self {
        Self::new()
    }
}

impl GameClock {
    pub fn new() -> Self {
        GameClock {
            tick: 0,
            accumulator: 0.0,
            alpha: 0.0,
            time_scale: 1.0,
            hitstop: 0.0,
            max_ticks_per_frame: 5,
        }
    }

    /// Feed real frame time; returns how many fixed ticks to simulate now.
    pub fn advance(&mut self, real_dt: f64) -> u32 {
        let mut dt = real_dt.min(0.25); // stall clamp
        if self.hitstop > 0.0 {
            let eaten = dt.min(self.hitstop);
            self.hitstop -= eaten;
            dt -= eaten;
        }
        self.accumulator += dt * self.time_scale;
        let step = 1.0 / TICK_RATE;
        let mut ticks = 0;
        while self.accumulator >= step && ticks < self.max_ticks_per_frame {
            self.accumulator -= step;
            ticks += 1;
        }
        if ticks == self.max_ticks_per_frame {
            // Drop the backlog rather than death-spiral.
            self.accumulator = self.accumulator.min(step);
        }
        self.alpha = (self.accumulator / step) as f32;
        ticks
    }

    /// Freeze sim time for `seconds` of real time (impact frames). Stacks by
    /// taking the max, so overlapping hits don't add up to slideshow.
    pub fn hitstop(&mut self, seconds: f64) {
        self.hitstop = self.hitstop.max(seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_ticks_accumulate() {
        let mut c = GameClock::new();
        let mut total = 0;
        for _ in 0..60 {
            total += c.advance(1.0 / 60.0);
        }
        assert!((59..=61).contains(&total));
    }

    #[test]
    fn hitstop_freezes_sim() {
        let mut c = GameClock::new();
        c.hitstop(0.1);
        let t = c.advance(0.1);
        assert_eq!(t, 0);
        let t = c.advance(0.1); // hitstop exhausted, sim resumes
        assert!(t >= 5);
    }
}
