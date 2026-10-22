//! The tuner screen.

use egui::RichText;
use fretwise_core::theory::{nearest_note, string_number};
use fretwise_core::PitchReading;

use crate::parts::caption_color;
use crate::theme::{rack_fill, GOOD};
use crate::widgets::tuner_gauge;
use crate::FretwiseUi;

impl FretwiseUi {
    pub(crate) fn tuner_tab(&mut self, ui: &mut egui::Ui, reading: &PitchReading) {
        let r = reading
            .frequency
            .filter(|_| reading.clarity >= self.stabilizer.min_clarity)
            .and_then(|f| nearest_note(f, self.a4));

        // A faceplate centred in the tab; its height is last frame's.
        let height_id = ui.id().with("tuner_height");
        let last_h = ui.data(|d| d.get_temp::<f32>(height_id)).unwrap_or(320.0);
        ui.add_space(((ui.available_height() - last_h) / 2.0).max(0.0));
        let frame = egui::Frame::new().fill(rack_fill(ui)).inner_margin(24).corner_radius(8);
        let shown = ui.vertical_centered(|ui| {
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width().min(560.0));
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
                let footer = format!("{}  ·  A4 = {} Hz", self.tuning.name, self.a4);
                ui.label(RichText::new(footer).color(caption_color(ui)));
            })
        });
        ui.data_mut(|d| d.insert_temp(height_id, shown.inner.response.rect.height()));
    }
}
