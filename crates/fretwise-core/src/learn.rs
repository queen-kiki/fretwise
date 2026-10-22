//! "Learn" mode: Anki-style spaced repetition (SM-2) over a selected [`DeckKind`].
//!
//! Each deck (`crates/fretwise-core/decks/*.json`) references individual scale
//! cards (`scales/*.json`), individual chord cards (`chords/*.json`), and/or
//! sub-decks. Cards are deduplicated when a collection combines overlapping
//! decks, and SM-2 card state is stored per individual card key so progress is
//! shared across every deck that includes that card.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::analyzer::PitchReading;
use crate::chord::{ChordKind, ChordListener, ChordVoicing};
use crate::deck::DeckKind;
use crate::rng::Rng;
use crate::scale::{Scale, ScaleKind};
use crate::srs::{unix_seconds, Grade, SrsCard};
use crate::theory::{Note, Position, Tuning};

/// Whether to show the scale pattern / chord diagram on the fretboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuideMode {
    /// Show the shape while a card is new or learning; hide it once memorized.
    Auto,
    /// Always show the shape on the neck.
    Show,
    /// Always hide the shape until the card is completed or missed.
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LearnConfig {
    pub deck: DeckKind,
    /// Fret range for scale cards (e.g. `5..=8` for a single position box).
    pub min_fret: u8,
    pub max_fret: u8,
    /// Whether to show the scale pattern / chord diagram on the neck.
    pub guide: GuideMode,
}

impl Default for LearnConfig {
    fn default() -> Self {
        Self {
            deck: DeckKind::default(),
            min_fret: 5,
            max_fret: 8,
            guide: GuideMode::Auto,
        }
    }
}

/// One individual Anki card in a deck.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LearnCard {
    Scale(ScaleKind),
    Chord(ChordKind),
}

impl LearnCard {
    /// Stable storage key for this individual card, shared across all decks.
    pub fn key(self) -> String {
        match self {
            LearnCard::Scale(k) => format!("scale:{}", k.id()),
            LearnCard::Chord(c) => format!("chord:{}", c.id()),
        }
    }

    /// Display name of the card.
    pub fn title(self) -> String {
        match self {
            LearnCard::Scale(k) => k.name().to_string(),
            LearnCard::Chord(c) => c.name().to_string(),
        }
    }

    /// Short category label (`"Scale"` or `"Chord"`).
    pub fn kind_label(self) -> &'static str {
        match self {
            LearnCard::Scale(_) => "Scale",
            LearnCard::Chord(_) => "Chord",
        }
    }

    /// Compact summary string (degree formula for scales, string frets for chords).
    pub fn detail(self) -> String {
        match self {
            LearnCard::Scale(k) => k.formula(),
            LearnCard::Chord(c) => c.voicing().shape_string(),
        }
    }
}

/// Live state of the card currently on stage.
#[derive(Clone, Debug, PartialEq)]
pub enum ActiveCard {
    Scale {
        scale: Scale,
        /// The full untrimmed positions in the fret range (for hand-position fingering).
        box_positions: Vec<Position>,
        /// The pitch-ordered sequence of notes to play from bottom to top.
        sequence: Vec<Position>,
        /// Index into `sequence` of the next note to play.
        step: usize,
        started: f64,
        misses: u32,
    },
    Chord {
        chord: ChordKind,
        started: f64,
        heard_classes: u16,
        heard_strings: u8,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum LearnFeedback {
    Completed {
        title: String,
        seconds: f64,
        grade: Grade,
        interval: f64,
    },
    WrongNote {
        played: Note,
        expected: Note,
    },
    Skipped {
        title: String,
    },
}

/// Saved SM-2 progress for the Learn tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LearnProgress {
    /// Individual card key (`"scale:9:minor-pentatonic"` or `"chord:em-open"`) -> SM-2 state.
    pub cards: HashMap<String, SrsCard>,
    pub reviewed: u32,
    pub streak: u32,
    pub best_streak: u32,
}

pub struct LearnTrainer {
    pub config: LearnConfig,
    pub tuning: Tuning,
    current: Option<ActiveCard>,
    cards: HashMap<String, SrsCard>,
    feedback: Option<(LearnFeedback, f64)>,
    /// `(card_key, card_before_review, unix_now)` so keys `1`–`4` can re-grade
    /// during the post-completion pause.
    last_review: Option<(String, SrsCard, f64)>,
    advance_at: Option<f64>,
    last_judged: Option<Note>,
    repick: Option<Note>,
    listener: ChordListener,
    rng: Rng,
    pub reviewed: u32,
    pub streak: u32,
    pub best_streak: u32,
}

impl LearnTrainer {
    pub fn new(tuning: Tuning, config: LearnConfig, seed: u64) -> Self {
        Self {
            config,
            tuning,
            current: None,
            cards: HashMap::new(),
            feedback: None,
            last_review: None,
            advance_at: None,
            last_judged: None,
            repick: None,
            listener: ChordListener::default(),
            rng: Rng::new(seed),
            reviewed: 0,
            streak: 0,
            best_streak: 0,
        }
    }

    pub fn current(&self) -> Option<&ActiveCard> {
        self.current.as_ref()
    }

    pub fn current_chord(&self) -> Option<&'static ChordVoicing> {
        match self.current {
            Some(ActiveCard::Chord { chord, .. }) => Some(chord.voicing()),
            _ => None,
        }
    }

    pub fn feedback(&self) -> Option<(&LearnFeedback, f64)> {
        self.feedback.as_ref().map(|(f, t)| (f, *t))
    }

    pub fn is_resolved(&self) -> bool {
        self.advance_at.is_some()
    }

    pub fn card_state(&self, key: &str) -> SrsCard {
        self.cards.get(key).copied().unwrap_or_default()
    }

    /// Whether the current card's fretboard guide should be shown right now.
    pub fn show_guide(&self) -> bool {
        if self.is_resolved() {
            return true;
        }
        match self.config.guide {
            GuideMode::Show => true,
            GuideMode::Hide => false,
            GuideMode::Auto => {
                let Some(key) = self.current_key() else { return true };
                !self.card_state(&key).is_memorized()
            }
        }
    }

    pub fn current_key(&self) -> Option<String> {
        match &self.current {
            Some(ActiveCard::Scale { scale, .. }) => Some(LearnCard::Scale(scale.kind).key()),
            Some(ActiveCard::Chord { chord, .. }) => Some(LearnCard::Chord(*chord).key()),
            None => None,
        }
    }

    /// Root pitch class anchored to the 6th string at `config.min_fret` (e.g. A at fret 5).
    pub fn box_root(&self) -> u8 {
        self.tuning
            .note_at(Position {
                string: 0,
                fret: self.config.min_fret,
            })
            .pitch_class()
    }

    pub fn progress(&self) -> LearnProgress {
        LearnProgress {
            cards: self.cards.clone(),
            reviewed: self.reviewed,
            streak: self.streak,
            best_streak: self.best_streak,
        }
    }

    pub fn set_progress(&mut self, p: LearnProgress) {
        self.cards = p.cards;
        self.reviewed = p.reviewed;
        self.streak = p.streak;
        self.best_streak = p.best_streak.max(p.streak);
    }

    pub fn reset_stats(&mut self) {
        self.cards.clear();
        self.reviewed = 0;
        self.streak = 0;
        self.best_streak = 0;
    }

    pub fn reset_card(&mut self, key: &str) {
        self.cards.remove(key);
    }

    /// Immediately put `card` on stage for practice.
    pub fn select_card(&mut self, card: LearnCard, now: f64) {
        self.advance_at = None;
        self.feedback = None;
        self.listener.clear();
        let root = self.box_root();
        self.current = Some(match card {
            LearnCard::Scale(kind) => {
                let scale = Scale { root, kind };
                let (box_positions, sequence) = self.scale_sequence(scale);
                ActiveCard::Scale {
                    scale,
                    box_positions,
                    sequence,
                    step: 0,
                    started: now,
                    misses: 0,
                }
            }
            LearnCard::Chord(chord) => ActiveCard::Chord {
                chord,
                started: now,
                heard_classes: 0,
                heard_strings: 0,
            },
        });
    }

    /// All deduplicated cards in the active deck.
    pub fn deck_cards(&self) -> Vec<LearnCard> {
        let mut out = Vec::new();
        let root = self.box_root();
        for &kind in self.config.deck.scales() {
            if kind.is_chromatic() {
                continue;
            }
            let scale = Scale { root, kind };
            if !self.scale_sequence(scale).1.is_empty() {
                out.push(LearnCard::Scale(kind));
            }
        }
        for &chord in self.config.deck.chords() {
            out.push(LearnCard::Chord(chord));
        }
        out
    }

    /// Counts of `(due, new, memorized)` cards in the active deck at `now`.
    pub fn deck_counts(&self, now: f64) -> (usize, usize, usize) {
        let unix_now = unix_seconds(now);
        let (mut due, mut new_cards, mut memorized) = (0, 0, 0);
        for card in self.deck_cards() {
            let srs = self.card_state(&card.key());
            if srs.due == 0.0 {
                new_cards += 1;
            } else if srs.is_due(unix_now) {
                due += 1;
            }
            if srs.is_memorized() {
                memorized += 1;
            }
        }
        (due, new_cards, memorized)
    }

    /// Override the most recent completed card's Anki grade (`1` Again .. `4` Easy).
    pub fn regrade(&mut self, grade: Grade, now: f64) -> bool {
        let Some((key, before, unix_t)) = self.last_review.clone() else {
            return false;
        };
        let reviewed = before.review(grade, unix_t);
        self.cards.insert(key, reviewed);
        if let Some((LearnFeedback::Completed { grade: g, interval, .. }, _)) = &mut self.feedback {
            *g = grade;
            *interval = reviewed.interval;
        }
        if self.advance_at.is_some() {
            self.advance_at = Some(now + 0.6);
        }
        true
    }

    pub fn restart(&mut self, now: f64) {
        self.current = None;
        self.advance_at = None;
        self.next(now);
    }

    pub fn next(&mut self, now: f64) {
        let prev_key = self.current_key();
        if self.advance_at.take().is_some() {
            if let Some(ActiveCard::Chord { chord, .. }) = self.current {
                self.listener.set_ringing(chord.voicing(), self.tuning);
            }
        }
        self.listener.clear();

        let mut pool = self.deck_cards();
        if pool.len() > 1 {
            pool.retain(|c| Some(c.key()) != prev_key);
        }
        if pool.is_empty() {
            self.current = None;
            return;
        }

        let unix_now = unix_seconds(now);
        let weights: Vec<f64> = pool
            .iter()
            .map(|c| self.card_state(&c.key()).weight(unix_now))
            .collect();
        let chosen = pool[self.rng.pick_weighted(&weights).unwrap_or(pool.len() - 1)];

        let root = self.box_root();
        self.current = Some(match chosen {
            LearnCard::Scale(kind) => {
                let scale = Scale { root, kind };
                let (box_positions, sequence) = self.scale_sequence(scale);
                ActiveCard::Scale {
                    scale,
                    box_positions,
                    sequence,
                    step: 0,
                    started: now,
                    misses: 0,
                }
            }
            LearnCard::Chord(chord) => ActiveCard::Chord {
                chord,
                started: now,
                heard_classes: 0,
                heard_strings: 0,
            },
        });
    }

    pub fn skip(&mut self, now: f64) {
        let unix_now = unix_seconds(now);
        if let Some(key) = self.current_key() {
            let before = self.card_state(&key);
            let reviewed = before.review(Grade::Again, unix_now);
            self.cards.insert(key.clone(), reviewed);
            self.last_review = Some((key, before, unix_now));
            self.streak = 0;
            let title = self.current_title().unwrap_or_default();
            self.feedback = Some((LearnFeedback::Skipped { title }, now));
        }
        self.next(now);
    }

    pub fn current_title(&self) -> Option<String> {
        match &self.current {
            Some(ActiveCard::Scale { scale, .. }) => Some(scale.name()),
            Some(ActiveCard::Chord { chord, .. }) => Some(format!("{} chord", chord.name())),
            None => None,
        }
    }

    pub fn update(&mut self, now: f64, reading: &PitchReading, mono: Option<Note>, a4: f32) {
        if mono.is_none() {
            self.last_judged = None;
        }
        if let Some(at) = self.advance_at {
            if now >= at {
                self.next(now);
            }
            return;
        }
        if self.current.is_none() {
            self.next(now);
        }
        match &self.current {
            Some(ActiveCard::Scale { .. }) => self.update_scale(now, mono),
            Some(ActiveCard::Chord { .. }) => self.update_chord(now, reading, mono, a4),
            None => {}
        }
    }

    fn update_scale(&mut self, now: f64, heard: Option<Note>) {
        let Some(note) = heard else { return };
        if self.last_judged == Some(note) {
            return;
        }
        self.last_judged = Some(note);

        let Some(ActiveCard::Scale {
            scale,
            sequence,
            step,
            started,
            misses,
            ..
        }) = &mut self.current
        else {
            return;
        };
        let Some(&target_pos) = sequence.get(*step) else { return };
        let expected = self.tuning.note_at(target_pos);

        if self.repick == Some(note) && note != expected {
            return;
        }
        if note == expected {
            self.repick = Some(note);
            *step += 1;
            if *step >= sequence.len() {
                let seconds = (now - *started).max(0.1);
                let per_note = seconds / sequence.len().max(1) as f64;
                let grade = Grade::for_note(per_note, *misses);
                let title = scale.name();
                let key = LearnCard::Scale(scale.kind).key();
                self.finish_card(key, title, seconds, grade, now);
            }
        } else {
            *misses += 1;
            self.streak = 0;
            self.feedback = Some((LearnFeedback::WrongNote { played: note, expected }, now));
        }
    }

    fn update_chord(&mut self, now: f64, reading: &PitchReading, mono: Option<Note>, a4: f32) {
        let Some(ActiveCard::Chord { chord, started, .. }) = self.current else {
            return;
        };
        let voicing = chord.voicing();
        let Some(heard) = self.listener.listen(now, reading, mono, a4, voicing, self.tuning) else {
            return;
        };
        if let Some(ActiveCard::Chord {
            heard_classes,
            heard_strings,
            ..
        }) = &mut self.current
        {
            *heard_classes = heard.classes;
            *heard_strings = heard.strings;
        }

        if heard.complete {
            let seconds = (now - started).max(0.1);
            let grade = Grade::for_chord(seconds);
            let key = LearnCard::Chord(chord).key();
            let title = format!("{} chord", voicing.name);
            self.finish_card(key, title, seconds, grade, now);
        }
    }

    fn finish_card(&mut self, key: String, title: String, seconds: f64, grade: Grade, now: f64) {
        let unix_now = unix_seconds(now);
        let before = self.card_state(&key);
        let reviewed = before.review(grade, unix_now);
        self.cards.insert(key.clone(), reviewed);
        self.last_review = Some((key, before, unix_now));
        self.reviewed += 1;
        if grade != Grade::Again {
            self.streak += 1;
            self.best_streak = self.best_streak.max(self.streak);
        } else {
            self.streak = 0;
        }
        self.feedback = Some((
            LearnFeedback::Completed {
                title,
                seconds,
                grade,
                interval: reviewed.interval,
            },
            now,
        ));
        self.advance_at = Some(now + 1.1);
    }

    /// `(box_positions, octave_trimmed_sequence)` for a scale in the configured fret range.
    fn scale_sequence(&self, scale: Scale) -> (Vec<Position>, Vec<Position>) {
        let c = &self.config;
        let mut box_positions = Vec::new();
        for string in 0..6 {
            for fret in c.min_fret..=c.max_fret.max(c.min_fret) {
                let pos = Position { string, fret };
                if scale.contains(self.tuning.note_at(pos)) {
                    box_positions.push(pos);
                }
            }
        }
        let mut candidates = box_positions.clone();
        let root = scale.root % 12;
        let roots = || {
            candidates
                .iter()
                .map(|&p| self.tuning.note_at(p))
                .filter(|n| n.pitch_class() == root)
        };
        if let (Some(lo), Some(hi)) = (roots().min(), roots().max()) {
            if lo < hi {
                candidates.retain(|&p| (lo..=hi).contains(&self.tuning.note_at(p)));
            }
        }
        let tuning = self.tuning;
        candidates.sort_by_key(|&p| (tuning.note_at(p), p.fret));
        candidates.dedup_by_key(|p| tuning.note_at(*p));
        (box_positions, candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_a_scale_card_and_schedules_it_with_sm2() {
        let cfg = LearnConfig {
            deck: DeckKind::by_id("basic-scales").unwrap(),
            min_fret: 5,
            max_fret: 8,
            ..Default::default()
        };
        let mut t = LearnTrainer::new(Tuning::STANDARD, cfg, 42);
        t.restart(0.0);

        let ActiveCard::Scale { sequence, .. } = t.current().unwrap().clone() else {
            panic!("expected scale card");
        };
        assert!(!sequence.is_empty());
        let key = t.current_key().unwrap();

        let mut now = 0.0;
        let empty = PitchReading::default();
        for &pos in &sequence {
            now += 0.4;
            let note = t.tuning.note_at(pos);
            t.update(now, &empty, Some(note), 440.0);
            t.update(now + 0.05, &empty, None, 440.0);
        }
        assert!(t.is_resolved());
        assert_eq!(t.reviewed, 1);
        assert_eq!(t.card_state(&key).interval, 600.0); // fast run -> Easy -> 10m

        // Override grade with Hard (key 2).
        assert!(t.regrade(Grade::Hard, now));
        assert_eq!(t.card_state(&key).interval, 60.0);
    }

    #[test]
    fn chord_card_hears_a_strum_and_ignores_it_ringing_on() {
        let em = ChordKind::by_id("em-open").unwrap();
        let cfg = LearnConfig { deck: DeckKind::by_id("basic-chords").unwrap(), ..Default::default() };
        let mut t = LearnTrainer::new(Tuning::STANDARD, cfg, 3);
        t.select_card(LearnCard::Chord(em), 0.0);

        // E2 B2 G3 held past the poly hold time, loud enough for the gate.
        let mut strum = PitchReading { level_db: -20.0, ..Default::default() };
        for (slot, midi) in strum.poly.iter_mut().zip([40u8, 47, 55]) {
            *slot = Note(midi).frequency(440.0);
        }
        t.update(0.5, &strum, None, 440.0);
        let Some(ActiveCard::Chord { heard_classes, .. }) = t.current() else { panic!("expected chord card") };
        assert_eq!(*heard_classes, 0, "not held long enough yet");
        t.update(0.6, &strum, None, 440.0);
        assert!(t.is_resolved());
        assert_eq!(t.reviewed, 1);
        assert_eq!(t.card_state("chord:em-open").reps, 1);

        // On to another card; Em still ringing doesn't count towards it.
        t.update(2.0, &strum, None, 440.0);
        assert!(!t.is_resolved());
        assert_ne!(t.current_key().as_deref(), Some("chord:em-open"));
        t.update(2.5, &strum, None, 440.0);
        assert!(!t.is_resolved());
        assert_eq!(t.reviewed, 1);
    }

    #[test]
    fn shared_card_progress_carries_across_overlapping_decks() {
        let mut t = LearnTrainer::new(Tuning::STANDARD, LearnConfig::default(), 7);
        t.cards.insert("chord:em-open".into(), SrsCard::default().review(Grade::Easy, 1000.0));
        t.cards.get_mut("chord:em-open").unwrap().reps = 3;

        t.config.deck = DeckKind::by_id("basic-chords").unwrap();
        assert_eq!(t.deck_counts(1010.0).2, 1); // 1 memorized in basic-chords

        t.config.deck = DeckKind::by_id("campfire-acoustic").unwrap();
        assert_eq!(t.deck_counts(1010.0).2, 1); // same card is memorized in campfire-acoustic
    }
}
