//! The deck browser: a deck's cards and their progress; click one to
//! practise it.

use egui::{Color32, RichText};
use fretwise_core::{format_interval, unix_seconds, DeckKind, LearnCard};

use crate::theme::{GOOD, HEARD, TARGET, TRANS_BLUE, TRANS_PINK};
use crate::widgets::dropdown;
use crate::{FretwiseUi, Tab};

impl FretwiseUi {
    pub(crate) fn deck_browser_window(&mut self, ctx: &egui::Context, now: f64) {
        let frame = egui::Frame::window(&ctx.style()).inner_margin(20).corner_radius(12);
        let mut close_requested = false;
        let modal = egui::Modal::new(egui::Id::new("fretwise_deck_browser")).frame(frame).show(ctx, |ui| {
            ui.set_width(580.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Deck Browser").size(20.0).strong());
                ui.add_space(8.0);
                let before = self.learn_trainer.config.deck;
                let groups: Vec<(&str, Vec<(DeckKind, &str)>)> = DeckKind::groups()
                    .map(|(group, kinds)| (group, kinds.map(|k| (k, k.name())).collect()))
                    .collect();
                let shown = self.learn_trainer.config.deck.name();
                dropdown(ui, "browser_learn_deck", &mut self.learn_trainer.config.deck, shown, &groups, 210.0, 14.0);
                if self.learn_trainer.config.deck != before {
                    self.learn_trainer.restart(now);
                }

                // No close button: Esc or a click outside closes it.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().extreme_bg_color)
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.browser_filter)
                                    .hint_text("Filter cards…")
                                    .frame(false)
                                    .desired_width(150.0),
                            );
                        });
                });
            });

            let cards = Self::deck_card_list(self.learn_trainer.config.deck);

            let unix_now = unix_seconds(now);
            let (mut due, mut new_cards, mut memorized) = (0usize, 0usize, 0usize);
            for &c in &cards {
                let srs = self.learn_trainer.card_state(&c.key());
                if srs.due == 0.0 {
                    new_cards += 1;
                } else if srs.is_due(unix_now) {
                    due += 1;
                }
                if srs.is_memorized() {
                    memorized += 1;
                }
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} cards", cards.len())).strong());
                ui.label(RichText::new("·").weak());
                ui.label(RichText::new(format!("{memorized} memorized")).color(GOOD));
                ui.label(RichText::new("·").weak());
                ui.label(RichText::new(format!("{due} due")).color(HEARD));
                ui.label(RichText::new("·").weak());
                ui.label(RichText::new(format!("{new_cards} new")).color(TRANS_BLUE));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Click a card to practice it now").size(12.0).weak());
                });
            });
            ui.separator();

            let active_key = self.learn_trainer.current_key();
            let query = self.browser_filter.trim().to_ascii_lowercase();
            let mut picked_card: Option<LearnCard> = None;
            let mut reset_key: Option<String> = None;

            // Only label the kind when the deck mixes scales and chords.
            let mixed = cards.iter().any(|c| matches!(c, LearnCard::Scale(_)))
                && cards.iter().any(|c| matches!(c, LearnCard::Chord(_)));
            egui::ScrollArea::vertical()
                .max_height(380.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let mut shown = 0;
                    for card in cards {
                        let title = card.title();
                        let detail = card.detail();
                        if !query.is_empty()
                            && !title.to_ascii_lowercase().contains(&query)
                            && !detail.to_ascii_lowercase().contains(&query)
                            && !card.kind_label().to_ascii_lowercase().contains(&query)
                        {
                            continue;
                        }
                        let key = card.key();
                        let is_active = active_key.as_deref() == Some(key.as_str());
                        let srs = self.learn_trainer.card_state(&key);
                        let (status, status_color) = if srs.due == 0.0 {
                            ("New".to_string(), TRANS_BLUE)
                        } else if srs.is_due(unix_now) {
                            (format!("Due · {}", format_interval(srs.interval)), HEARD)
                        } else if srs.is_memorized() {
                            (format!("Memorized · {}", format_interval(srs.interval)), GOOD)
                        } else {
                            (format!("Learning · {}", format_interval(srs.interval)), HEARD)
                        };
                        if browser_row(
                            ui,
                            shown,
                            is_active,
                            mixed.then(|| card.kind_label()),
                            &title,
                            &detail,
                            (&status, status_color),
                            srs.due > 0.0,
                            &mut reset_key,
                            &key,
                        ) {
                            picked_card = Some(card);
                        }
                        shown += 1;
                    }
                });

            if let Some(key) = reset_key {
                self.learn_trainer.reset_card(&key);
            }
            if let Some(card) = picked_card {
                self.set_tab(Tab::Learn);
                self.keep_going = true;
                self.learn_trainer.select_card(card, now);
                close_requested = true;
            }
        });
        if modal.should_close() || close_requested {
            self.browser_open = false;
        }
    }
}
/// One row of the deck browser, striped across the full width; clicking it
/// practises the card. Returns true when clicked.
#[allow(clippy::too_many_arguments)]
fn browser_row(
    ui: &mut egui::Ui,
    index: usize,
    active: bool,
    kind: Option<&str>,
    title: &str,
    detail: &str,
    (status, status_color): (&str, Color32),
    resettable: bool,
    reset_key: &mut Option<String>,
    key: &str,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    let visuals = ui.visuals().clone();
    let painter = ui.painter();
    let fill = if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else if index % 2 == 1 {
        visuals.faint_bg_color
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 6.0, fill);
    if active {
        painter.rect_stroke(rect.shrink(1.0), 6.0, egui::Stroke::new(1.5_f32, TARGET), egui::StrokeKind::Inside);
    }
    let y = rect.center().y;
    let mut x = rect.left() + 12.0;
    if let Some(kind) = kind {
        let color = if kind.eq_ignore_ascii_case("scale") { TRANS_PINK } else { TRANS_BLUE };
        painter.text(egui::pos2(x, y), egui::Align2::LEFT_CENTER, kind, egui::FontId::proportional(12.0), color);
        x += 56.0;
    }
    let title_color = if active { TARGET } else { visuals.strong_text_color() };
    painter.text(egui::pos2(x, y), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(15.0), title_color);
    painter.text(
        egui::pos2(x + 150.0, y),
        egui::Align2::LEFT_CENTER,
        detail,
        egui::FontId::monospace(13.0),
        visuals.weak_text_color(),
    );
    painter.text(
        egui::pos2(rect.right() - 48.0, y),
        egui::Align2::RIGHT_CENTER,
        status,
        egui::FontId::proportional(13.0),
        status_color,
    );
    if resettable {
        let reset_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - 22.0, y), egui::vec2(24.0, 24.0));
        let reset = ui
            .interact(reset_rect, response.id.with("reset"), egui::Sense::click())
            .on_hover_text("Forget this card's progress");
        let color = if reset.hovered() { visuals.strong_text_color() } else { visuals.weak_text_color() };
        ui.painter()
            .text(reset_rect.center(), egui::Align2::CENTER_CENTER, "↺", egui::FontId::proportional(16.0), color);
        if reset.clicked() {
            *reset_key = Some(key.to_string());
            return false;
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}
