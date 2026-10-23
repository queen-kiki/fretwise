//! Fretwise's GUI, written against plain `egui` so the same views can be
//! hosted by eframe (standalone app) or by a plugin's egui editor.
//!
//! The host's only job is to hand [`FretwiseUi::show`] the latest
//! [`PitchReading`] each frame, plus anything it adds through [`Host`].
//!
//! This file holds the state, what's saved, and the frame loop. Each screen
//! is its own module (`learn`, `notes`, `chords`, `tuner`, `stats`, plus the
//! `header`, `settings` and the deck `browser`), all adding methods to
//! [`FretwiseUi`]; `parts` has what they share, `widgets` the custom-painted
//! controls, and `theme` the colours and fonts.

mod browser;
mod chords;
mod fx_rack;
mod header;
mod icons;
mod learn;
mod notes;
mod parts;
mod scope;
mod settings;
mod stats;
mod theme;
mod tuner;
mod widgets;

use fretwise_core::{
    AmpConfig, ChordFeedback, ChordProgress, ChordTrainer, ChordTrainerConfig, Feedback, Grade, LearnConfig, LearnFeedback,
    LearnProgress, LearnTrainer, DEFAULT_GATE_DB, GATE_RANGE_DB, Note, NoteStabilizer, PitchReading, Position, Progress, Trainer, TrainerConfig, Tuning,
};
use serde::{Deserialize, Serialize};

use parts::NECK_FRETS;
use scope::{ScopeConfig, ScopeScreen};

pub use theme::BAD;
use theme::rack_fill;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Tab {
    Tuner,
    Trainer,
    Chords,
    Learn,
    Progress,
}

/// Colour scheme; `System` follows the OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Theme {
    System,
    Dark,
    Light,
}

impl From<Theme> for egui::ThemePreference {
    fn from(t: Theme) -> Self {
        match t {
            Theme::System => Self::System,
            Theme::Dark => Self::Dark,
            Theme::Light => Self::Light,
        }
    }
}

/// What the host adds to the UI. The standalone app has an audio device to
/// pick; a plugin gets its audio from the DAW and can pass `&mut ()`.
pub trait Host {
    /// A problem worth showing in the header, e.g. "no input device".
    fn warning(&self) -> Option<String> {
        None
    }

    /// Audio settings, drawn at the top of the settings panel.
    fn audio_settings(&mut self, _ui: &mut egui::Ui) {}
}

impl Host for () {}

pub struct FretwiseUi {
    tab: Tab,
    settings_open: bool,
    amp_open: bool,
    about_open: bool,
    browser_open: bool,
    amp_resized: bool,
    /// Reviews done before this run, so the header can count this session's.
    session_start: Option<u32>,
    /// Carry on with the deck after the done-for-today screen.
    keep_going: bool,
    /// When a card was completed, and where on the neck to sparkle.
    sparkle: Option<(f64, Vec<Position>)>,
    browser_filter: String,
    theme: Theme,
    a4: f32,
    /// The practice amp's settings, for the host to pass on to its effect chain.
    amp: AmpConfig,
    /// The host's output level (dBFS), for the OUT meter.
    out_level_db: f32,
    /// What the host's rack sends out, for the oscilloscope; none without audio.
    scope: Option<std::sync::Arc<fretwise_core::SharedScope>>,
    scope_cfg: ScopeConfig,
    scope_screen: ScopeScreen,
    /// Input gate (dBFS), for the host to pass on to the analyzer.
    gate_db: f32,
    tuning: Tuning,
    stabilizer: NoteStabilizer,
    trainer: Trainer,
    chord_trainer: ChordTrainer,
    learn_trainer: LearnTrainer,
    smoothed_cents: f32,
    chime_enabled: bool,
    chime_pending: bool,
    show_pattern: bool,
    show_fingering: bool,
}

impl Default for FretwiseUi {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        Self {
            tab: Tab::Trainer,
            settings_open: false,
            amp_open: false,
            amp_resized: false,
            about_open: false,
            browser_open: false,
            session_start: None,
            keep_going: false,
            sparkle: None,
            browser_filter: String::new(),
            theme: Theme::System,
            a4: 440.0,
            gate_db: DEFAULT_GATE_DB,
            amp: AmpConfig::default(),
            out_level_db: -100.0,
            scope: None,
            scope_cfg: ScopeConfig::default(),
            scope_screen: ScopeScreen::default(),
            tuning: Tuning::STANDARD,
            stabilizer: NoteStabilizer::default(),
            trainer: Trainer::new(Tuning::STANDARD, TrainerConfig::default(), seed),
            chord_trainer: ChordTrainer::new(Tuning::STANDARD, ChordTrainerConfig::default(), seed ^ 0x9e37_79b9),
            learn_trainer: LearnTrainer::new(Tuning::STANDARD, LearnConfig::default(), seed ^ 0x517c_c1b7),
            smoothed_cents: 0.0,
            chime_enabled: true,
            chime_pending: false,
            show_pattern: true,
            show_fingering: true,
        }
    }
}

/// What the UI keeps between runs: settings and training progress. Opaque to
/// the host, which only decides where it's stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    tab: Tab,
    a4: f32,
    amp: AmpConfig,
    /// The rack oscilloscope's front panel.
    scope: ScopeConfig,
    /// Input gate in dBFS: quieter input is ignored.
    gate_db: f32,
    /// Open strings, lowest first; matched against [`Tuning::ALL`] on load.
    tuning: [Note; 6],
    trainer: TrainerConfig,
    progress: Progress,
    chords: ChordTrainerConfig,
    chord_progress: ChordProgress,
    learn: LearnConfig,
    learn_progress: LearnProgress,
    /// Play a sound on correct answers.
    chime: bool,
    /// Draw the scale's positions on the trainer's fretboard.
    show_pattern: bool,
    /// Name the finger for the target in the prompt, when the pattern fits
    /// one hand position.
    show_fingering: bool,
    settings_open: bool,
    amp_open: bool,
    theme: Theme,
}

impl Default for UiState {
    fn default() -> Self {
        FretwiseUi::default().state()
    }
}

impl FretwiseUi {
    pub fn from_state(state: UiState) -> Self {
        let defaults = Self::default();
        let mut ui = Self {
            // Learn is home; the other screens are tools you step into.
            tab: Tab::Learn,
            settings_open: state.settings_open,
            amp_open: state.amp_open,
            theme: state.theme,
            a4: if (415.0..=466.0).contains(&state.a4) { state.a4 } else { defaults.a4 },
            gate_db: if GATE_RANGE_DB.contains(&state.gate_db) { state.gate_db } else { defaults.gate_db },
            amp: state.amp.sanitized(),
            scope_cfg: state.scope.sanitized(),
            tuning: Tuning::from_strings(state.tuning).unwrap_or(Tuning::STANDARD),
            chime_enabled: state.chime,
            show_pattern: state.show_pattern,
            show_fingering: state.show_fingering,
            ..defaults
        };
        let mut cfg = state.trainer;
        cfg.max_fret = cfg.max_fret.min(NECK_FRETS);
        cfg.min_fret = cfg.min_fret.min(cfg.max_fret);
        ui.trainer.tuning = ui.tuning;
        ui.trainer.config = cfg;
        ui.trainer.set_progress(state.progress);
        ui.chord_trainer.tuning = ui.tuning;
        ui.chord_trainer.config = state.chords;
        ui.chord_trainer.set_progress(state.chord_progress);
        let mut learn_cfg = state.learn;
        learn_cfg.max_fret = learn_cfg.max_fret.min(NECK_FRETS);
        learn_cfg.min_fret = learn_cfg.min_fret.min(learn_cfg.max_fret);
        ui.learn_trainer.tuning = ui.tuning;
        ui.learn_trainer.config = learn_cfg;
        ui.learn_trainer.set_progress(state.learn_progress);
        ui
    }

    pub fn state(&self) -> UiState {
        UiState {
            tab: self.tab,
            a4: self.a4,
            gate_db: self.gate_db,
            amp: self.amp,
            scope: self.scope_cfg,
            tuning: self.tuning.strings,
            trainer: self.trainer.config,
            progress: self.trainer.progress(),
            chords: self.chord_trainer.config,
            chord_progress: self.chord_trainer.progress(),
            learn: self.learn_trainer.config,
            learn_progress: self.learn_trainer.progress(),
            chime: self.chime_enabled,
            show_pattern: self.show_pattern,
            show_fingering: self.show_fingering,
            settings_open: self.settings_open,
            amp_open: self.amp_open,
            theme: self.theme,
        }
    }

    /// The practice amp settings. The host passes them to its effect chain
    /// every frame with [`fretwise_core::SharedAmp::store`].
    pub fn amp(&self) -> AmpConfig {
        self.amp
    }

    /// Where to read what the effect chain sends out, for the rack's
    /// oscilloscope (put a [`fretwise_core::ScopeTap`] at the end of the chain).
    pub fn set_scope(&mut self, scope: std::sync::Arc<fretwise_core::SharedScope>) {
        self.scope = Some(scope);
    }

    /// The level (dBFS) the host is sending out, for the OUT meter.
    pub fn set_out_level_db(&mut self, db: f32) {
        self.out_level_db = db;
    }

    /// The input gate the player set, in dBFS. The host passes it to the
    /// analyzer every frame with [`fretwise_core::SharedPitch::set_gate_db`].
    pub fn gate_db(&self) -> f32 {
        self.gate_db
    }

    /// True once per correct answer (when sound is on). The host plays the
    /// sound, e.g. by triggering a [`fretwise_core::Chime`] in its output.
    pub fn take_chime(&mut self) -> bool {
        std::mem::take(&mut self.chime_pending)
    }

    /// Space does what the skip button does (which is nothing in the pause
    /// after a right answer), unless a text field has the keyboard or a panel
    /// is open over the page.
    fn skip_on_space(&mut self, ui: &egui::Ui, now: f64) {
        let pressed = ui.input(|i| i.key_pressed(egui::Key::Space));
        if !pressed || ui.ctx().wants_keyboard_input() || self.settings_open || self.about_open || self.browser_open {
            return;
        }
        match self.tab {
            Tab::Trainer => self.trainer.skip(now),
            Tab::Chords => self.chord_trainer.skip(now),
            Tab::Learn if !self.learn_done(now) => self.learn_trainer.skip(now),
            _ => {}
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, reading: PitchReading, host: &mut dyn Host) {
        let now = ui.input(|i| i.time);
        let held = self.stabilizer.update(now, &reading, self.a4);
        self.skip_on_space(ui, now);
        if self.tab == Tab::Trainer {
            let before = self.trainer.feedback();
            self.trainer.update(now, held.map(|h| h.note));
            let after = self.trainer.feedback();
            if after != before && matches!(after, Some((Feedback::Correct { .. }, _))) {
                self.chime_pending |= self.chime_enabled;
            }
        } else if self.tab == Tab::Chords {
            let before = self.chord_trainer.feedback().map(|(_, t)| t);
            self.chord_trainer.update(now, &reading, held.map(|h| h.note), self.a4);
            let after = self.chord_trainer.feedback();
            if after.map(|(_, t)| t) != before && matches!(after, Some((ChordFeedback::Correct { .. }, _))) {
                self.chime_pending |= self.chime_enabled;
            }
        } else if self.tab == Tab::Learn {
            for (key, grade) in [
                (egui::Key::Num1, Grade::Again),
                (egui::Key::Num2, Grade::Hard),
                (egui::Key::Num3, Grade::Good),
                (egui::Key::Num4, Grade::Easy),
            ] {
                if ui.input(|i| i.key_pressed(key)) {
                    self.learn_trainer.regrade(grade, now);
                }
            }
            self.session_start.get_or_insert(self.learn_trainer.reviewed);
            let burst_at = self.learn_target_positions();
            let before = self.learn_trainer.feedback().map(|(_, t)| t);
            self.learn_trainer.update(now, &reading, held.map(|h| h.note), self.a4);
            let after = self.learn_trainer.feedback();
            if after.map(|(_, t)| t) != before && matches!(after, Some((LearnFeedback::Completed { .. }, _))) {
                self.chime_pending |= self.chime_enabled;
                self.sparkle = Some((now, burst_at));
            }
        }
        // Pitch moves continuously; keep repainting while anything is heard.
        ui.ctx().request_repaint();
        theme::apply(ui.ctx());
        icons::install(ui.ctx());
        let theme = egui::ThemePreference::from(self.theme);
        if ui.ctx().options(|o| o.theme_preference) != theme {
            ui.ctx().set_theme(theme);
        }

        // As of the start of the frame, so the click that opens a panel
        // doesn't also count as a click outside it.
        let (settings_was_open, about_was_open) = (self.settings_open, self.about_open);

        // Same faceplate as the rack at the bottom, framing the stage.
        let header = egui::Frame::new()
            .fill(rack_fill(ui))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .outer_margin(egui::Margin { bottom: 10, ..Default::default() })
            .corner_radius(8);
        egui::TopBottomPanel::top("fretwise_header")
            .frame(header)
            .show_separator_line(false)
            .show_inside(ui, |ui| self.header(ui, &reading, host.warning()));

        // Grow or shrink the window when the amp drawer opens/closes so the
        // main UI above it keeps its room.
        const AMP_DRAWER_SPACE: f32 = fx_rack::FX_PANE_SPACE;
        if self.amp_open != self.amp_resized {
            if let Some(rect) = ui.ctx().input(|i| i.viewport().inner_rect).filter(|r| r.width() >= 400.0 && r.height() >= 300.0) {
                let min_h = if self.amp_open { 560.0 + AMP_DRAWER_SPACE } else { 560.0 };
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(800.0, min_h)));
                let delta = if self.amp_open { AMP_DRAWER_SPACE } else { -AMP_DRAWER_SPACE };
                let new_h = (rect.height() + delta).max(min_h);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(rect.width(), new_h)));
                self.amp_resized = self.amp_open;
            }
        }

        if self.amp_open {
            let (fill, stroke) = theme::amp_chassis(ui);
            let amp_frame = egui::Frame::new()
                .fill(fill)
                .stroke(stroke)
                .inner_margin(egui::Margin::symmetric(14, 12))
                .outer_margin(egui::Margin { top: 10, ..Default::default() })
                .corner_radius(8);
            egui::TopBottomPanel::bottom("fretwise_amp_drawer")
                .frame(amp_frame)
                .show_separator_line(false)
                .show_inside(ui, |ui| self.fx_pane(ui));
        }

        egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| match self.tab {
            Tab::Tuner => self.tuner_tab(ui, &reading),
            Tab::Trainer => self.trainer_tab(ui, now),
            Tab::Chords => self.chords_tab(ui, now),
            Tab::Learn => self.learn_tab(ui, now),
            Tab::Progress => self.progress_tab(ui),
        });

        // Settings float over the page rather than squeezing it, so the
        // neck and controls keep their layout at any window size.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !about_was_open {
            self.settings_open = false;
        }
        if self.settings_open {
            let area = ui.max_rect();
            let frame = egui::Frame::window(ui.style()).fill(rack_fill(ui)).inner_margin(16).corner_radius(8);
            let shown = egui::Window::new("Settings")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .frame(frame)
                .pivot(egui::Align2::RIGHT_TOP)
                .fixed_pos(egui::pos2(area.right(), area.top() + 52.0))
                .show(ui.ctx(), |ui| {
                    ui.set_width(300.0);
                    self.settings_panel(ui, now, host);
                });
            // A click elsewhere closes it, unless it lands in one of its own
            // dropdowns (a popup outside its rect) or dismisses About.
            let press = ui.input(|i| if i.pointer.primary_pressed() { i.pointer.interact_pos() } else { None });
            let popup_open = ui.ctx().memory(|m| m.any_popup_open());
            if let (Some(p), Some(shown)) = (press, shown) {
                if settings_was_open && !about_was_open && !popup_open && !shown.response.rect.contains(p) {
                    self.settings_open = false;
                }
            }
        }
        if self.about_open {
            self.about_window(ui.ctx());
        }
        if self.browser_open {
            self.deck_browser_window(ui.ctx(), now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_is_saved_and_checked_on_load() {
        let ui = FretwiseUi { gate_db: -62.0, ..Default::default() };
        let json = serde_json::to_string(&ui.state()).unwrap();
        let back = FretwiseUi::from_state(serde_json::from_str(&json).unwrap());
        assert_eq!(back.gate_db(), -62.0);
        let silly: UiState = serde_json::from_str(r#"{"gate_db": 12.0}"#).unwrap();
        assert_eq!(FretwiseUi::from_state(silly).gate_db(), DEFAULT_GATE_DB);
    }

    #[test]
    fn state_from_before_settings_panel_loads() {
        let old = r#"{ "tab": "Tuner", "a4": 442.0, "chime": false, "show_pattern": false }"#;
        let state: UiState = serde_json::from_str(old).unwrap();
        assert!(!state.settings_open);
        assert_eq!(state.theme, Theme::System);
        let ui = FretwiseUi::from_state(state.clone());
        assert_eq!(ui.state().a4, 442.0);
        assert_eq!(ui.gate_db(), DEFAULT_GATE_DB);
        // Everything round-trips except the screen: the app always opens on Learn.
        assert_eq!(ui.state().tab, Tab::Learn);
        assert_eq!(UiState { tab: state.tab, ..ui.state() }, state);
    }
}
