//! Stereo bucket-brigade (BBD) chorus in the spirit of late-70s analog units.
//!
//! One delay line read by two taps whose triangle LFOs run in opposite
//! phase, so the left and right copies drift apart and back. Around the delay
//! sit what gives a BBD its character: band-limiting filters on the way in
//! and out, gentle saturation in the compander, and a little hiss.

use std::sync::Arc;

use crate::fx::{Biquad, SharedAmp, StereoEffect};
use crate::rng::Rng;

/// Shortest delay either tap reaches, in seconds.
const MIN_DELAY_S: f32 = 0.0016;
/// Longest delay the line must hold (base plus the deepest sweep), with room
/// for interpolation, in seconds.
const MAX_DELAY_S: f32 = 0.012;
/// BBD clock-limited bandwidth: the anti-alias and reconstruction filters.
const BBD_CUTOFF_HZ: f32 = 6_000.0;

/// Stereo vintage chorus, controlled by the `chorus_*` fields of
/// [`crate::AmpConfig`].
pub struct VintageChorus {
    shared: Arc<SharedAmp>,
    sample_rate: f32,
    line: Vec<f32>,
    write: usize,
    /// LFO position, 0..1.
    phase: f32,
    /// Smoothed parameters, so turning a knob or bypassing doesn't click.
    rate_hz: f32,
    sweep_s: f32,
    wet: f32,
    pre: [Biquad; 2],
    post_left: [Biquad; 2],
    post_right: [Biquad; 2],
    rng: Rng,
}

impl VintageChorus {
    pub fn new(sample_rate: f32, shared: Arc<SharedAmp>) -> Self {
        let mut chorus = Self {
            shared,
            sample_rate,
            line: Vec::new(),
            write: 0,
            phase: 0.0,
            rate_hz: 0.0,
            sweep_s: 0.0,
            wet: 0.0,
            pre: [Biquad::default(); 2],
            post_left: [Biquad::default(); 2],
            post_right: [Biquad::default(); 2],
            rng: Rng::new(0x5eed_c0de),
        };
        chorus.prepare(sample_rate);
        chorus
    }

    /// The LFO rate for a Rate step (1..=10): exponential, 0.15 Hz to 6 Hz.
    pub fn rate_hz(step: f32) -> f32 {
        0.15 * 40.0_f32.powf((step - 1.0) / 9.0)
    }

    /// How far each tap sweeps either side of its centre for a Depth step
    /// (0..=10), in seconds.
    fn sweep_s(step: f32) -> f32 {
        0.0004 * step
    }

    /// The line read `delay` samples behind the write head, by cubic
    /// (Hermite) interpolation.
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let len = self.line.len();
        let pos = self.write as f32 - delay + len as f32;
        let i = pos.floor();
        let t = pos - i;
        let i = i as usize;
        let at = |k: usize| self.line[(i + len - 1 + k) % len];
        let (y0, y1, y2, y3) = (at(0), at(1), at(2), at(3));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }
}

/// `x` through both filters of a 4-pole pair.
#[inline]
fn cascade([a, b]: &mut [Biquad; 2], x: f32) -> f32 {
    b.process(a.process(x))
}

/// Symmetric triangle over one cycle of `phase` (0..1), from -1 up to 1 and back.
#[inline]
fn triangle(phase: f32) -> f32 {
    let p = phase.fract();
    4.0 * (p - 0.5).abs() - 1.0
}

impl StereoEffect for VintageChorus {
    fn name(&self) -> &str {
        "Vintage Chorus"
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
        let len = (MAX_DELAY_S * self.sample_rate) as usize + 8;
        self.line = vec![0.0; len];
        self.write = 0;
        for f in self.pre.iter_mut().chain(&mut self.post_left).chain(&mut self.post_right) {
            f.set_lowpass(self.sample_rate, BBD_CUTOFF_HZ, 0.707);
        }
        let cfg = self.shared.load();
        self.rate_hz = Self::rate_hz(cfg.chorus_rate);
        self.sweep_s = Self::sweep_s(cfg.chorus_depth);
    }

    fn reset(&mut self) {
        self.line.fill(0.0);
        for f in self.pre.iter_mut().chain(&mut self.post_left).chain(&mut self.post_right) {
            f.reset();
        }
        self.wet = 0.0;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let cfg = self.shared.load();
        let wet_target = if cfg.chorus_enabled { cfg.chorus_mix / 10.0 } else { 0.0 };
        if wet_target == 0.0 && self.wet < 1e-4 {
            if self.wet != 0.0 {
                self.reset();
            }
            return;
        }
        let sr = self.sample_rate;
        // ~20 ms parameter glide.
        let glide = 1.0 - (-1.0 / (0.02 * sr)).exp();
        let rate_target = Self::rate_hz(cfg.chorus_rate);
        let sweep_target = Self::sweep_s(cfg.chorus_depth);

        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.rate_hz += (rate_target - self.rate_hz) * glide;
            self.sweep_s += (sweep_target - self.sweep_s) * glide;
            self.wet += (wet_target - self.wet) * glide;

            // Into the bucket brigade: band-limited, softly compressed, with hiss.
            let dry = 0.5 * (*l + *r);
            let mut x = cascade(&mut self.pre, dry);
            x = x / (1.0 + 0.3 * x.abs());
            x += (self.rng.next_f64() as f32 - 0.5) * 1.2e-4;
            self.line[self.write] = x;

            let centre = MIN_DELAY_S + self.sweep_s;
            let delay_l = (centre + self.sweep_s * triangle(self.phase)) * sr;
            let delay_r = (centre + self.sweep_s * triangle(self.phase + 0.5)) * sr;
            let wet_l = self.read(delay_l);
            let wet_l = cascade(&mut self.post_left, wet_l);
            let wet_r = self.read(delay_r);
            let wet_r = cascade(&mut self.post_right, wet_r);

            self.write = (self.write + 1) % self.line.len();
            self.phase = (self.phase + self.rate_hz / sr).fract();

            // At full mix, dry and wet sit about level, as on the classic units.
            let dry_gain = 1.0 - 0.3 * self.wet;
            let wet_gain = 0.75 * self.wet;
            *l = (*l * dry_gain + wet_l * wet_gain).clamp(-1.0, 1.0);
            *r = (*r * dry_gain + wet_r * wet_gain).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AmpConfig;
    use std::f32::consts::TAU;

    fn sine(sr: f32, hz: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (TAU * hz * i as f32 / sr).sin() * 0.4).collect()
    }

    #[test]
    fn bypassed_chorus_passes_signal_untouched() {
        let shared = Arc::new(SharedAmp::default());
        let mut chorus = VintageChorus::new(48_000.0, shared);
        let mut left = sine(48_000.0, 220.0, 1024);
        let mut right = left.clone();
        let dry = left.clone();
        chorus.process(&mut left, &mut right);
        assert_eq!(left, dry);
        assert_eq!(right, dry);
    }

    #[test]
    fn chorus_widens_a_mono_signal_into_stereo() {
        let sr = 48_000.0;
        let shared = Arc::new(SharedAmp::default());
        shared.store(AmpConfig { chorus_enabled: true, chorus_depth: 8.0, chorus_mix: 10.0, ..Default::default() });
        let mut chorus = VintageChorus::new(sr, shared);
        let mut left = sine(sr, 330.0, 48_000);
        let mut right = left.clone();
        chorus.process(&mut left, &mut right);

        let tail = 4_800..;
        let diff = left[tail.clone()].iter().zip(&right[tail.clone()]).map(|(l, r)| (l - r).powi(2)).sum::<f32>();
        let energy = left[tail.clone()].iter().map(|l| l * l).sum::<f32>();
        assert!(diff / energy > 0.01, "left and right should differ (ratio {})", diff / energy);
        assert!(left.iter().chain(&right).all(|s| s.is_finite() && s.abs() <= 1.0));
    }

    #[test]
    fn rate_steps_span_the_vintage_range() {
        assert!((VintageChorus::rate_hz(1.0) - 0.15).abs() < 1e-4);
        assert!((VintageChorus::rate_hz(10.0) - 6.0).abs() < 1e-3);
    }
}
