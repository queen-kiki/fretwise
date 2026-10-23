//! The header: Learn's session summary, or a tool's way home and name, the
//! embossed Fretwise nameplate in the centre, and the meters, amp switch,
//! and sandwich menu on the right.

use egui::text::LayoutJob;
use egui::{pos2, vec2, Color32, CursorIcon, PopupCloseBehavior, Rect, RichText, Sense};
use fretwise_core::PitchReading;

use crate::parts::{caption_color, readout, token};
use crate::theme::{self, BAD, GOOD, TRANS_BLUE, TRANS_PINK};
use crate::widgets::{
    amp_button, flame, icon_button, level_meter, menu_item, output_meter, paint_embossed_plate, segmented, session_bar,
    wippschalter, Icon,
};
use crate::{icons, FretwiseUi, Tab};

impl FretwiseUi {
    pub(crate) fn header(&mut self, ui: &mut egui::Ui, reading: &PitchReading, warning: Option<String>) {
        let bar_rect = ui.max_rect();
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
            let left_end = ui.cursor().left();

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Rightmost: Sandwich menu folding the navigation tools & settings
                let menu_id = egui::Id::new("fretwise_sandwich_menu");
                let menu_open = ui.memory(|m| m.is_popup_open(menu_id));
                let menu_btn = icon_button(ui, Icon::Menu, menu_open || self.settings_open)
                    .on_hover_text("Menu: practice, tuner, stats, settings");
                if menu_btn.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(menu_id));
                }
                egui::popup_below_widget(ui, menu_id, &menu_btn, PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(174.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let tools = [
                        (Icon::Practice, Tab::Trainer, "Free practice"),
                        (Icon::Tuner, Tab::Tuner, "Tuner"),
                        (Icon::Stats, Tab::Progress, "Stats"),
                    ];
                    for (icon, tab, label) in tools {
                        let on = self.tab == tab || (tab == Tab::Trainer && self.tab == Tab::Chords);
                        if menu_item(ui, icon, label, on) {
                            self.tab = if on { Tab::Learn } else { tab };
                        }
                    }
                    ui.separator();
                    if menu_item(ui, Icon::Settings, "Settings", self.settings_open) {
                        self.settings_open = !self.settings_open;
                    }
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                // Amp/Output section (left-to-right visually: OUT [out meter] [0|I switch] [amp settings icon])
                if amp_button(ui, self.amp_open, self.amp.enabled)
                    .on_hover_text("Effects rack: amp and cabinet settings")
                    .clicked()
                {
                    self.amp_open = !self.amp_open;
                }
                ui.add_space(4.0);
                wippschalter(ui, &mut self.amp.enabled);
                ui.add_space(4.0);
                output_meter(ui, self.out_level_db, &mut self.amp.out_gain_db, self.amp.enabled);
                let out_color = if self.amp.enabled {
                    caption_color(ui)
                } else {
                    ui.visuals().weak_text_color().gamma_multiply(0.45)
                };
                ui.label(RichText::new("OUT").size(11.0).color(out_color));

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                // Input meter (left-to-right visually: IN [in meter])
                level_meter(ui, reading.level_db, &mut self.gate_db);
                ui.label(RichText::new("IN").size(11.0).color(caption_color(ui)));

                if let Some(w) = warning {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    let chip = ui.add(egui::Button::new(RichText::new(format!("⚠ {w}")).color(BAD)).frame(false));
                    if chip.on_hover_text("Open audio settings").clicked() {
                        self.settings_open = true;
                    }
                }
                if self.tab == Tab::Trainer {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(10.0);
                    let t = &self.trainer;
                    readout(ui, "wrong", t.wrong, BAD);
                    readout(ui, "right", t.correct, GOOD);
                    readout(ui, "best", t.best_streak, ui.visuals().text_color());
                    readout(ui, "streak", t.streak, ui.visuals().strong_text_color());
                } else if self.tab == Tab::Chords {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(10.0);
                    let t = &self.chord_trainer;
                    readout(ui, "right", t.correct, GOOD);
                    readout(ui, "best", t.best_streak, ui.visuals().text_color());
                    readout(ui, "streak", t.streak, ui.visuals().strong_text_color());
                }

                let right_start = ui.cursor().right();
                let plate_w = 136.0;
                let plate_h = 28.0;
                if right_start - left_end >= plate_w + 16.0 {
                    let cx = bar_rect
                        .center()
                        .x
                        .clamp(left_end + plate_w * 0.5 + 8.0, right_start - plate_w * 0.5 - 8.0);
                    let plate_rect = Rect::from_center_size(pos2(cx, bar_rect.center().y), vec2(plate_w, plate_h));
                    if brand_nameplate(ui, plate_rect, self.about_open) {
                        self.about_open = true;
                    }
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
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 26.0), egui::Sense::hover());
        flame(ui.painter(), rect.center(), 24.0);
        ui.label(RichText::new(self.learn_trainer.streak.to_string()).size(20.0).strong())
            .on_hover_text("Cards in a row without skipping");
    }
}

/// Embossed hardware nameplate with corner screws and stylized Monocraft logo in the centre of the header.
fn brand_nameplate(ui: &mut egui::Ui, rect: Rect, active: bool) -> bool {
    let response = ui
        .interact(rect, egui::Id::new("fretwise_brand_plate"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("About Fretwise");
    let hot = response.hovered() || active;
    let dark = ui.visuals().dark_mode;
    paint_embossed_plate(ui, rect, hot, true);
    let painter = ui.painter();

    let font = theme::pixel_font(ui, 13.5);
    let mut job = LayoutJob::default();
    let fret_col = if dark { Color32::WHITE } else { Color32::from_rgb(64, 30, 50) };
    let wise_col = if dark { TRANS_PINK } else { theme::DEEP_PINK };
    let chevron_col = if dark { TRANS_BLUE } else { Color32::from_rgb(45, 145, 200) };
    token(&mut job, "fret", &font, fret_col);
    token(&mut job, "wise", &font, wise_col);
    if hot {
        token(&mut job, "_", &font, chevron_col);
    }
    let shaped = ui.fonts(|f| f.layout_job(job));
    let base_job = {
        let mut bj = LayoutJob::default();
        token(&mut bj, "fretwise", &font, fret_col);
        ui.fonts(|f| f.layout_job(bj))
    };
    let pick_w = 16.0;
    let gap = 6.0;
    let content_w = pick_w + gap + base_job.size().x;
    let start_x = rect.center().x - content_w * 0.5;
    let pick_rect = Rect::from_center_size(pos2(start_x + pick_w * 0.5, rect.center().y), vec2(pick_w, pick_w));
    icons::paint(painter, pick_rect, &icons::LOGO, Color32::WHITE);
    let text_pos = pos2(pick_rect.right() + gap, rect.center().y - shaped.size().y * 0.5);
    painter.galley(text_pos, shaped, Color32::WHITE);

    response.clicked()
}
