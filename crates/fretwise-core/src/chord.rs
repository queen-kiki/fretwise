//! Chords and chord curricula, loaded from JSON files just like scales.
//!
//! Each curriculum file in `crates/fretwise-core/chords/` (or the user's
//! `%APPDATA%\Fretwise\chords\`) defines a lesson group of chord voicings:
//!
//! ```json
//! {
//!   "id": "beginner-minor",
//!   "name": "Em & Am",
//!   "group": "beginner",
//!   "order": 1,
//!   "chords": [
//!     {
//!       "id": "em-open",
//!       "name": "Em",
//!       "frets": [0, 2, 2, 0, 0, 0],
//!       "fingers": [0, 2, 3, 0, 0, 0]
//!     }
//!   ]
//! }
//! ```
//!
//! - `frets`: 6 entries from string 6 (low E, index 0) to string 1 (high E,
//!   index 5). `null` means the string is muted (`x`), `0` is open, `1..=15`
//!   is a fretted note.
//! - `fingers` (optional): 0 for muted/open, 1 (index) to 4 (pinky).
//! - `barre` (optional): fret barred by the index finger.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::analyzer::PitchReading;
use crate::theory::{nearest_note, Note, Position, Tuning};
use crate::trainer::PositionStats;

const BUILTIN: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/builtin_chords.rs"));

const DEFAULT_ID: &str = "open-chords";

/// One chord voicing in a curriculum.
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

    /// Sounded notes from lowest string to highest under `tuning`.
    pub fn notes(&self, tuning: Tuning) -> Vec<Note> {
        self.positions().into_iter().map(|p| tuning.note_at(p)).collect()
    }
}

/// One curriculum JSON file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChordSetDef {
    pub id: String,
    pub name: String,
    pub group: String,
    #[serde(default = "default_order")]
    pub order: i32,
    pub chords: Vec<ChordVoicing>,
}

fn default_order() -> i32 {
    1000
}

impl ChordSetDef {
    pub fn parse(source: &str, json: &str) -> Result<Self, String> {
        let def: Self = serde_json::from_str(json).map_err(|e| format!("{source}: {e}"))?;
        def.validate().map_err(|e| format!("{source}: {e}"))?;
        Ok(def)
    }

    fn validate(&self) -> Result<(), String> {
        for (field, value) in [("id", &self.id), ("name", &self.name), ("group", &self.group)] {
            if value.trim().is_empty() {
                return Err(format!("\"{field}\" is empty"));
            }
        }
        if self.chords.is_empty() {
            return Err("\"chords\" is empty".into());
        }
        let mut ids = HashSet::new();
        for c in &self.chords {
            c.validate()?;
            if !ids.insert(c.id.as_str()) {
                return Err(format!("duplicate chord id \"{}\" in set", c.id));
            }
        }
        Ok(())
    }
}

struct Registry {
    entries: Vec<ChordSetDef>,
    groups: Vec<(String, Range<usize>)>,
}

impl Registry {
    fn build(sources: impl IntoIterator<Item = (String, String)>) -> (Self, Vec<String>) {
        let mut errors = Vec::new();
        let mut ids = HashSet::new();
        let mut defs = Vec::new();
        for (source, json) in sources {
            match ChordSetDef::parse(&source, &json) {
                Ok(def) if !ids.insert(def.id.clone()) => {
                    errors.push(format!("{source}: id \"{}\" is already taken; skipped", def.id))
                }
                Ok(def) => defs.push(def),
                Err(e) => errors.push(e),
            }
        }

        let group_order = |group: &str, defs: &[ChordSetDef]| {
            defs.iter().filter(|d| d.group == group).map(|d| d.order).min().unwrap_or(i32::MAX)
        };
        let keys: Vec<i32> = defs.iter().map(|d| group_order(&d.group, &defs)).collect();
        let mut keyed: Vec<_> = keys.into_iter().zip(defs).collect();
        keyed.sort_by(|(ga, a), (gb, b)| {
            (ga, &a.group, a.order, &a.name, &a.id).cmp(&(gb, &b.group, b.order, &b.name, &b.id))
        });

        let mut entries = Vec::new();
        let mut groups: Vec<(String, Range<usize>)> = Vec::new();
        for (_, def) in keyed {
            match groups.last_mut() {
                Some((name, range)) if *name == def.group => range.end += 1,
                _ => groups.push((def.group.clone(), entries.len()..entries.len() + 1)),
            }
            entries.push(def);
        }
        (Self { entries, groups }, errors)
    }

    fn builtin() -> (Self, Vec<String>) {
        Self::build(builtin_sources())
    }
}

fn builtin_sources() -> impl Iterator<Item = (String, String)> {
    BUILTIN.iter().map(|(name, json)| (format!("built-in {name}"), json.to_string()))
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

/// Loads built-in chord sets plus every `*.json` in `user_dir` (if it exists).
pub fn init(user_dir: Option<&Path>) -> Vec<String> {
    let mut sources: Vec<(String, String)> = builtin_sources().collect();
    let mut errors = Vec::new();
    if let Some(dir) = user_dir {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                let mut paths: Vec<_> = entries
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
                    .collect();
                paths.sort();
                for path in paths {
                    match std::fs::read_to_string(&path) {
                        Ok(json) => sources.push((path.display().to_string(), json)),
                        Err(e) => errors.push(format!("{}: {e}", path.display())),
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => errors.push(format!("{}: {e}", dir.display())),
        }
    }
    let (registry, more) = Registry::build(sources);
    errors.extend(more);
    if REGISTRY.set(registry).is_err() {
        errors.push("chords were already loaded; extra chords take effect after a restart".into());
    }
    errors
}

/// Which chord curriculum set, out of those loaded. Saved by its id.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChordSetKind(u16);

impl ChordSetKind {
    pub fn all() -> impl Iterator<Item = Self> {
        (0..registry().entries.len()).map(|i| Self(i as u16))
    }

    pub fn groups() -> impl Iterator<Item = (&'static str, impl Iterator<Item = Self>)> {
        registry()
            .groups
            .iter()
            .map(|(name, range)| (name.as_str(), range.clone().map(|i| Self(i as u16))))
    }

    pub fn by_id(id: &str) -> Option<Self> {
        registry().entries.iter().position(|e| e.id == id).map(|i| Self(i as u16))
    }

    fn entry(self) -> &'static ChordSetDef {
        &registry().entries[self.0 as usize]
    }

    pub fn id(self) -> &'static str {
        &self.entry().id
    }

    pub fn name(self) -> &'static str {
        &self.entry().name
    }

    pub fn group(self) -> &'static str {
        &self.entry().group
    }

    pub fn chords(self) -> &'static [ChordVoicing] {
        &self.entry().chords
    }
}

impl Default for ChordSetKind {
    fn default() -> Self {
        Self::by_id(DEFAULT_ID).unwrap_or(Self(0))
    }
}

impl fmt::Debug for ChordSetKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ChordSetKind({:?})", self.id())
    }
}

impl Serialize for ChordSetKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for ChordSetKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        Ok(Self::by_id(&id).unwrap_or_else(|| {
            eprintln!("unknown chord set \"{id}\"; using {DEFAULT_ID}");
            Self::default()
        }))
    }
}

/// How the chord trainer walks through the set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChordOrder {
    /// Weighted towards chords you're slow on or miss.
    Random,
    /// In curriculum order, looping.
    InOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChordTrainerConfig {
    pub set: ChordSetKind,
    pub order: ChordOrder,
    /// Show the chord's dots and finger numbers on the neck before it's played.
    pub show_diagram: bool,
}

impl Default for ChordTrainerConfig {
    fn default() -> Self {
        Self {
            set: ChordSetKind::default(),
            order: ChordOrder::Random,
            show_diagram: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChordChallenge {
    /// Index into `config.set.chords()`.
    pub index: usize,
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

/// Chord trainer: asks for a chord from the active curriculum set and accepts
/// either a full strum (via [`PitchReading::poly`]) or a slow strum / arpeggio
/// where the chord's pitch classes arrive over ~1.4 seconds.
pub struct ChordTrainer {
    pub config: ChordTrainerConfig,
    pub tuning: Tuning,
    current: Option<ChordChallenge>,
    stats: HashMap<String, PositionStats>,
    feedback: Option<(ChordFeedback, f64)>,
    advance_at: Option<f64>,
    /// When each pitch class started holding continuously in the poly detector.
    poly_since: [Option<f64>; 12],
    /// Last time each pitch class (0..12) was confirmed (held for `POLY_HOLD` or via `NoteStabilizer`).
    class_seen_at: [f64; 12],
    /// Pitch-class mask of the chord that just resolved, so a still-ringing
    /// chord doesn't immediately count toward the next prompt until silence.
    ringing_mask: u16,
    last_voiced: f64,
    seq_step: usize,
    rng: u64,
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
            poly_since: [None; 12],
            class_seen_at: [f64::NEG_INFINITY; 12],
            ringing_mask: 0,
            last_voiced: f64::NEG_INFINITY,
            seq_step: 0,
            rng: seed | 1,
            streak: 0,
            best_streak: 0,
            correct: 0,
        }
    }

    pub fn current(&self) -> Option<&ChordChallenge> {
        self.current.as_ref()
    }

    pub fn current_chord(&self) -> Option<&'static ChordVoicing> {
        let ch = self.current?;
        self.config.set.chords().get(ch.index)
    }

    pub fn feedback(&self) -> Option<(&ChordFeedback, f64)> {
        self.feedback.as_ref().map(|(f, t)| (f, *t))
    }

    pub fn is_resolved(&self) -> bool {
        self.advance_at.is_some()
    }

    pub fn stats(&self, chord_id: &str) -> Option<&PositionStats> {
        self.stats.get(chord_id)
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

    pub fn restart(&mut self, now: f64) {
        self.current = None;
        self.advance_at = None;
        self.seq_step = 0;
        self.next(now);
    }

    pub fn next(&mut self, now: f64) {
        let prev = self.current.map(|c| c.index);
        if self.advance_at.take().is_some() {
            if let Some(idx) = prev {
                if let Some(voicing) = self.config.set.chords().get(idx) {
                    self.ringing_mask = voicing.pitch_class_mask(self.tuning);
                }
            }
        }
        self.poly_since = [None; 12];
        self.class_seen_at = [f64::NEG_INFINITY; 12];
        let chords = self.config.set.chords();
        if chords.is_empty() {
            self.current = None;
            return;
        }
        let index = match self.config.order {
            ChordOrder::InOrder => {
                let i = self.seq_step % chords.len();
                self.seq_step += 1;
                i
            }
            ChordOrder::Random => self.pick_random(prev),
        };
        self.current = Some(ChordChallenge {
            index,
            started: now,
            heard_classes: 0,
            heard_strings: 0,
        });
    }

    fn pick_random(&mut self, prev: Option<usize>) -> usize {
        let chords = self.config.set.chords();
        let pool: Vec<usize> = (0..chords.len())
            .filter(|&i| chords.len() == 1 || Some(i) != prev)
            .collect();
        let weights: Vec<f64> = pool.iter().map(|&i| self.weight(&chords[i].id)).collect();
        let total: f64 = weights.iter().sum();
        let mut pick = self.next_f64() * total;
        for (&i, &w) in pool.iter().zip(&weights) {
            if pick < w {
                return i;
            }
            pick -= w;
        }
        *pool.last().unwrap_or(&0)
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
        let target_mask = voicing.pitch_class_mask(self.tuning);

        // Gate on the short-term input level so decaying tail after stopping
        // or quiet background noise can't feed the chord detector.
        let raw_poly_mask = if reading.level_db >= CHORD_GATE_DB {
            reading
                .poly
                .iter()
                .copied()
                .filter(|&f| f > 0.0)
                .filter_map(|f| nearest_note(f, a4))
                .filter(|r| r.cents.abs() <= 35.0)
                .fold(0u16, |m, r| m | (1 << r.note.pitch_class()))
        } else {
            0
        };

        // Require each poly pitch class to hold steady for `POLY_HOLD` before it counts.
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
            if now - self.last_voiced > 0.2 {
                self.ringing_mask = 0;
                self.class_seen_at = [f64::NEG_INFINITY; 12];
            }
        } else {
            self.last_voiced = now;
        }

        // Ignore the previous chord while it's still ringing out (unless the
        // new chord has the same pitch classes).
        if self.ringing_mask != 0
            && frame_mask != 0
            && (frame_mask & !self.ringing_mask) == 0
            && self.ringing_mask != target_mask
        {
            return;
        }
        if frame_mask & !self.ringing_mask != 0 {
            self.ringing_mask = 0;
        }

        for pc in 0..12usize {
            if (frame_mask & (1 << pc)) != 0 {
                self.class_seen_at[pc] = now;
            }
        }

        // Active pitch classes within the rolling strum window.
        let mut active_classes = 0u16;
        for (pc, &t) in self.class_seen_at.iter().enumerate() {
            if now - t <= STRUM_WINDOW {
                active_classes |= 1 << pc;
            }
        }

        // Which strings of the voicing have had their pitch class heard.
        let mut heard_strings = 0u8;
        for (string, fret) in voicing.frets.iter().enumerate() {
            if let Some(fret) = *fret {
                let pc = self.tuning.note_at(Position { string, fret }).pitch_class();
                if active_classes & (1 << pc) != 0 {
                    heard_strings |= 1 << string;
                }
            }
        }

        if let Some(ch) = self.current.as_mut() {
            ch.heard_classes = active_classes & target_mask;
            ch.heard_strings = heard_strings;
        }

        // Require every pitch class of the chord to be present in the window,
        // and no out-of-chord pitch classes in the current frame.
        let extra_in_frame = (frame_mask & !target_mask).count_ones();
        if (active_classes & target_mask) == target_mask && extra_in_frame == 0 {
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
            self.advance_at = Some(now + 0.75);
        }
    }

    fn weight(&self, id: &str) -> f64 {
        match self.stats.get(id) {
            None => 3.0,
            Some(s) => 1.0 + (s.avg_time() / 2.0).min(3.0),
        }
    }

    fn next_f64(&mut self) -> f64 {
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

    #[test]
    fn every_builtin_chord_set_loads() {
        let (registry, errors) = Registry::builtin();
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(registry.entries.len(), BUILTIN.len());
        for id in ["beginner-minor", "beginner-major", "open-chords", "open-sevenths", "barre-e-shapes", "barre-a-shapes"] {
            assert!(ChordSetKind::by_id(id).is_some(), "{id}");
        }
        let groups: Vec<&str> = ChordSetKind::groups().map(|(name, _)| name).collect();
        assert_eq!(groups, ["beginner", "barre"]);
    }

    #[test]
    fn recognizes_strum_and_slow_arpeggio() {
        let cfg = ChordTrainerConfig {
            set: ChordSetKind::by_id("beginner-minor").unwrap(),
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
        assert!(!t.is_resolved()); // single frame blip doesn't trigger
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
    }
}
