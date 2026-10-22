//! Scales: which notes the trainer asks for, and how to name them as degrees.

use serde::{Deserialize, Serialize};

use crate::theory::{pitch_class_name, Note};

/// Interval names by semitones above the root. ASCII "b" rather than "♭" so
/// any font can draw them.
const DEGREES: [&str; 12] = ["1", "b2", "2", "b3", "3", "4", "b5", "5", "b6", "6", "b7", "7"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleKind {
    Chromatic,
    Major,
    NaturalMinor,
    MajorPentatonic,
    MinorPentatonic,
}

impl ScaleKind {
    pub const ALL: [ScaleKind; 5] = [
        Self::Chromatic,
        Self::Major,
        Self::NaturalMinor,
        Self::MajorPentatonic,
        Self::MinorPentatonic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Chromatic => "chromatic",
            Self::Major => "major",
            Self::NaturalMinor => "minor",
            Self::MajorPentatonic => "major pentatonic",
            Self::MinorPentatonic => "minor pentatonic",
        }
    }

    /// Semitones above the root.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Self::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Self::Major => &[0, 2, 4, 5, 7, 9, 11],
            Self::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Self::MajorPentatonic => &[0, 2, 4, 7, 9],
            Self::MinorPentatonic => &[0, 3, 5, 7, 10],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scale {
    /// Pitch class of the root, 0 = C.
    pub root: u8,
    pub kind: ScaleKind,
}

impl Default for Scale {
    /// C major: the natural notes.
    fn default() -> Self {
        Self { root: 0, kind: ScaleKind::Major }
    }
}

impl Scale {
    fn semitones(self, note: Note) -> u8 {
        (note.pitch_class() + 12 - self.root % 12) % 12
    }

    pub fn contains(self, note: Note) -> bool {
        self.kind.intervals().contains(&self.semitones(note))
    }

    /// The note's interval above the root, e.g. "b3". Notes outside the
    /// scale get their chromatic name too.
    pub fn degree(self, note: Note) -> &'static str {
        DEGREES[self.semitones(note) as usize]
    }

    pub fn name(self) -> String {
        match self.kind {
            ScaleKind::Chromatic => "chromatic".to_string(),
            kind => format!("{} {}", pitch_class_name(self.root), kind.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_and_degrees() {
        let a_minor_pent = Scale { root: 9, kind: ScaleKind::MinorPentatonic };
        // A2 C3 D3 E3 G3
        for n in [45, 48, 50, 52, 55] {
            assert!(a_minor_pent.contains(Note(n)), "{}", Note(n));
        }
        assert!(!a_minor_pent.contains(Note(47))); // B2
        assert_eq!(a_minor_pent.degree(Note(48)), "b3");
        assert_eq!(a_minor_pent.degree(Note(57)), "1");
        assert_eq!(a_minor_pent.degree(Note(47)), "2");
        assert_eq!(a_minor_pent.name(), "A minor pentatonic");

        let c_major = Scale::default();
        for n in 40..64 {
            assert_eq!(c_major.contains(Note(n)), Note(n).is_natural());
        }
    }
}
