//! Simulated data so the UI works before any strap or WHOOP keys are set up.

use crate::whoop::Metrics;

pub fn metrics() -> Metrics {
    Metrics {
        recovery_pct: Some(84.0),
        hrv_ms: Some(62.4),
        rhr: Some(54.0),
        sleep_pct: Some(91.0),
        strain: Some(8.7),
    }
}

/// Resting-ish heart rate with slow drift, beat-to-beat noise, and a
/// ~40 s spike every 3 minutes so the Desk Stress Alert can be seen firing.
pub struct HrSim {
    t: u64,
    seed: u32,
    value: f64,
}

impl HrSim {
    pub fn new() -> Self {
        Self { t: 0, seed: 0x2545_F491, value: 66.0 }
    }

    fn noise(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f64 / (1u32 << 24) as f64 - 0.5
    }

    pub fn next(&mut self) -> u16 {
        self.t += 1;
        let t = self.t as f64;
        let phase = self.t % 180;
        let spike = if (100..140).contains(&phase) { 18.0 } else { 0.0 };
        let target = 66.0 + 4.0 * (t / 37.0).sin() + 2.0 * (t / 11.0).sin() + spike;
        self.value += (target - self.value) * 0.18 + self.noise() * 1.6;
        self.value.round().clamp(40.0, 200.0) as u16
    }
}
