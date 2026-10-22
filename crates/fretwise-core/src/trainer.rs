//! Fretboard note trainer: asks for a note at a position, listens for it,
//! keeps per-position stats, and asks more often about the spots you're weak on.

use std::collections::HashMap;

use crate::theory::{Note, Position, Tuning};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptStyle {
    /// "Play C on the A string" — learn where notes live.
    NameOnString,
    /// A dot on the fretboard — learn what a position is called.
    FretboardDot,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrainerConfig {
    pub min_fret: u8,
    pub max_fret: u8,
    /// Which strings to quiz, lowest first.
    pub strings: [bool; 6],
    pub naturals_only: bool,
    pub style: PromptStyle,
}

impl Default for TrainerConfig {
    fn default() -> Self {
        Self {
            min_fret: 0,
            max_fret: 12,
            strings: [true; 6],
            naturals_only: true,
            style: PromptStyle::NameOnString,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Challenge {
    pub position: Position,
    pub note: Note,
    pub started: f64,
    pub misses: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PositionStats {
    pub attempts: u32,
    pub first_try: u32,
    pub total_time: f64,
}

impl PositionStats {
    pub fn accuracy(&self) -> f64 {
        if self.attempts == 0 {
            0.0
        } else {
            self.first_try as f64 / self.attempts as f64
        }
    }

    pub fn avg_time(&self) -> f64 {
        if self.attempts == 0 {
            0.0
        } else {
            self.total_time / self.attempts as f64
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Feedback {
    Correct { note: Note, seconds: f64 },
    Wrong { played: Note, expected: Note },
    Skipped { expected: Note },
}

pub struct Trainer {
    pub config: TrainerConfig,
    pub tuning: Tuning,
    current: Option<Challenge>,
    stats: HashMap<Position, PositionStats>,
    feedback: Option<(Feedback, f64)>,
    /// Pause after a correct answer before the next prompt appears.
    advance_at: Option<f64>,
    /// The note we last judged; it must change (or go silent) before we judge
    /// again, so one ringing note isn't counted twice.
    last_judged: Option<Note>,
    rng: u64,
    pub streak: u32,
    pub best_streak: u32,
    pub correct: u32,
    pub wrong: u32,
}

impl Trainer {
    pub fn new(tuning: Tuning, config: TrainerConfig, seed: u64) -> Self {
        Self {
            config,
            tuning,
            current: None,
            stats: HashMap::new(),
            feedback: None,
            advance_at: None,
            last_judged: None,
            rng: seed | 1,
            streak: 0,
            best_streak: 0,
            correct: 0,
            wrong: 0,
        }
    }

    pub fn current(&self) -> Option<&Challenge> {
        self.current.as_ref()
    }

    /// Latest feedback and when it happened.
    pub fn feedback(&self) -> Option<(Feedback, f64)> {
        self.feedback
    }

    /// True between a correct answer and the next prompt.
    pub fn is_resolved(&self) -> bool {
        self.advance_at.is_some()
    }

    pub fn stats(&self, pos: Position) -> Option<&PositionStats> {
        self.stats.get(&pos)
    }

    pub fn reset_stats(&mut self) {
        self.stats.clear();
        self.streak = 0;
        self.best_streak = 0;
        self.correct = 0;
        self.wrong = 0;
    }

    pub fn candidates(&self) -> Vec<Position> {
        let c = &self.config;
        let mut out = Vec::new();
        for string in 0..6 {
            if !c.strings[string] {
                continue;
            }
            for fret in c.min_fret..=c.max_fret.max(c.min_fret) {
                let pos = Position { string, fret };
                if c.naturals_only && !self.tuning.note_at(pos).is_natural() {
                    continue;
                }
                out.push(pos);
            }
        }
        out
    }

    /// Pick the next challenge, favouring positions that are slow or missed.
    pub fn next(&mut self, now: f64) {
        self.advance_at = None;
        let prev_note = self.current.map(|c| c.note);
        let mut pool = self.candidates();
        // Never repeat the note that may still be ringing.
        if pool.iter().any(|&p| Some(self.tuning.note_at(p)) != prev_note) {
            pool.retain(|&p| Some(self.tuning.note_at(p)) != prev_note);
        }
        if pool.is_empty() {
            self.current = None;
            return;
        }
        let weights: Vec<f64> = pool.iter().map(|&p| self.weight(p)).collect();
        let total: f64 = weights.iter().sum();
        let mut pick = self.next_f64() * total;
        let mut chosen = pool[pool.len() - 1];
        for (&p, &w) in pool.iter().zip(&weights) {
            if pick < w {
                chosen = p;
                break;
            }
            pick -= w;
        }
        self.current = Some(Challenge {
            position: chosen,
            note: self.tuning.note_at(chosen),
            started: now,
            misses: 0,
        });
    }

    pub fn skip(&mut self, now: f64) {
        if let Some(ch) = self.current {
            let s = self.stats.entry(ch.position).or_default();
            s.attempts += 1;
            s.total_time += now - ch.started;
            self.streak = 0;
            self.wrong += 1;
            self.feedback = Some((Feedback::Skipped { expected: ch.note }, now));
        }
        self.next(now);
    }

    /// Feed the currently held note (from [`crate::NoteStabilizer`]).
    pub fn update(&mut self, now: f64, heard: Option<Note>) {
        if heard.is_none() {
            self.last_judged = None;
        }
        if let Some(at) = self.advance_at {
            if now >= at {
                self.next(now);
            }
            return;
        }
        let Some(ch) = self.current else {
            self.next(now);
            return;
        };
        let Some(note) = heard else { return };
        if self.last_judged == Some(note) {
            return;
        }
        self.last_judged = Some(note);

        if note == ch.note {
            let seconds = now - ch.started;
            let s = self.stats.entry(ch.position).or_default();
            s.attempts += 1;
            s.total_time += seconds;
            if ch.misses == 0 {
                s.first_try += 1;
                self.streak += 1;
                self.best_streak = self.best_streak.max(self.streak);
            }
            self.correct += 1;
            self.feedback = Some((Feedback::Correct { note, seconds }, now));
            self.advance_at = Some(now + 0.6);
        } else {
            if let Some(c) = self.current.as_mut() {
                c.misses += 1;
            }
            self.streak = 0;
            self.wrong += 1;
            self.feedback = Some((Feedback::Wrong { played: note, expected: ch.note }, now));
        }
    }

    fn weight(&self, pos: Position) -> f64 {
        match self.stats.get(&pos) {
            None => 3.0,
            Some(s) => 1.0 + 4.0 * (1.0 - s.accuracy()) + (s.avg_time() / 2.0).min(3.0),
        }
    }

    fn next_f64(&mut self) -> f64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let x = self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trainer() -> Trainer {
        let mut t = Trainer::new(Tuning::STANDARD, TrainerConfig::default(), 42);
        t.next(0.0);
        t
    }

    #[test]
    fn correct_answer_scores_and_advances() {
        let mut t = trainer();
        let target = t.current().unwrap().note;
        t.update(1.5, Some(target));
        assert!(t.is_resolved());
        assert_eq!(t.correct, 1);
        assert_eq!(t.streak, 1);
        t.update(2.5, Some(target));
        let next = t.current().unwrap();
        assert_ne!(next.note, target);
        // The still-ringing old note isn't held against the new prompt.
        assert_eq!(t.wrong, 0);
    }

    #[test]
    fn wrong_note_counts_once_until_it_changes() {
        let mut t = trainer();
        let target = t.current().unwrap().note;
        let wrong = Note(target.0 + 1);
        t.update(0.5, Some(wrong));
        t.update(0.6, Some(wrong));
        assert_eq!(t.wrong, 1);
        t.update(0.7, None);
        t.update(0.8, Some(wrong));
        assert_eq!(t.wrong, 2);
        t.update(1.0, Some(target));
        let s = t.stats(t.current().unwrap().position).unwrap();
        assert_eq!((s.attempts, s.first_try), (1, 0));
    }

    #[test]
    fn candidates_respect_config() {
        let mut t = trainer();
        t.config.strings = [false, false, false, false, false, true];
        t.config.max_fret = 5;
        t.config.naturals_only = true;
        // High E, frets 0..=5 naturals: E F G A
        assert_eq!(t.candidates().len(), 4);
    }
}
