//! The stats screen: a heat map of the note trainer's results.

use egui::{Color32, RichText};
use fretwise_core::Position;

use crate::parts::{caption_color, NECK_FRETS};
use crate::theme::{self, NEEDS_WORK, TRANS_BLUE};
use crate::widgets::{fretboard, plain_button, Marker};
use crate::FretwiseUi;

impl FretwiseUi {
    pub(crate) fn progress_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("First-try accuracy per position: blue = solid, pink = needs work. Number = average seconds.")
                .color(caption_color(ui)),
        );
        ui.add_space(8.0);
        let mut markers = Vec::new();
        for string in 0..6 {
            for fret in 0..=NECK_FRETS {
                let pos = Position { string, fret };
                if let Some(s) = self.trainer.stats(pos) {
                    let acc = s.accuracy() as f32;
                    let color = theme::lerp(NEEDS_WORK, TRANS_BLUE, acc);
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
        fretboard(ui, NECK_FRETS, height, &markers, None);
        ui.add_space(8.0);
        if plain_button(ui, "Reset progress", 14.0).clicked() {
            self.trainer.reset_stats();
            self.learn_trainer.reset_stats();
        }
    }
}
