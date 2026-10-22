//! Turns jittery per-frame pitch readings into "the player is holding note X".
//!
//! A pluck's attack often reads as a harmonic or the wrong octave for a few
//! milliseconds, so a note only counts once it has held steady for a moment.

use crate::analyzer::PitchReading;
use crate::theory::{nearest_note, Note};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StableNote {
    pub note: Note,
    pub cents: f32,
}

pub struct NoteStabilizer {
    /// Seconds a note must persist before it counts.
    pub hold: f64,
    /// Seconds of silence before a held note is considered released.
    pub release: f64,
    pub min_clarity: f32,
    candidate: Option<(Note, f64)>,
    stable: Option<StableNote>,
    last_voiced: f64,
}

impl Default for NoteStabilizer {
    fn default() -> Self {
        Self {
            hold: 0.08,
            release: 0.15,
            min_clarity: 0.8,
            candidate: None,
            stable: None,
            last_voiced: f64::NEG_INFINITY,
        }
    }
}

impl NoteStabilizer {
    /// Call regularly (e.g. every GUI frame) with the latest reading and the
    /// current time in seconds.
    pub fn update(&mut self, now: f64, reading: &PitchReading, a4: f32) -> Option<StableNote> {
        let heard = reading
            .frequency
            .filter(|_| reading.clarity >= self.min_clarity)
            .and_then(|f| nearest_note(f, a4));

        match heard {
            Some(r) => {
                self.last_voiced = now;
                match self.candidate {
                    Some((n, _)) if n == r.note => {}
                    _ => self.candidate = Some((r.note, now)),
                }
                if let Some((n, since)) = self.candidate {
                    if now - since >= self.hold {
                        self.stable = Some(StableNote { note: n, cents: r.cents });
                    }
                }
            }
            None if now - self.last_voiced > self.release => {
                self.candidate = None;
                self.stable = None;
            }
            None => {}
        }
        self.stable
    }

    pub fn current(&self) -> Option<StableNote> {
        self.stable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(freq: Option<f32>) -> PitchReading {
        PitchReading { frequency: freq, clarity: 0.95, level_db: -20.0, seq: 1, ..Default::default() }
    }

    #[test]
    fn requires_hold_and_ignores_blips() {
        let mut s = NoteStabilizer::default();
        let a = reading(Some(110.0));
        assert_eq!(s.update(0.00, &a, 440.0), None);
        // A one-frame octave blip resets the candidate rather than flipping.
        s.update(0.05, &reading(Some(220.0)), 440.0);
        assert_eq!(s.update(0.06, &a, 440.0), None);
        let held = s.update(0.15, &a, 440.0).unwrap();
        assert_eq!(held.note, Note(45));
    }

    #[test]
    fn releases_after_silence() {
        let mut s = NoteStabilizer::default();
        let a = reading(Some(110.0));
        s.update(0.0, &a, 440.0);
        assert!(s.update(0.1, &a, 440.0).is_some());
        assert!(s.update(0.2, &reading(None), 440.0).is_some());
        assert!(s.update(0.3, &reading(None), 440.0).is_none());
    }
}
