//! The frame-rate counter behind the console's `fps` command: smoothed frames per second, frame time and the worst frame of the last
//! second, measured from the real-time clock (so it does not depend on the profiling switch or on the game speed).

use std::collections::VecDeque;

use bevy::prelude::*;

/// Frames averaged for the displayed rate and frame time.
const SMOOTH: f64 = 0.5;
/// How far back the worst frame looks.
const WORST: f64 = 1.0;

#[derive(Resource, Default)]
pub struct FrameStats {
    /// `(real time at the end of the frame, its length)`, oldest first, covering the last `WORST` seconds.
    samples: VecDeque<(f64, f64)>,
}

impl FrameStats {
    /// Records one frame that ended at real time `now` (seconds) and took `dt`.
    pub fn push(&mut self, now: f64, dt: f64) {
        self.samples.push_back((now, dt));
        while self.samples.front().is_some_and(|s| now - s.0 > WORST) {
            self.samples.pop_front();
        }
    }
    pub fn clear(&mut self) {
        self.samples.clear();
    }
    /// The frames of the last `SMOOTH` seconds: their count and total time.
    fn recent(&self) -> (usize, f64) {
        let Some(last) = self.samples.back() else { return (0, 0.0) };
        self.samples.iter().filter(|s| last.0 - s.0 <= SMOOTH).fold((0, 0.0), |(n, t), s| (n + 1, t + s.1))
    }
    /// Frames per second over the last half second (0 before any frame).
    pub fn fps(&self) -> f64 {
        let (n, t) = self.recent();
        if t > 0.0 { n as f64 / t } else { 0.0 }
    }
    /// Average frame time over the last half second, in milliseconds.
    pub fn frame_ms(&self) -> f64 {
        let (n, t) = self.recent();
        if n > 0 { t / n as f64 * 1000.0 } else { 0.0 }
    }
    /// The longest frame of the last second, in milliseconds.
    pub fn worst_ms(&self) -> f64 {
        self.samples.iter().map(|s| s.1).fold(0.0, f64::max) * 1000.0
    }
    /// The overlay text; the game speed is added when it is not 1.
    pub fn text(&self, speed: f32) -> String {
        let mut s = format!("{:.0} fps  {:.1} ms  worst {:.1} ms", self.fps(), self.frame_ms(), self.worst_ms());
        if (speed - 1.0).abs() > 0.01 {
            s.push_str(&format!("  x{speed:.1}"));
        }
        s
    }
}

#[cfg(test)]
mod tests;
