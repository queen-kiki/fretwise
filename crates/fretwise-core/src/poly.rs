//! Hears several notes at once, for strummed chords.
//!
//! The mono detector ([`crate::Yin`]) finds one pitch. A chord needs the
//! spectrum: the input is low-passed and decimated to about 12 kHz, and every
//! ~40 ms a 4096-point Hann-windowed FFT (about a third of a second) is taken.
//! Its peaks are refined by parabolic interpolation, then walked from low to
//! high: a peak that sits on a whole-number multiple of a note already found
//! is that note's overtone and is dropped, anything else is a new note. What's
//! left are the fundamentals, strongest first.
//!
//! An overtone that is also a played note (A3 over A2, or E4 over A2 in an A
//! minor chord) gets dropped with the real overtones. Chord checking works
//! with pitch classes, so that costs nothing as long as the class shows up
//! somewhere else, which in a guitar voicing it nearly always does.
//!
//! Allocation-free after [`PolyDetector::new`], for the audio thread.

use crate::fft::Fft;

/// The most notes reported at once: a guitar has six strings, plus a little room.
pub const MAX_NOTES: usize = 8;

const FFT_LEN: usize = 4096;
const TARGET_RATE: f32 = 12_000.0;
/// Lowest fundamental reported: drop D's low D is 73.4 Hz.
const MIN_FREQ: f32 = 70.0;
/// Highest fundamental reported: fret 15 on the high E is 784 Hz; leave room.
const MAX_FREQ: f32 = 1100.0;
/// Overtones are looked for up to here.
const MAX_PEAK_FREQ: f32 = 2600.0;
/// Peaks quieter than this, relative to the loudest, are ignored (-18 dB).
const PEAK_FLOOR: f32 = 0.12;
/// How much taller a peak must be than the median spectrum bin around it (~21 dB),
/// so broadband noise / interface hiss can't produce 8 random peaks.
const MIN_SNR: f32 = 12.0;
/// How close (in cents) a peak must be to a multiple to count as an overtone.
const OVERTONE_CENTS: f32 = 35.0;
/// Highest overtone number checked.
const MAX_OVERTONE: f32 = 10.0;

/// Second-order low-pass section (RBJ cookbook), transposed direct form II.
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
    fn lowpass(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let w0 = std::f32::consts::TAU * cutoff / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

#[derive(Clone, Copy, Default)]
struct Peak {
    freq: f32,
    amp: f32,
}

pub struct PolyDetector {
    decimation: usize,
    /// 4th-order Butterworth anti-aliasing filter, as two sections.
    lowpass: [Biquad; 2],
    acc_count: usize,
    rate: f32,
    history: Vec<f32>,
    pos: usize,
    hop: usize,
    since_last: usize,
    gate: f32,
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    mag: Vec<f32>,
    fft: Fft,
    peaks: Vec<Peak>,
    notes: Vec<Peak>,
    found: [f32; MAX_NOTES],
}

impl PolyDetector {
    /// `sample_rate` is the rate samples are pushed at; `gate_db` the input
    /// level (dBFS) below which nothing is reported.
    pub fn new(sample_rate: f32, gate_db: f32) -> Self {
        let decimation = ((sample_rate / TARGET_RATE).round() as usize).max(1);
        let rate = sample_rate / decimation as f32;
        // Anything that would fold below MAX_PEAK_FREQ is far above 3 kHz,
        // where a 4th-order filter has it down 40 dB or more.
        let cutoff = (rate * 0.25).min(3000.0);
        let window = (0..FFT_LEN)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_LEN as f32).cos())
            .collect();
        Self {
            decimation,
            lowpass: [Biquad::lowpass(sample_rate, cutoff, 0.5412), Biquad::lowpass(sample_rate, cutoff, 1.3066)],
            acc_count: 0,
            rate,
            history: vec![0.0; FFT_LEN],
            pos: 0,
            hop: ((rate * 0.04) as usize).max(1),
            since_last: 0,
            gate: 10f32.powf(gate_db / 10.0),
            window,
            re: vec![0.0; FFT_LEN],
            im: vec![0.0; FFT_LEN],
            mag: vec![0.0; FFT_LEN / 2],
            fft: Fft::new(FFT_LEN),
            peaks: Vec::with_capacity(FFT_LEN / 4),
            notes: Vec::with_capacity(FFT_LEN / 4),
            found: [0.0; MAX_NOTES],
        }
    }

    /// Feed one sample. Returns true when [`Self::notes`] was just updated.
    #[inline]
    pub fn push(&mut self, sample: f32) -> bool {
        let s = self.lowpass[0].process(sample);
        let s = self.lowpass[1].process(s);
        self.acc_count += 1;
        if self.acc_count < self.decimation {
            return false;
        }
        self.acc_count = 0;
        self.history[self.pos] = s;
        self.pos = (self.pos + 1) % FFT_LEN;
        self.since_last += 1;
        if self.since_last < self.hop {
            return false;
        }
        self.since_last = 0;
        self.analyze();
        true
    }

    /// Fundamentals heard in the last third of a second, in Hz, strongest
    /// first; unused slots are 0.
    pub fn notes(&self) -> [f32; MAX_NOTES] {
        self.found
    }

    fn analyze(&mut self) {
        self.found = [0.0; MAX_NOTES];
        let (newer, older) = self.history.split_at(self.pos);
        let mut power = 0.0;
        for (i, &x) in older.iter().chain(newer).enumerate() {
            power += x * x;
            self.re[i] = x * self.window[i];
            self.im[i] = 0.0;
        }
        if power / (FFT_LEN as f32) < self.gate {
            return;
        }
        self.fft.process(&mut self.re, &mut self.im);
        for (k, m) in self.mag.iter_mut().enumerate() {
            *m = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt();
        }

        let bin_hz = self.rate / FFT_LEN as f32;
        let lo = ((MIN_FREQ / bin_hz).floor() as usize).max(2);
        let hi = ((MAX_PEAK_FREQ / bin_hz).ceil() as usize).min(FFT_LEN / 2 - 3);
        let loudest = self.mag[lo..=hi].iter().copied().fold(0.0, f32::max);
        if loudest <= 0.0 {
            return;
        }
        let mean_mag = self.mag[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32;
        if loudest < mean_mag * MIN_SNR {
            return;
        }
        let floor = (loudest * PEAK_FLOOR).max(mean_mag * MIN_SNR);

        // Local maxima over two bins each side, refined on a log scale.
        self.peaks.clear();
        let m = &self.mag;
        for k in lo..=hi {
            let b = m[k];
            if b < floor || b <= m[k - 1] || b < m[k + 1] || b <= m[k - 2] || b < m[k + 2] {
                continue;
            }
            let (a, b, c) = (m[k - 1].max(1e-12).ln(), b.ln(), m[k + 1].max(1e-12).ln());
            let denom = a - 2.0 * b + c;
            let p = if denom.abs() > 1e-12 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
            self.peaks.push(Peak { freq: (k as f32 + p) * bin_hz, amp: (b - 0.25 * (a - c) * p).exp() });
        }

        // Low to high: overtones of a note already found are dropped, and a
        // candidate fundamental must have at least one audible overtone (guitar
        // strings are rich in harmonics; mains hum and random noise spikes aren't).
        self.notes.clear();
        for &peak in &self.peaks {
            if peak.freq > MAX_FREQ {
                break;
            }
            let overtone = self.notes.iter().any(|n| {
                let ratio = peak.freq / n.freq;
                let h = ratio.round();
                // A note far quieter than the peak can't explain it.
                (2.0..=MAX_OVERTONE).contains(&h)
                    && (1200.0 * (ratio / h).log2()).abs() < OVERTONE_CENTS
                    && n.amp >= 0.1 * peak.amp
            });
            if overtone {
                continue;
            }
            let has_harmonic = self.peaks.iter().any(|p| {
                let ratio = p.freq / peak.freq;
                let h = ratio.round();
                (2.0..=4.0).contains(&h)
                    && (1200.0 * (ratio / h).log2()).abs() < OVERTONE_CENTS
                    && p.amp >= 0.08 * peak.amp
            });
            if has_harmonic {
                self.notes.push(peak);
            }
        }
        self.notes.sort_by(|a, b| b.amp.total_cmp(&a.amp));
        for (slot, n) in self.found.iter_mut().zip(&self.notes) {
            *slot = n.freq;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::tests::guitar_like;
    use crate::theory::{nearest_note, Note};

    /// Pitch classes heard after half a second of `notes` (MIDI numbers).
    fn hear(notes: &[u8], sample_rate: f32) -> Vec<&'static str> {
        let len = (sample_rate * 0.5) as usize;
        let mut mix = vec![0.0f32; len];
        for &n in notes {
            for (m, s) in mix.iter_mut().zip(guitar_like(Note(n).frequency(440.0), sample_rate, len)) {
                *m += s / notes.len() as f32;
            }
        }
        let mut d = PolyDetector::new(sample_rate, -60.0);
        for s in mix {
            d.push(s);
        }
        let mut names: Vec<&str> = d
            .notes()
            .iter()
            .filter(|&&f| f > 0.0)
            .filter_map(|&f| nearest_note(f, 440.0))
            .map(|r| r.note.name())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn hears_open_chords() {
        for sr in [44_100.0, 48_000.0, 96_000.0] {
            // A minor, x02210: A2 E3 A3 C4 E4.
            assert_eq!(hear(&[45, 52, 57, 60, 64], sr), ["A", "C", "E"], "{sr}");
            // C major, x32010: C3 E3 G3 C4 E4.
            assert_eq!(hear(&[48, 52, 55, 60, 64], sr), ["C", "E", "G"], "{sr}");
            // G major, 320003: G2 B2 D3 G3 B3 G4.
            assert_eq!(hear(&[43, 47, 50, 55, 59, 67], sr), ["B", "D", "G"], "{sr}");
            // E major, 022100: E2 B2 E3 G#3 B3 E4.
            assert_eq!(hear(&[40, 47, 52, 56, 59, 64], sr), ["B", "E", "G#"], "{sr}");
        }
    }

    #[test]
    fn one_note_is_one_note() {
        // Overtones (the 3rd of A2 is an E, the 5th a C#) don't add notes.
        assert_eq!(hear(&[45], 48_000.0), ["A"]);
        assert_eq!(hear(&[40], 48_000.0), ["E"]);
        assert_eq!(hear(&[64], 48_000.0), ["E"]);
    }

    #[test]
    fn silence_is_nothing() {
        let mut d = PolyDetector::new(48_000.0, -55.0);
        for _ in 0..24_000 {
            d.push(0.0);
        }
        assert_eq!(d.notes(), [0.0; MAX_NOTES]);
    }

    #[test]
    fn broadband_noise_above_gate_reports_no_notes() {
        let mut d = PolyDetector::new(48_000.0, -55.0);
        let mut x = 0x1234_5678u32;
        for _ in 0..24_000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let sample = (x as f32 / u32::MAX as f32 - 0.5) * 0.2;
            d.push(sample);
        }
        assert_eq!(d.notes(), [0.0; MAX_NOTES]);
    }
}
