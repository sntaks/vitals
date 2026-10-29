//! Animation primitives.
//!
//! Every animated figure in the app is a [`Tween`]: when a new reading arrives,
//! the app calls `set(new_value)`, then calls `step(dt, duration)` once per frame
//! and draws `value()`. See docs/animation.md for the bigger picture.

/// A value that glides to each new target over a fixed time with an ease-out curve.
///
/// A fixed duration (rather than a spring that only approaches its target)
/// means animations really finish, so the app can stop repainting between
/// readings. Retargeting mid-flight starts from the value currently shown,
/// so nothing ever jumps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tween {
    from: f32,
    to: f32,
    /// 0..=1 through the current transition.
    progress: f32,
}

impl Tween {
    /// Already resting at `value`.
    pub fn new(value: f32) -> Self {
        Self { from: value, to: value, progress: 1.0 }
    }

    pub fn value(&self) -> f32 {
        if self.progress >= 1.0 {
            return self.to;
        }
        self.from + (self.to - self.from) * ease_out(self.progress)
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    /// Start gliding from the current value to `target`.
    /// Setting the target it's already heading to does nothing, so the transition isn't restarted.
    pub fn set(&mut self, target: f32) {
        if target != self.to {
            *self = Self { from: self.value(), to: target, progress: 0.0 };
        }
    }

    /// Advance by `dt` seconds of a `duration`-second transition. Returns true while
    /// another frame is needed: false once `value()` has landed on the target, so the
    /// frame that draws the final value is the last one. A `duration` of 0 lands at once.
    pub fn step(&mut self, dt: f32, duration: f32) -> bool {
        if self.progress >= 1.0 {
            return false;
        }
        self.progress = (self.progress + dt / duration.max(1e-4)).min(1.0);
        self.progress < 1.0
    }
}

/// Fast start, gentle landing ("ease-out cubic").
///
/// Maps linear progress 0..=1 to eased progress 0..=1. Since `(1 - t)^3` shrinks
/// quickly, most of the motion happens early and the last stretch is slow,
/// which reads as responsive but not abrupt.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn reaches_target_exactly_and_stops() {
        let mut t = Tween::new(0.0);
        t.set(100.0);
        let mut prev = 0.0;
        let mut frames = 0;
        while t.step(DT, 0.5) {
            assert!(t.value() >= prev && t.value() <= 100.0);
            prev = t.value();
            frames += 1;
        }
        assert_eq!(t.value(), 100.0);
        assert!((29..=30).contains(&frames), "took {frames} frames");
        assert!(!t.step(DT, 0.5));
    }

    #[test]
    fn retarget_mid_flight_starts_from_shown_value() {
        let mut t = Tween::new(0.0);
        t.set(100.0);
        for _ in 0..10 {
            t.step(DT, 0.5);
        }
        let shown = t.value();
        t.set(0.0);
        assert_eq!(t.value(), shown);
    }

    #[test]
    fn same_target_does_not_restart() {
        let mut t = Tween::new(0.0);
        t.set(10.0);
        t.step(DT, 0.5);
        let shown = t.value();
        t.set(10.0);
        assert_eq!(t.value(), shown);
        assert!(t.step(DT, 0.5));
    }

    #[test]
    fn zero_duration_lands_in_one_step_without_another_frame() {
        let mut t = Tween::new(0.0);
        t.set(42.0);
        assert!(!t.step(DT, 0.0));
        assert_eq!(t.value(), 42.0);
    }

    #[test]
    fn huge_frame_time_lands_on_target() {
        let mut t = Tween::new(0.0);
        t.set(16.0e9);
        t.step(5.0, 0.5);
        assert_eq!(t.value(), 16.0e9);
    }
}
