//! Learn, the home screen: spaced repetition over a deck, and the "all
//! caught up" card when nothing is waiting.

use egui::text::LayoutJob;
use egui::{Color32, RichText};
use fretwise_core::theory::string_number;
use fretwise_core::{
    fingering, format_interval, unix_seconds, ActiveCard, DeckKind, Grade, GuideMode, LearnCard, LearnConfig,
    LearnFeedback, Position,
};

use crate::parts::{
    caption_color, center_stage, chord_markers, heard_dot, pattern_dot, pitch_class_tokens, rack_frame, rack_group,
    target_marker, terminal_screen, token, NECK_FRETS, STAGE_GAP,
};
use crate::theme::{self, pattern_fills, rack_fill, BAD, GOOD, HEARD, TARGET};
use crate::widgets::{
    dropdown, fretboard, neck_point, plain_button, segmented, skip_button, sparkles, Marker, NeckControls,
};
use crate::{icons, FretwiseUi};

impl FretwiseUi {
    /// Where the current Learn card sits on the neck: the chord's fretted
    /// notes, or the scale's next note. Sparkles start there.
    pub(crate) fn learn_target_positions(&self) -> Vec<Position> {
        match self.learn_trainer.current() {
            Some(ActiveCard::Scale { sequence, step, .. }) => {
                sequence.get(*step).or(sequence.last()).copied().into_iter().collect()
            }
            Some(ActiveCard::Chord { .. }) => self
                .learn_trainer
                .current_chord()
                .map(|v| {
                    (0..6)
                        .filter_map(|string| v.frets[string].map(|fret| Position { string, fret }))
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Nothing due or new in the deck, and the done screen not dismissed.
    fn learn_done(&self, now: f64) -> bool {
        let (due, new_cards, _) = self.learn_trainer.deck_counts(now);
        due + new_cards == 0 && !self.keep_going && !self.learn_trainer.deck_cards().is_empty()
    }

    /// Every card in `deck`: its scales (chromatic has no shape to learn),
    /// then its chords.
    pub(crate) fn deck_card_list(deck: DeckKind) -> Vec<LearnCard> {
        let scales = deck.scales().iter().filter(|k| !k.is_chromatic()).map(|&k| LearnCard::Scale(k));
        let chords = deck.chords().iter().map(|&c| LearnCard::Chord(c));
        scales.chain(chords).collect()
    }

    /// Cards in `deck`, as Learn keys.
    fn deck_keys(deck: DeckKind) -> Vec<String> {
        Self::deck_card_list(deck).into_iter().map(LearnCard::key).collect()
    }

    /// A deck to try next: one with cards waiting (due or new) that aren't
    /// in the current deck, preferring a focused deck over a huge one.
    fn suggest_deck(&self, unix_now: f64) -> Option<(DeckKind, usize)> {
        let current = self.learn_trainer.config.deck;
        let here = Self::deck_keys(current);
        DeckKind::all()
            .filter(|&d| d != current)
            .filter_map(|d| {
                let keys = Self::deck_keys(d);
                let waiting = keys
                    .iter()
                    .filter(|k| !here.contains(k))
                    .filter(|k| {
                        let srs = self.learn_trainer.card_state(k);
                        srs.due == 0.0 || srs.is_due(unix_now)
                    })
                    .count();
                (waiting > 0).then_some((d, waiting, keys.len()))
            })
            // Enough to be worth it (up to 8), then the smallest deck.
            .max_by_key(|&(_, waiting, size)| (waiting.min(8), std::cmp::Reverse(size)))
            .map(|(d, waiting, _)| (d, waiting))
    }

    /// Shown instead of the neck when nothing in the deck is due or new.
    fn done_for_today(&mut self, ui: &mut egui::Ui, now: f64) {
        let unix_now = unix_seconds(now);
        let next_due = self
            .learn_trainer
            .deck_cards()
            .iter()
            .map(|c| self.learn_trainer.card_state(&c.key()).due - unix_now)
            .filter(|d| *d > 0.0)
            .fold(f64::INFINITY, f64::min);
        let suggestion = self.suggest_deck(unix_now);

        let syn = &theme::SYNTAX;
        let big = theme::pixel_font(ui, 28.0);
        let small = theme::pixel_font(ui, 16.0);
        let height_id = ui.id().with("done_height");
        let last_h = ui.data(|d| d.get_temp::<f32>(height_id)).unwrap_or(320.0);
        ui.add_space(((ui.available_height() - last_h) / 2.0).max(0.0));
        let frame = egui::Frame::new().fill(rack_fill(ui)).inner_margin(28).corner_radius(8);
        let shown = ui.vertical_centered(|ui| {
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width().min(520.0));
                ui.vertical_centered(|ui| {
                    let (pick, _) = ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
                    icons::paint(ui.painter(), pick, &icons::LOGO, Color32::WHITE);
                    ui.add_space(12.0);
                    let mut title = LayoutJob::default();
                    token(&mut title, "all caught up ", &big, syn.text);
                    token(&mut title, "<3", &big, syn.keyword);
                    ui.label(title);
                    ui.add_space(4.0);
                    let mut next = LayoutJob::default();
                    if next_due.is_finite() {
                        token(&mut next, "next card in ", &small, syn.comment);
                        token(&mut next, &format_interval(next_due), &small, syn.number);
                    } else {
                        token(&mut next, "every card here is done", &small, syn.comment);
                    }
                    ui.label(next);
                    ui.add_space(18.0);
                    if let Some((deck, waiting)) = suggestion {
                        let plural = if waiting == 1 { "card" } else { "cards" };
                        ui.label(
                            RichText::new(format!("{} has {waiting} {plural} for you", deck.name()))
                                .color(caption_color(ui)),
                        );
                        ui.add_space(6.0);
                        if plain_button(ui, &format!("Practise {}", deck.name()), 15.0).clicked() {
                            self.learn_trainer.config.deck = deck;
                        }
                        ui.add_space(10.0);
                    }
                    if plain_button(ui, "Keep going with this deck", 13.0).clicked() {
                        self.keep_going = true;
                    }
                });
            })
        });
        ui.data_mut(|d| d.insert_temp(height_id, shown.inner.response.rect.height()));
    }

    pub(crate) fn learn_tab(&mut self, ui: &mut egui::Ui, now: f64) {
        let before = self.learn_trainer.config;
        egui::TopBottomPanel::bottom("learn_rack")
            .frame(rack_frame(ui))
            .show_separator_line(false)
            .show_inside(ui, |ui| self.learn_rack(ui, now));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| self.learn_stage(ui, now));
        let display_only = LearnConfig { guide: before.guide, ..self.learn_trainer.config };
        if display_only != before {
            self.learn_trainer.restart(now);
        }
    }

    fn learn_stage(&mut self, ui: &mut egui::Ui, now: f64) {
        if self.learn_done(now) {
            self.done_for_today(ui, now);
            return;
        }
        let resolved = self.learn_trainer.is_resolved();
        let show_guide = self.learn_trainer.show_guide();
        let held = self.stabilizer.current();
        let visuals = ui.visuals().clone();

        let mut markers = Vec::new();
        let is_scale = matches!(self.learn_trainer.current(), Some(ActiveCard::Scale { .. }));

        match self.learn_trainer.current().cloned() {
            Some(ActiveCard::Scale {
                scale,
                box_positions,
                sequence,
                step,
                ..
            }) => {
                let interval = |pos: Position| (self.tuning.note_at(pos).pitch_class() + 12 - scale.root % 12) % 12;
                let (root_fill, other_fill) = pattern_fills(&visuals);
                for (idx, &pos) in sequence.iter().enumerate() {
                    if idx < step || resolved {
                        markers.push(Marker::dot(pos, GOOD, 0.48));
                    } else if show_guide {
                        markers.push(pattern_dot(pos, interval(pos), root_fill, other_fill));
                    }
                }
                if let Some(h) = held {
                    markers.extend(self.tuning.positions_of(h.note, NECK_FRETS).into_iter().map(heard_dot));
                }
                if show_guide {
                    if let Some(&target_pos) = sequence.get(step) {
                        let finger = self
                            .show_fingering
                            .then(|| fingering(&box_positions, self.learn_trainer.config.min_fret))
                            .flatten()
                            .and_then(|f| box_positions.iter().position(|&p| p == target_pos).map(|i| f[i]));
                        markers.push(target_marker(&markers, target_pos, finger, TARGET));
                    }
                }
            }
            Some(ActiveCard::Chord { heard_strings, .. }) => {
                if let Some(v) = self.learn_trainer.current_chord() {
                    markers = chord_markers(v, heard_strings, resolved, show_guide, self.show_fingering, &visuals);
                }
            }
            None => {}
        }

        let terminal_id = ui.id().with("learn_terminal_height");
        let height = center_stage(ui, terminal_id);
        if is_scale {
            let cfg = &mut self.learn_trainer.config;
            let controls = NeckControls { min_fret: &mut cfg.min_fret, max_fret: &mut cfg.max_fret, strings: None };
            fretboard(ui, NECK_FRETS, height, &markers, Some(controls));
        } else {
            // A chord shape is fixed: no fret range to drag, no strings to toggle.
            fretboard(ui, NECK_FRETS, height, &markers, None);
        }
        // A little burst of flag colours where the card was finished.
        if let Some((t0, positions)) = &self.sparkle {
            let age = (now - t0) as f32;
            let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("fretwise_sparkles"));
            let painter = ui.ctx().layer_painter(layer);
            for &pos in positions {
                if let Some(p) = neck_point(ui.ctx(), pos) {
                    sparkles(&painter, p, age);
                }
            }
            if age > 1.0 {
                self.sparkle = None;
            }
        }
        ui.add_space(STAGE_GAP);
        let h = self.learn_terminal(ui, now);
        ui.data_mut(|d| d.insert_temp(terminal_id, h));
    }

    fn learn_terminal(&self, ui: &mut egui::Ui, now: f64) -> f32 {
        let syn = &theme::SYNTAX;
        let big = theme::pixel_font(ui, 24.0);
        let small = theme::pixel_font(ui, 16.0);

        let memorized = self
            .learn_trainer
            .current_key()
            .is_some_and(|k| self.learn_trainer.card_state(&k).is_memorized());

        let mut cmd = LayoutJob::default();
        let c = &mut cmd;
        token(c, "> ", &big, syn.comment);
        match self.learn_trainer.current() {
            Some(ActiveCard::Scale {
                scale,
                box_positions,
                sequence,
                step,
                ..
            }) => {
                token(c, "walk ", &big, syn.keyword);
                token(c, &scale.name(), &big, syn.note);
                token(
                    c,
                    &format!(" ({}/{})", (*step).min(sequence.len()), sequence.len()),
                    &big,
                    syn.number,
                );
                if self.show_fingering {
                    if let (Some(&target_pos), Some(fingers)) =
                        (sequence.get(*step), fingering(box_positions, self.learn_trainer.config.min_fret))
                    {
                        if let Some(f) = box_positions.iter().position(|&p| p == target_pos).map(|i| fingers[i]) {
                            match f {
                                0 => token(c, " open", &big, TARGET),
                                f => {
                                    token(c, " finger ", &big, syn.text);
                                    token(c, &f.to_string(), &big, TARGET);
                                }
                            }
                        }
                    }
                }
                if memorized && !self.learn_trainer.show_guide() {
                    token(c, "  [from memory]", &big, syn.comment);
                }
            }
            Some(ActiveCard::Chord { .. }) => {
                if let Some(v) = self.learn_trainer.current_chord() {
                    token(c, "strum ", &big, syn.keyword);
                    token(c, &v.name, &big, syn.note);
                    if let Some(barre) = v.barre.filter(|_| self.show_fingering && self.learn_trainer.show_guide()) {
                        token(c, "  barre fret ", &big, syn.text);
                        token(c, &barre.to_string(), &big, TARGET);
                    }
                    if memorized && !self.learn_trainer.show_guide() {
                        token(c, "  [from memory]", &big, syn.comment);
                    }
                }
            }
            None => {
                token(c, "nothing in deck", &big, syn.text);
            }
        }

        let mut out = LayoutJob::default();
        let o = &mut out;
        match self.learn_trainer.feedback().filter(|(_, t)| now - t < 2.2) {
            Some((
                LearnFeedback::Completed {
                    title,
                    seconds,
                    grade,
                    interval,
                },
                _,
            )) => {
                let g_color = match grade {
                    Grade::Again => BAD,
                    Grade::Hard => HEARD,
                    Grade::Good | Grade::Easy => GOOD,
                };
                token(o, "ok ", &small, GOOD);
                token(o, title, &small, syn.note);
                token(o, &format!(" in {seconds:.1}s · "), &small, syn.text);
                token(o, grade.label(), &small, g_color);
                token(o, &format!(" → {} ", format_interval(*interval)), &small, syn.number);
                token(o, "[1-4 grade]", &small, syn.comment);
            }
            Some((LearnFeedback::WrongNote { played, expected }, _)) => {
                token(o, "err ", &small, BAD);
                token(o, &format!("that was {played}, need {expected}"), &small, syn.text);
            }
            Some((LearnFeedback::Skipped { title }, _)) => {
                token(o, "again ", &small, BAD);
                token(o, title, &small, syn.note);
                token(o, " → 30s", &small, syn.number);
            }
            None => match self.learn_trainer.current() {
                Some(ActiveCard::Chord { heard_classes, .. }) if *heard_classes != 0 => {
                    token(o, "hearing ", &small, syn.comment);
                    pitch_class_tokens(o, *heard_classes, &small, GOOD);
                }
                Some(ActiveCard::Scale { sequence, step, .. }) => {
                    if let Some(&pos) = sequence.get(*step) {
                        let target = self.tuning.note_at(pos);
                        token(o, "next ", &small, syn.comment);
                        token(o, &target.to_string(), &small, syn.note);
                        token(o, &format!(" on string {}", string_number(pos.string)), &small, syn.text);
                    }
                }
                _ => token(o, "anki spaced repetition · play the card...", &small, syn.comment),
            },
        }

        terminal_screen(ui, cmd, out, &big, now)
    }

    fn learn_rack(&mut self, ui: &mut egui::Ui, now: f64) {
        let done = self.learn_done(now);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            let count = self.learn_trainer.deck_cards().len();
            let cfg = &mut self.learn_trainer.config;
            let mut open_browser = false;
            rack_group(ui, "Deck", |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let groups: Vec<(&str, Vec<(DeckKind, &str)>)> = DeckKind::groups()
                        .map(|(group, kinds)| (group, kinds.map(|k| (k, k.name())).collect()))
                        .collect();
                    let shown = cfg.deck.name();
                    dropdown(ui, "learn_deck", &mut cfg.deck, shown, &groups, 210.0, 15.0);
                    let browse = plain_button(ui, &format!("Cards ({count})"), 14.0);
                    if browse.on_hover_text("Browse cards in this deck").clicked() {
                        open_browser = true;
                    }
                });
            });
            if open_browser {
                self.browser_open = true;
            }
            // Nothing to guide or skip on the done-for-today screen.
            if !done {
                rack_group(ui, "Guide", |ui| {
                    segmented(
                        ui,
                        &mut self.learn_trainer.config.guide,
                        &[
                            (GuideMode::Auto, "Auto"),
                            (GuideMode::Show, "Show"),
                            (GuideMode::Hide, "Hide"),
                        ],
                        15.0,
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let skip = skip_button(ui, 56.0);
                    if skip.on_hover_text("Grade Again (30s) and move on").clicked() {
                        self.learn_trainer.skip(now);
                    }
                });
            }
        });
    }
}
