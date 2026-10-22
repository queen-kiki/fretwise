//! The header: Learn's session summary, or a tool's way home and name, then
//! the input meter and the tool buttons.

use egui::RichText;
use fretwise_core::PitchReading;

use crate::parts::{caption_color, readout};
use crate::theme::{BAD, GOOD};
use crate::widgets::{flame, gear_button, icon_button, level_meter, segmented, session_bar, Icon};
use crate::{FretwiseUi, Tab};

impl FretwiseUi {
    pub(crate) fn header(&mut self, ui: &mut egui::Ui, reading: &PitchReading, warning: Option<String>) {
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            if self.tab == Tab::Learn {
                self.session_summary(ui);
            } else {
                // A tool screen: the way home, its name, and for free
                // practice which drill.
                if icon_button(ui, Icon::Back, false).on_hover_text("Back to Learn").clicked() {
                    self.tab = Tab::Learn;
                }
                let title = match self.tab {
                    Tab::Trainer | Tab::Chords => "Free practice",
                    Tab::Tuner => "Tuner",
                    Tab::Progress => "Stats",
                    Tab::Learn => "",
                };
                ui.label(RichText::new(title).size(18.0).strong());
                if matches!(self.tab, Tab::Trainer | Tab::Chords) {
                    ui.add_space(12.0);
                    segmented(ui, &mut self.tab, &[(Tab::Trainer, "Notes"), (Tab::Chords, "Chords")], 15.0);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let gear = gear_button(ui, self.settings_open).on_hover_text("Settings: audio input, tuning, display");
                if gear.clicked() {
                    self.settings_open = !self.settings_open;
                }
                // The tools, right to left: stats, tuner, free practice.
                // Clicking the open one goes back to Learn.
                let tools = [
                    (Icon::Stats, Tab::Progress, "Stats"),
                    (Icon::Tuner, Tab::Tuner, "Tuner"),
                    (Icon::Practice, Tab::Trainer, "Free practice: notes and chords drills"),
                ];
                for (icon, tab, hint) in tools {
                    let on = self.tab == tab || (tab == Tab::Trainer && self.tab == Tab::Chords);
                    if icon_button(ui, icon, on).on_hover_text(hint).clicked() {
                        self.tab = if on { Tab::Learn } else { tab };
                    }
                }
                ui.add_space(14.0);
                level_meter(ui, reading.level_db, reading.gate_db);
                ui.label(RichText::new("IN").size(11.0).color(caption_color(ui)));
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

    /// Learn's header: how this session is going, as one bar, plus the streak.
    fn session_summary(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        let (due, new_cards, _) = self.learn_trainer.deck_counts(now);
        let reviewed = self.learn_trainer.reviewed;
        let done = reviewed.saturating_sub(self.session_start.unwrap_or(reviewed)) as usize;
        let to_go = due + new_cards;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            let caption = format!("THIS SESSION · {}", self.learn_trainer.config.deck.name().to_uppercase());
            ui.label(RichText::new(caption).size(11.0).color(caption_color(ui)));
            ui.horizontal(|ui| {
                session_bar(ui, done, to_go);
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{done} done · {to_go} to go")).size(14.0));
            });
        });
        ui.add_space(20.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 26.0), egui::Sense::hover());
        flame(ui.painter(), rect.center(), 24.0);
        ui.label(RichText::new(self.learn_trainer.streak.to_string()).size(20.0).strong())
            .on_hover_text("Cards in a row without skipping");
    }
}
