//! Decks of scales and chords (`crates/fretwise-core/decks/*.json`), with
//! recursive deck composition and automatic card deduplication.
//!
//! Each deck is a JSON file that references individual scale ids
//! (`scales/*.json`), individual chord ids (`chords/*.json`), and/or other
//! deck ids (`decks/*.json`):
//!
//! ```json
//! {
//!   "id": "beginner-essentials",
//!   "name": "Beginner Essentials",
//!   "group": "Collections",
//!   "order": 201,
//!   "decks": ["basic-chords", "pentatonic-blues"]
//! }
//! ```
//!
//! When a deck combines overlapping sub-decks or lists a card more than once,
//! each scale or chord is included only once (preserving first-seen order).
//! Because every card is stored individually by its scale or chord id, progress
//! and SM-2 scheduling are shared across every deck that includes that card.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::chord::ChordKind;
use crate::registry::{self, SortKey};
use crate::scale::ScaleKind;

const BUILTIN: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/builtin_decks.rs"));

const DEFAULT_ID: &str = "beginner-essentials";
const DEFAULT_CHORD_DECK_ID: &str = "basic-chords";

/// One deck JSON file, as written.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckDef {
    pub id: String,
    pub name: String,
    pub group: String,
    #[serde(default = "default_order")]
    pub order: i32,
    /// Included deck ids (expanded recursively, deduplicated).
    #[serde(default)]
    pub decks: Vec<String>,
    /// Included scale ids (from `scales/*.json`).
    #[serde(default)]
    pub scales: Vec<String>,
    /// Included chord ids (from `chords/*.json`).
    #[serde(default)]
    pub chords: Vec<String>,
}

fn default_order() -> i32 {
    1000
}

impl DeckDef {
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
        if self.decks.is_empty() && self.scales.is_empty() && self.chords.is_empty() {
            return Err("deck must reference at least one scale, chord, or sub-deck".into());
        }
        Ok(())
    }
}

struct Entry {
    def: DeckDef,
    /// Resolved, deduplicated scales in this deck.
    scales: Vec<ScaleKind>,
    /// Resolved, deduplicated chords in this deck.
    chords: Vec<ChordKind>,
}

struct Registry {
    entries: Vec<Entry>,
    groups: Vec<(String, Range<usize>)>,
}

impl Registry {
    fn build(sources: impl IntoIterator<Item = registry::Source>) -> (Self, Vec<String>) {
        let (defs, mut errors) = registry::parse_all(sources, DeckDef::parse, |d| &d.id);
        let sorted_defs = registry::sort_by_group(defs, |d| SortKey {
            group: &d.group,
            order: d.order,
            name: &d.name,
            id: &d.id,
        });
        let by_id: HashMap<&str, &DeckDef> = sorted_defs.iter().map(|d| (d.id.as_str(), d)).collect();

        let mut entries = Vec::new();
        let mut groups: Vec<(String, Range<usize>)> = Vec::new();
        for def in &sorted_defs {
            let mut scales = Vec::new();
            let mut chords = Vec::new();
            let mut seen_scales = HashSet::new();
            let mut seen_chords = HashSet::new();
            let mut visiting = HashSet::new();
            resolve_deck(
                def,
                &by_id,
                &mut visiting,
                &mut scales,
                &mut chords,
                &mut seen_scales,
                &mut seen_chords,
                &mut errors,
            );
            if scales.is_empty() && chords.is_empty() {
                errors.push(format!("deck \"{}\": resolved to 0 valid cards; skipped", def.id));
                continue;
            }
            registry::add_to_groups(&mut groups, &def.group, entries.len());
            entries.push(Entry {
                def: def.clone(),
                scales,
                chords,
            });
        }
        (Self { entries, groups }, errors)
    }

    fn builtin() -> (Self, Vec<String>) {
        Self::build(registry::builtin_sources(BUILTIN))
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_deck<'a>(
    def: &'a DeckDef,
    by_id: &HashMap<&'a str, &'a DeckDef>,
    visiting: &mut HashSet<&'a str>,
    scales: &mut Vec<ScaleKind>,
    chords: &mut Vec<ChordKind>,
    seen_scales: &mut HashSet<ScaleKind>,
    seen_chords: &mut HashSet<ChordKind>,
    errors: &mut Vec<String>,
) {
    if !visiting.insert(def.id.as_str()) {
        errors.push(format!("deck \"{}\": cycle in sub-decks", def.id));
        return;
    }
    for sub_id in &def.decks {
        match by_id.get(sub_id.as_str()) {
            Some(sub) => resolve_deck(sub, by_id, visiting, scales, chords, seen_scales, seen_chords, errors),
            None => errors.push(format!("deck \"{}\": unknown sub-deck \"{sub_id}\"", def.id)),
        }
    }
    for scale_id in &def.scales {
        match ScaleKind::by_id(scale_id) {
            Some(k) => {
                if seen_scales.insert(k) {
                    scales.push(k);
                }
            }
            None => errors.push(format!("deck \"{}\": unknown scale \"{scale_id}\"", def.id)),
        }
    }
    for chord_id in &def.chords {
        match ChordKind::by_id(chord_id) {
            Some(c) => {
                if seen_chords.insert(c) {
                    chords.push(c);
                }
            }
            None => errors.push(format!("deck \"{}\": unknown chord \"{chord_id}\"", def.id)),
        }
    }
    visiting.remove(def.id.as_str());
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| {
        let (registry, errors) = Registry::builtin();
        for e in errors {
            eprintln!("deck {e}");
        }
        registry
    })
}

/// Loads built-in decks plus every `*.json` in `user_dir` (if it exists).
/// Call after `scale::init` and `chord::init`.
pub fn init(user_dir: Option<&Path>) -> Vec<String> {
    let (sources, mut errors) = registry::sources(BUILTIN, user_dir);
    let (registry, more) = Registry::build(sources);
    errors.extend(more);
    if REGISTRY.set(registry).is_err() {
        errors.push("decks were already loaded; extra decks take effect after a restart".into());
    }
    errors
}

/// Which deck, out of those loaded. Saved by its id.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeckKind(u16);

impl DeckKind {
    pub fn all() -> impl Iterator<Item = Self> {
        (0..registry().entries.len()).map(|i| Self(i as u16))
    }

    /// Every deck group and its decks, in menu order.
    pub fn groups() -> impl Iterator<Item = (&'static str, impl Iterator<Item = Self>)> {
        registry()
            .groups
            .iter()
            .map(|(name, range)| (name.as_str(), range.clone().map(|i| Self(i as u16))))
    }

    pub fn by_id(id: &str) -> Option<Self> {
        registry().entries.iter().position(|e| e.def.id == id).map(|i| Self(i as u16))
    }

    pub fn default_chord_deck() -> Self {
        Self::by_id(DEFAULT_CHORD_DECK_ID).unwrap_or_default()
    }

    fn entry(self) -> &'static Entry {
        &registry().entries[self.0 as usize]
    }

    pub fn id(self) -> &'static str {
        &self.entry().def.id
    }

    pub fn name(self) -> &'static str {
        &self.entry().def.name
    }

    pub fn group(self) -> &'static str {
        &self.entry().def.group
    }

    /// Resolved, deduplicated scales in this deck.
    pub fn scales(self) -> &'static [ScaleKind] {
        &self.entry().scales
    }

    /// Resolved, deduplicated chords in this deck.
    pub fn chords(self) -> &'static [ChordKind] {
        &self.entry().chords
    }
}

impl Default for DeckKind {
    fn default() -> Self {
        Self::by_id(DEFAULT_ID).unwrap_or(Self(0))
    }
}

impl fmt::Debug for DeckKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DeckKind({:?})", self.id())
    }
}

impl Serialize for DeckKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for DeckKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        // Map legacy chord-set ids from earlier commits cleanly.
        let mapped = match id.as_str() {
            "open-chords" | "beginner-minor" | "beginner-major" => "basic-chords",
            other => other,
        };
        Ok(Self::by_id(mapped).unwrap_or_else(|| {
            eprintln!("unknown deck \"{id}\"; using {DEFAULT_ID}");
            Self::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_deck_loads_and_deduplicates_cards() {
        let (registry, errors) = Registry::builtin();
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(registry.entries.len(), BUILTIN.len());

        for id in [
            "basic-scales",
            "basic-chords",
            "power-chords",
            "barre-chords",
            "jazz-chords",
            "exotic-scales",
            "beginner-essentials",
            "campfire-acoustic",
            "blues-session",
            "rock-metal",
            "jazz-session",
            "everything",
        ] {
            assert!(DeckKind::by_id(id).is_some(), "missing deck {id}");
        }

        // Overlapping decks in "everything" (e.g. minor-pentatonic is in
        // basic-scales AND minor-scales) only appear once.
        let all = DeckKind::by_id("everything").unwrap();
        let unique_scales: HashSet<_> = all.scales().iter().copied().collect();
        let unique_chords: HashSet<_> = all.chords().iter().copied().collect();
        assert_eq!(unique_scales.len(), all.scales().len());
        assert_eq!(unique_chords.len(), all.chords().len());
    }
}
