//! Noise gate on the monitored DI signal, sharing the analyzer's input gate.

use std::sync::Arc;

use crate::analyzer::SharedPitch;
use crate::fx::Effect;

/// How far below the threshold the level must fall before an open gate closes.
const HYSTERESIS_DB: f32 = 4.0;
/// How long the gate stays open after the level drops, in seconds.
const HOLD_S: f32 = 0.06;
/// Level detector window: about the analyzer's, so the IN meter's gate
/// bracket means the same thing for monitoring as for pitch detection.
const DETECT_S: f32 = 0.01;
const ATTACK_S: f32 = 0.001;
/// Fades out over this long, so a ringing note tails off instead of chopping.
const RELEASE_S: f32 = 0.08;

/// Mutes the DI below the input gate set on the IN meter
/// ([`SharedPitch::gate_db`]), so hum and string noise don't reach the amp.
pub struct NoiseGate {
    shared: Arc<SharedPitch>,
    sample_rate: f32,
    mean_sq: f32,
    open: bool,
    hold_left: u32,
    gain: f32,
}

impl NoiseGate {
    pub fn new(sample_rate: f32, shared: Arc<SharedPitch>) -> Self {
        let mut gate = Self { shared, sample_rate, mean_sq: 0.0, open: false, hold_left: 0, gain: 0.0 };
        gate.prepare(sample_rate);
        gate
    }
}

/// The per-sample one-pole coefficient for a time constant of `seconds`.
fn coeff(seconds: f32, sample_rate: f32) -> f32 {
    1.0 - (-1.0 / (seconds * sample_rate)).exp()
}

impl Effect for NoiseGate {
    fn name(&self) -> &str {
        "Noise Gate"
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
    }

    fn reset(&mut self) {
        self.mean_sq = 0.0;
        self.open = false;
        self.hold_left = 0;
        self.gain = 0.0;
    }

    fn process(&mut self, samples: &mut [f32]) {
        let sr = self.sample_rate;
        let open_at = 10.0_f32.powf(self.shared.gate_db() / 10.0);
        let close_at = open_at * 10.0_f32.powf(-HYSTERESIS_DB / 10.0);
        let (detect, attack, release) = (coeff(DETECT_S, sr), coeff(ATTACK_S, sr), coeff(RELEASE_S, sr));
        let hold = (HOLD_S * sr) as u32;
        for s in samples.iter_mut() {
            self.mean_sq += (*s * *s - self.mean_sq) * detect;
            if self.mean_sq >= open_at {
                self.open = true;
                self.hold_left = hold;
            } else if self.mean_sq < close_at {
                if self.hold_left == 0 {
                    self.open = false;
                } else {
                    self.hold_left -= 1;
                }
            }
            let target = if self.open { 1.0 } else { 0.0 };
            let rate = if target > self.gain { attack } else { release };
            self.gain += (target - self.gain) * rate;
            *s *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn gate_mutes_noise_and_passes_notes() {
        let sr = 48_000.0;
        let shared = Arc::new(SharedPitch::default());
        shared.set_gate_db(-50.0);
        let mut gate = NoiseGate::new(sr, shared);

        // Hum around -66 dBFS stays muted.
        let mut hum: Vec<f32> = (0..9_600).map(|i| (TAU * 60.0 * i as f32 / sr).sin() * 0.0007).collect();
        gate.process(&mut hum);
        assert!(rms(&hum[4_800..]) < 1e-5, "hum leaked: {}", rms(&hum[4_800..]));

        // A note around -20 dBFS opens it.
        let mut note: Vec<f32> = (0..9_600).map(|i| (TAU * 220.0 * i as f32 / sr).sin() * 0.14).collect();
        let dry = rms(&note[4_800..]);
        gate.process(&mut note);
        assert!((rms(&note[4_800..]) - dry).abs() < dry * 0.01);

        // Back to hum: it closes after the hold and release.
        let mut tail: Vec<f32> = (0..48_000).map(|i| (TAU * 60.0 * i as f32 / sr).sin() * 0.0007).collect();
        gate.process(&mut tail);
        assert!(rms(&tail[43_200..]) < 1e-5);
    }
}
