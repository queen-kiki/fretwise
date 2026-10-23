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

/// A realtime-safe stereo effect, run after every mono [`Effect`] in an
/// [`EffectChain`]. The first one receives the mono signal on both sides.
///
/// The same rules as [`Effect::process`] apply: no allocating, locking or blocking.
pub trait StereoEffect: Send {
    /// Human-readable name of the effect (e.g. `"Vintage Chorus"`).
    fn name(&self) -> &str;

    /// Called when the audio stream starts or changes sample rate.
    fn prepare(&mut self, sample_rate: f32);

    /// Clear internal filter / delay-line state without changing parameters.
    fn reset(&mut self);

    /// Process a block of left and right samples (of equal length) in-place.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
}

/// An ordered series of [`Effect`] plugins processed in-place on the audio
/// thread: the mono effects first, then the [`StereoEffect`]s.
pub struct EffectChain {
    sample_rate: f32,
    effects: Vec<Box<dyn Effect>>,
    stereo: Vec<Box<dyn StereoEffect>>,
    scratch: Vec<f32>,
    scratch_right: Vec<f32>,
    reader: RingReader,
}

impl EffectChain {
    /// Create an empty chain prepared for `sample_rate` with pre-allocated block scratch space.
    pub fn new(sample_rate: f32, max_block_size: usize) -> Self {
        Self {
            sample_rate,
            effects: Vec::new(),
            stereo: Vec::new(),
            scratch: Vec::with_capacity(max_block_size),
            scratch_right: Vec::with_capacity(max_block_size),
            reader: RingReader::default(),
        }
    }

    /// Append an effect to the end of the signal chain and prepare it at the chain's sample rate.
    pub fn push(&mut self, mut effect: Box<dyn Effect>) {
        effect.prepare(self.sample_rate);
        self.effects.push(effect);
    }

    /// Append a stereo effect after every mono one (and the stereo ones
    /// already added) and prepare it at the chain's sample rate.
    pub fn push_stereo(&mut self, mut effect: Box<dyn StereoEffect>) {
        effect.prepare(self.sample_rate);
        self.stereo.push(effect);
    }

    /// Update the sample rate for all effects in the chain.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for effect in &mut self.effects {
            effect.prepare(sample_rate);
        }
        for effect in &mut self.stereo {
            effect.prepare(sample_rate);
        }
    }

    /// Reset all effects in the chain.
    pub fn reset(&mut self) {
        for effect in &mut self.effects {
            effect.reset();
        }
        for effect in &mut self.stereo {
            effect.reset();
        }
    }

    /// Process a slice of mono samples in-place through every mono effect in
    /// order (the stereo ones run in [`EffectChain::process_from_ring`]).
    #[inline]
    pub fn process(&mut self, samples: &mut [f32]) {
        for effect in &mut self.effects {
            effect.process(samples);
        }
    }

    /// Pull `total_frames` mono samples from `ring`, run them through the
    /// effect chain, and call `emit(i, left, right)` for each processed frame.
    ///
    /// The input runs on its own clock, often at another rate (another
    /// device, or ASIO at 44.1 kHz into a 48 kHz output), so a [`RingReader`]
    /// resamples from the ring's [`SampleRing::sample_rate`] to the chain's
    /// and keeps a jitter cushion between the two callbacks.
    ///
    /// Uses pre-allocated scratch storage up to `scratch.capacity()` chunks so
    /// the audio callback never allocates even if the host requests a large buffer.
    pub fn process_from_ring<F>(&mut self, ring: &SampleRing, total_frames: usize, mut emit: F)
    where
        F: FnMut(usize, f32, f32),
    {
        let cap = self.scratch.capacity().max(64);
        let step = self.reader.plan(ring, total_frames, self.sample_rate);
        let mut offset = 0;
        while offset < total_frames {
            let chunk = (total_frames - offset).min(cap);
            self.scratch.clear();
            match step {
                Some(step) => self.scratch.extend((0..chunk).map(|_| self.reader.resampler.next(ring, step))),
                None => self.scratch.resize(chunk, 0.0),
            }
            for effect in &mut self.effects {
                effect.process(&mut self.scratch);
            }
            self.scratch_right.clear();
            self.scratch_right.extend_from_slice(&self.scratch);
            for effect in &mut self.stereo {
                effect.process(&mut self.scratch, &mut self.scratch_right);
            }
            for (i, (&l, &r)) in self.scratch.iter().zip(&self.scratch_right).enumerate() {
                emit(offset + i, l, r);
            }
            offset += chunk;
        }
    }
}

/// The output side of a [`SampleRing`]: keeps just enough input queued to
/// ride out how the two callbacks actually line up, and no more.
///
/// - It watches the headroom each output block starts with (queued input
///   beyond what the block reads) and steers its low point to a small jitter
///   margin. Two callbacks driven by one clock (ASIO, or one interface's
///   WASAPI in and out) settle near the margin alone; independent ones keep
///   whatever extra their phase drift and block sizes need.
/// - It steers by nudging the resampling rate (by up to 1%, and far less
///   once settled), which also absorbs clock drift, rather than skipping.
/// - Starting up, or after running dry, it plays silence until the cushion
///   has built up again, rather than reading the ring empty mid-block, which
///   tears holes in the waveform.
/// - The margin starts at 2 ms and grows whenever a block starts nearly
///   drained (or runs dry), so a busy machine settles at what it needs.
#[derive(Default)]
struct RingReader {
    resampler: LinearResampler,
    playing: bool,
    /// The lowest recent headroom (input samples): drops at once, recovers slowly.
    low: f32,
    /// Extra queue for scheduling jitter, in seconds; 0 until first planned.
    margin: f32,
    /// Blocks to wait after growing the margin, while the queue fills to match.
    cooldown: u32,
}

impl RingReader {
    /// The resampling step for the next `frames` output samples, or `None`
    /// to play silence while the cushion builds.
    fn plan(&mut self, ring: &SampleRing, frames: usize, out_rate: f32) -> Option<f32> {
        let in_rate = match ring.sample_rate() {
            0 => return None,
            rate => rate as f32,
        };
        let base = in_rate / out_rate;
        if self.margin == 0.0 {
            self.margin = 0.002;
        }
        let margin = self.margin * in_rate;
        let avail = ring.available() as f32;
        // What this block reads, at the fastest the steering may run.
        let needed = frames as f32 * base * 1.01 + 2.0;

        if !self.playing {
            // Start with room for one input block of swing; steering trims
            // whatever turns out not to be needed.
            if avail < needed + ring.block() as f32 + margin {
                return None;
            }
            self.playing = true;
            self.low = avail - needed;
            self.resampler = LinearResampler::default();
        } else if avail < needed {
            // Ran dry: drop out cleanly and rebuild a larger cushion.
            self.playing = false;
            self.margin = (self.margin * 1.5).min(0.05);
            return None;
        }
        let headroom = avail - needed;
        self.cooldown = self.cooldown.saturating_sub(1);
        if self.cooldown == 0 && headroom < margin * 0.5 {
            // A near miss: grow the margin before it becomes a dropout.
            self.margin = (self.margin * 1.25).min(0.05);
            self.cooldown = 20;
        }
        // Track the low point: fall straight to a new minimum, rise over ~2 s
        // so one early block doesn't hold the latency up forever.
        let release = frames as f32 / out_rate / 2.0;
        self.low = headroom.min(self.low + (headroom - self.low) * release);
        // Correct the excess over about half a second, by up to 1%.
        let excess = self.low - self.margin * in_rate;
        Some(base * (1.0 + (excess / (in_rate * 0.5)).clamp(-0.01, 0.01)))
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
/// the input stream callback to the output stream callback. How much to keep
/// queued is up to the reader (see [`RingReader`]); a full ring drops new
/// samples.
pub struct SampleRing {
    buffer: Box<[UnsafeCell<f32>]>,
    mask: usize,
    write_idx: AtomicUsize,
    read_idx: AtomicUsize,
    /// The producer's sample rate in Hz, or 0 while there is none.
    sample_rate: AtomicU32,
    /// The producer's callback block size in samples, for sizing the cushion.
    block: AtomicUsize,
}

// SAFETY: `SampleRing` is a lock-free SPSC queue where only the producer writes
// to slots before advancing `write_idx` (Release) and only the consumer reads
// slots after observing `write_idx` (Acquire).
unsafe impl Sync for SampleRing {}
unsafe impl Send for SampleRing {}

impl SampleRing {
    /// Create a ring buffer holding `capacity` samples (rounded up to a power of two).
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(64).next_power_of_two();
        let buffer = (0..cap)
            .map(|_| UnsafeCell::new(0.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            buffer,
            mask: cap - 1,
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
            sample_rate: AtomicU32::new(0),
            block: AtomicUsize::new(0),
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
        let r = self.read_idx.load(Ordering::Relaxed);
        if w == r {
            return 0.0;
        }
        let sample = unsafe { *self.buffer[r & self.mask].get() };
        self.read_idx.store(r.wrapping_add(1), Ordering::Release);
        sample
    }

    /// Samples queued for the consumer.
    pub fn available(&self) -> usize {
        let w = self.write_idx.load(Ordering::Acquire);
        w.wrapping_sub(self.read_idx.load(Ordering::Relaxed))
    }

    /// Drop up to `n` of the oldest queued samples (consumer side).
    pub fn skip(&self, n: usize) {
        let r = self.read_idx.load(Ordering::Relaxed);
        self.read_idx.store(r.wrapping_add(n.min(self.available())), Ordering::Release);
    }

    /// Record the rate the producer pushes at, so the consumer can resample.
    pub fn set_sample_rate(&self, hz: u32) {
        self.sample_rate.store(hz, Ordering::Relaxed);
        self.block.store(0, Ordering::Relaxed);
    }

    /// The producer's sample rate in Hz, or 0 if none was set.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Record a producer callback's block size; the ring keeps the largest seen.
    pub fn note_block(&self, samples: usize) {
        self.block.fetch_max(samples, Ordering::Relaxed);
    }

    /// The largest producer block seen since the rate was set.
    pub fn block(&self) -> usize {
        self.block.load(Ordering::Relaxed)
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
    /// Whether the stereo vintage chorus is active.
    pub chorus_enabled: bool,
    /// Chorus LFO rate step (`1.0..=10.0`, about 0.15 Hz to 6 Hz).
    pub chorus_rate: f32,
    /// Chorus modulation depth (`0.0..=10.0`).
    pub chorus_depth: f32,
    /// Chorus wet/dry mix (`0.0..=10.0`, where 10 is the classic even blend).
    pub chorus_mix: f32,
}

pub const AMP_GAIN_RANGE_DB: std::ops::RangeInclusive<i32> = 0..=24;
pub const AMP_TONE_RANGE: std::ops::RangeInclusive<i32> = -5..=5;
pub const AMP_LEVEL_RANGE_DB: std::ops::RangeInclusive<i32> = -24..=6;
pub const CAB_MIC_RANGE: std::ops::RangeInclusive<i32> = -5..=5;
pub const CHORUS_RATE_RANGE: std::ops::RangeInclusive<i32> = 1..=10;
pub const CHORUS_DEPTH_RANGE: std::ops::RangeInclusive<i32> = 0..=10;
pub const CHORUS_MIX_RANGE: std::ops::RangeInclusive<i32> = 0..=10;
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
            chorus_enabled: false,
            chorus_rate: 3.0,
            chorus_depth: 6.0,
            chorus_mix: 8.0,
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
            chorus_rate: within(self.chorus_rate, CHORUS_RATE_RANGE, d.chorus_rate),
            chorus_depth: within(self.chorus_depth, CHORUS_DEPTH_RANGE, d.chorus_depth),
            chorus_mix: within(self.chorus_mix, CHORUS_MIX_RANGE, d.chorus_mix),
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
    chorus_enabled: AtomicBool,
    chorus_rate: AtomicU32,
    chorus_depth: AtomicU32,
    chorus_mix: AtomicU32,
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
            chorus_enabled: AtomicBool::new(cfg.chorus_enabled),
            chorus_rate: AtomicU32::new(cfg.chorus_rate.to_bits()),
            chorus_depth: AtomicU32::new(cfg.chorus_depth.to_bits()),
            chorus_mix: AtomicU32::new(cfg.chorus_mix.to_bits()),
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
        self.chorus_enabled.store(cfg.chorus_enabled, Ordering::Relaxed);
        self.chorus_rate.store(cfg.chorus_rate.to_bits(), Ordering::Relaxed);
        self.chorus_depth.store(cfg.chorus_depth.to_bits(), Ordering::Relaxed);
        self.chorus_mix.store(cfg.chorus_mix.to_bits(), Ordering::Relaxed);
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
            chorus_enabled: self.chorus_enabled.load(Ordering::Relaxed),
            chorus_rate: f32::from_bits(self.chorus_rate.load(Ordering::Relaxed)),
            chorus_depth: f32::from_bits(self.chorus_depth.load(Ordering::Relaxed)),
            chorus_mix: f32::from_bits(self.chorus_mix.load(Ordering::Relaxed)),
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
pub(crate) struct Biquad {
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
    pub(crate) fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub(crate) fn reset(&mut self) {
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

    pub(crate) fn set_highpass(&mut self, sr: f32, freq: f32, q: f32) {
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

    pub(crate) fn set_lowpass(&mut self, sr: f32, freq: f32, q: f32) {
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
    fn sample_ring_queues_and_handles_underrun() {
        let ring = SampleRing::new(128);
        assert_eq!(ring.pop(), 0.0);
        for i in 1..=64 {
            ring.push(i as f32);
        }
        assert_eq!(ring.available(), 64);
        ring.skip(10);
        assert_eq!(ring.pop(), 11.0);
        // Full: one slot stays open, the rest are dropped.
        for i in 0..200 {
            ring.push(i as f32);
        }
        assert_eq!(ring.available(), 127);
    }

    #[test]
    fn chain_resamples_from_the_ring_rate() {
        // A 24 kHz ramp read by a 48 kHz chain: twice the samples, halfway
        // points interpolated, so a 1 kHz tone keeps its pitch. (Steps can
        // stray up to 1% while the reader steers its queue.)
        let ring = SampleRing::new(1024);
        ring.set_sample_rate(24_000);
        for i in 0..512 {
            ring.push(i as f32);
        }
        let mut chain = EffectChain::new(48_000.0, 128);
        let mut out = Vec::new();
        chain.process_from_ring(&ring, 40, |_, s, _| out.push(s));
        // The first few interpolate from the silence before the ramp's 0.
        assert!(out[4..].windows(2).all(|w| (w[1] - w[0] - 0.5).abs() < 0.01), "{out:?}");

        // Same rates pass straight through.
        ring.set_sample_rate(48_000);
        ring.clear();
        for i in 0..512 {
            ring.push(i as f32);
        }
        let mut out = Vec::new();
        chain.process_from_ring(&ring, 40, |_, s, _| out.push(s));
        assert!(out[4..].windows(2).all(|w| (w[1] - w[0] - 1.0).abs() < 0.02), "{out:?}");
    }

    /// Input and output callbacks on their own clocks, with up to 3 ms of
    /// scheduling jitter each, the way two WASAPI (or ASIO + WASAPI) streams
    /// run; `drift_ppm` makes the input clock run that much fast. Feeds a
    /// counting ramp, so every dropped, repeated or zeroed sample shows up as
    /// a step unlike the rest. Returns the glitch events after the first second.
    fn simulate_streams(input: (u32, usize), output: (u32, usize), drift_ppm: f64, seconds: i64) -> usize {
        let ((in_rate, in_block), (out_rate, out_block)) = (input, output);
        let ring = SampleRing::new(8192);
        ring.set_sample_rate(in_rate);
        let mut chain = EffectChain::new(out_rate as f32, 512);
        let mut rng = 0x2545_f491_u32;
        let mut jitter = move || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng % 3000) as f64
        };
        let in_period = in_block as f64 * 1e6 / (in_rate as f64 * (1.0 + drift_ppm * 1e-6));
        let out_period = out_block as f64 * 1e6 / out_rate as f64;
        let (mut n_in, mut n_out, mut pushed) = (0.0, 0.0, 0);
        let (mut next_in, mut next_out) = (jitter(), 4_000.0 + jitter());
        let mut out = Vec::new();
        let end = seconds as f64 * 1e6;
        while next_in < end || next_out < end {
            if next_in <= next_out {
                for _ in 0..in_block {
                    pushed += 1;
                    // A triangle rather than an endless ramp, so f32 keeps
                    // sub-sample precision.
                    let t = pushed % 8192;
                    ring.push(t.min(8192 - t) as f32);
                }
                ring.note_block(in_block);
                n_in += 1.0;
                next_in = n_in * in_period + jitter();
            } else {
                chain.process_from_ring(&ring, out_block, |_, s, _| out.push(s));
                n_out += 1.0;
                next_out = 4_000.0 + n_out * out_period + jitter();
            }
        }
        // Count glitch events (runs of bad steps), after the first second.
        let step = in_rate as f32 / out_rate as f32;
        let bad: Vec<bool> = out[out_rate as usize..]
            .windows(2)
            .map(|w| w.iter().any(|v| (4.0..4092.0).contains(v)) && ((w[1] - w[0]).abs() - step).abs() > 0.05)
            .collect();
        bad.windows(2).filter(|w| w[1] && !w[0]).count()
    }

    #[test]
    fn monitoring_survives_callback_jitter() {
        let cases = [
            ("same blocks", (48_000, 480), (48_000, 480), 0.0),
            ("mismatched blocks", (48_000, 512), (48_000, 441), 0.0),
            ("44.1k input into 48k output", (44_100, 512), (48_000, 480), 0.0),
            ("input clock fast", (48_000, 480), (48_000, 480), 300.0),
            ("input clock slow", (48_000, 256), (48_000, 480), -300.0),
        ];
        for (name, input, output, drift) in cases {
            let glitches = simulate_streams(input, output, drift, 20);
            assert_eq!(glitches, 0, "{name}: {glitches} glitches in 20 s");
        }
    }

    #[test]
    fn locked_callbacks_add_little_latency() {
        // One ASIO driver tick: the input block arrives, then the output
        // block is read, every time. The queue should settle at the margin.
        let ring = SampleRing::new(8192);
        ring.set_sample_rate(48_000);
        let mut chain = EffectChain::new(48_000.0, 512);
        let mut queued = Vec::new();
        for n in 0..(48_000 * 10 / 128) {
            for i in 0..128 {
                ring.push(((n * 128 + i) % 1000) as f32);
            }
            ring.note_block(128);
            queued.push(ring.available());
            chain.process_from_ring(&ring, 128, |_, _, _| {});
        }
        // Queue beyond the block being read, over the last 5 s, in ms.
        let tail = &queued[queued.len() / 2..];
        let extra = tail.iter().map(|&q| q as f32 - 128.0).sum::<f32>() / tail.len() as f32 / 48.0;
        assert!(extra < 3.5, "{extra:.2} ms of extra latency");
        assert!(tail.iter().all(|&q| q >= 128), "ran dry");
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
