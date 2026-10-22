//! Fretwise's GUI, written against plain `egui` so the same views can be
//! hosted by eframe (standalone app) or by a plugin's egui editor.
//!
//! The host's only job is to hand [`FretwiseUi::show`] the latest
//! [`PitchReading`] each frame.

mod widgets;

use egui::{Color32, RichText};
use fretwise_core::theory::{nearest_note, string_number};
use fretwise_core::{
    Feedback, Note, NoteStabilizer, PitchReading, Position, Progress, PromptStyle, Trainer, TrainerConfig, Tuning,
};
use serde::{Deserialize, Serialize};

use widgets::{fretboard, level_meter, tuner_gauge, Marker};

const GOOD: Color32 = Color32::from_rgb(80, 200, 120);
const BAD: Color32 = Color32::from_rgb(230, 90, 80);
const TARGET: Color32 = Color32::from_rgb(90, 160, 255);
const HEARD: Color32 = Color32::from_rgb(240, 190, 70);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Tab {
    Tuner,
    Trainer,
    Progress,
}

pub struct FretwiseUi {
    tab: Tab,
    a4: f32,
    tuning: Tuning,
    stabilizer: NoteStabilizer,
    trainer: Trainer,
    smoothed_cents: f32,
    chime_enabled: bool,
    chime_pending: bool,
}

impl Default for FretwiseUi {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        Self {
            tab: Tab::Trainer,
            a4: 440.0,
            tuning: Tuning::STANDARD,
            stabilizer: NoteStabilizer::default(),
            trainer: Trainer::new(Tuning::STANDARD, TrainerConfig::default(), seed),
            smoothed_cents: 0.0,
            chime_enabled: true,
            chime_pending: false,
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
    /// Open strings, lowest first; matched against [`Tuning::ALL`] on load.
    tuning: [Note; 6],
    trainer: TrainerConfig,
    progress: Progress,
    /// Play a sound on correct answers.
    chime: bool,
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
            tab: state.tab,
            a4: if (415.0..=466.0).contains(&state.a4) { state.a4 } else { defaults.a4 },
            tuning: Tuning::from_strings(state.tuning).unwrap_or(Tuning::STANDARD),
            chime_enabled: state.chime,
            ..defaults
        };
        let mut cfg = state.trainer;
        cfg.max_fret = cfg.max_fret.min(15);
        cfg.min_fret = cfg.min_fret.min(cfg.max_fret);
        ui.trainer.tuning = ui.tuning;
        ui.trainer.config = cfg;
        ui.trainer.set_progress(state.progress);
        ui
    }

    pub fn state(&self) -> UiState {
        UiState {
            tab: self.tab,
            a4: self.a4,
            tuning: self.tuning.strings,
            trainer: self.trainer.config,
            progress: self.trainer.progress(),
            chime: self.chime_enabled,
        }
    }

    /// True once per correct answer (when sound is on). The host plays the
    /// sound, e.g. by triggering a [`fretwise_core::Chime`] in its output.
    pub fn take_chime(&mut self) -> bool {
        std::mem::take(&mut self.chime_pending)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, reading: PitchReading) {
        let now = ui.input(|i| i.time);
        let held = self.stabilizer.update(now, &reading, self.a4);
        if self.tab == Tab::Trainer {
            let before = self.trainer.feedback();
            self.trainer.update(now, held.map(|h| h.note));
            let after = self.trainer.feedback();
            if after != before && matches!(after, Some((Feedback::Correct { .. }, _))) {
                self.chime_pending |= self.chime_enabled;
            }
        }
        // Pitch moves continuously; keep repainting while anything is heard.
        ui.ctx().request_repaint();

        let tuning_before = self.tuning;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Trainer, "Fretboard trainer");
            ui.selectable_value(&mut self.tab, Tab::Tuner, "Tuner");
            ui.selectable_value(&mut self.tab, Tab::Progress, "Progress");
            ui.separator();
            egui::ComboBox::from_id_salt("tuning")
                .selected_text(self.tuning.name)
                .show_ui(ui, |ui| {
                    for t in Tuning::ALL {
                        ui.selectable_value(&mut self.tuning, t, t.name);
                    }
                });
            ui.add(egui::DragValue::new(&mut self.a4).range(415.0..=466.0).suffix(" Hz").prefix("A4 = "));
        });
        ui.separator();
        if self.tuning != tuning_before {
            self.trainer.tuning = self.tuning;
            self.trainer.next(now);
        }

        match self.tab {
            Tab::Tuner => self.tuner_tab(ui, &reading),
            Tab::Trainer => self.trainer_tab(ui, now),
            Tab::Progress => self.progress_tab(ui),
        }
    }

    fn tuner_tab(&mut self, ui: &mut egui::Ui, reading: &PitchReading) {
        let r = reading
            .frequency
            .filter(|_| reading.clarity >= self.stabilizer.min_clarity)
            .and_then(|f| nearest_note(f, self.a4));

        ui.vertical_centered(|ui| {
            ui.add_space(12.0);
            match r {
                Some(r) => {
                    self.smoothed_cents += (r.cents - self.smoothed_cents) * 0.3;
                    let in_tune = self.smoothed_cents.abs() < 5.0;
                    ui.label(
                        RichText::new(r.note.to_string())
                            .size(72.0)
                            .strong()
                            .color(if in_tune { GOOD } else { ui.visuals().strong_text_color() }),
                    );
                    ui.label(format!(
                        "{:.1} Hz   {:+.0} cents",
                        reading.frequency.unwrap_or(0.0),
                        self.smoothed_cents
                    ));
                }
                None => {
                    ui.label(RichText::new("–").size(72.0).weak());
                    ui.label(RichText::new("play a string").weak());
                }
            }
            ui.add_space(8.0);
            tuner_gauge(ui, r.map(|_| self.smoothed_cents));
            ui.add_space(16.0);

            // Which open string you're closest to, and how far off.
            if let Some(f) = reading.frequency.filter(|_| r.is_some()) {
                let midi = fretwise_core::theory::freq_to_midi(f, self.a4);
                let s = self.tuning.nearest_string(midi);
                let open = self.tuning.strings[s];
                let off = (midi - open.0 as f32) * 100.0;
                if off.abs() < 150.0 {
                    ui.label(format!(
                        "String {} ({}): {}",
                        string_number(s),
                        open,
                        if off.abs() < 5.0 {
                            "in tune".to_string()
                        } else if off < 0.0 {
                            format!("{:.0} cents flat — tune up", -off)
                        } else {
                            format!("{:.0} cents sharp — tune down", off)
                        }
                    ));
                }
            }
        });
    }

    fn trainer_tab(&mut self, ui: &mut egui::Ui, now: f64) {
        let ch = self.trainer.current().copied();
        let held = self.stabilizer.current();
        let style = self.trainer.config.style;

        // Prompt
        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            if let Some(ch) = ch {
                let text = match style {
                    PromptStyle::NameOnString => format!(
                        "Play {} on string {} ({})",
                        ch.note,
                        string_number(ch.position.string),
                        self.tuning.strings[ch.position.string],
                    ),
                    PromptStyle::FretboardDot => "Play the marked note".to_string(),
                };
                ui.label(RichText::new(text).size(32.0).strong());
            } else {
                ui.label(RichText::new("No positions selected — enable a string below").size(20.0));
            }

            let fb = self.trainer.feedback().filter(|(_, t)| now - t < 2.0);
            let line = match fb {
                Some((Feedback::Correct { note, seconds }, _)) => {
                    RichText::new(format!("✔ {note} in {seconds:.1}s")).color(GOOD)
                }
                Some((Feedback::Wrong { played, .. }, _)) => {
                    RichText::new(format!("✘ that was {played}")).color(BAD)
                }
                Some((Feedback::Skipped { expected }, _)) => {
                    RichText::new(format!("skipped — it was {expected}")).weak()
                }
                None => RichText::new(match held {
                    Some(h) => format!("hearing {}", h.note),
                    None => " ".to_string(),
                })
                .weak(),
            };
            ui.label(line.size(20.0));
        });
        ui.add_space(8.0);

        // Fretboard
        let mut markers = Vec::new();
        if let Some(h) = held {
            for pos in self.tuning.positions_of(h.note, 15) {
                markers.push(Marker { pos, color: HEARD.gamma_multiply(0.5), label: Some(h.note.to_string()) });
            }
        }
        if let Some(ch) = ch {
            let show_target = style == PromptStyle::FretboardDot || self.trainer.is_resolved();
            if show_target {
                let color = if self.trainer.is_resolved() { GOOD } else { TARGET };
                let label = self.trainer.is_resolved().then(|| ch.note.to_string());
                markers.push(Marker { pos: ch.position, color, label });
            }
        }
        fretboard(ui, &self.tuning, 15, &markers);

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Skip").clicked() {
                self.trainer.skip(now);
            }
            ui.label(format!(
                "streak {}   best {}   ✔ {}   ✘ {}",
                self.trainer.streak, self.trainer.best_streak, self.trainer.correct, self.trainer.wrong
            ));
        });

        ui.separator();
        let before = self.trainer.config;
        let cfg = &mut self.trainer.config;
        ui.horizontal(|ui| {
            ui.label("Prompt:");
            ui.selectable_value(&mut cfg.style, PromptStyle::NameOnString, "Note name");
            ui.selectable_value(&mut cfg.style, PromptStyle::FretboardDot, "Fretboard dot");
            ui.separator();
            ui.checkbox(&mut cfg.naturals_only, "Naturals only");
            ui.separator();
            ui.checkbox(&mut self.chime_enabled, "Sound on correct");
        });
        ui.horizontal(|ui| {
            ui.label("Strings:");
            for s in (0..6).rev() {
                let name = format!("{} ({})", string_number(s), self.tuning.strings[s]);
                ui.checkbox(&mut cfg.strings[s], name);
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut cfg.min_fret, 0..=15).text("from fret"));
            ui.add(egui::Slider::new(&mut cfg.max_fret, 0..=15).text("to fret"));
            cfg.max_fret = cfg.max_fret.max(cfg.min_fret);
        });
        if self.trainer.config != before {
            self.trainer.next(now);
        }
    }

    fn progress_tab(&mut self, ui: &mut egui::Ui) {
        ui.label("First-try accuracy per position (green = solid, red = needs work). Number = average seconds.");
        ui.add_space(8.0);
        let mut markers = Vec::new();
        for string in 0..6 {
            for fret in 0..=15u8 {
                let pos = Position { string, fret };
                if let Some(s) = self.trainer.stats(pos) {
                    let acc = s.accuracy() as f32;
                    let color = Color32::from_rgb(
                        (BAD.r() as f32 * (1.0 - acc) + GOOD.r() as f32 * acc) as u8,
                        (BAD.g() as f32 * (1.0 - acc) + GOOD.g() as f32 * acc) as u8,
                        (BAD.b() as f32 * (1.0 - acc) + GOOD.b() as f32 * acc) as u8,
                    );
                    markers.push(Marker { pos, color, label: Some(format!("{:.1}", s.avg_time())) });
                }
            }
        }
        fretboard(ui, &self.tuning, 15, &markers);
        ui.add_space(8.0);
        if ui.button("Reset progress").clicked() {
            self.trainer.reset_stats();
        }
    }
}

/// Compact input meter for the host's toolbar.
pub fn show_level(ui: &mut egui::Ui, reading: &PitchReading) {
    level_meter(ui, reading.level_db);
}
