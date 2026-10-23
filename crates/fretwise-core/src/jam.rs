//! Jam: names whatever is being played, with no target to check against.
//!
//! [`JamListener`] turns successive readings into the set of pitch classes
//! sounding now, plus the lowest note, and [`name_chord`] puts a chord name on
//! that set. A full strum is read straight from [`PitchReading::poly`]; single
//! notes and arpeggios are gathered over a short window, the way the chord
//! drill hears a slow strum.

use crate::theory::{nearest_note, pitch_class_name, Note};
use crate::PitchReading;

/// How long a single note keeps counting towards an arpeggiated chord (seconds).
const ARPEGGIO_WINDOW: f64 = 1.0;
/// How long a poly note must persist before it counts (seconds).
const POLY_HOLD: f64 = 0.08;
/// Poly notes count only this far (dB) above the input gate; quieter partials
/// are mostly the tail of the last strum.
const POLY_GATE_ABOVE_DB: f32 = 10.0;
/// How far (in cents) a poly peak may be from a note and still count.
const POLY_CENTS: f32 = 35.0;
/// Quiet for this long ends a phrase: the window starts again from nothing.
const SILENCE: f64 = 0.25;

/// A chord shape relative to its root: suffix and intervals in semitones.
/// Earlier entries win ties, so plainer names come first.
const QUALITIES: &[(&str, &[u8])] = &[
    ("", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("5", &[0, 7]),
    ("7", &[0, 4, 7, 10]),
    ("m7", &[0, 3, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("sus4", &[0, 5, 7]),
    ("sus2", &[0, 2, 7]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("6", &[0, 4, 7, 9]),
    ("m6", &[0, 3, 7, 9]),
    ("add9", &[0, 2, 4, 7]),
    ("madd9", &[0, 2, 3, 7]),
    ("m7b5", &[0, 3, 6, 10]),
    ("dim7", &[0, 3, 6, 9]),
    ("7sus4", &[0, 5, 7, 10]),
    ("mMaj7", &[0, 3, 7, 11]),
    ("9", &[0, 2, 4, 7, 10]),
    ("maj9", &[0, 2, 4, 7, 11]),
    ("m9", &[0, 2, 3, 7, 10]),
    ("7#9", &[0, 3, 4, 7, 10]),
    ("13", &[0, 4, 7, 9, 10]),
];

/// Names for two notes that aren't a chord, by semitones apart.
const INTERVALS: [&str; 12] = [
    "unison", "minor 2nd", "major 2nd", "minor 3rd", "major 3rd", "perfect 4th", "tritone", "perfect 5th",
    "minor 6th", "major 6th", "minor 7th", "major 7th",
];

fn mask_of(intervals: &[u8], root: u8) -> u16 {
    intervals.iter().fold(0u16, |m, &i| m | (1 << ((root + i) % 12)))
}

/// A chord name for a set of pitch classes (12-bit mask), e.g. "Am7" or
/// "C/E". `bass` is the lowest pitch class heard: a chord rooted there is
/// preferred (A C E G is Am7 over A, C6 over C), otherwise it becomes a slash
/// chord. Chords of four notes or more may leave out their fifth, as guitar
/// voicings often do. `None` when nothing fits.
pub fn name_chord(classes: u16, bass: Option<u8>) -> Option<String> {
    if classes.count_ones() < 2 {
        return None;
    }
    // (rank, root, suffix): lower rank is better.
    let mut best: Option<(u32, u8, &str)> = None;
    for (order, &(suffix, intervals)) in QUALITIES.iter().enumerate() {
        for root in 0..12u8 {
            if classes & (1 << root) == 0 {
                continue;
            }
            let full = mask_of(intervals, root);
            let fifth = 1u16 << ((root + 7) % 12);
            let exact = full == classes;
            let no_fifth = !exact && intervals.len() >= 4 && intervals.contains(&7) && full & !fifth == classes;
            if !exact && !no_fifth {
                continue;
            }
            let rank = (u32::from(bass != Some(root)) << 16) | (u32::from(no_fifth) << 8) | order as u32;
            if best.is_none_or(|(r, _, _)| rank < r) {
                best = Some((rank, root, suffix));
            }
        }
    }
    let (_, root, suffix) = best?;
    let mut name = format!("{}{suffix}", pitch_class_name(root));
    if let Some(b) = bass.filter(|&b| b != root) {
        name.push('/');
        name.push_str(pitch_class_name(b));
    }
    Some(name)
}

/// What a set of pitch classes is: a note, an interval, or a chord.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JamName {
    Note(Note),
    /// Two notes that aren't a power chord, lower first, and how far apart.
    Interval(u8, u8, &'static str),
    Chord(String),
    /// Notes that don't make a chord we know.
    Unknown,
}

/// What [`JamListener`] hears right now.
#[derive(Clone, Debug, PartialEq)]
pub struct JamHeard {
    /// Pitch classes sounding (12-bit mask).
    pub classes: u16,
    /// The lowest note among them.
    pub bass: Option<Note>,
    pub name: JamName,
    /// For a single note: how far off it is, in cents.
    pub cents: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct JamListener {
    poly_since: [Option<f64>; 12],
    class_seen_at: [f64; 12],
    /// The lowest note in the window and when it was last heard.
    low: Option<(Note, f64)>,
    last_voiced: f64,
}

impl Default for JamListener {
    fn default() -> Self {
        Self {
            poly_since: [None; 12],
            class_seen_at: [f64::NEG_INFINITY; 12],
            low: None,
            last_voiced: f64::NEG_INFINITY,
        }
    }
}

impl JamListener {
    /// Feed one reading and the stabilized mono note; returns what is sounding,
    /// or `None` while nothing is.
    pub fn listen(&mut self, now: f64, reading: &PitchReading, mono: Option<Note>, a4: f32) -> Option<JamHeard> {
        let loud = reading.level_db >= reading.gate_db + POLY_GATE_ABOVE_DB;
        let mut raw = 0u16;
        let mut poly_low: Option<Note> = None;
        for r in reading.poly.iter().filter(|&&f| f > 0.0 && loud).filter_map(|&f| nearest_note(f, a4)) {
            if r.cents.abs() <= POLY_CENTS {
                raw |= 1 << r.note.pitch_class();
                poly_low = Some(poly_low.map_or(r.note, |l| l.min(r.note)));
            }
        }
        let mut strum = 0u16;
        for pc in 0..12 {
            if raw & (1 << pc) != 0 {
                if now - *self.poly_since[pc].get_or_insert(now) >= POLY_HOLD {
                    strum |= 1 << pc;
                }
            } else {
                self.poly_since[pc] = None;
            }
        }

        if strum == 0 && mono.is_none() {
            if now - self.last_voiced > SILENCE {
                self.class_seen_at = [f64::NEG_INFINITY; 12];
                self.low = None;
            }
            return None;
        }
        self.last_voiced = now;

        let frame = strum | mono.map_or(0, |n| 1 << n.pitch_class());
        for pc in 0..12 {
            if frame & (1 << pc) != 0 {
                self.class_seen_at[pc] = now;
            }
        }
        let frame_low = match (poly_low.filter(|_| strum != 0), mono) {
            (Some(p), Some(m)) => Some(p.min(m)),
            (p, m) => p.or(m),
        };

        // A strum of three notes or more is the chord as it sounds now, so a
        // change of chord isn't muddied by the last one. Fewer may be an
        // arpeggio: gather what was played in the last moment.
        let (classes, bass) = if strum.count_ones() >= 3 {
            self.class_seen_at = [f64::NEG_INFINITY; 12];
            for pc in 0..12 {
                if frame & (1 << pc) != 0 {
                    self.class_seen_at[pc] = now;
                }
            }
            self.low = frame_low.map(|n| (n, now));
            (frame, frame_low)
        } else {
            let mut classes = 0u16;
            for (pc, &t) in self.class_seen_at.iter().enumerate() {
                if now - t <= ARPEGGIO_WINDOW {
                    classes |= 1 << pc;
                }
            }
            if let Some(n) = frame_low {
                let stale = self.low.is_none_or(|(_, t)| now - t > ARPEGGIO_WINDOW);
                if stale || self.low.is_some_and(|(l, _)| n <= l) {
                    self.low = Some((n, now));
                }
            }
            (classes, self.low.map(|(n, _)| n))
        };

        let name = match classes.count_ones() {
            1 => match mono.or(bass) {
                Some(n) => JamName::Note(n),
                None => JamName::Unknown,
            },
            2 => match name_chord(classes, bass.map(Note::pitch_class)) {
                Some(chord) => JamName::Chord(chord),
                None => {
                    let low = bass.map_or_else(|| classes.trailing_zeros() as u8, Note::pitch_class);
                    let high = (0..12u8).find(|&pc| pc != low && classes & (1 << pc) != 0).unwrap_or(low);
                    JamName::Interval(low, high, INTERVALS[((high + 12 - low) % 12) as usize])
                }
            },
            _ => name_chord(classes, bass.map(Note::pitch_class)).map_or(JamName::Unknown, JamName::Chord),
        };
        let cents = match name {
            JamName::Note(n) => reading
                .frequency
                .and_then(|f| nearest_note(f, a4))
                .filter(|r| r.note == n)
                .map(|r| r.cents),
            _ => None,
        };
        Some(JamHeard { classes, bass, name, cents })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChordKind, Tuning};

    fn pcs(names: &[&str]) -> u16 {
        names.iter().fold(0, |m, n| m | 1 << (0..12).find(|&pc| pitch_class_name(pc) == *n).unwrap())
    }

    #[test]
    fn names_common_chords() {
        assert_eq!(name_chord(pcs(&["C", "E", "G"]), Some(0)).as_deref(), Some("C"));
        assert_eq!(name_chord(pcs(&["A", "C", "E"]), Some(9)).as_deref(), Some("Am"));
        assert_eq!(name_chord(pcs(&["E", "B"]), Some(4)).as_deref(), Some("E5"));
        assert_eq!(name_chord(pcs(&["C", "E", "G"]), Some(4)).as_deref(), Some("C/E"));
        // Same notes, the bass decides.
        assert_eq!(name_chord(pcs(&["A", "C", "E", "G"]), Some(9)).as_deref(), Some("Am7"));
        assert_eq!(name_chord(pcs(&["A", "C", "E", "G"]), Some(0)).as_deref(), Some("C6"));
        // A shell voicing without its fifth.
        assert_eq!(name_chord(pcs(&["G", "B", "F"]), Some(7)).as_deref(), Some("G7"));
        assert_eq!(name_chord(pcs(&["C", "E"]), Some(0)), None);
    }

    #[test]
    fn names_every_built_in_voicing() {
        let tuning = Tuning::STANDARD;
        let mut wrong = Vec::new();
        for kind in ChordKind::all() {
            let v = kind.voicing();
            let bass = v.positions().first().map(|&p| tuning.note_at(p).pitch_class());
            let named = name_chord(v.pitch_class_mask(tuning), bass).unwrap_or_default();
            // "A (Barre)" is an A. Two jazz shapes are labelled 7ths but
            // sound the 9th too.
            let expected = match v.id.as_str() {
                "cmaj7-jazz" => "Cmaj9",
                "dm7-jazz" => "Dm9",
                _ => v.name.split(" (").next().unwrap(),
            };
            if named != expected {
                wrong.push(format!("{}: {named}", v.id));
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    fn reading(notes: &[u8]) -> PitchReading {
        let mut r = PitchReading { level_db: -20.0, gate_db: -55.0, ..Default::default() };
        for (slot, &n) in r.poly.iter_mut().zip(notes) {
            *slot = Note(n).frequency(440.0);
        }
        r
    }

    #[test]
    fn hears_a_strum_then_a_change() {
        let mut l = JamListener::default();
        let am = reading(&[45, 52, 57, 60, 64]);
        // Not yet: a peak has to last a moment to count.
        assert_eq!(l.listen(0.0, &am, None, 440.0), None);
        let heard = l.listen(0.1, &am, None, 440.0).unwrap();
        assert_eq!(heard.name, JamName::Chord("Am".into()));
        assert_eq!(heard.bass, Some(Note(45)));
        // Straight to C: no A left over from the last chord.
        let c = reading(&[48, 52, 55, 60, 64]);
        l.listen(0.2, &c, None, 440.0);
        let heard = l.listen(0.3, &c, None, 440.0).unwrap();
        assert_eq!(heard.name, JamName::Chord("C".into()));
    }

    #[test]
    fn gathers_an_arpeggio() {
        let mut l = JamListener::default();
        let quiet = PitchReading { level_db: -50.0, gate_db: -55.0, ..Default::default() };
        let mut last = None;
        for (i, n) in [40u8, 47, 52, 55].into_iter().enumerate() {
            last = l.listen(i as f64 * 0.2, &quiet, Some(Note(n)), 440.0);
        }
        let heard = last.unwrap();
        assert_eq!(heard.name, JamName::Chord("Em".into()));
        assert_eq!(heard.bass, Some(Note(40)));
    }

    #[test]
    fn one_note_and_two() {
        let mut l = JamListener::default();
        let mut r = PitchReading { level_db: -50.0, gate_db: -55.0, ..Default::default() };
        r.frequency = Some(Note(57).frequency(440.0) * 1.003);
        let heard = l.listen(0.0, &r, Some(Note(57)), 440.0).unwrap();
        assert_eq!(heard.name, JamName::Note(Note(57)));
        assert!(heard.cents.unwrap() > 4.0);
        let heard = l.listen(0.2, &r, Some(Note(61)), 440.0).unwrap();
        assert_eq!(heard.name, JamName::Interval(9, 1, "major 3rd"));
        // Silence starts over.
        assert_eq!(l.listen(1.0, &PitchReading::default(), None, 440.0), None);
        let heard = l.listen(1.1, &r, Some(Note(64)), 440.0).unwrap();
        assert_eq!(heard.name, JamName::Note(Note(64)));
    }
}
