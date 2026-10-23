//! Realtime-safe audio effect chain (`dyn Effect`) and built-in clean guitar amp.
//!
//! Designed around block processing (`&mut [f32]`) so stateful DSP effects
//! (reverb, cabinet IR convolution) and external plugin wrappers (VST3/CLAP)
//! can slot into [`EffectChain`] without per-sample dynamic dispatch.

use std::f32::consts::TAU;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A realtime-safe mono audio effect or plugin instance in an [`EffectChain`].
///
/// Implementations must not allocate, lock, or block inside [`Effect::process`].
pub trait Effect: Send {
    /// Human-readable name of the effect (e.g. `"Clean Amp"`).
    fn name(&self) -> &str;

    /// Called when the audio stream starts or changes sample rate.
    fn prepare(&mut self, sample_rate: f32);

    /// Clear internal filter / delay-line state without changing parameters.
    fn reset(&mut self);

    /// Process a block of mono samples in-place.
    fn process(&mut self, samples: &mut [f32]);
}

/// An ordered series of [`Effect`] plugins processed in-place on the audio thread.
pub struct EffectChain {
    sample_rate: f32,
    effects: Vec<Box<dyn Effect>>,
    scratch: Vec<f32>,
    resampler: LinearResampler,
}

impl EffectChain {
    /// Create an empty chain prepared for `sample_rate` with pre-allocated block scratch space.
    pub fn new(sample_rate: f32, max_block_size: usize) -> Self {
        Self {
            sample_rate,
            effects: Vec::new(),
            scratch: Vec::with_capacity(max_block_size),
            resampler: LinearResampler::default(),
        }
    }

    /// Append an effect to the end of the signal chain and prepare it at the chain's sample rate.
    pub fn push(&mut self, mut effect: Box<dyn Effect>) {
        effect.prepare(self.sample_rate);
        self.effects.push(effect);
    }

    /// Update the sample rate for all effects in the chain.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for effect in &mut self.effects {
            effect.prepare(sample_rate);
        }
    }

    /// Reset all effects in the chain.
    pub fn reset(&mut self) {
        for effect in &mut self.effects {
            effect.reset();
        }
    }

    /// Process a slice of mono samples in-place through every effect in order.
    #[inline]
    pub fn process(&mut self, samples: &mut [f32]) {
        for effect in &mut self.effects {
            effect.process(samples);
        }
    }

    /// Pull `frames` mono samples from `ring`, run them through the effect chain,
    /// and call `emit(i, sample)` for each processed sample.
    ///
    /// The input may run at a different rate from the chain (another device,
    /// or the same one set differently), so samples are resampled from the
    /// ring's [`SampleRing::sample_rate`] to the chain's.
    ///
    /// Uses pre-allocated scratch storage up to `scratch.capacity()` chunks so
    /// the audio callback never allocates even if the host requests a large buffer.
    pub fn process_from_ring<F>(&mut self, ring: &SampleRing, total_frames: usize, mut emit: F)
    where
        F: FnMut(usize, f32),
    {
        let cap = self.scratch.capacity().max(64);
        let step = match ring.sample_rate() {
            0 => 1.0,
            rate => rate as f32 / self.sample_rate,
        };
        let mut offset = 0;
        while offset < total_frames {
            let chunk = (total_frames - offset).min(cap);
            self.scratch.clear();
            for _ in 0..chunk {
                self.scratch.push(self.resampler.next(ring, step));
            }
            for effect in &mut self.effects {
                effect.process(&mut self.scratch);
            }
            for (i, &s) in self.scratch.iter().enumerate() {
                emit(offset + i, s);
            }
            offset += chunk;
        }
    }
}

/// Reads a [`SampleRing`] at another rate by linear interpolation between
/// neighbouring input samples. Plenty for monitoring a guitar, whose energy
/// sits far below either Nyquist; the cabinet then rolls off what aliases.
#[derive(Default)]
struct LinearResampler {
    /// Position between `a` and `b`, in input samples; 1 or more pulls the next one.
    phase: f32,
    a: f32,
    b: f32,
}

impl LinearResampler {
    /// The next output sample, advancing `step` input samples (input rate / output rate).
    #[inline]
    fn next(&mut self, ring: &SampleRing, step: f32) -> f32 {
        while self.phase >= 1.0 {
            self.a = self.b;
            self.b = ring.pop();
            self.phase -= 1.0;
        }
        let out = self.a + (self.b - self.a) * self.phase;
        self.phase += step;
        out
    }
}

/// Lock-free single-producer, single-consumer (SPSC) ring buffer for bridging
/// the input stream callback to the output stream callback.
///
/// Bounded to a small latency window: if the input callback runs ahead of the
/// output callback (e.g. while output is starting up or due to clock drift),
/// older samples are dropped so monitoring latency never builds up.
pub struct SampleRing {
    buffer: Box<[UnsafeCell<f32>]>,
    mask: usize,
    max_latency: usize,
    write_idx: AtomicUsize,
    read_idx: AtomicUsize,
    /// The producer's sample rate in Hz, or 0 while there is none.
    sample_rate: AtomicU32,
}

// SAFETY: `SampleRing` is a lock-free SPSC queue where only the producer writes
// to slots before advancing `write_idx` (Release) and only the consumer reads
// slots after observing `write_idx` (Acquire).
unsafe impl Sync for SampleRing {}
unsafe impl Send for SampleRing {}

impl SampleRing {
    /// Create a ring buffer with `capacity` (rounded up to a power of two) and
    /// a maximum queued latency of `max_latency` samples before old samples are skipped.
    pub fn new(capacity: usize, max_latency: usize) -> Self {
        let cap = capacity.max(64).next_power_of_two();
        let buffer = (0..cap)
            .map(|_| UnsafeCell::new(0.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            buffer,
            mask: cap - 1,
            max_latency: max_latency.clamp(32, cap - 1),
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
            sample_rate: AtomicU32::new(0),
        }
    }

    /// Push one mono sample from the input callback.
    #[inline]
    pub fn push(&self, sample: f32) {
        let w = self.write_idx.load(Ordering::Relaxed);
        let r = self.read_idx.load(Ordering::Acquire);
        // Leave one slot open so full vs. empty is unambiguous.
        if w.wrapping_sub(r) >= self.mask {
            return;
        }
        unsafe {
            *self.buffer[w & self.mask].get() = sample;
        }
        self.write_idx.store(w.wrapping_add(1), Ordering::Release);
    }

    /// Pop one mono sample from the output callback, returning `0.0` on underrun.
    #[inline]
    pub fn pop(&self) -> f32 {
        let w = self.write_idx.load(Ordering::Acquire);
        let mut r = self.read_idx.load(Ordering::Relaxed);
        let avail = w.wrapping_sub(r);
        if avail == 0 {
            return 0.0;
        }
        // If too much audio queued up (e.g. output started after input), jump
        // forward to keep live monitoring latency tight.
        if avail > self.max_latency {
            r = w.wrapping_sub(self.max_latency / 2);
        }
        let sample = unsafe { *self.buffer[r & self.mask].get() };
        self.read_idx.store(r.wrapping_add(1), Ordering::Release);
        sample
    }

    /// Record the rate the producer pushes at, so the consumer can resample.
    pub fn set_sample_rate(&self, hz: u32) {
        self.sample_rate.store(hz, Ordering::Relaxed);
    }

    /// The producer's sample rate in Hz, or 0 if none was set.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Discard all queued samples.
    pub fn clear(&self) {
        let w = self.write_idx.load(Ordering::Acquire);
        self.read_idx.store(w, Ordering::Release);
    }
}

/// Serializable user settings for the built-in amp and cabinet chain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AmpConfig {
    /// Whether the clean amp head is active.
    pub enabled: bool,
    /// Preamp drive / input gain in dB (`0.0..=24.0`).
    pub gain_db: f32,
    /// Tone control (`-5.0..=5.0`, where 0 is balanced, negative is warm, positive is bright).
    pub tone: f32,
    /// Output level in dB (`-24.0..=6.0`).
    pub level_db: f32,
    /// Whether the 1×12" speaker cabinet simulation is active.
    pub cab_enabled: bool,
    /// Cabinet mic placement (`-5.0..=5.0`: negative is warm cone edge, positive is bright cap).
    pub cab_mic: f32,
    /// Final master output gain in dB (`-24.0..=6.0`), applied after the entire effect chain.
    pub out_gain_db: f32,
}

pub const AMP_GAIN_RANGE_DB: std::ops::RangeInclusive<i32> = 0..=24;
pub const AMP_TONE_RANGE: std::ops::RangeInclusive<i32> = -5..=5;
pub const AMP_LEVEL_RANGE_DB: std::ops::RangeInclusive<i32> = -24..=6;
pub const CAB_MIC_RANGE: std::ops::RangeInclusive<i32> = -5..=5;
pub const OUT_GAIN_RANGE_DB: std::ops::RangeInclusive<f32> = -24.0..=6.0;

impl Default for AmpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gain_db: 6.0,
            tone: 0.0,
            level_db: -6.0,
            cab_enabled: true,
            cab_mic: 0.0,
            out_gain_db: 0.0,
        }
    }
}

impl AmpConfig {
    /// Replaces anything out of range (a hand-edited or older save) with its default.
    pub fn sanitized(self) -> Self {
        let d = Self::default();
        let within = |v: f32, r: std::ops::RangeInclusive<i32>, default: f32| {
            if (*r.start() as f32..=*r.end() as f32).contains(&v) {
                v
            } else {
                default
            }
        };
        Self {
            gain_db: within(self.gain_db, AMP_GAIN_RANGE_DB, d.gain_db),
            tone: within(self.tone, AMP_TONE_RANGE, d.tone),
            level_db: within(self.level_db, AMP_LEVEL_RANGE_DB, d.level_db),
            cab_mic: within(self.cab_mic, CAB_MIC_RANGE, d.cab_mic),
            out_gain_db: if OUT_GAIN_RANGE_DB.contains(&self.out_gain_db) { self.out_gain_db } else { d.out_gain_db },
            ..self
        }
    }
}

/// Lock-free parameter mailbox between the GUI thread and the audio effect chain.
pub struct SharedAmp {
    enabled: AtomicBool,
    gain_db: AtomicU32,
    tone: AtomicU32,
    level_db: AtomicU32,
    cab_enabled: AtomicBool,
    cab_mic: AtomicU32,
    out_gain_db: AtomicU32,
    out_level_db: AtomicU32,
}

impl Default for SharedAmp {
    fn default() -> Self {
        let cfg = AmpConfig::default();
        Self {
            enabled: AtomicBool::new(cfg.enabled),
            gain_db: AtomicU32::new(cfg.gain_db.to_bits()),
            tone: AtomicU32::new(cfg.tone.to_bits()),
            level_db: AtomicU32::new(cfg.level_db.to_bits()),
            cab_enabled: AtomicBool::new(cfg.cab_enabled),
            cab_mic: AtomicU32::new(cfg.cab_mic.to_bits()),
            out_gain_db: AtomicU32::new(cfg.out_gain_db.to_bits()),
            out_level_db: AtomicU32::new((-100.0_f32).to_bits()),
        }
    }
}

impl SharedAmp {
    pub fn store(&self, cfg: AmpConfig) {
        let cfg = cfg.sanitized();
        self.enabled.store(cfg.enabled, Ordering::Relaxed);
        self.gain_db.store(cfg.gain_db.to_bits(), Ordering::Relaxed);
        self.tone.store(cfg.tone.to_bits(), Ordering::Relaxed);
        self.level_db.store(cfg.level_db.to_bits(), Ordering::Relaxed);
        self.cab_enabled.store(cfg.cab_enabled, Ordering::Relaxed);
        self.cab_mic.store(cfg.cab_mic.to_bits(), Ordering::Relaxed);
        self.out_gain_db.store(cfg.out_gain_db.to_bits(), Ordering::Relaxed);
    }

    pub fn load(&self) -> AmpConfig {
        AmpConfig {
            enabled: self.enabled.load(Ordering::Relaxed),
            gain_db: f32::from_bits(self.gain_db.load(Ordering::Relaxed)),
            tone: f32::from_bits(self.tone.load(Ordering::Relaxed)),
            level_db: f32::from_bits(self.level_db.load(Ordering::Relaxed)),
            cab_enabled: self.cab_enabled.load(Ordering::Relaxed),
            cab_mic: f32::from_bits(self.cab_mic.load(Ordering::Relaxed)),
            out_gain_db: f32::from_bits(self.out_gain_db.load(Ordering::Relaxed)),
        }
    }

    pub fn set_out_level_db(&self, db: f32) {
        self.out_level_db.store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn out_level_db(&self) -> f32 {
        f32::from_bits(self.out_level_db.load(Ordering::Relaxed))
    }
}

/// Direct-Form II Transposed 2nd-order biquad filter.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    fn set_normalized(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        let inv_a0 = 1.0 / a0;
        self.b0 = b0 * inv_a0;
        self.b1 = b1 * inv_a0;
        self.b2 = b2 * inv_a0;
        self.a1 = a1 * inv_a0;
        self.a2 = a2 * inv_a0;
    }

    fn set_highpass(&mut self, sr: f32, freq: f32, q: f32) {
        let w0 = TAU * freq.clamp(10.0, sr * 0.45) / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = (1.0 + cos_w0) * 0.5;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }

    fn set_lowpass(&mut self, sr: f32, freq: f32, q: f32) {
        let w0 = TAU * freq.clamp(40.0, sr * 0.45) / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = (1.0 - cos_w0) * 0.5;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }

    fn set_peaking(&mut self, sr: f32, freq: f32, q: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = TAU * freq.clamp(20.0, sr * 0.45) / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }

    fn set_high_shelf(&mut self, sr: f32, freq: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = TAU * freq.clamp(100.0, sr * 0.45) / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 * 0.5 * std::f32::consts::SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }
}

/// Built-in American-voiced clean amp head (preamp + tone stack + tube stage).
///
/// Signal path:
/// 1. Input sub-cut HPF (75 Hz)
/// 2. Preamp tone stack (warm low-mid body at 180 Hz, gentle Fender-style 520 Hz scoop,
///    and user-adjustable high-shelf `Tone` control around 1.8 kHz)
/// 3. Asymmetric tube-like soft saturation stage with DC blocker
///
/// Pair with [`Cabinet`] in an [`EffectChain`] for a complete amp + speaker rig.
pub struct CleanAmp {
    shared: Arc<SharedAmp>,
    sample_rate: f32,
    last_tone: f32,
    was_enabled: bool,
    in_hpf: Biquad,
    pre_warmth: Biquad,
    pre_scoop: Biquad,
    tone_shelf: Biquad,
    dc_block_x1: f32,
    dc_block_y1: f32,
}

impl CleanAmp {
    pub fn new(sample_rate: f32, shared: Arc<SharedAmp>) -> Self {
        let mut amp = Self {
            shared,
            sample_rate,
            last_tone: f32::NAN,
            was_enabled: false,
            in_hpf: Biquad::default(),
            pre_warmth: Biquad::default(),
            pre_scoop: Biquad::default(),
            tone_shelf: Biquad::default(),
            dc_block_x1: 0.0,
            dc_block_y1: 0.0,
        };
        amp.prepare(sample_rate);
        amp
    }

    fn update_tone(&mut self, tone: f32) {
        if self.last_tone != tone {
            self.last_tone = tone;
            // Each step of Tone (-5..+5) shifts treble by ~1.5 dB above 1.8 kHz.
            self.tone_shelf.set_high_shelf(self.sample_rate, 1800.0, tone * 1.5);
        }
    }
}

impl Effect for CleanAmp {
    fn name(&self) -> &str {
        "Clean Amp"
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
        self.in_hpf.set_highpass(self.sample_rate, 75.0, 0.707);
        self.pre_warmth.set_peaking(self.sample_rate, 180.0, 0.9, 2.5);
        self.pre_scoop.set_peaking(self.sample_rate, 520.0, 1.1, -3.5);
        self.last_tone = f32::NAN;
        let cfg = self.shared.load();
        self.update_tone(cfg.tone);
    }

    fn reset(&mut self) {
        self.in_hpf.reset();
        self.pre_warmth.reset();
        self.pre_scoop.reset();
        self.tone_shelf.reset();
        self.dc_block_x1 = 0.0;
        self.dc_block_y1 = 0.0;
    }

    fn process(&mut self, samples: &mut [f32]) {
        let cfg = self.shared.load();
        if !cfg.enabled {
            if self.was_enabled {
                self.reset();
                self.was_enabled = false;
            }
            samples.fill(0.0);
            return;
        }
        self.was_enabled = true;
        self.update_tone(cfg.tone);

        let drive = 10.0_f32.powf(cfg.gain_db / 20.0);
        // Compensate slightly as drive increases so turning up Gain mostly adds
        // compression/warmth rather than blowing out headphone volume.
        let drive_comp = 1.0 / drive.powf(0.45);
        let out_gain = 10.0_f32.powf(cfg.level_db / 20.0) * drive_comp;

        for s in samples.iter_mut() {
            let mut x = self.in_hpf.process(*s);
            x = self.pre_warmth.process(x);
            x = self.pre_scoop.process(x);
            x = self.tone_shelf.process(x);

            // Asymmetric triode-style soft clipping: adds subtle even-order harmonics
            // on clean notes and smooth edge-of-breakup warmth when pushed.
            let driven = x * drive;
            let biased = driven + 0.08 * driven * driven;
            let shaped = biased / (1.0 + biased.abs());

            // 1st-order DC blocker after asymmetric waveshaping
            let dc = shaped - self.dc_block_x1 + 0.995 * self.dc_block_y1;
            self.dc_block_x1 = shaped;
            self.dc_block_y1 = dc;

            *s = (dc * out_gain).clamp(-1.0, 1.0);
        }
    }
}

/// 1x12" open-back guitar speaker cabinet simulation (`Effect` plugin).
///
/// Models the frequency response of a guitar speaker in an open-back cabinet:
/// - 90 Hz resonant low-frequency thump ($Q = 1.15$)
/// - Cone breakup / presence peak around 2.35 kHz (tunable via `cab_mic`)
/// - Steep 24 dB/octave roll-off above 4.2 kHz to remove harsh DI fizz
pub struct Cabinet {
    shared: Arc<SharedAmp>,
    sample_rate: f32,
    last_mic: f32,
    was_enabled: bool,
    resonance: Biquad,
    presence: Biquad,
    lpf1: Biquad,
    lpf2: Biquad,
}

impl Cabinet {
    pub fn new(sample_rate: f32, shared: Arc<SharedAmp>) -> Self {
        let mut cab = Self {
            shared,
            sample_rate,
            last_mic: f32::NAN,
            was_enabled: true,
            resonance: Biquad::default(),
            presence: Biquad::default(),
            lpf1: Biquad::default(),
            lpf2: Biquad::default(),
        };
        cab.prepare(sample_rate);
        cab
    }

    fn update_mic(&mut self, mic: f32) {
        if self.last_mic != mic {
            self.last_mic = mic;
            let sr = self.sample_rate;
            // Moving the mic from cone edge (-5) to dust cap (+5) brightens presence and top-end rolloff.
            self.presence.set_peaking(sr, 2250.0 + mic * 90.0, 1.35, 3.0 + mic * 0.45);
            self.lpf1.set_lowpass(sr, 4200.0 + mic * 220.0, 0.85);
            self.lpf2.set_lowpass(sr, 5400.0 + mic * 260.0, 0.707);
        }
    }
}

impl Effect for Cabinet {
    fn name(&self) -> &str {
        "1x12 Cabinet"
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
        self.resonance.set_highpass(self.sample_rate, 90.0, 1.15);
        self.last_mic = f32::NAN;
        let cfg = self.shared.load();
        self.update_mic(cfg.cab_mic);
    }

    fn reset(&mut self) {
        self.resonance.reset();
        self.presence.reset();
        self.lpf1.reset();
        self.lpf2.reset();
    }

    fn process(&mut self, samples: &mut [f32]) {
        let cfg = self.shared.load();
        if !cfg.cab_enabled {
            if self.was_enabled {
                self.reset();
                self.was_enabled = false;
            }
            return;
        }
        self.was_enabled = true;
        self.update_mic(cfg.cab_mic);
        for s in samples.iter_mut() {
            let mut y = self.resonance.process(*s);
            y = self.presence.process(y);
            y = self.lpf1.process(y);
            y = self.lpf2.process(y);
            *s = y.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct GainEffect(f32);
    impl Effect for GainEffect {
        fn name(&self) -> &str {
            "Gain"
        }
        fn prepare(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn process(&mut self, samples: &mut [f32]) {
            for s in samples {
                *s *= self.0;
            }
        }
    }

    #[test]
    fn effect_chain_runs_plugins_in_order() {
        let mut chain = EffectChain::new(48_000.0, 128);
        chain.push(Box::new(GainEffect(2.0)));
        chain.push(Box::new(GainEffect(0.25)));

        let mut buf = [1.0, -0.5, 0.2];
        chain.process(&mut buf);
        assert_eq!(buf, [0.5, -0.25, 0.1]);
    }

    #[test]
    fn sample_ring_bounds_latency_and_handles_underrun() {
        let ring = SampleRing::new(128, 32);
        assert_eq!(ring.pop(), 0.0);
        for i in 1..=64 {
            ring.push(i as f32);
        }
        // Because 64 > max_latency (32), pop jumps forward to keep latency tight.
        let first = ring.pop();
        assert!(first > 30.0, "expected old samples to be skipped, got {first}");
    }

    #[test]
    fn chain_resamples_from_the_ring_rate() {
        // A 24 kHz ramp read by a 48 kHz chain: twice the samples, halfway
        // points interpolated, so a 1 kHz tone keeps its pitch.
        let ring = SampleRing::new(256, 200);
        ring.set_sample_rate(24_000);
        for i in 0..64 {
            ring.push(i as f32);
        }
        let mut chain = EffectChain::new(48_000.0, 128);
        let mut out = Vec::new();
        chain.process_from_ring(&ring, 40, |_, s| out.push(s));
        // The first few interpolate from the silence before the ramp's 0.
        let ramp: Vec<f32> = out[4..].windows(2).map(|w| w[1] - w[0]).collect();
        assert!(ramp.iter().all(|&d| (d - 0.5).abs() < 1e-6), "{out:?}");

        // Same rates pass straight through.
        ring.set_sample_rate(48_000);
        ring.clear();
        for i in 0..8 {
            ring.push(i as f32);
        }
        let mut out = Vec::new();
        chain.process_from_ring(&ring, 6, |_, s| out.push(s));
        assert!(out[2..].windows(2).all(|w| w[1] - w[0] == 1.0), "{out:?}");
    }

    #[test]
    fn clean_amp_silences_when_disabled_and_shapes_when_enabled() {
        let shared = Arc::new(SharedAmp::default());
        let mut chain = EffectChain::new(48_000.0, 256);
        chain.push(Box::new(CleanAmp::new(48_000.0, shared.clone())));

        let mut buf: Vec<f32> = (0..480).map(|i| (TAU * 220.0 * i as f32 / 48_000.0).sin() * 0.5).collect();
        chain.process(&mut buf);
        assert!(buf.iter().all(|&s| s == 0.0));

        shared.store(AmpConfig {
            enabled: true,
            gain_db: 6.0,
            tone: 0.0,
            level_db: 0.0,
            ..Default::default()
        });
        let mut buf: Vec<f32> = (0..2400).map(|i| (TAU * 220.0 * i as f32 / 48_000.0).sin() * 0.4).collect();
        chain.process(&mut buf);
        let peak = buf[400..].iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05 && peak <= 1.0, "unexpected peak {peak}");
    }

    #[test]
    fn cabinet_attenuates_harsh_high_frequencies() {
        let sr = 48_000.0;
        let shared = Arc::new(SharedAmp::default());
        let mut cab = Cabinet::new(sr, shared);

        let mut mid_note: Vec<f32> = (0..2400).map(|i| (TAU * 330.0 * i as f32 / sr).sin() * 0.2).collect();
        cab.process(&mut mid_note);
        let mid_rms = (mid_note[800..].iter().map(|x| x * x).sum::<f32>() / 1600.0).sqrt();

        cab.reset();
        let mut fizz: Vec<f32> = (0..2400).map(|i| (TAU * 10_000.0 * i as f32 / sr).sin() * 0.2).collect();
        cab.process(&mut fizz);
        let fizz_rms = (fizz[800..].iter().map(|x| x * x).sum::<f32>() / 1600.0).sqrt();

        assert!(
            fizz_rms < mid_rms * 0.2,
            "expected cabinet roll-off to strongly attenuate 10 kHz fizz (mid={mid_rms}, fizz={fizz_rms})"
        );
    }
}
