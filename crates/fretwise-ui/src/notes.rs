//! Free practice, notes: the fretboard note trainer.

use egui::text::LayoutJob;
use fretwise_core::theory::{pitch_class_name, string_number};
use fretwise_core::{fingering, Feedback, Labels, Note, Order, Position, PromptStyle, ScaleKind, TrainerConfig};

use crate::parts::{
    center_stage, heard_dot, pattern_dot, rack_frame, rack_group, target_marker, terminal_screen, token, NECK_FRETS,
    STAGE_GAP,
};
use crate::theme::{self, pattern_fills, BAD, GOOD, TARGET};
use crate::widgets::{dropdown, fretboard, knob, segmented, skip_button, NeckControls};
use crate::FretwiseUi;

impl FretwiseUi {
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

    pub(crate) fn trainer_tab(&mut self, ui: &mut egui::Ui, now: f64) {
        let before = self.trainer.config;
        egui::TopBottomPanel::bottom("trainer_rack")
            .frame(rack_frame(ui))
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
            let (root_fill, other_fill) = pattern_fills(visuals);
            for pos in positions {
                markers.push(pattern_dot(pos, interval(pos), root_fill, other_fill));
            }
        }
        if let Some(h) = held {
            markers.extend(self.tuning.positions_of(h.note, NECK_FRETS).into_iter().map(heard_dot));
        }
        if let Some(ch) = ch {
            let resolved = self.trainer.is_resolved();
            if self.show_pattern || style == PromptStyle::FretboardDot || resolved {
                let color = if resolved { GOOD } else { TARGET };
                markers.push(target_marker(&markers, ch.position, self.target_finger(), color));
            }
        }
        let terminal_id = ui.id().with("terminal_height");
        let height = center_stage(ui, terminal_id);
        let cfg = &mut self.trainer.config;
        let controls = NeckControls {
            min_fret: &mut cfg.min_fret,
            max_fret: &mut cfg.max_fret,
            strings: Some(&mut cfg.strings),
        };
        fretboard(ui, NECK_FRETS, height, &markers, Some(controls));
        ui.add_space(STAGE_GAP);
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

        terminal_screen(ui, cmd, out, &big, now)
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
                if skip_button(ui, 56.0).on_hover_text("Skip: show the answer and move on (Space)").clicked() {
                    self.trainer.skip(now);
                }
            });
        });
    }
}
