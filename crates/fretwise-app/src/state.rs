//! Saved state: audio settings, UI settings and training progress, as JSON in
//! the user's config folder (`%APPDATA%\Fretwise\state.json` on Windows).
//!
//! Writes go to a temp file that is then renamed over the old one, so a crash
//! mid-save can't leave a half-written file. A file that fails to parse is set
//! aside as `state.json.bad` rather than silently overwritten.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use fretwise_ui::UiState;
use serde::{Deserialize, Serialize};

/// Bump when the format changes in a way `#[serde(default)]` can't absorb,
/// and migrate older files in [`Store::load`].
const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// cpal host (driver model) name, e.g. "WASAPI" or "ASIO".
    pub host: Option<String>,
    pub device: Option<String>,
    /// 0-based input channel.
    pub channel: usize,
    /// Output device; `None` is the driver's default. Ignored under ASIO,
    /// where the output is always the input device.
    pub output: Option<String>,
    /// 0-based output channel pair: 0 is channels 1-2, 1 is 3-4, ...
    pub output_pair: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedState {
    pub version: u32,
    pub audio: AudioSettings,
    pub ui: UiState,
}

pub struct Store {
    path: Option<PathBuf>,
    /// Last JSON written (or loaded), to skip writes when nothing changed.
    last: Option<String>,
}

impl Store {
    /// The store in the user's config folder, and whatever it holds.
    pub fn open() -> (Self, SavedState) {
        let path = dirs::config_dir().map(|d| d.join("Fretwise").join("state.json"));
        if path.is_none() {
            eprintln!("no config folder; settings won't be saved");
        }
        Self::load(path)
    }

    fn load(path: Option<PathBuf>) -> (Self, SavedState) {
        let mut store = Self { path, last: None };
        let Some(path) = store.path.clone() else {
            return (store, SavedState::default());
        };
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == ErrorKind::NotFound => return (store, SavedState::default()),
            Err(e) => {
                // Don't overwrite a file we couldn't read; it may be fine.
                eprintln!("couldn't read {}: {e}; settings won't be saved this run", path.display());
                store.path = None;
                return (store, SavedState::default());
            }
        };
        match serde_json::from_str::<SavedState>(&text) {
            Ok(state) => {
                store.last = Some(text);
                (store, state)
            }
            Err(e) => {
                let bad = path.with_extension("json.bad");
                eprintln!("couldn't parse {}: {e}; moving it to {}", path.display(), bad.display());
                if let Err(e) = fs::rename(&path, &bad) {
                    eprintln!("couldn't move it aside: {e}; settings won't be saved this run");
                    store.path = None;
                }
                (store, SavedState::default())
            }
        }
    }

    /// Write `state` if it changed since the last save.
    pub fn save(&mut self, state: &SavedState) {
        let Some(path) = &self.path else { return };
        let state = SavedState { version: VERSION, ..state.clone() };
        let json = match serde_json::to_string_pretty(&state) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("couldn't serialize settings: {e}");
                return;
            }
        };
        if self.last.as_deref() == Some(json.as_str()) {
            return;
        }
        match write_atomic(path, json.as_bytes()) {
            Ok(()) => self.last = Some(json),
            Err(e) => eprintln!("couldn't save {}: {e}", path.display()),
        }
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    // Replaces the old file in one step (MoveFileEx on Windows, rename(2) elsewhere).
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fretwise-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("state.json")
    }

    fn sample() -> SavedState {
        SavedState {
            version: VERSION,
            audio: AudioSettings {
                host: Some("ASIO".into()),
                device: Some("Scarlett 2i2".into()),
                channel: 1,
                output: None,
                output_pair: 1,
            },
            ui: UiState::default(),
        }
    }

    #[test]
    fn round_trips_and_skips_unchanged_writes() {
        let path = temp_path("round-trip");
        let (mut store, loaded) = Store::load(Some(path.clone()));
        assert_eq!(loaded, SavedState::default());

        store.save(&sample());
        let (_, loaded) = Store::load(Some(path.clone()));
        assert_eq!(loaded, sample());
        assert!(!path.with_extension("json.tmp").exists());

        // Unchanged state isn't rewritten.
        fs::write(&path, "sentinel").unwrap();
        store.save(&sample());
        assert_eq!(fs::read_to_string(&path).unwrap(), "sentinel");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn corrupt_file_is_set_aside() {
        let path = temp_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();

        let (mut store, loaded) = Store::load(Some(path.clone()));
        assert_eq!(loaded, SavedState::default());
        assert_eq!(fs::read_to_string(path.with_extension("json.bad")).unwrap(), "{ not json");

        store.save(&sample());
        assert_eq!(Store::load(Some(path.clone())).1, sample());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn partial_file_keeps_what_it_has() {
        let path = temp_path("partial");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"audio": {"channel": 3}}"#).unwrap();
        let (_, loaded) = Store::load(Some(path.clone()));
        assert_eq!(loaded.audio.channel, 3);
        assert_eq!(loaded.ui, UiState::default());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
