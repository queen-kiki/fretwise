//! The settings panel and the About window.

use egui::{Color32, RichText};
use fretwise_core::{PromptStyle, Tuning, GATE_RANGE_DB};

use crate::parts::{rack_group, section, setting_switch};
use crate::theme::{self, paint_flag};
use crate::widgets::{dropdown, segmented, stepped_knob};
use crate::{icons, FretwiseUi, Host, Theme};

impl FretwiseUi {
    /// Everything that's set once and left alone.
    pub(crate) fn settings_panel(&mut self, ui: &mut egui::Ui, now: f64, host: &mut dyn Host) {
        ui.label(RichText::new("Settings").size(18.0).strong());
        egui::ScrollArea::vertical().max_height(ui.ctx().screen_rect().height() - 130.0).show(ui, |ui| {
            section(ui, "Audio");
            host.audio_settings(ui);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let gates = *GATE_RANGE_DB.start() as i32..=*GATE_RANGE_DB.end() as i32;
                stepped_knob(ui, "gate", "Gate", &mut self.gate_db, gates, |db| format!("{db} dB"));
                let hint = "Quieter input is ignored, so hum and string noise don't count as notes. \
                            The IN meter turns grey below it.";
                ui.add(egui::Label::new(RichText::new(hint).weak().size(12.0)).wrap());
            });

            section(ui, "Instrument");
            let tuning_before = self.tuning;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                rack_group(ui, "Tuning", |ui| {
                    let tunings = [("Tunings", Tuning::ALL.iter().map(|t| (*t, t.name)).collect())];
                    let shown = self.tuning.name;
                    dropdown(ui, "tuning", &mut self.tuning, shown, &tunings, 170.0, 14.0);
                });
                // Whole hertz, as tuners show it; a saved fractional A4 stays
                // until the knob is turned.
                stepped_knob(ui, "a4", "A4", &mut self.a4, 415..=466, |hz| format!("{hz} Hz"));
            });
            if self.tuning != tuning_before {
                self.trainer.tuning = self.tuning;
                self.trainer.restart(now);
                self.chord_trainer.tuning = self.tuning;
                self.chord_trainer.restart(now);
                self.learn_trainer.tuning = self.tuning;
                self.learn_trainer.restart(now);
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
        });
    }

    pub(crate) fn about_window(&mut self, ctx: &egui::Context) {
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
                icons::paint(ui.painter(), pick, &icons::LOGO, Color32::WHITE);
                ui.add_space(4.0);
                ui.label(RichText::new("Fretwise").size(30.0).strong());
                ui.label(RichText::new("guitar fretboard trainer").weak());
                ui.add_space(10.0);
                ui.label(format!("version {}", env!("CARGO_PKG_VERSION")));
                ui.label(format!("built {}", env!("FRETWISE_BUILD_DATE")));
                ui.add_space(12.0);
                let pink = theme::pink_text(ui.visuals());
                ui.label(RichText::new("proudly slopped by kiki").italics().size(15.0).color(pink));
            });
        });
        if modal.should_close() {
            self.about_open = false;
        }
    }
}
