//! Scales: which notes the trainer asks for, and how to name them as degrees.
//!
//! Scales are data, not code. Each one is a JSON file:
//!
//! ```json
//! {
//!   "id": "lydian",
//!   "name": "Lydian",
//!   "group": "modes",
//!   "order": 103,
//!   "intervals": [0, 2, 4, 6, 7, 9, 11],
//!   "degrees": ["1", "2", "3", "#4", "5", "6", "7"]
//! }
//! ```
//!
//! - `id`: stable and unique; it's what saved settings store.
//! - `name`: shown after the key, e.g. "A Lydian".
//! - `group`: free text; the scale menu has one section per group.
//! - `intervals`: semitones above the root, starting at 0, rising, below 12.
//! - `order` (optional, default 1000): lower sorts first. Groups are sorted by
//!   the smallest `order` among their scales, then by name; scales within a
//!   group by `order`, then by name.
//! - `degrees` (optional): a name per interval, e.g. "#4". Without it, and
//!   for notes outside the scale, degrees come from a flat-based table.
//!
//! The built-in scales live in `crates/fretwise-core/scales/` and are compiled
//! into the binary. [`init`] can add more from a folder; a bad file there is
//! skipped with a message, never fatal. Every scale lives in one process-wide
//! registry, so [`Scale`] stays a small `Copy` value.

use std::collections::HashSet;
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::theory::{pitch_class_name, Note};

/// Interval names by semitones above the root. ASCII "b" rather than "♭" so
/// any font can draw them.
const DEGREES: [&str; 12] = ["1", "b2", "2", "b3", "3", "4", "b5", "5", "b6", "6", "b7", "7"];

/// The scales compiled in, as `(file name, JSON)`; see `build.rs`.
const BUILTIN: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/builtin_scales.rs"));

/// What the default scale is, and what an unknown saved id falls back to.
const DEFAULT_ID: &str = "major";

/// One scale file, as written.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleDef {
    pub id: String,
    pub name: String,
    pub group: String,
    #[serde(default = "default_order")]
    pub order: i32,
    pub intervals: Vec<u8>,
    #[serde(default)]
    pub degrees: Option<Vec<String>>,
}

fn default_order() -> i32 {
    1000
}

impl ScaleDef {
    /// Parse and check one scale file. `source` names it in errors.
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
        if self.intervals.first() != Some(&0) {
            return Err("\"intervals\" must start at 0 (the root)".into());
        }
        if self.intervals.windows(2).any(|w| w[0] >= w[1]) {
            return Err("\"intervals\" must rise, each one higher than the last".into());
        }
        if self.intervals.iter().any(|&i| i >= 12) {
            return Err("\"intervals\" must stay below 12".into());
        }
        if let Some(degrees) = &self.degrees {
            if degrees.len() != self.intervals.len() {
                return Err(format!(
                    "{} \"degrees\" for {} \"intervals\"",
                    degrees.len(),
                    self.intervals.len()
                ));
            }
            if degrees.iter().any(|d| d.trim().is_empty()) {
                return Err("a degree name is empty".into());
            }
        }
        Ok(())
    }
}

struct Entry {
    def: ScaleDef,
    /// Bit `i` set if the note `i` semitones above the root is in the scale.
    mask: u16,
    /// A name for every semitone above the root.
    degrees: [String; 12],
}

struct Registry {
    /// In menu order: by group, then within the group.
    entries: Vec<Entry>,
    /// Each group's name and its slice of `entries`.
    groups: Vec<(String, Range<usize>)>,
}

impl Registry {
    /// Collects scales, skipping bad files and repeated ids. Returns the
    /// registry and a message per skipped file.
    fn build(sources: impl IntoIterator<Item = (String, String)>) -> (Self, Vec<String>) {
        let mut errors = Vec::new();
        let mut ids = HashSet::new();
        let mut defs = Vec::new();
        for (source, json) in sources {
            match ScaleDef::parse(&source, &json) {
                Ok(def) if !ids.insert(def.id.clone()) => {
                    errors.push(format!("{source}: id \"{}\" is already taken; skipped", def.id))
                }
                Ok(def) => defs.push(def),
                Err(e) => errors.push(e),
            }
        }

        let group_order = |group: &str, defs: &[ScaleDef]| {
            defs.iter().filter(|d| d.group == group).map(|d| d.order).min().unwrap_or(i32::MAX)
        };
        let keys: Vec<i32> = defs.iter().map(|d| group_order(&d.group, &defs)).collect();
        let mut keyed: Vec<_> = keys.into_iter().zip(defs).collect();
        keyed.sort_by(|(ga, a), (gb, b)| {
            (ga, &a.group, a.order, &a.name, &a.id).cmp(&(gb, &b.group, b.order, &b.name, &b.id))
        });

        let mut entries: Vec<Entry> = Vec::new();
        let mut groups: Vec<(String, Range<usize>)> = Vec::new();
        for (_, def) in keyed {
            match groups.last_mut() {
                Some((name, range)) if *name == def.group => range.end += 1,
                _ => groups.push((def.group.clone(), entries.len()..entries.len() + 1)),
            }
            let mut degrees = DEGREES.map(String::from);
            if let Some(names) = &def.degrees {
                for (&i, name) in def.intervals.iter().zip(names) {
                    degrees[i as usize] = name.clone();
                }
            }
            let mask = def.intervals.iter().fold(0u16, |m, &i| m | 1 << i);
            entries.push(Entry { def, mask, degrees });
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
            eprintln!("scale {e}");
        }
        registry
    })
}

/// Loads the built-in scales plus every `*.json` in `user_dir` (if it exists).
/// Call once at startup, before anything uses a scale; returns a message per
/// file that was skipped. Without it, only the built-in scales are there.
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
        errors.push("scales were already loaded; extra scales take effect after a restart".into());
    }
    errors
}

/// Which scale, out of those loaded. Saved by its id.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScaleKind(u16);

impl ScaleKind {
    /// Every scale, in menu order.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..registry().entries.len()).map(|i| Self(i as u16))
    }

    /// Each group's name and its scales, in menu order.
    pub fn groups() -> impl Iterator<Item = (&'static str, impl Iterator<Item = Self>)> {
        registry().groups.iter().map(|(name, range)| (name.as_str(), range.clone().map(|i| Self(i as u16))))
    }

    pub fn by_id(id: &str) -> Option<Self> {
        registry().entries.iter().position(|e| e.def.id == id).map(|i| Self(i as u16))
    }

    /// The enum names saved before scales came from files.
    fn by_legacy_name(name: &str) -> Option<Self> {
        let id = match name {
            "Chromatic" => "chromatic",
            "Major" => "major",
            "NaturalMinor" => "natural-minor",
            "MajorPentatonic" => "major-pentatonic",
            "MinorPentatonic" => "minor-pentatonic",
            _ => return None,
        };
        Self::by_id(id)
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

    /// Semitones above the root.
    pub fn intervals(self) -> &'static [u8] {
        &self.entry().def.intervals
    }

    /// All twelve notes: no key, no root.
    pub fn is_chromatic(self) -> bool {
        self.entry().mask == 0xfff
    }
}

impl Default for ScaleKind {
    fn default() -> Self {
        Self::by_id(DEFAULT_ID).unwrap_or(Self(0))
    }
}

impl fmt::Debug for ScaleKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ScaleKind({:?})", self.id())
    }
}

impl Serialize for ScaleKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for ScaleKind {
    /// An id, or an old enum name. A scale that's gone (say, a deleted user
    /// file) falls back to the default rather than failing the whole load.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        Ok(Self::by_id(&id).or_else(|| Self::by_legacy_name(&id)).unwrap_or_else(|| {
            eprintln!("unknown scale \"{id}\"; using {DEFAULT_ID}");
            Self::default()
        }))
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
        Self { root: 0, kind: ScaleKind::default() }
    }
}

impl Scale {
    fn semitones(self, note: Note) -> u8 {
        (note.pitch_class() + 12 - self.root % 12) % 12
    }

    pub fn contains(self, note: Note) -> bool {
        self.kind.entry().mask & 1 << self.semitones(note) != 0
    }

    /// The note's interval above the root, e.g. "b3", or "#4" where the
    /// scale spells it so. Notes outside the scale get their chromatic name.
    pub fn degree(self, note: Note) -> &'static str {
        &self.kind.entry().degrees[self.semitones(note) as usize]
    }

    pub fn name(self) -> String {
        if self.kind.is_chromatic() {
            self.kind.name().to_string()
        } else {
            format!("{} {}", pitch_class_name(self.root), self.kind.name())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(id: &str) -> ScaleKind {
        ScaleKind::by_id(id).unwrap_or_else(|| panic!("no scale {id}"))
    }

    #[test]
    fn every_builtin_scale_loads() {
        let (registry, errors) = Registry::builtin();
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(registry.entries.len(), BUILTIN.len());
        for id in ["chromatic", "major", "natural-minor", "major-pentatonic", "minor-pentatonic", "dorian"] {
            assert!(ScaleKind::by_id(id).is_some(), "{id}");
        }
        // Degree names are spelled right: "#4" is 6 semitones up, "b7" 10.
        for e in &registry.entries {
            for (&i, d) in e.def.intervals.iter().zip(e.def.degrees.iter().flatten()) {
                let number: usize = d.trim_start_matches(['b', '#']).parse().expect(d);
                let natural: i32 = [0, 2, 4, 5, 7, 9, 11][number - 1];
                let shift = d.matches('#').count() as i32 - d.matches('b').count() as i32;
                assert_eq!(natural + shift, i as i32, "{} {d}", e.def.id);
            }
        }
    }

    #[test]
    fn groups_sort_by_their_lowest_order() {
        let groups: Vec<&str> = ScaleKind::groups().map(|(name, _)| name).collect();
        assert_eq!(groups, ["basic", "modes", "minor", "exotic"]);
        let basic: Vec<&str> = ScaleKind::groups().next().unwrap().1.map(|k| k.id()).collect();
        assert_eq!(basic[..3], ["chromatic", "major", "natural-minor"]);
        assert_eq!(ScaleKind::all().count(), BUILTIN.len());
    }

    #[test]
    fn bad_files_are_skipped() {
        let ok = r#"{"id": "x", "name": "x", "group": "g", "intervals": [0, 7]}"#;
        let files = [
            ("ok.json", ok),
            ("dup.json", ok),
            ("syntax.json", "{"),
            ("root.json", r#"{"id": "a", "name": "a", "group": "g", "intervals": [2, 7]}"#),
            ("falling.json", r#"{"id": "b", "name": "b", "group": "g", "intervals": [0, 7, 5]}"#),
            ("octave.json", r#"{"id": "c", "name": "c", "group": "g", "intervals": [0, 12]}"#),
            ("degrees.json", r#"{"id": "d", "name": "d", "group": "g", "intervals": [0, 7], "degrees": ["1"]}"#),
            ("typo.json", r#"{"id": "e", "name": "e", "group": "g", "intervals": [0], "degres": ["1"]}"#),
        ];
        let (registry, errors) = Registry::build(files.map(|(s, j)| (s.to_string(), j.to_string())));
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(errors.len(), files.len() - 1);
        for (e, (source, _)) in errors.iter().zip(&files[1..]) {
            assert!(e.starts_with(source), "{e}");
        }
    }

    #[test]
    fn user_scales_sort_into_groups() {
        let files = [
            ("z.json", r#"{"id": "z", "name": "zeta", "group": "mine", "intervals": [0, 5]}"#),
            ("a.json", r#"{"id": "a", "name": "alpha", "group": "mine", "intervals": [0, 7]}"#),
            ("b.json", r#"{"id": "b", "name": "beta", "group": "first", "order": -1, "intervals": [0]}"#),
        ];
        let (registry, errors) = Registry::build(files.map(|(s, j)| (s.to_string(), j.to_string())));
        assert!(errors.is_empty());
        let names: Vec<&str> = registry.entries.iter().map(|e| e.def.name.as_str()).collect();
        assert_eq!(names, ["beta", "alpha", "zeta"]);
        assert_eq!(registry.groups, [("first".to_string(), 0..1), ("mine".to_string(), 1..3)]);
    }

    #[test]
    fn membership_and_degrees() {
        let a_minor_pent = Scale { root: 9, kind: kind("minor-pentatonic") };
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
        assert_eq!(c_major.kind.id(), "major");
        for n in 40..64 {
            assert_eq!(c_major.contains(Note(n)), Note(n).is_natural());
        }

        let f_lydian = Scale { root: 5, kind: kind("lydian") };
        assert_eq!(f_lydian.degree(Note(47)), "#4"); // B2
        assert_eq!(f_lydian.name(), "F Lydian");
        assert_eq!(Scale { root: 4, kind: kind("chromatic") }.name(), "chromatic");
    }

    #[test]
    fn saves_ids_and_reads_old_enum_names() {
        let s = Scale { root: 9, kind: kind("dorian") };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"root":9,"kind":"dorian"}"#);
        assert_eq!(serde_json::from_str::<Scale>(&json).unwrap(), s);

        for (old, id) in [
            ("Chromatic", "chromatic"),
            ("Major", "major"),
            ("NaturalMinor", "natural-minor"),
            ("MajorPentatonic", "major-pentatonic"),
            ("MinorPentatonic", "minor-pentatonic"),
        ] {
            let s: Scale = serde_json::from_str(&format!(r#"{{"root": 2, "kind": "{old}"}}"#)).unwrap();
            assert_eq!(s, Scale { root: 2, kind: kind(id) });
        }

        let gone: Scale = serde_json::from_str(r#"{"root": 2, "kind": "no-such-scale"}"#).unwrap();
        assert_eq!(gone, Scale { root: 2, kind: ScaleKind::default() });
    }
}
