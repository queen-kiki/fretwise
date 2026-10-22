//! Free practice, chords: the chord drill.

use egui::text::LayoutJob;
use fretwise_core::{ChordFamily, ChordFeedback, ChordOrder, ChordPick, ChordTrainerConfig};

use crate::parts::{
    center_stage, chord_markers, pitch_class_tokens, rack_frame, rack_group, terminal_screen, token, NECK_FRETS,
    STAGE_GAP,
};
use crate::theme::{self, GOOD, TARGET};
use crate::widgets::{dropdown, fretboard, segmented, skip_button};
use crate::FretwiseUi;

impl FretwiseUi {
    pub(crate) fn chords_tab(&mut self, ui: &mut egui::Ui, now: f64) {
        let before = self.chord_trainer.config;
        egui::TopBottomPanel::bottom("chords_rack")
            .frame(rack_frame(ui))
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
        if let (Some(ch), Some(v)) = (ch, voicing) {
            markers = chord_markers(v, ch.heard_strings, resolved, show_diagram, self.show_fingering, ui.visuals());
        }

        let terminal_id = ui.id().with("chord_terminal_height");
        let height = center_stage(ui, terminal_id);
        // A chord shape is fixed: no fret range to drag, no strings to toggle.
        fretboard(ui, NECK_FRETS, height, &markers, None);
        ui.add_space(STAGE_GAP);
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
                    pitch_class_tokens(o, ch.heard_classes, &small, GOOD);
                    let missing = v.pitch_class_mask(self.tuning) & !ch.heard_classes;
                    if missing != 0 {
                        token(o, "· need ", &small, syn.comment);
                        pitch_class_tokens(o, missing, &small, syn.note);
                    }
                }
                _ => token(o, "strum or arpeggiate the chord...", &small, syn.comment),
            },
        }

        terminal_screen(ui, cmd, out, &big, now)
    }

    fn chords_rack(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            let cfg = &mut self.chord_trainer.config;
            // Like the scale picker: grouped by family, each group led by
            // "All ..." to drill the lot, then single chords to get one clean.
            rack_group(ui, "Chords", |ui| {
                let groups: Vec<(&str, Vec<(ChordPick, &str)>)> = ChordFamily::ALL
                    .iter()
                    .map(|&family| {
                        let mut items = vec![(ChordPick::Family(family), family.all_label())];
                        items.extend(family.chords().into_iter().map(|c| (ChordPick::One(c), c.name())));
                        (family.name(), items)
                    })
                    .collect();
                let shown = cfg.pick.name();
                dropdown(ui, "chord_pick", &mut cfg.pick, shown, &groups, 210.0, 15.0);
            });
            rack_group(ui, "Order", |ui| {
                segmented(
                    ui,
                    &mut self.chord_trainer.config.order,
                    &[(ChordOrder::Random, "Random"), (ChordOrder::InOrder, "In order")],
                    15.0,
                );
            });
            rack_group(ui, "Diagram", |ui| {
                segmented(
                    ui,
                    &mut self.chord_trainer.config.show_diagram,
                    &[(true, "Show"), (false, "Hide")],
                    15.0,
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let skip = skip_button(ui, 56.0);
                if skip.on_hover_text("Show the answer and move on").clicked() {
                    self.chord_trainer.skip(now);
                }
            });
        });
    }
}
