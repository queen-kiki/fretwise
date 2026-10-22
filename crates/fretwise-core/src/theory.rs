//! Notes, frequencies, tunings and fretboard positions.

use std::fmt;

use serde::{Deserialize, Serialize};

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// A note identified by its MIDI number (E2 = 40, A4 = 69).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Note(pub u8);

impl Note {
    pub fn pitch_class(self) -> u8 {
        self.0 % 12
    }

    pub fn octave(self) -> i32 {
        self.0 as i32 / 12 - 1
    }

    pub fn name(self) -> &'static str {
        NOTE_NAMES[self.pitch_class() as usize]
    }

    pub fn is_natural(self) -> bool {
        !self.name().ends_with('#')
    }

    pub fn frequency(self, a4: f32) -> f32 {
        a4 * 2f32.powf((self.0 as f32 - 69.0) / 12.0)
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.name(), self.octave())
    }
}

/// Fractional MIDI number for a frequency.
pub fn freq_to_midi(freq: f32, a4: f32) -> f32 {
    69.0 + 12.0 * (freq / a4).log2()
}

/// The note closest to a frequency, and how far off it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteReading {
    pub note: Note,
    /// Deviation from `note` in cents, in -50..=50.
    pub cents: f32,
}

pub fn nearest_note(freq: f32, a4: f32) -> Option<NoteReading> {
    if !(freq.is_finite() && freq > 0.0) {
        return None;
    }
    let midi = freq_to_midi(freq, a4);
    let rounded = midi.round();
    if !(0.0..=127.0).contains(&rounded) {
        return None;
    }
    Some(NoteReading {
        note: Note(rounded as u8),
        cents: (midi - rounded) * 100.0,
    })
}

/// A spot on the neck. `string` 0 is the lowest (thickest) string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    pub string: usize,
    pub fret: u8,
}

/// Open-string notes, lowest string first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tuning {
    pub name: &'static str,
    pub strings: [Note; 6],
}

impl Tuning {
    pub const STANDARD: Tuning = Tuning {
        name: "Standard (EADGBE)",
        strings: [Note(40), Note(45), Note(50), Note(55), Note(59), Note(64)],
    };
    pub const DROP_D: Tuning = Tuning {
        name: "Drop D (DADGBE)",
        strings: [Note(38), Note(45), Note(50), Note(55), Note(59), Note(64)],
    };
    pub const EB_STANDARD: Tuning = Tuning {
        name: "Eb standard",
        strings: [Note(39), Note(44), Note(49), Note(54), Note(58), Note(63)],
    };
    pub const ALL: [Tuning; 3] = [Self::STANDARD, Self::DROP_D, Self::EB_STANDARD];

    /// The built-in tuning with these open strings, e.g. when loading saved state.
    pub fn from_strings(strings: [Note; 6]) -> Option<Tuning> {
        Self::ALL.into_iter().find(|t| t.strings == strings)
    }

    pub fn note_at(&self, pos: Position) -> Note {
        Note(self.strings[pos.string].0 + pos.fret)
    }

    /// Every position where `note` can be played, up to `max_fret`.
    pub fn positions_of(&self, note: Note, max_fret: u8) -> Vec<Position> {
        (0..6)
            .filter_map(|string| {
                let open = self.strings[string].0;
                let fret = note.0.checked_sub(open)?;
                (fret <= max_fret).then_some(Position { string, fret })
            })
            .collect()
    }

    /// The open string closest in pitch to `midi`, for tuning up.
    pub fn nearest_string(&self, midi: f32) -> usize {
        (0..6)
            .min_by(|&a, &b| {
                let da = (self.strings[a].0 as f32 - midi).abs();
                let db = (self.strings[b].0 as f32 - midi).abs();
                da.total_cmp(&db)
            })
            .unwrap_or(0)
    }
}

/// Guitarists number strings from the thinnest: string index 0 (low E) is "6".
pub fn string_number(string: usize) -> usize {
    6 - string
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_frequencies() {
        assert_eq!(Note(69).to_string(), "A4");
        assert_eq!(Note(40).to_string(), "E2");
        assert!((Note(40).frequency(440.0) - 82.4069).abs() < 1e-3);
    }

    #[test]
    fn nearest_note_reports_cents() {
        let r = nearest_note(445.0, 440.0).unwrap();
        assert_eq!(r.note, Note(69));
        assert!((r.cents - 19.56).abs() < 0.1);
        assert!(nearest_note(0.0, 440.0).is_none());
    }

    #[test]
    fn positions() {
        let t = Tuning::STANDARD;
        assert_eq!(t.note_at(Position { string: 1, fret: 3 }).to_string(), "C3");
        // A2: open A string and 5th fret of low E.
        let p = t.positions_of(Note(45), 12);
        assert!(p.contains(&Position { string: 0, fret: 5 }));
        assert!(p.contains(&Position { string: 1, fret: 0 }));
        assert_eq!(p.len(), 2);
        assert_eq!(string_number(0), 6);
    }
}
