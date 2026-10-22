//! Anki-style SM-2 spaced repetition for fretboard positions and chords.
//!
//! Each Learn card (a scale or a chord voicing) has an [`SrsCard`], saved in
//! [`crate::LearnProgress`] under the card's key. Because guitar practice happens
//! hands-free with a live instrument, cards are graded automatically from
//! response time and misses, with `1`–`4` keys available during the post-answer
//! pause to override the grade (`1` Again, `2` Hard, `3` Good, `4` Easy).
//!
//! Intervals run from short intra-session learning steps (30s → 2m → 10m)
//! up to multi-day review intervals, measured in Unix epoch seconds so due
//! cards persist across app restarts.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Anki's four review grades.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    /// `1`: lapsed or skipped — reset to the 30s learning step.
    Again,
    /// `2`: slow or hesitant — modest interval growth, slight ease penalty.
    Hard,
    /// `3`: solid recall — standard SM-2 interval growth.
    Good,
    /// `4`: instant recall — bonus interval growth and ease boost.
    Easy,
}

impl Grade {
    pub fn label(self) -> &'static str {
        match self {
            Grade::Again => "Again",
            Grade::Hard => "Hard",
            Grade::Good => "Good",
            Grade::Easy => "Easy",
        }
    }

    /// Auto-grade a note challenge from response time and wrong-note misses.
    pub fn for_note(seconds: f64, misses: u32) -> Self {
        if misses > 0 {
            Grade::Again
        } else if seconds <= 1.2 {
            Grade::Easy
        } else if seconds <= 2.8 {
            Grade::Good
        } else {
            Grade::Hard
        }
    }

    /// Auto-grade a chord challenge from response time.
    pub fn for_chord(seconds: f64) -> Self {
        if seconds <= 2.0 {
            Grade::Easy
        } else if seconds <= 4.5 {
            Grade::Good
        } else {
            Grade::Hard
        }
    }
}

/// SM-2 state for one scale or chord card.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SrsCard {
    /// Ease factor (SM-2 default 2.5, minimum 1.3).
    pub ease: f32,
    /// Current interval in seconds (0 when new).
    pub interval: f64,
    /// Consecutive non-`Again` reviews.
    pub reps: u32,
    /// Unix timestamp (seconds) when this card is next due (`0.0` = new / due now).
    pub due: f64,
}

impl Default for SrsCard {
    fn default() -> Self {
        Self {
            ease: 2.5,
            interval: 0.0,
            reps: 0,
            due: 0.0,
        }
    }
}

impl SrsCard {
    /// True once the card has graduated past the initial learning steps
    /// (at least 2 consecutive good/easy reviews and an interval of >= 2 minutes).
    /// Used by [`crate::GuideMode::Auto`] to hide the shape so the player
    /// recalls it from memory.
    pub fn is_memorized(&self) -> bool {
        self.reps >= 2 && self.interval >= 120.0
    }

    /// True when the card has been reviewed at least once and its due time has passed.
    pub fn is_due(&self, unix_now: f64) -> bool {
        self.reps > 0 && self.due <= unix_now
    }

    /// Apply a review grade at `unix_now`, returning the new card state.
    pub fn review(mut self, grade: Grade, unix_now: f64) -> Self {
        match grade {
            Grade::Again => {
                self.reps = 0;
                self.ease = (self.ease - 0.2).max(1.3);
                self.interval = 30.0;
            }
            Grade::Hard => {
                self.ease = (self.ease - 0.15).max(1.3);
                self.interval = if self.reps == 0 {
                    60.0
                } else {
                    (self.interval * 1.4).max(90.0)
                };
                self.reps += 1;
            }
            Grade::Good => {
                self.interval = match self.reps {
                    0 => 120.0,
                    1 => 600.0,
                    _ => (self.interval * self.ease as f64).max(600.0),
                };
                self.reps += 1;
            }
            Grade::Easy => {
                self.ease = (self.ease + 0.15).min(3.2);
                self.interval = match self.reps {
                    0 => 600.0,
                    1 => 3600.0,
                    _ => (self.interval * self.ease as f64 * 1.3).max(3600.0),
                };
                self.reps += 1;
            }
        }
        self.due = unix_now + self.interval;
        self
    }

    /// Selection weight for Anki-weighted picking at `unix_now`:
    /// - Overdue cards get the highest priority (`12..24`), scaled by how overdue they are.
    /// - New / unseen cards get `6.0` so they're steadily introduced when nothing is due.
    /// - Not-yet-due cards get a small weight (`0.15..1.5`) proportional to how close
    ///   they are to their due time, so continuous practice keeps flowing even when
    ///   every card in a small set is scheduled in the future.
    pub fn weight(&self, unix_now: f64) -> f64 {
        if self.due == 0.0 {
            return 6.0;
        }
        let remaining = self.due - unix_now;
        if remaining <= 0.0 {
            let overdue_ratio = (-remaining / self.interval.max(30.0)).min(4.0);
            12.0 + 3.0 * overdue_ratio
        } else {
            let progress = (1.0 - remaining / self.interval.max(30.0)).clamp(0.1, 1.0);
            0.15 + 1.35 * progress
        }
    }
}

/// Format an interval in seconds compactly for the terminal (`30s`, `2m`, `10m`, `1h`, `3d`).
pub fn format_interval(seconds: f64) -> String {
    let s = seconds.round() as i64;
    if s < 60 {
        format!("{}s", s.max(1))
    } else if s < 3600 {
        format!("{}m", (s + 30) / 60)
    } else if s < 86_400 {
        format!("{}h", (s + 1800) / 3600)
    } else {
        format!("{}d", (s + 43_200) / 86_400)
    }
}

/// Current wall-clock time as seconds since the Unix epoch.
/// If `now` is already a large timestamp (e.g. in a host that passes Unix seconds)
/// we use it directly; otherwise we read `SystemTime::now()` and fall back to `now`
/// in unit tests where `now` starts near 0.
pub fn unix_seconds(now: f64) -> f64 {
    if now > 1_000_000_000.0 {
        return now;
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sm2_intervals_grow_and_lapse_resets() {
        let mut c = SrsCard::default();
        assert!(!c.is_memorized());
        assert_eq!(c.weight(1000.0), 6.0);

        c = c.review(Grade::Good, 1000.0);
        assert_eq!(c.interval, 120.0);
        assert_eq!(c.due, 1120.0);
        assert!(!c.is_memorized());

        // Not due yet at t=1050, due at t=1130.
        assert!(!c.is_due(1050.0));
        assert!(c.is_due(1130.0));
        assert!(c.weight(1130.0) > 12.0);
        assert!(c.weight(1050.0) < 2.0);

        c = c.review(Grade::Good, 1130.0);
        assert_eq!(c.interval, 600.0);
        assert!(c.is_memorized());

        // Lapsing resets reps and drops interval to 30s.
        c = c.review(Grade::Again, 1800.0);
        assert_eq!(c.reps, 0);
        assert_eq!(c.interval, 30.0);
        assert!(!c.is_memorized());
    }

    #[test]
    fn formats_intervals_compactly() {
        assert_eq!(format_interval(30.0), "30s");
        assert_eq!(format_interval(120.0), "2m");
        assert_eq!(format_interval(600.0), "10m");
        assert_eq!(format_interval(3600.0), "1h");
        assert_eq!(format_interval(86_400.0 * 3.0), "3d");
    }
}
