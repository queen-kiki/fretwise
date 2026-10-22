//! The audio-thread half: turns a stream of samples into pitch readings.
//!
//! [`Analyzer::push`] is what an audio callback (cpal) or a plugin's
//! `process()` calls per sample. It does not allocate or lock; results go out
//! through [`SharedPitch`], which the GUI thread polls.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use crate::pitch::Yin;
use crate::poly::{PolyDetector, MAX_NOTES};

#[derive(Clone, Copy, Debug)]
pub struct AnalyzerConfig {
    /// Lowest frequency to detect. 70 Hz covers drop D (73.4 Hz).
    pub min_freq: f32,
    /// Highest frequency to detect. 1400 Hz covers the 24th fret of high E.
    pub max_freq: f32,
    pub yin_threshold: f32,
    /// Below this input level (dBFS) we report silence.
    pub gate_db: f32,
    /// How often to run detection.
    pub hop_ms: f32,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            min_freq: 70.0,
            max_freq: 1400.0,
            yin_threshold: 0.15,
            gate_db: -55.0,
            hop_ms: 10.0,
        }
    }
}

/// One snapshot of what the analyzer hears.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PitchReading {
    /// `None` when the input is silent or not clearly pitched.
    pub frequency: Option<f32>,
    pub clarity: f32,
    /// RMS input level in dBFS.
    pub level_db: f32,
    /// Increments on every analysis; lets readers tell fresh data from stale.
    pub seq: u64,
    /// Every note heard at once (for chords), in Hz, strongest first; unused
    /// slots are 0. Updated less often than `frequency`, about every 40 ms.
    pub poly: [f32; MAX_NOTES],
}

/// Lock-free mailbox from the audio thread to the GUI. Fields are stored
/// independently, so a reader may see a mix of two consecutive readings; for
/// display purposes that's harmless.
#[derive(Default)]
pub struct SharedPitch {
    frequency: AtomicU32,
    clarity: AtomicU32,
    level_db: AtomicU32,
    poly: [AtomicU32; MAX_NOTES],
    seq: AtomicU64,
}

impl SharedPitch {
    pub fn store(&self, r: PitchReading) {
        let f = r.frequency.unwrap_or(0.0);
        self.frequency.store(f.to_bits(), Ordering::Relaxed);
        self.clarity.store(r.clarity.to_bits(), Ordering::Relaxed);
        self.level_db.store(r.level_db.to_bits(), Ordering::Relaxed);
        for (slot, f) in self.poly.iter().zip(r.poly) {
            slot.store(f.to_bits(), Ordering::Relaxed);
        }
        self.seq.store(r.seq, Ordering::Release);
    }

    pub fn load(&self) -> PitchReading {
        let seq = self.seq.load(Ordering::Acquire);
        let f = f32::from_bits(self.frequency.load(Ordering::Relaxed));
        PitchReading {
            frequency: (f > 0.0).then_some(f),
            clarity: f32::from_bits(self.clarity.load(Ordering::Relaxed)),
            level_db: f32::from_bits(self.level_db.load(Ordering::Relaxed)),
            seq,
            poly: std::array::from_fn(|i| f32::from_bits(self.poly[i].load(Ordering::Relaxed))),
        }
    }
}

pub struct Analyzer {
    yin: Yin,
    poly: PolyDetector,
    /// The poly detector's latest notes, sent along with every reading.
    poly_notes: [f32; MAX_NOTES],
    /// Circular history of (decimated) input; `pos` is the oldest sample.
    history: Vec<f32>,
    pos: usize,
    /// Contiguous copy of `history` for YIN.
    frame: Vec<f32>,
    decimation: usize,
    acc: f32,
    acc_count: usize,
    hop: usize,
    since_last: usize,
    gate_db: f32,
    seq: u64,
    shared: Arc<SharedPitch>,
}

impl Analyzer {
    pub fn new(sample_rate: f32, config: AnalyzerConfig, shared: Arc<SharedPitch>) -> Self {
        // Guitar needs nothing above ~48 kHz; at 96/192 kHz average groups of
        // samples so the O(n²) YIN cost stays flat.
        let decimation = ((sample_rate / 48_000.0).round() as usize).max(1);
        let rate = sample_rate / decimation as f32;
        let yin = Yin::new(rate, config.min_freq, config.max_freq, config.yin_threshold);
        let len = yin.required_len();
        Self {
            yin,
            poly: PolyDetector::new(rate, config.gate_db),
            poly_notes: [0.0; MAX_NOTES],
            history: vec![0.0; len],
            pos: 0,
            frame: vec![0.0; len],
            decimation,
            acc: 0.0,
            acc_count: 0,
            hop: ((rate * config.hop_ms / 1000.0) as usize).max(1),
            since_last: 0,
            gate_db: config.gate_db,
            seq: 0,
            shared,
        }
    }

    pub fn shared(&self) -> Arc<SharedPitch> {
        self.shared.clone()
    }

    /// Feed one mono sample. Realtime-safe.
    #[inline]
    pub fn push(&mut self, sample: f32) {
        self.acc += sample;
        self.acc_count += 1;
        if self.acc_count < self.decimation {
            return;
        }
        let s = self.acc / self.decimation as f32;
        self.acc = 0.0;
        self.acc_count = 0;

        if self.poly.push(s) {
            self.poly_notes = self.poly.notes();
        }
        self.history[self.pos] = s;
        self.pos = (self.pos + 1) % self.history.len();
        self.since_last += 1;
        if self.since_last >= self.hop {
            self.since_last = 0;
            self.analyze();
        }
    }

    /// Feed a block of mono samples. Realtime-safe.
    pub fn process(&mut self, samples: &[f32]) {
        for &s in samples {
            self.push(s);
        }
    }

    fn analyze(&mut self) {
        let (newer, older) = self.history.split_at(self.pos);
        self.frame[..older.len()].copy_from_slice(older);
        self.frame[older.len()..].copy_from_slice(newer);

        let mean_sq = self.frame.iter().map(|x| x * x).sum::<f32>() / self.frame.len() as f32;
        let level_db = 10.0 * mean_sq.max(1e-12).log10();

        let estimate = if level_db >= self.gate_db {
            self.yin.detect(&self.frame)
        } else {
            None
        };

        self.seq += 1;
        self.shared.store(PitchReading {
            frequency: estimate.map(|e| e.frequency),
            clarity: estimate.map_or(0.0, |e| e.clarity),
            level_db,
            seq: self.seq,
            poly: self.poly_notes,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::tests::guitar_like;

    #[test]
    fn streams_and_publishes_at_high_sample_rate() {
        let sr = 96_000.0;
        let shared = Arc::new(SharedPitch::default());
        let mut a = Analyzer::new(sr, AnalyzerConfig::default(), shared.clone());
        let signal = guitar_like(110.0, sr, 48_000);
        for block in signal.chunks(256) {
            a.process(block);
        }
        let r = shared.load();
        assert!(r.seq > 0);
        let f = r.frequency.expect("voiced");
        assert!((1200.0 * (f / 110.0).log2()).abs() < 5.0, "{f}");
        // The chord detector hears it too, through the shared mailbox.
        let poly = r.poly[0];
        assert!((1200.0 * (poly / 110.0).log2()).abs() < 10.0, "{:?}", r.poly);
    }

    #[test]
    fn gate_reports_silence() {
        let shared = Arc::new(SharedPitch::default());
        let mut a = Analyzer::new(48_000.0, AnalyzerConfig::default(), shared.clone());
        a.process(&vec![0.0; 4_800]);
        let r = shared.load();
        assert!(r.seq > 0);
        assert_eq!(r.frequency, None);
    }
}
