//! Fretboard note trainer: asks for a note at a position, listens for it,
//! and keeps per-position stats. Notes come from a scale, either in random
//! order (favouring the spots you're weak on) or walked up and down as a
//! pattern. Plain note-finding is the chromatic scale (or C major for
//! naturals only) in random order.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::rng::Rng;
use crate::scale::{self, Scale};
use crate::theory::{Note, Position, Tuning};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptStyle {
    /// "Play C on the A string" — learn where notes live.
    NameOnString,
    /// A dot on the fretboard — learn what a position is called.
    FretboardDot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Order {
    /// Weighted towards positions that are slow or missed.
    Random,
    /// Low to high through the scale, then start over.
    Ascending,
    /// Low to high and back down.
    UpDown,
}

/// How prompts and markers name notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Labels {
    /// "C3"
    NoteName,
    /// "b3", relative to the scale's root.
    Degree,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrainerConfig {
    pub min_fret: u8,
    pub max_fret: u8,
    /// Which strings to quiz, lowest first.
    pub strings: [bool; 6],
    /// Which notes to ask for. C major is the natural notes.
    pub scale: Scale,
    pub order: Order,
    pub labels: Labels,
    pub style: PromptStyle,
}

impl Default for TrainerConfig {
    fn default() -> Self {
        Self {
            min_fret: 0,
            max_fret: 12,
            strings: [true; 6],
            scale: Scale::default(),
            order: Order::Random,
            labels: Labels::NoteName,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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

/// Everything the trainer has learned about the player, for saving between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    /// Stored as a list: JSON object keys can only be strings.
    #[serde(with = "stats_list")]
    pub stats: HashMap<Position, PositionStats>,
    pub streak: u32,
    pub best_streak: u32,
    pub correct: u32,
    pub wrong: u32,
}

mod stats_list {
    use super::*;
    use serde::{Deserializer, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Entry {
        #[serde(flatten)]
        position: Position,
        #[serde(flatten)]
        stats: PositionStats,
    }

    pub fn serialize<S: Serializer>(map: &HashMap<Position, PositionStats>, s: S) -> Result<S::Ok, S::Error> {
        // Sorted so saved files diff cleanly.
        let mut entries: Vec<Entry> = map.iter().map(|(&position, &stats)| Entry { position, stats }).collect();
        entries.sort_by_key(|e| (e.position.string, e.position.fret));
        entries.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<HashMap<Position, PositionStats>, D::Error> {
        let entries = Vec::<Entry>::deserialize(d)?;
        Ok(entries
            .into_iter()
            .filter(|e| e.position.string < 6)
            .map(|e| (e.position, e.stats))
            .collect())
    }
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
    /// The previous challenge's note, if it was answered correctly. Picking it
    /// again during the next challenge is ignored rather than counted wrong.
    repick: Option<Note>,
    /// Steps taken through the pattern in [`Order::Ascending`]/[`Order::UpDown`].
    seq_step: usize,
    rng: Rng,
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
            repick: None,
            seq_step: 0,
            rng: Rng::new(seed),
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

    pub fn progress(&self) -> Progress {
        Progress {
            stats: self.stats.clone(),
            streak: self.streak,
            best_streak: self.best_streak,
            correct: self.correct,
            wrong: self.wrong,
        }
    }

    pub fn set_progress(&mut self, p: Progress) {
        self.stats = p.stats;
        self.streak = p.streak;
        self.best_streak = p.best_streak.max(p.streak);
        self.correct = p.correct;
        self.wrong = p.wrong;
    }

    pub fn reset_stats(&mut self) {
        self.stats.clear();
        self.streak = 0;
        self.best_streak = 0;
        self.correct = 0;
        self.wrong = 0;
    }

    /// Every position of the scale in the fret range on the strings that are
    /// on, untrimmed: the hand position fingering is worked out from.
    pub fn fingering_positions(&self) -> Vec<Position> {
        let c = &self.config;
        c.scale.positions(self.tuning, c.min_fret..=c.max_fret.max(c.min_fret), c.strings)
    }

    /// The positions to ask about: [`Self::fingering_positions`] trimmed to
    /// whole octaves of the root.
    pub fn candidates(&self) -> Vec<Position> {
        let mut out = self.fingering_positions();
        self.config.scale.trim_to_octaves(self.tuning, &mut out);
        out
    }

    /// The candidates in pitch order, one per pitch (see [`scale::walk`]).
    pub fn sequence(&self) -> Vec<Position> {
        scale::walk(self.tuning, self.candidates())
    }

    /// Start over, e.g. after the settings changed: patterns begin again
    /// from the bottom.
    pub fn restart(&mut self, now: f64) {
        self.current = None;
        self.advance_at = None;
        self.seq_step = 0;
        self.next(now);
    }

    /// Move on to the next challenge.
    pub fn next(&mut self, now: f64) {
        let prev_note = self.current.map(|c| c.note);
        // `advance_at` is only set after a correct answer.
        self.repick = self.advance_at.take().and(prev_note);
        let chosen = match self.config.order {
            Order::Random => self.pick_random(prev_note),
            Order::Ascending | Order::UpDown => self.pick_in_sequence(),
        };
        self.current = chosen.map(|position| Challenge {
            position,
            note: self.tuning.note_at(position),
            started: now,
            misses: 0,
        });
    }

    fn pick_random(&mut self, prev_note: Option<Note>) -> Option<Position> {
        let mut pool = self.candidates();
        // Never repeat the note that may still be ringing.
        if pool.iter().any(|&p| Some(self.tuning.note_at(p)) != prev_note) {
            pool.retain(|&p| Some(self.tuning.note_at(p)) != prev_note);
        }
        if pool.is_empty() {
            return None;
        }
        let weights: Vec<f64> = pool.iter().map(|&p| self.weight(p)).collect();
        self.rng.pick_weighted(&weights).map(|i| pool[i])
    }

    fn pick_in_sequence(&mut self) -> Option<Position> {
        let mut cycle = self.sequence();
        if self.config.order == Order::UpDown && cycle.len() > 2 {
            // a b c d → a b c d c b, then a again.
            let down: Vec<Position> = cycle[1..cycle.len() - 1].iter().rev().copied().collect();
            cycle.extend(down);
        }
        if cycle.is_empty() {
            return None;
        }
        let p = cycle[self.seq_step % cycle.len()];
        self.seq_step += 1;
        Some(p)
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

        if self.repick == Some(note) && note != ch.note {
            return;
        }
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
    fn repicking_previous_target_is_not_wrong() {
        let mut t = trainer();
        let first = t.current().unwrap().note;
        t.update(1.0, Some(first));
        t.update(2.0, None);
        assert!(!t.is_resolved());
        let target = t.current().unwrap().note;
        // Pick the old note again, a couple of times.
        t.update(2.5, Some(first));
        t.update(2.6, None);
        t.update(2.7, Some(first));
        assert_eq!(t.wrong, 0);
        assert_eq!(t.current().unwrap().misses, 0);
        // Other wrong notes still count.
        t.update(3.0, Some(Note(target.0.max(first.0) + 1)));
        assert_eq!(t.wrong, 1);
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
    fn progress_round_trips_through_json() {
        let mut t = trainer();
        for i in 0..5 {
            let note = t.current().unwrap().note;
            t.update(i as f64 * 2.0 + 0.5, Some(Note(note.0 + 1)));
            t.update(i as f64 * 2.0 + 1.0, Some(note));
            t.update(i as f64 * 2.0 + 1.9, None);
        }
        let p = t.progress();
        assert!(!p.stats.is_empty());
        let json = serde_json::to_string(&p).unwrap();
        let back: Progress = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);

        let mut fresh = Trainer::new(Tuning::STANDARD, TrainerConfig::default(), 1);
        fresh.set_progress(back);
        assert_eq!(fresh.progress(), p);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let p: Progress = serde_json::from_str(r#"{"correct": 3}"#).unwrap();
        assert_eq!(p.correct, 3);
        assert!(p.stats.is_empty());
        let c: TrainerConfig = serde_json::from_str(r#"{"max_fret": 5}"#).unwrap();
        assert_eq!(c, TrainerConfig { max_fret: 5, ..TrainerConfig::default() });
    }

    #[test]
    fn config_from_before_scale_files_loads() {
        use crate::scale::ScaleKind;
        let old = r#"{"min_fret": 5, "max_fret": 8, "strings": [true, true, true, true, true, true],
            "scale": {"root": 9, "kind": "MinorPentatonic"}, "order": "UpDown", "labels": "Degree",
            "style": "NameOnString"}"#;
        let c: TrainerConfig = serde_json::from_str(old).unwrap();
        assert_eq!(c.scale, Scale { root: 9, kind: ScaleKind::by_id("minor-pentatonic").unwrap() });
        assert_eq!(c.order, Order::UpDown);
        // Saved again, it's the new format, and reads back the same.
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""kind":"minor-pentatonic""#), "{json}");
        assert_eq!(serde_json::from_str::<TrainerConfig>(&json).unwrap(), c);
    }

    #[test]
    fn up_down_walks_the_pattern() {
        use crate::scale::ScaleKind;
        let mut t = trainer();
        // A minor pentatonic, box 1.
        t.config.scale = Scale { root: 9, kind: ScaleKind::by_id("minor-pentatonic").unwrap() };
        t.config.min_fret = 5;
        t.config.max_fret = 8;
        t.config.order = Order::UpDown;
        t.restart(0.0);

        let seq = t.sequence();
        let names: Vec<String> = seq.iter().map(|&p| t.tuning.note_at(p).to_string()).collect();
        // Trimmed to root-to-root octaves (A2..=A4, leaving out the C5 on fret 8).
        assert_eq!(names, ["A2", "C3", "D3", "E3", "G3", "A3", "C4", "D4", "E4", "G4", "A4"]);

        let mut played = Vec::new();
        let mut now = 0.0;
        for _ in 0..(2 * seq.len() - 1) {
            let ch = *t.current().unwrap();
            played.push(ch.position);
            now += 1.0;
            t.update(now, Some(ch.note));
            t.update(now + 0.7, None);
        }
        let mut expected = seq.clone();
        expected.extend(seq[1..seq.len() - 1].iter().rev());
        expected.push(seq[0]);
        assert_eq!(played, expected);
        assert_eq!(t.wrong, 0);
    }

    #[test]
    fn candidates_respect_config() {
        let mut t = trainer();
        t.config.strings = [false, false, false, false, false, true];
        t.config.max_fret = 5;
        t.config.scale = Scale::default();
        // High E, frets 0..=5 naturals: E F G A
        assert_eq!(t.candidates().len(), 4);
    }
}
