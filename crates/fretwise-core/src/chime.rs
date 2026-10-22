//! A short two-note "ding" for correct answers. Realtime-safe: no allocation
//! or locking, so it can run inside any audio callback.
//!
//! It fades out (and the stabilizer releases it) within the trainer's pause
//! between a correct answer and the next prompt, during which nothing is
//! judged, so a mic picking it up can't count as a played note.

use std::f32::consts::TAU;

/// (start seconds, frequency Hz): a rising perfect fifth.
const NOTES: [(f32, f32); 2] = [(0.0, 1568.0), (0.07, 2349.3)];
/// Each note's amplitude decay time constant.
const DECAY: f32 = 0.06;
/// After this the chime is silent and stops costing anything.
pub const LENGTH: f32 = 0.45;

pub struct Chime {
    sample_rate: f32,
    /// Samples since the last trigger; `None` when idle.
    pos: Option<u32>,
    pub volume: f32,
}

impl Chime {
    pub fn new(sample_rate: f32) -> Self {
        Self { sample_rate, pos: None, volume: 0.25 }
    }

    pub fn trigger(&mut self) {
        self.pos = Some(0);
    }

    pub fn is_playing(&self) -> bool {
        self.pos.is_some()
    }

    pub fn next_sample(&mut self) -> f32 {
        let Some(pos) = self.pos else { return 0.0 };
        let t = pos as f32 / self.sample_rate;
        let mut out = 0.0;
        for (start, freq) in NOTES {
            let local = t - start;
            if local >= 0.0 {
                // 2 ms fade-in so the onset doesn't click.
                let attack = (local / 0.002).min(1.0);
                let env = attack * (-local / DECAY).exp();
                // A quiet octave partial makes it bell-like rather than a beep.
                out += env * ((TAU * freq * local).sin() + 0.2 * (TAU * 2.0 * freq * local).sin());
            }
        }
        let next = pos + 1;
        self.pos = ((next as f32) < LENGTH * self.sample_rate).then_some(next);
        out * self.volume * 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_once_then_goes_quiet() {
        let mut c = Chime::new(48_000.0);
        assert_eq!(c.next_sample(), 0.0);
        c.trigger();
        let samples: Vec<f32> = (0..48_000).map(|_| c.next_sample()).collect();
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05 && peak <= 1.0, "peak {peak}");
        assert!(!c.is_playing());
        // Fully silent well before the end of the second.
        assert!(samples[(LENGTH * 48_000.0) as usize..].iter().all(|&s| s == 0.0));
        // And already faint near the end of its tail.
        let tail = samples[(0.4 * 48_000.0) as usize];
        assert!(tail.abs() < 1e-3, "tail {tail}");
    }
}
