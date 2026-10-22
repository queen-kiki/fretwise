//! Monophonic pitch detection (YIN, de Cheveigné & Kawahara 2002).
//!
//! All buffers are allocated in [`Yin::new`]; [`Yin::detect`] never allocates,
//! so it is safe to call from an audio callback.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitchEstimate {
    pub frequency: f32,
    /// 0..=1, how periodic the signal is (1 - YIN's aperiodicity).
    pub clarity: f32,
}

pub struct Yin {
    sample_rate: f32,
    window: usize,
    min_tau: usize,
    max_tau: usize,
    threshold: f32,
    /// Difference function, then its cumulative mean normalized form, in place.
    cmndf: Vec<f32>,
}

impl Yin {
    /// `threshold` around 0.1–0.2 works well for guitar.
    pub fn new(sample_rate: f32, min_freq: f32, max_freq: f32, threshold: f32) -> Self {
        let max_tau = (sample_rate / min_freq).ceil() as usize;
        let min_tau = ((sample_rate / max_freq).floor() as usize).max(2);
        Self {
            sample_rate,
            // Integrate over one period of the lowest note we care about.
            window: max_tau,
            min_tau,
            max_tau,
            threshold,
            cmndf: vec![0.0; max_tau + 2],
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Samples `detect` needs to see.
    pub fn required_len(&self) -> usize {
        self.window + self.max_tau + 1
    }

    pub fn detect(&mut self, buf: &[f32]) -> Option<PitchEstimate> {
        if buf.len() < self.required_len() {
            return None;
        }
        let w = self.window;
        let max_tau = self.max_tau;
        let d = &mut self.cmndf;

        // Squared difference function.
        let head = &buf[..w];
        d[0] = 0.0;
        for tau in 1..=max_tau {
            d[tau] = head
                .iter()
                .zip(&buf[tau..tau + w])
                .map(|(a, b)| {
                    let x = a - b;
                    x * x
                })
                .sum();
        }

        // Cumulative mean normalization.
        d[0] = 1.0;
        let mut running = 0.0f32;
        for tau in 1..=max_tau {
            running += d[tau];
            d[tau] = if running > 0.0 {
                d[tau] * tau as f32 / running
            } else {
                1.0
            };
        }

        // First dip below the threshold, followed down to its local minimum.
        let mut tau = self.min_tau;
        let mut found = None;
        while tau <= max_tau {
            if d[tau] < self.threshold {
                while tau < max_tau && d[tau + 1] < d[tau] {
                    tau += 1;
                }
                found = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = found?;

        // Parabolic interpolation for sub-sample accuracy.
        let refined = if tau > 1 && tau < max_tau {
            let (s0, s1, s2) = (d[tau - 1], d[tau], d[tau + 1]);
            let denom = s0 + s2 - 2.0 * s1;
            if denom.abs() > 1e-12 {
                tau as f32 + 0.5 * (s0 - s2) / denom
            } else {
                tau as f32
            }
        } else {
            tau as f32
        };

        Some(PitchEstimate {
            frequency: self.sample_rate / refined,
            clarity: (1.0 - d[tau]).clamp(0.0, 1.0),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// A plucked-string-ish tone: decaying harmonics, strong upper partials.
    pub fn guitar_like(freq: f32, sample_rate: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let t = i as f32 / sample_rate;
                (1..=8)
                    .map(|h| {
                        let amp = if h == 2 { 0.9 } else { 1.0 / h as f32 };
                        amp * (TAU * freq * h as f32 * t).sin()
                    })
                    .sum::<f32>()
                    * 0.2
            })
            .collect()
    }

    fn cents(a: f32, b: f32) -> f32 {
        1200.0 * (a / b).log2()
    }

    #[test]
    fn detects_sines_across_the_neck() {
        let sr = 48_000.0;
        let mut yin = Yin::new(sr, 70.0, 1400.0, 0.15);
        for f in [82.41, 110.0, 196.0, 329.63, 659.26, 1318.5] {
            let buf: Vec<f32> = (0..yin.required_len())
                .map(|i| (TAU * f * i as f32 / sr).sin())
                .collect();
            let est = yin.detect(&buf).expect("voiced");
            assert!(cents(est.frequency, f).abs() < 2.0, "{f} -> {}", est.frequency);
        }
    }

    #[test]
    fn no_octave_error_on_harmonic_rich_low_e() {
        let sr = 44_100.0;
        let mut yin = Yin::new(sr, 70.0, 1400.0, 0.15);
        let buf = guitar_like(82.41, sr, yin.required_len());
        let est = yin.detect(&buf).expect("voiced");
        assert!(cents(est.frequency, 82.41).abs() < 3.0, "{}", est.frequency);
    }

    #[test]
    fn noise_is_unvoiced() {
        let mut yin = Yin::new(48_000.0, 70.0, 1400.0, 0.15);
        let mut x = 0x1234_5678u32;
        let buf: Vec<f32> = (0..yin.required_len())
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as f32 / u32::MAX as f32 - 0.5
            })
            .collect();
        assert!(yin.detect(&buf).is_none());
    }
}
