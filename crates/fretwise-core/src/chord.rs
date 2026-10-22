//! Individual chord cards (`crates/fretwise-core/chords/*.json`) and the chord
//! trainer.
//!
//! Each chord voicing is stored as its own JSON file and referenced by id from
//! decks (`crates/fretwise-core/decks/*.json`), so overlapping decks share the
//! exact same chord card and stats:
//!
//! ```json
//! {
//!   "id": "em-open",
//!   "name": "Em",
//!   "frets": [0, 2, 2, 0, 0, 0],
//!   "fingers": [0, 2, 3, 0, 0, 0]
//! }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::analyzer::PitchReading;
use crate::deck::DeckKind;
use crate::registry;
use crate::rng::Rng;
use crate::theory::{nearest_note, Note, Position, Tuning};
use crate::trainer::PositionStats;

const BUILTIN: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/builtin_chords.rs"));

const DEFAULT_ID: &str = "em-open";

/// One chord voicing file (`chords/*.json`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChordVoicing {
    pub id: String,
    pub name: String,
    /// Low E (index 0) to high E (index 5); `None` = muted string.
    pub frets: [Option<u8>; 6],
    #[serde(default)]
    pub fingers: [u8; 6],
    #[serde(default)]
    pub barre: Option<u8>,
}

impl ChordVoicing {
    pub fn parse(source: &str, json: &str) -> Result<Self, String> {
        let def: Self = serde_json::from_str(json).map_err(|e| format!("{source}: {e}"))?;
        def.validate().map_err(|e| format!("{source}: {e}"))?;
        Ok(def)
    }

    fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("a chord \"id\" is empty".into());
        }
        if self.name.trim().is_empty() {
            return Err(format!("chord \"{}\": \"name\" is empty", self.id));
        }
        let sounded = self.frets.iter().flatten().count();
        if sounded < 2 {
            return Err(format!("chord \"{}\": needs at least 2 sounded strings", self.id));
        }
        if self.frets.iter().flatten().any(|&f| f > 15) {
            return Err(format!("chord \"{}\": fret above 15", self.id));
        }
        if self.fingers.iter().any(|&f| f > 4) {
            return Err(format!("chord \"{}\": finger must be 0..=4", self.id));
        }
        Ok(())
    }

    /// Sounded positions, from lowest string to highest.
    pub fn positions(&self) -> Vec<Position> {
        self.frets
            .iter()
            .enumerate()
            .filter_map(|(string, &fret)| fret.map(|fret| Position { string, fret }))
            .collect()
    }

    /// Distinct pitch classes (0..12) in this chord under `tuning`, as a 12-bit mask.
    pub fn pitch_class_mask(&self, tuning: Tuning) -> u16 {
        self.positions()
            .into_iter()
            .fold(0u16, |m, pos| m | (1 << tuning.note_at(pos).pitch_class()))
    }

    /// Compact string-by-string fret diagram, e.g. `"x 3 2 0 1 0"`.
    pub fn shape_string(&self) -> String {
        self.frets
            .iter()
            .map(|f| match f {
                Some(fret) => fret.to_string(),
                None => "x".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

struct Registry {
    entries: Vec<ChordVoicing>,
}

impl Registry {
    fn build(sources: impl IntoIterator<Item = registry::Source>) -> (Self, Vec<String>) {
        let (entries, errors) = registry::parse_all(sources, ChordVoicing::parse, |c| &c.id);
        (Self { entries }, errors)
    }

    fn builtin() -> (Self, Vec<String>) {
        Self::build(registry::builtin_sources(BUILTIN))
    }
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| {
        let (registry, errors) = Registry::builtin();
        for e in errors {
            eprintln!("chord {e}");
        }
        registry
    })
}

/// Loads built-in chords plus every `*.json` in `user_dir` (if it exists).
pub fn init(user_dir: Option<&Path>) -> Vec<String> {
    let (sources, mut errors) = registry::sources(BUILTIN, user_dir);
    let (registry, more) = Registry::build(sources);
    errors.extend(more);
    if REGISTRY.set(registry).is_err() {
        errors.push("chords were already loaded; extra chords take effect after a restart".into());
    }
    errors
}

/// Handle to one loaded chord voicing. Saved by its id.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChordKind(u16);

impl ChordKind {
    pub fn all() -> impl Iterator<Item = Self> {
        (0..registry().entries.len()).map(|i| Self(i as u16))
    }

    pub fn by_id(id: &str) -> Option<Self> {
        registry().entries.iter().position(|e| e.id == id).map(|i| Self(i as u16))
    }

    pub fn voicing(self) -> &'static ChordVoicing {
        &registry().entries[self.0 as usize]
    }

    pub fn id(self) -> &'static str {
        &self.voicing().id
    }

    pub fn name(self) -> &'static str {
        &self.voicing().name
    }
}

impl Default for ChordKind {
    fn default() -> Self {
        Self::by_id(DEFAULT_ID).unwrap_or(Self(0))
    }
}

impl fmt::Debug for ChordKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ChordKind({:?})", self.id())
    }
}

impl Serialize for ChordKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for ChordKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        Ok(Self::by_id(&id).unwrap_or_else(|| {
            eprintln!("unknown chord \"{id}\"; using {DEFAULT_ID}");
            Self::default()
        }))
    }
}

/// Chord families, read from the voicing file's id suffix (`a-open`,
/// `c-barre`, `g5-power`, `g7-jazz`); anything else counts as jazz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChordFamily {
    Open,
    Barre,
    Power,
    Jazz,
}

impl ChordFamily {
    pub const ALL: [ChordFamily; 4] = [Self::Open, Self::Barre, Self::Power, Self::Jazz];

    fn of(id: &str) -> Self {
        if id.ends_with("-open") {
            Self::Open
        } else if id.ends_with("-barre") {
            Self::Barre
        } else if id.ends_with("-power") {
            Self::Power
        } else {
            Self::Jazz
        }
    }

    /// "Open chords", as a group heading.
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "Open chords",
            Self::Barre => "Barre chords",
            Self::Power => "Power chords",
            Self::Jazz => "Jazz chords",
        }
    }

    /// "All open chords", as a choice that drills the whole family.
    pub fn all_label(self) -> &'static str {
        match self {
            Self::Open => "All open chords",
            Self::Barre => "All barre chords",
            Self::Power => "All power chords",
            Self::Jazz => "All jazz chords",
        }
    }

    /// The family's chords, sorted by name.
    pub fn chords(self) -> Vec<ChordKind> {
        let mut chords: Vec<ChordKind> = ChordKind::all().filter(|c| c.family() == self).collect();
        chords.sort_by_key(|c| c.name());
        chords
    }
}

impl ChordKind {
    pub fn family(self) -> ChordFamily {
        ChordFamily::of(self.id())
    }
}

/// What the chord drill asks for: a whole family, or one chord to get clean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChordPick {
    Family(ChordFamily),
    One(ChordKind),
    /// A Learn deck's chords, in deck order (not offered in the UI).
    Deck(DeckKind),
}

impl Default for ChordPick {
    fn default() -> Self {
        Self::Family(ChordFamily::Open)
    }
}

impl ChordPick {
    pub fn chords(self) -> Vec<ChordKind> {
        match self {
            Self::Family(f) => f.chords(),
            Self::One(c) => vec![c],
            Self::Deck(d) => d.chords().to_vec(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Family(f) => f.all_label(),
            Self::One(c) => c.name(),
            Self::Deck(d) => d.name(),
        }
    }
}

/// How the chord trainer walks through the selected chords.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChordOrder {
    /// Weighted towards chords you're slow on or miss.
    Random,
    /// In deck order, looping.
    InOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChordTrainerConfig {
    /// Unused since the drill follows `pick`; kept so saved settings from
    /// before chord picks still load and save unchanged.
    #[serde(alias = "set")]
    pub deck: DeckKind,
    /// Which chords the drill asks for.
    pub pick: ChordPick,
    pub order: ChordOrder,
    /// Show the chord's dots and finger numbers on the neck before it's played.
    pub show_diagram: bool,
}

impl Default for ChordTrainerConfig {
    fn default() -> Self {
        Self {
            deck: DeckKind::default_chord_deck(),
            pick: ChordPick::default(),
            order: ChordOrder::Random,
            show_diagram: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChordChallenge {
    pub chord: ChordKind,
    pub started: f64,
    /// Pitch classes (12-bit mask) heard so far during the current strum/arpeggio.
    pub heard_classes: u16,
    /// Specific chord notes heard so far (by string index 0..6, bit `s` set).
    pub heard_strings: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChordFeedback {
    Correct { name: String, seconds: f64 },
    Skipped { name: String },
}

/// Saved per-chord stats.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChordProgress {
    pub stats: HashMap<String, PositionStats>,
    pub streak: u32,
    pub best_streak: u32,
    pub correct: u32,
}

/// Seconds a rolling strum / arpeggio keeps accumulated notes before silence clears them.
const STRUM_WINDOW: f64 = 1.4;
/// Seconds a polyphonic FFT pitch class must persist before it counts (mirrors `NoteStabilizer`).
const POLY_HOLD: f64 = 0.08;
/// Minimum short-term input level (dBFS) to accept polyphonic chord notes.
const CHORD_GATE_DB: f32 = -45.0;
/// How far (in cents) a polyphonic note may sit from the nearest semitone.
const POLY_CENTS: f32 = 35.0;
/// Seconds of silence after which a ringing chord and the notes heard so far are forgotten.
const SILENCE: f64 = 0.2;
/// Seconds between a correct chord in the drill and the next prompt.
const ADVANCE_DELAY: f64 = 0.75;

/// What [`ChordListener::listen`] heard of the target chord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChordHeard {
    /// The chord's pitch classes heard within the strum window (12-bit mask).
    pub classes: u16,
    /// Strings whose note has been heard (bit `s` for string index `s`).
    pub strings: u8,
    /// Every pitch class of the chord heard, and nothing outside it this frame.
    pub complete: bool,
}

/// Listens for a chord over successive readings: a full strum (via
/// [`PitchReading::poly`]) or a slow strum / arpeggio whose pitch classes
/// arrive within [`STRUM_WINDOW`] seconds. Shared by the chord drill and
/// Learn's chord cards.
#[derive(Clone, Debug)]
pub(crate) struct ChordListener {
    poly_since: [Option<f64>; 12],
    class_seen_at: [f64; 12],
    /// The previous chord, still ringing: its notes alone don't count
    /// towards the next one until something new is played or it goes quiet.
    ringing_mask: u16,
    last_voiced: f64,
}

impl Default for ChordListener {
    fn default() -> Self {
        Self {
            poly_since: [None; 12],
            class_seen_at: [f64::NEG_INFINITY; 12],
            ringing_mask: 0,
            last_voiced: f64::NEG_INFINITY,
        }
    }
}

impl ChordListener {
    /// Forget the notes heard so far, for a new chord.
    pub fn clear(&mut self) {
        self.poly_since = [None; 12];
        self.class_seen_at = [f64::NEG_INFINITY; 12];
    }

    /// `chord` was just played and may ring on into the next prompt.
    pub fn set_ringing(&mut self, chord: &ChordVoicing, tuning: Tuning) {
        self.ringing_mask = chord.pitch_class_mask(tuning);
    }

    /// Feed one reading and the stabilized mono note. `None` when the frame
    /// is only the previous chord ringing on, and should be ignored.
    pub fn listen(
        &mut self,
        now: f64,
        reading: &PitchReading,
        mono: Option<Note>,
        a4: f32,
        voicing: &ChordVoicing,
        tuning: Tuning,
    ) -> Option<ChordHeard> {
        let target_mask = voicing.pitch_class_mask(tuning);

        let raw_poly_mask = if reading.level_db >= CHORD_GATE_DB {
            reading
                .poly
                .iter()
                .copied()
                .filter(|&f| f > 0.0)
                .filter_map(|f| nearest_note(f, a4))
                .filter(|r| r.cents.abs() <= POLY_CENTS)
                .fold(0u16, |m, r| m | (1 << r.note.pitch_class()))
        } else {
            0
        };

        let mut frame_mask = 0u16;
        for pc in 0..12usize {
            if (raw_poly_mask & (1 << pc)) != 0 {
                let since = *self.poly_since[pc].get_or_insert(now);
                if now - since >= POLY_HOLD {
                    frame_mask |= 1 << pc;
                }
            } else {
                self.poly_since[pc] = None;
            }
        }
        if let Some(n) = mono {
            frame_mask |= 1 << n.pitch_class();
        }

        if raw_poly_mask == 0 && mono.is_none() {
            if now - self.last_voiced > SILENCE {
                self.ringing_mask = 0;
                self.class_seen_at = [f64::NEG_INFINITY; 12];
            }
        } else {
            self.last_voiced = now;
        }

        if self.ringing_mask != 0
            && frame_mask != 0
            && (frame_mask & !self.ringing_mask) == 0
            && self.ringing_mask != target_mask
        {
            return None;
        }
        if frame_mask & !self.ringing_mask != 0 {
            self.ringing_mask = 0;
        }

        for pc in 0..12usize {
            if (frame_mask & (1 << pc)) != 0 {
                self.class_seen_at[pc] = now;
            }
        }

        let mut active_classes = 0u16;
        for (pc, &t) in self.class_seen_at.iter().enumerate() {
            if now - t <= STRUM_WINDOW {
                active_classes |= 1 << pc;
            }
        }

        let mut heard_strings = 0u8;
        for (string, fret) in voicing.frets.iter().enumerate() {
            if let Some(fret) = *fret {
                let pc = tuning.note_at(Position { string, fret }).pitch_class();
                if active_classes & (1 << pc) != 0 {
                    heard_strings |= 1 << string;
                }
            }
        }

        let extra_in_frame = (frame_mask & !target_mask).count_ones();
        Some(ChordHeard {
            classes: active_classes & target_mask,
            strings: heard_strings,
            complete: (active_classes & target_mask) == target_mask && extra_in_frame == 0,
        })
    }
}

/// Chord trainer: asks for a chord from the active pick and accepts either a
/// full strum (via [`PitchReading::poly`]) or a slow strum / arpeggio.
pub struct ChordTrainer {
    pub config: ChordTrainerConfig,
    pub tuning: Tuning,
    current: Option<ChordChallenge>,
    stats: HashMap<String, PositionStats>,
    feedback: Option<(ChordFeedback, f64)>,
    advance_at: Option<f64>,
    listener: ChordListener,
    seq_step: usize,
    rng: Rng,
    pub streak: u32,
    pub best_streak: u32,
    pub correct: u32,
}

impl ChordTrainer {
    pub fn new(tuning: Tuning, config: ChordTrainerConfig, seed: u64) -> Self {
        Self {
            config,
            tuning,
            current: None,
            stats: HashMap::new(),
            feedback: None,
            advance_at: None,
            listener: ChordListener::default(),
            seq_step: 0,
            rng: Rng::new(seed),
            streak: 0,
            best_streak: 0,
            correct: 0,
        }
    }

    pub fn current(&self) -> Option<&ChordChallenge> {
        self.current.as_ref()
    }

    pub fn current_chord(&self) -> Option<&'static ChordVoicing> {
        Some(self.current?.chord.voicing())
    }

    pub fn feedback(&self) -> Option<(&ChordFeedback, f64)> {
        self.feedback.as_ref().map(|(f, t)| (f, *t))
    }

    pub fn is_resolved(&self) -> bool {
        self.advance_at.is_some()
    }

    pub fn progress(&self) -> ChordProgress {
        ChordProgress {
            stats: self.stats.clone(),
            streak: self.streak,
            best_streak: self.best_streak,
            correct: self.correct,
        }
    }

    pub fn set_progress(&mut self, p: ChordProgress) {
        self.stats = p.stats;
        self.streak = p.streak;
        self.best_streak = p.best_streak.max(p.streak);
        self.correct = p.correct;
    }

    pub fn reset_stats(&mut self) {
        self.stats.clear();
        self.streak = 0;
        self.best_streak = 0;
        self.correct = 0;
    }

    pub fn restart(&mut self, now: f64) {
        self.current = None;
        self.advance_at = None;
        self.seq_step = 0;
        self.next(now);
    }

    /// Immediately put `chord` on stage.
    pub fn select_chord(&mut self, chord: ChordKind, now: f64) {
        self.advance_at = None;
        self.feedback = None;
        self.listener.clear();
        self.current = Some(ChordChallenge {
            chord,
            started: now,
            heard_classes: 0,
            heard_strings: 0,
        });
    }

    pub fn next(&mut self, now: f64) {
        let prev = self.current.map(|c| c.chord);
        if self.advance_at.take().is_some() {
            if let Some(chord) = prev {
                self.listener.set_ringing(chord.voicing(), self.tuning);
            }
        }
        self.listener.clear();
        let chords = self.config.pick.chords();
        if chords.is_empty() {
            self.current = None;
            return;
        }
        let chord = match self.config.order {
            ChordOrder::InOrder => {
                let c = chords[self.seq_step % chords.len()];
                self.seq_step += 1;
                c
            }
            ChordOrder::Random => self.pick_random(&chords, prev),
        };
        self.current = Some(ChordChallenge {
            chord,
            started: now,
            heard_classes: 0,
            heard_strings: 0,
        });
    }

    fn pick_random(&mut self, chords: &[ChordKind], prev: Option<ChordKind>) -> ChordKind {
        let pool: Vec<ChordKind> = chords
            .iter()
            .copied()
            .filter(|&c| chords.len() == 1 || Some(c) != prev)
            .collect();
        let weights: Vec<f64> = pool.iter().map(|&c| self.weight(c.id())).collect();
        self.rng.pick_weighted(&weights).map_or(chords[0], |i| pool[i])
    }

    pub fn skip(&mut self, now: f64) {
        if let Some(voicing) = self.current_chord() {
            let s = self.stats.entry(voicing.id.clone()).or_default();
            s.attempts += 1;
            if let Some(ch) = self.current {
                s.total_time += now - ch.started;
            }
            self.streak = 0;
            self.feedback = Some((ChordFeedback::Skipped { name: voicing.name.clone() }, now));
        }
        self.next(now);
    }

    /// Update with the latest [`PitchReading`] and optional stabilized mono note.
    pub fn update(&mut self, now: f64, reading: &PitchReading, mono: Option<Note>, a4: f32) {
        if let Some(at) = self.advance_at {
            if now >= at {
                self.next(now);
            }
            return;
        }
        if self.current.is_none() {
            self.next(now);
        }
        let Some(voicing) = self.current_chord() else { return };
        let Some(heard) = self.listener.listen(now, reading, mono, a4, voicing, self.tuning) else {
            return;
        };
        if let Some(ch) = self.current.as_mut() {
            ch.heard_classes = heard.classes;
            ch.heard_strings = heard.strings;
        }

        if heard.complete {
            let started = self.current.map_or(now, |c| c.started);
            let seconds = (now - started).max(0.0);
            let s = self.stats.entry(voicing.id.clone()).or_default();
            s.attempts += 1;
            s.first_try += 1;
            s.total_time += seconds;
            self.streak += 1;
            self.best_streak = self.best_streak.max(self.streak);
            self.correct += 1;
            self.feedback = Some((ChordFeedback::Correct { name: voicing.name.clone(), seconds }, now));
            self.advance_at = Some(now + ADVANCE_DELAY);
        }
    }

    fn weight(&self, id: &str) -> f64 {
        match self.stats.get(id) {
            None => 3.0,
            Some(s) => 1.0 + (s.avg_time() / 2.0).min(3.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_chord_loads() {
        let (registry, errors) = Registry::builtin();
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(registry.entries.len(), BUILTIN.len());
        for id in ["em-open", "am-open", "c-open", "g-open", "e5-power", "f-barre", "bm-barre"] {
            assert!(ChordKind::by_id(id).is_some(), "{id}");
        }
    }

    #[test]
    fn recognizes_strum_and_slow_arpeggio() {
        let cfg = ChordTrainerConfig {
            deck: DeckKind::by_id("first-chords").unwrap(),
            pick: ChordPick::Deck(DeckKind::by_id("first-chords").unwrap()),
            order: ChordOrder::InOrder,
            show_diagram: true,
        };
        let mut t = ChordTrainer::new(Tuning::STANDARD, cfg, 7);
        t.restart(0.0);
        assert_eq!(t.current_chord().unwrap().name, "Em");

        // 1) Strum via poly (held for at least POLY_HOLD, above CHORD_GATE_DB): E2 B2 G3.
        let mut r = PitchReading { level_db: -20.0, ..Default::default() };
        for (slot, midi) in r.poly.iter_mut().zip([40u8, 47, 55]) {
            *slot = Note(midi).frequency(440.0);
        }
        t.update(0.4, &r, None, 440.0);
        assert!(!t.is_resolved());
        t.update(0.5, &r, None, 440.0);
        assert!(t.is_resolved());
        assert_eq!(t.correct, 1);

        // Advance to Am, let silence clear the ringing Em, then arpeggiate A-C-E one by one.
        let silent = PitchReading { level_db: -80.0, ..Default::default() };
        t.update(1.5, &silent, None, 440.0);
        assert!(!t.is_resolved());
        assert_eq!(t.current_chord().unwrap().name, "Am");
        t.update(2.0, &silent, None, 440.0);
        t.update(2.1, &silent, Some(Note(45)), 440.0); // A2
        assert!(!t.is_resolved());
        t.update(2.5, &silent, Some(Note(52)), 440.0); // E3
        assert!(!t.is_resolved());
        t.update(2.9, &silent, Some(Note(60)), 440.0); // C4
        assert!(t.is_resolved());
        assert_eq!(t.correct, 2);

        t.reset_stats();
        assert_eq!(t.progress(), ChordProgress::default());
    }
}
