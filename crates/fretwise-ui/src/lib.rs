//! Fretwise's GUI, written against plain `egui` so the same views can be
//! hosted by eframe (standalone app) or by a plugin's egui editor.
//!
//! The host's only job is to hand [`FretwiseUi::show`] the latest
//! [`PitchReading`] each frame, plus anything it adds through [`Host`].

mod theme;
mod widgets;

use egui::text::LayoutJob;
use egui::{Color32, RichText};
use fretwise_core::theory::{nearest_note, pitch_class_name, string_number};
use fretwise_core::{
    fingering, ChordFeedback, ChordOrder, ChordProgress, ChordSetKind, ChordTrainer, ChordTrainerConfig, Feedback,
    Labels, Note, NoteStabilizer, Order, PitchReading, Position, Progress, PromptStyle, ScaleKind, Trainer,
    TrainerConfig, Tuning,
};
use serde::{Deserialize, Serialize};

use theme::{paint_flag, paint_pick, rack_fill, TRANS_BLUE, TRANS_PINK};
use widgets::{dropdown, fretboard, knob, level_meter, segmented, toggle, tuner_gauge, Marker, NeckControls};

const GOOD: Color32 = Color32::from_rgb(80, 200, 120);
const BAD: Color32 = Color32::from_rgb(230, 90, 80);
const TARGET: Color32 = TRANS_BLUE;
const HEARD: Color32 = Color32::from_rgb(240, 190, 70);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Tab {
    Tuner,
    Trainer,
    Chords,
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
    about_open: bool,
    theme: Theme,
    a4: f32,
    tuning: Tuning,
    stabilizer: NoteStabilizer,
    trainer: Trainer,
    chord_trainer: ChordTrainer,
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
            about_open: false,
            theme: Theme::System,
            a4: 440.0,
            tuning: Tuning::STANDARD,
            stabilizer: NoteStabilizer::default(),
            trainer: Trainer::new(Tuning::STANDARD, TrainerConfig::default(), seed),
            chord_trainer: ChordTrainer::new(Tuning::STANDARD, ChordTrainerConfig::default(), seed ^ 0x9e37_79b9),
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
    /// Open strings, lowest first; matched against [`Tuning::ALL`] on load.
    tuning: [Note; 6],
    trainer: TrainerConfig,
    progress: Progress,
    chords: ChordTrainerConfig,
    chord_progress: ChordProgress,
    /// Play a sound on correct answers.
    chime: bool,
    /// Draw the scale's positions on the trainer's fretboard.
    show_pattern: bool,
    /// Name the finger for the target in the prompt, when the pattern fits
    /// one hand position.
    show_fingering: bool,
    settings_open: bool,
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
            tab: state.tab,
            settings_open: state.settings_open,
            theme: state.theme,
            a4: if (415.0..=466.0).contains(&state.a4) { state.a4 } else { defaults.a4 },
            tuning: Tuning::from_strings(state.tuning).unwrap_or(Tuning::STANDARD),
            chime_enabled: state.chime,
            show_pattern: state.show_pattern,
            show_fingering: state.show_fingering,
            ..defaults
        };
        let mut cfg = state.trainer;
        cfg.max_fret = cfg.max_fret.min(15);
        cfg.min_fret = cfg.min_fret.min(cfg.max_fret);
        ui.trainer.tuning = ui.tuning;
        ui.trainer.config = cfg;
        ui.trainer.set_progress(state.progress);
        ui.chord_trainer.tuning = ui.tuning;
        ui.chord_trainer.config = state.chords;
        ui.chord_trainer.set_progress(state.chord_progress);
        ui
    }

    pub fn state(&self) -> UiState {
        UiState {
            tab: self.tab,
            a4: self.a4,
            tuning: self.tuning.strings,
            trainer: self.trainer.config,
            progress: self.trainer.progress(),
            chords: self.chord_trainer.config,
            chord_progress: self.chord_trainer.progress(),
            chime: self.chime_enabled,
            show_pattern: self.show_pattern,
            show_fingering: self.show_fingering,
            settings_open: self.settings_open,
            theme: self.theme,
        }
    }

    /// True once per correct answer (when sound is on). The host plays the
    /// sound, e.g. by triggering a [`fretwise_core::Chime`] in its output.
    pub fn take_chime(&mut self) -> bool {
        std::mem::take(&mut self.chime_pending)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, reading: PitchReading, host: &mut dyn Host) {
        let now = ui.input(|i| i.time);
        let held = self.stabilizer.update(now, &reading, self.a4);
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
        }
        // Pitch moves continuously; keep repainting while anything is heard.
        ui.ctx().request_repaint();
        theme::apply(ui.ctx());
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

        egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| match self.tab {
            Tab::Tuner => self.tuner_tab(ui, &reading),
            Tab::Trainer => self.trainer_tab(ui, now),
            Tab::Chords => self.chords_tab(ui, now),
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
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        // A modal: it dims the page and swallows the click that closes it,
        // so dismissing it can't also turn a knob or toggle a string.
        let frame = egui::Frame::window(&ctx.style()).inner_margin(24).corner_radius(14);
        let modal = egui::Modal::new(egui::Id::new("fretwise_about")).frame(frame).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, 180.0), egui::Sense::hover());
                paint_flag(ui.painter(), rect, 10.0);
                ui.add_space(14.0);
                let (pick, _) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                paint_pick(ui.painter(), pick);
                ui.add_space(4.0);
                ui.label(RichText::new("Fretwise").size(30.0).strong());
                ui.label(RichText::new("guitar fretboard trainer").weak());
                ui.add_space(10.0);
                ui.label(format!("version {}", env!("CARGO_PKG_VERSION")));
                ui.label(format!("built {}", env!("FRETWISE_BUILD_DATE")));
                ui.add_space(12.0);
                let pink = if ui.visuals().dark_mode { TRANS_PINK } else { Color32::from_rgb(200, 70, 120) };
                ui.label(RichText::new("proudly slopped by kiki").italics().size(15.0).color(pink));
            });
        });
        if modal.should_close() {
            self.about_open = false;
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, reading: &PitchReading, warning: Option<String>) {
        ui.horizontal(|ui| {
            segmented(
                ui,
                &mut self.tab,
                &[
                    (Tab::Trainer, "Notes"),
                    (Tab::Chords, "Chords"),
                    (Tab::Tuner, "Tuner"),
                    (Tab::Progress, "Progress"),
                ],
                16.0,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let gear = ui
                    .add(egui::Button::new(RichText::new("⚙").size(22.0)).selected(self.settings_open).frame(false))
                    .on_hover_text("Settings: audio input, tuning, display");
                if gear.clicked() {
                    self.settings_open = !self.settings_open;
                }
                ui.add_space(10.0);
                level_meter(ui, reading.level_db);
                ui.label(RichText::new("IN").size(11.0).weak());
                if let Some(w) = warning {
                    ui.add_space(10.0);
                    let chip = ui.add(egui::Button::new(RichText::new(format!("⚠ {w}")).color(BAD)).frame(false));
                    if chip.on_hover_text("Open audio settings").clicked() {
                        self.settings_open = true;
                    }
                }
                if self.tab == Tab::Trainer {
                    ui.add_space(24.0);
                    let t = &self.trainer;
                    readout(ui, "wrong", t.wrong, BAD);
                    readout(ui, "right", t.correct, GOOD);
                    readout(ui, "best", t.best_streak, ui.visuals().text_color());
                    readout(ui, "streak", t.streak, ui.visuals().strong_text_color());
                } else if self.tab == Tab::Chords {
                    ui.add_space(24.0);
                    let t = &self.chord_trainer;
                    readout(ui, "right", t.correct, GOOD);
                    readout(ui, "best", t.best_streak, ui.visuals().text_color());
                    readout(ui, "streak", t.streak, ui.visuals().strong_text_color());
                }
            });
        });
    }

    /// Everything that's set once and left alone.
    fn settings_panel(&mut self, ui: &mut egui::Ui, now: f64, host: &mut dyn Host) {
        ui.label(RichText::new("Settings").size(18.0).strong());
        egui::ScrollArea::vertical().max_height(ui.ctx().screen_rect().height() - 130.0).show(ui, |ui| {
            section(ui, "Audio input");
            host.audio_settings(ui);

            section(ui, "Instrument");
            let tuning_before = self.tuning;
            egui::Grid::new("instrument").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Tuning");
                egui::ComboBox::from_id_salt("tuning")
                    .selected_text(self.tuning.name)
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for t in Tuning::ALL {
                            ui.selectable_value(&mut self.tuning, t, t.name);
                        }
                    });
                ui.end_row();
                ui.label("Reference");
                ui.add(egui::DragValue::new(&mut self.a4).range(415.0..=466.0).suffix(" Hz").prefix("A4 = "));
                ui.end_row();
            });
            if self.tuning != tuning_before {
                self.trainer.tuning = self.tuning;
                self.trainer.restart(now);
                self.chord_trainer.tuning = self.tuning;
                self.chord_trainer.restart(now);
            }

            section(ui, "Trainer");
            ui.label("Ask with");
            segmented(
                ui,
                &mut self.trainer.config.style,
                &[(PromptStyle::NameOnString, "Note name"), (PromptStyle::FretboardDot, "Dot on the neck")],
                14.0,
            );
            ui.add_space(6.0);
            setting_switch(ui, &mut self.show_pattern, "Show the scale pattern");
            setting_switch(ui, &mut self.show_fingering, "Say which finger plays the note");
            ui.label(RichText::new("For fret ranges up to 5 frets wide, e.g. 5 to 8.").weak().size(12.0));
            setting_switch(ui, &mut self.chime_enabled, "Chime on correct notes");
            ui.add_space(4.0);
            ui.label(RichText::new("Click string numbers on the neck to leave strings out.").weak());

            section(ui, "Appearance");
            let themes = [(Theme::System, "System"), (Theme::Dark, "Dark"), (Theme::Light, "Light")];
            segmented(ui, &mut self.theme, &themes, 14.0);

            // Tucked away at the bottom, where the curious will find it.
            ui.add_space(18.0);
            let about = ui.add(egui::Label::new(RichText::new("about").size(12.0).weak()).sense(egui::Sense::click()));
            if about.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.about_open = true;
            }
        });
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
            ui.add_space(16.0);
            ui.label(RichText::new(format!("{}  ·  A4 = {} Hz", self.tuning.name, self.a4)).weak());
        });
    }

    /// A note as prompts and markers show it: "C3", or "b3" in degree mode.
    fn label(&self, note: Note) -> String {
        match self.trainer.config.labels {
            Labels::NoteName => note.to_string(),
            Labels::Degree => self.trainer.config.scale.degree(note).to_string(),
        }
    }

    /// Like [`Self::label`], plus the note name in degree mode, for feedback
    /// once the answer no longer needs hiding.
    fn label_full(&self, note: Note) -> String {
        match self.trainer.config.labels {
            Labels::NoteName => note.to_string(),
            Labels::Degree => format!("{} ({note})", self.label(note)),
        }
    }

    fn trainer_tab(&mut self, ui: &mut egui::Ui, now: f64) {
        let before = self.trainer.config;
        egui::TopBottomPanel::bottom("trainer_rack")
            .frame(
                egui::Frame::new()
                    .fill(rack_fill(ui))
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .corner_radius(8),
            )
            .show_separator_line(false)
            .show_inside(ui, |ui| self.trainer_rack(ui, now));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| self.trainer_stage(ui, now));
        // Labels and prompt style only change what's shown; don't send a
        // pattern back to the bottom for them.
        let display_only = TrainerConfig { labels: before.labels, style: before.style, ..self.trainer.config };
        if display_only != before {
            self.trainer.restart(now);
        }
    }

    /// The prompt and the neck: what you look at while playing.
    fn trainer_stage(&mut self, ui: &mut egui::Ui, now: f64) {
        let ch = self.trainer.current().copied();
        let held = self.stabilizer.current();
        let style = self.trainer.config.style;
        let scale = self.trainer.config.scale;

        // Fretboard: the pattern underneath, then what's heard, then the target.
        let mut markers = Vec::new();
        if self.show_pattern {
            let visuals = ui.visuals();
            let interval = |pos: Position| (self.tuning.note_at(pos).pitch_class() + 12 - scale.root % 12) % 12;
            let mut positions = self.pattern();
            // Every note is in the chromatic scale, so only the key's root is marked.
            if scale.kind.is_chromatic() {
                positions.retain(|&pos| interval(pos) == 0);
            }
            // Small and unlabelled, so the shape reads at a glance: a bright
            // tonic, a slightly bigger 5th, the rest smallest.
            let (root_fill, other_fill) = if visuals.dark_mode {
                (TRANS_PINK, Color32::from_rgb(118, 86, 110))
            } else {
                (Color32::from_rgb(214, 92, 140), Color32::from_rgb(222, 184, 202))
            };
            for pos in positions {
                let (color, size) = match interval(pos) {
                    0 => (root_fill, 0.55),
                    7 => (other_fill, 0.42),
                    _ => (other_fill, 0.32),
                };
                markers.push(Marker { pos, color, size, ring: false, label: None, text: Color32::BLACK });
            }
        }
        if let Some(h) = held {
            for pos in self.tuning.positions_of(h.note, 15) {
                markers.push(Marker {
                    pos,
                    color: HEARD,
                    size: 0.42,
                    ring: false,
                    label: None,
                    text: Color32::BLACK,
                });
            }
        }
        if let Some(ch) = ch {
            let resolved = self.trainer.is_resolved();
            if self.show_pattern || style == PromptStyle::FretboardDot || resolved {
                let color = if resolved { GOOD } else { TARGET };
                // A ring around whatever dot sits there, a little bigger than it.
                let under = markers.iter().filter(|m| m.pos == ch.position).map(|m| m.size).fold(0.32, f32::max);
                markers.push(Marker {
                    pos: ch.position,
                    color,
                    size: under + 0.28,
                    ring: true,
                    label: None,
                    text: Color32::BLACK,
                });
            }
        }
        // Neck and terminal sit centred between header and rack; the
        // terminal's height is last frame's, measured below.
        let terminal_id = ui.id().with("terminal_height");
        let terminal_h = ui.data(|d| d.get_temp::<f32>(terminal_id)).unwrap_or(100.0);
        let gap = 8.0;
        let height = (ui.available_height() - terminal_h - gap - 12.0).clamp(220.0, 360.0);
        let spare = ui.available_height() - height - gap - terminal_h;
        ui.add_space((spare / 2.0).max(0.0));
        let cfg = &mut self.trainer.config;
        let controls = NeckControls {
            min_fret: &mut cfg.min_fret,
            max_fret: &mut cfg.max_fret,
            strings: &mut cfg.strings,
        };
        fretboard(ui, 15, height, &markers, Some(controls));
        ui.add_space(gap);
        let h = self.terminal(ui, now);
        ui.data_mut(|d| d.insert_temp(terminal_id, h));
    }

    /// The positions the pattern shows: every candidate for random order,
    /// the walk for Up and Up & down.
    fn pattern(&self) -> Vec<Position> {
        match self.trainer.config.order {
            Order::Random => self.trainer.candidates(),
            Order::Ascending | Order::UpDown => self.trainer.sequence(),
        }
    }

    /// Which finger plays the target (0 = open string), if fingering is on
    /// and the pattern fits one hand position.
    fn target_finger(&self) -> Option<u8> {
        let ch = self.trainer.current().filter(|_| self.show_fingering)?;
        let pattern = self.trainer.fingering_positions();
        let fingers = fingering(&pattern, self.trainer.config.min_fret)?;
        pattern.iter().position(|&p| p == ch.position).map(|i| fingers[i])
    }

    /// The prompt as a command line under the neck, what to play and then
    /// how it went, highlighted in the flag colours.
    /// Returns the height it took.
    fn terminal(&self, ui: &mut egui::Ui, now: f64) -> f32 {
        let syn = &theme::SYNTAX;
        let big = theme::pixel_font(ui, 24.0);
        let small = theme::pixel_font(ui, 16.0);

        let mut cmd = LayoutJob::default();
        let c = &mut cmd;
        token(c, "> ", &big, syn.comment);
        match self.trainer.current() {
            Some(ch) => {
                token(c, "play ", &big, syn.keyword);
                match self.trainer.config.style {
                    PromptStyle::NameOnString => {
                        if self.trainer.config.labels == Labels::Degree {
                            token(c, "degree ", &big, syn.text);
                        }
                        token(c, &self.label(ch.note), &big, syn.note);
                        token(c, " on string ", &big, syn.text);
                        token(c, &string_number(ch.position.string).to_string(), &big, syn.number);
                    }
                    PromptStyle::FretboardDot => {
                        token(c, "the ", &big, syn.text);
                        token(c, "ringed", &big, syn.note);
                        token(c, " note", &big, syn.text);
                    }
                }
                // In the target ring's blue, so it reads as part of that spot.
                match self.target_finger() {
                    Some(0) => token(c, " open string", &big, TARGET),
                    Some(f) => {
                        token(c, " with finger ", &big, syn.text);
                        token(c, &f.to_string(), &big, TARGET);
                    }
                    None => {}
                }
            }
            None => {
                token(c, "nothing to play ", &big, syn.text);
                token(c, "# widen the frets or turn strings on", &big, syn.comment);
            }
        }

        let mut out = LayoutJob::default();
        let o = &mut out;
        let held = self.stabilizer.current();
        match self.trainer.feedback().filter(|(_, t)| now - t < 2.0) {
            Some((Feedback::Correct { note, seconds }, _)) => {
                token(o, "ok ", &small, GOOD);
                token(o, &self.label_full(note), &small, syn.note);
                token(o, " in ", &small, syn.text);
                token(o, &format!("{seconds:.1}s"), &small, syn.number);
            }
            Some((Feedback::Wrong { played, .. }, _)) => {
                token(o, "err ", &small, BAD);
                token(o, "that was ", &small, syn.text);
                token(o, &self.label_full(played), &small, syn.note);
            }
            Some((Feedback::Skipped { expected }, _)) => {
                token(o, "skip ", &small, syn.comment);
                token(o, "it was ", &small, syn.text);
                token(o, &self.label_full(expected), &small, syn.note);
            }
            None => match held {
                Some(h) => {
                    token(o, "hearing ", &small, syn.comment);
                    token(o, &self.label(h.note), &small, syn.note);
                }
                None => token(o, "listening...", &small, syn.comment),
            },
        }

        egui::Frame::new()
            .fill(syn.screen)
            .stroke(egui::Stroke::new(1.0_f32, syn.border))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 8.0;
                let line = ui.label(cmd).rect;
                // A blinking block cursor after the command.
                if (now * 1.6).fract() < 0.55 {
                    let cursor = egui::Rect::from_min_size(
                        egui::pos2(line.right() + 4.0, line.top() + 2.0),
                        egui::vec2(big.size * 0.55, line.height() - 4.0),
                    );
                    ui.painter().rect_filled(cursor, 0.0, syn.keyword);
                }
                ui.label(out);
            })
            .response
            .rect
            .height()
    }

    /// The everyday controls, laid out like a plug-in's front panel.
    fn trainer_rack(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            let cfg = &mut self.trainer.config;
            let mut root = cfg.scale.root as usize % 12;
            let roots: Vec<&str> = (0..12).map(pitch_class_name).collect();
            if knob(ui, "root", "Key", &mut root, &roots) {
                cfg.scale.root = root as u8;
            }
            rack_group(ui, "Scale", |ui| {
                let groups: Vec<(&str, Vec<(ScaleKind, &str)>)> = ScaleKind::groups()
                    .map(|(group, kinds)| (group, kinds.map(|k| (k, k.name())).collect()))
                    .collect();
                let shown = cfg.scale.kind.name();
                dropdown(ui, "scale", &mut cfg.scale.kind, shown, &groups, 210.0, 15.0);
            });
            rack_group(ui, "Order", |ui| {
                segmented(
                    ui,
                    &mut cfg.order,
                    &[(Order::Random, "Random"), (Order::Ascending, "Up"), (Order::UpDown, "Up & down")],
                    15.0,
                );
            });
            rack_group(ui, "Labels", |ui| {
                segmented(ui, &mut cfg.labels, &[(Labels::NoteName, "Notes"), (Labels::Degree, "Degrees")], 15.0);
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let skip = egui::Button::new(RichText::new("Skip").size(16.0))
                    .min_size(egui::vec2(76.0, 40.0))
                    .fill(ui.visuals().extreme_bg_color)
                    .corner_radius(8);
                if ui.add(skip).on_hover_text("Show the answer and move on").clicked() {
                    self.trainer.skip(now);
                }
            });
        });
    }

    fn chords_tab(&mut self, ui: &mut egui::Ui, now: f64) {
        let before = self.chord_trainer.config;
        egui::TopBottomPanel::bottom("chords_rack")
            .frame(
                egui::Frame::new()
                    .fill(rack_fill(ui))
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .corner_radius(8),
            )
            .show_separator_line(false)
            .show_inside(ui, |ui| self.chords_rack(ui, now));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| self.chords_stage(ui, now));
        let display_only = ChordTrainerConfig { show_diagram: before.show_diagram, ..self.chord_trainer.config };
        if display_only != before {
            self.chord_trainer.restart(now);
        }
    }

    fn chords_stage(&mut self, ui: &mut egui::Ui, now: f64) {
        let ch = self.chord_trainer.current().copied();
        let voicing = self.chord_trainer.current_chord();
        let resolved = self.chord_trainer.is_resolved();
        let show_diagram = self.chord_trainer.config.show_diagram || resolved;

        let mut markers = Vec::new();
        let mut active_strings = [true; 6];
        if let (Some(ch), Some(v)) = (ch, voicing) {
            for (string, slot) in active_strings.iter_mut().enumerate() {
                *slot = v.frets[string].is_some();
                if let Some(fret) = v.frets[string] {
                    let pos = Position { string, fret };
                    let heard = (ch.heard_strings & (1 << string)) != 0;
                    if show_diagram {
                        let color = if resolved || heard { GOOD } else { TARGET };
                        let finger = v.fingers[string];
                        let label = if finger > 0 && self.show_fingering {
                            Some(finger.to_string())
                        } else if fret == 0 {
                            Some("O".into())
                        } else {
                            None
                        };
                        markers.push(Marker {
                            pos,
                            color,
                            size: 0.78,
                            ring: fret == 0 && !heard && !resolved,
                            label,
                            text: if fret == 0 && !heard && !resolved { color } else { Color32::BLACK },
                        });
                    } else if heard {
                        markers.push(Marker {
                            pos,
                            color: GOOD,
                            size: 0.65,
                            ring: false,
                            label: None,
                            text: Color32::BLACK,
                        });
                    }
                }
            }
        }

        let terminal_id = ui.id().with("chord_terminal_height");
        let terminal_h = ui.data(|d| d.get_temp::<f32>(terminal_id)).unwrap_or(100.0);
        let gap = 8.0;
        let height = (ui.available_height() - terminal_h - gap - 12.0).clamp(220.0, 360.0);
        let spare = ui.available_height() - height - gap - terminal_h;
        ui.add_space((spare / 2.0).max(0.0));
        let mut min_fret = 0u8;
        let mut max_fret = 15u8;
        let controls = NeckControls {
            min_fret: &mut min_fret,
            max_fret: &mut max_fret,
            strings: &mut active_strings,
        };
        fretboard(ui, 15, height, &markers, Some(controls));
        ui.add_space(gap);
        let h = self.chords_terminal(ui, now);
        ui.data_mut(|d| d.insert_temp(terminal_id, h));
    }

    fn chords_terminal(&self, ui: &mut egui::Ui, now: f64) -> f32 {
        let syn = &theme::SYNTAX;
        let big = theme::pixel_font(ui, 24.0);
        let small = theme::pixel_font(ui, 16.0);

        let mut cmd = LayoutJob::default();
        let c = &mut cmd;
        token(c, "> ", &big, syn.comment);
        match self.chord_trainer.current_chord() {
            Some(v) => {
                token(c, "strum ", &big, syn.keyword);
                token(c, &v.name, &big, syn.note);
                if let Some(barre) = v.barre.filter(|_| self.show_fingering) {
                    token(c, "  barre fret ", &big, syn.text);
                    token(c, &barre.to_string(), &big, TARGET);
                }
            }
            None => {
                token(c, "no chords in set", &big, syn.text);
            }
        }

        let mut out = LayoutJob::default();
        let o = &mut out;
        match self.chord_trainer.feedback().filter(|(_, t)| now - t < 2.0) {
            Some((ChordFeedback::Correct { name, seconds }, _)) => {
                token(o, "ok ", &small, GOOD);
                token(o, name, &small, syn.note);
                token(o, " in ", &small, syn.text);
                token(o, &format!("{seconds:.1}s"), &small, syn.number);
            }
            Some((ChordFeedback::Skipped { name }, _)) => {
                token(o, "skip ", &small, syn.comment);
                token(o, "it was ", &small, syn.text);
                token(o, name, &small, syn.note);
            }
            None => match (self.chord_trainer.current(), self.chord_trainer.current_chord()) {
                (Some(ch), Some(v)) if ch.heard_classes != 0 => {
                    token(o, "hearing ", &small, syn.comment);
                    for pc in 0..12u8 {
                        if (ch.heard_classes & (1 << pc)) != 0 {
                            token(o, pitch_class_name(pc), &small, GOOD);
                            token(o, " ", &small, syn.text);
                        }
                    }
                    let missing = v.pitch_class_mask(self.tuning) & !ch.heard_classes;
                    if missing != 0 {
                        token(o, "· need ", &small, syn.comment);
                        for pc in 0..12u8 {
                            if (missing & (1 << pc)) != 0 {
                                token(o, pitch_class_name(pc), &small, syn.note);
                                token(o, " ", &small, syn.text);
                            }
                        }
                    }
                }
                _ => token(o, "strum or arpeggiate the chord...", &small, syn.comment),
            },
        }

        egui::Frame::new()
            .fill(syn.screen)
            .stroke(egui::Stroke::new(1.0_f32, syn.border))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 8.0;
                let line = ui.label(cmd).rect;
                if (now * 1.6).fract() < 0.55 {
                    let cursor = egui::Rect::from_min_size(
                        egui::pos2(line.right() + 4.0, line.top() + 2.0),
                        egui::vec2(big.size * 0.55, line.height() - 4.0),
                    );
                    ui.painter().rect_filled(cursor, 0.0, syn.keyword);
                }
                ui.label(out);
            })
            .response
            .rect
            .height()
    }

    fn chords_rack(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            let cfg = &mut self.chord_trainer.config;
            rack_group(ui, "Curriculum", |ui| {
                let groups: Vec<(&str, Vec<(ChordSetKind, &str)>)> = ChordSetKind::groups()
                    .map(|(group, kinds)| (group, kinds.map(|k| (k, k.name())).collect()))
                    .collect();
                let shown = cfg.set.name();
                dropdown(ui, "chord_set", &mut cfg.set, shown, &groups, 210.0, 15.0);
            });
            rack_group(ui, "Order", |ui| {
                segmented(
                    ui,
                    &mut cfg.order,
                    &[(ChordOrder::Random, "Random"), (ChordOrder::InOrder, "In order")],
                    15.0,
                );
            });
            rack_group(ui, "Diagram", |ui| {
                segmented(
                    ui,
                    &mut cfg.show_diagram,
                    &[(true, "Show"), (false, "Hide")],
                    15.0,
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let skip = egui::Button::new(RichText::new("Skip").size(16.0))
                    .min_size(egui::vec2(76.0, 40.0))
                    .fill(ui.visuals().extreme_bg_color)
                    .corner_radius(8);
                if ui.add(skip).on_hover_text("Show the answer and move on").clicked() {
                    self.chord_trainer.skip(now);
                }
            });
        });
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
                    markers.push(Marker {
                        pos,
                        color,
                        size: 1.0,
                        ring: false,
                        label: Some(format!("{:.1}", s.avg_time())),
                        text: Color32::BLACK,
                    });
                }
            }
        }
        let height = (ui.available_height() - 48.0).clamp(220.0, 360.0);
        fretboard(ui, 15, height, &markers, None);
        ui.add_space(8.0);
        if ui.button("Reset progress").clicked() {
            self.trainer.reset_stats();
        }
    }
}

/// Append `text` to a terminal line in one colour.
fn token(job: &mut LayoutJob, text: &str, font: &egui::FontId, color: Color32) {
    job.append(text, 0.0, egui::TextFormat::simple(font.clone(), color));
}

/// A small caps caption over a control, as printed on hardware.
fn rack_group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(title.to_uppercase()).size(11.0).weak());
        add(ui);
    });
}

/// A number with a caption under it, for the score.
fn readout(ui: &mut egui::Ui, caption: &str, value: u32, color: Color32) {
    let layout = egui::Layout::top_down(egui::Align::Center);
    ui.allocate_ui_with_layout(egui::vec2(48.0, 40.0), layout, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(value.to_string()).size(20.0).strong().color(color));
        ui.label(RichText::new(caption.to_uppercase()).size(10.0).weak());
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(title.to_uppercase()).size(11.0).weak());
    ui.separator();
}

fn setting_switch(ui: &mut egui::Ui, on: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        toggle(ui, on);
        ui.label(label);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_from_before_settings_panel_loads() {
        let old = r#"{ "tab": "Tuner", "a4": 442.0, "chime": false, "show_pattern": false }"#;
        let state: UiState = serde_json::from_str(old).unwrap();
        assert!(!state.settings_open);
        assert_eq!(state.theme, Theme::System);
        let ui = FretwiseUi::from_state(state.clone());
        assert_eq!(ui.state().a4, 442.0);
        assert_eq!(ui.state(), state);
    }
}
