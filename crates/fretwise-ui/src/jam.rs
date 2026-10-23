//! Jam: play through the amp and see what you're playing. The neck sits dark
//! and idle; a screen over it names the note, interval or chord.

use egui::text::LayoutJob;
use egui::{pos2, vec2, Color32, Rect, Stroke, StrokeKind};
use fretwise_core::theory::pitch_class_name;
use fretwise_core::{JamHeard, JamName};

use crate::parts::{token, NECK_FRETS};
use crate::theme::{self, GOOD, HEARD};
use crate::widgets::fretboard;
use crate::FretwiseUi;

/// A new name must hold this long (seconds) before it replaces the shown one,
/// so a chord change doesn't flash the notes in between.
const DEBOUNCE: f64 = 0.12;
/// After the last sound, the name stays up, fading, for this long.
const LINGER: f64 = 6.0;
/// Chords listed under "before".
const HISTORY: usize = 6;

/// What the jam screen shows, steadied over frames.
#[derive(Default)]
pub(crate) struct JamView {
    shown: Option<JamHeard>,
    heard_at: f64,
    pending: Option<(JamName, f64)>,
    /// Chords played before the one shown, oldest first.
    history: Vec<String>,
}

impl JamView {
    pub(crate) fn update(&mut self, now: f64, heard: Option<JamHeard>) {
        let Some(heard) = heard else {
            self.pending = None;
            return;
        };
        self.heard_at = now;
        if self.shown.as_ref().is_some_and(|s| s.name == heard.name) {
            self.shown = Some(heard);
            self.pending = None;
            return;
        }
        let since = match &self.pending {
            Some((name, t)) if *name == heard.name => *t,
            _ => {
                self.pending = Some((heard.name.clone(), now));
                now
            }
        };
        if self.shown.is_some() && now - since < DEBOUNCE {
            return;
        }
        if let Some(JamName::Chord(old)) = self.shown.take().map(|s| s.name) {
            if self.history.last() != Some(&old) {
                self.history.push(old);
                if self.history.len() > HISTORY {
                    self.history.remove(0);
                }
            }
        }
        self.shown = Some(heard);
        self.pending = None;
    }
}

impl FretwiseUi {
    pub(crate) fn jam_tab(&mut self, ui: &mut egui::Ui, now: f64) {
        let height = (ui.available_height() - 24.0).clamp(220.0, 360.0);
        ui.add_space(((ui.available_height() - height) / 2.0).max(0.0));
        let top = ui.cursor().top();
        fretboard(ui, NECK_FRETS, height, &[], None);
        let full = ui.max_rect();
        let board = Rect::from_min_max(pos2(full.left(), top), pos2(full.right(), top + height));
        // Dark and idle: nothing to aim at here, just play.
        let screen = theme::SYNTAX.screen;
        let shade = if ui.visuals().dark_mode { 170 } else { 150 };
        let shade = Color32::from_rgba_unmultiplied(screen.r(), screen.g(), screen.b(), shade);
        ui.painter().rect_filled(board.expand(4.0), 8.0, shade);
        self.jam_screen(ui, board, now);
    }

    /// The screen floating over the neck.
    fn jam_screen(&self, ui: &egui::Ui, board: Rect, now: f64) {
        let syn = &theme::SYNTAX;
        let small = theme::pixel_font(ui, 16.0);
        let big = theme::pixel_font(ui, 64.0);
        let mid = theme::pixel_font(ui, 44.0);

        let age = now - self.jam.heard_at;
        let shown = self.jam.shown.as_ref().filter(|_| age < LINGER);
        // Full colour while sounding, then fading out.
        let fade = if age < 0.3 { 1.0 } else { (0.7 * (1.0 - (age - 0.3) / LINGER)).max(0.25) as f32 };
        let dim = |c: Color32| c.gamma_multiply(fade);

        let mut prompt = LayoutJob::default();
        token(&mut prompt, "> ", &small, syn.comment);
        token(&mut prompt, "listen", &small, syn.keyword);
        if !self.amp.enabled {
            token(&mut prompt, "  amp is off", &small, syn.comment);
        }

        let mut name = LayoutJob::default();
        let mut detail = LayoutJob::default();
        match shown {
            None => {
                token(&mut name, "-", &big, syn.comment);
                token(&mut detail, "play a note or strum a chord...", &small, syn.comment);
            }
            Some(heard) => {
                match &heard.name {
                    JamName::Chord(chord) => token(&mut name, chord.as_str(), &big, dim(syn.note)),
                    JamName::Note(note) => token(&mut name, &note.to_string(), &big, dim(syn.number)),
                    &JamName::Interval(low, high, _) => {
                        token(&mut name, pitch_class_name(low), &mid, dim(syn.note));
                        token(&mut name, " + ", &mid, dim(syn.comment));
                        token(&mut name, pitch_class_name(high), &mid, dim(syn.note));
                    }
                    JamName::Unknown => token(&mut name, "?", &big, dim(syn.comment)),
                }
                match (&heard.name, heard.cents) {
                    (JamName::Note(_), Some(cents)) => {
                        let color = if cents.abs() < 5.0 { GOOD } else { HEARD };
                        token(&mut detail, &format!("{cents:+.0}"), &small, dim(color));
                        token(&mut detail, " cents", &small, dim(syn.comment));
                    }
                    (JamName::Note(_), None) => token(&mut detail, " ", &small, syn.comment),
                    (JamName::Interval(_, _, interval), _) => token(&mut detail, interval, &small, dim(syn.text)),
                    _ => {
                        token(&mut detail, "notes ", &small, syn.comment);
                        // From the bass up, so the voicing reads as played.
                        let start = heard.bass.map_or(0, |b| b.pitch_class());
                        for pc in (0..12u8).map(|i| (start + i) % 12).filter(|pc| heard.classes & (1 << pc) != 0) {
                            token(&mut detail, pitch_class_name(pc), &small, dim(syn.text));
                            token(&mut detail, " ", &small, syn.text);
                        }
                    }
                }
            }
        }

        let mut history = LayoutJob::default();
        if !self.jam.history.is_empty() {
            token(&mut history, "before  ", &small, syn.comment);
            for (i, chord) in self.jam.history.iter().enumerate() {
                if i > 0 {
                    token(&mut history, " · ", &small, syn.comment);
                }
                token(&mut history, chord, &small, syn.text.gamma_multiply(0.75));
            }
        }

        let layout = |job: LayoutJob| ui.fonts(|f| f.layout_job(job));
        let lines = [layout(prompt), layout(name), layout(detail), layout(history)];
        let gaps = [10.0, 6.0, 12.0];
        let pad = vec2(28.0, 20.0);
        let content_w = lines.iter().map(|g| g.size().x).fold(0.0, f32::max);
        let width = (content_w + 2.0 * pad.x).clamp(380.0, (board.width() - 40.0).max(380.0));
        let has_history = !self.jam.history.is_empty();
        let shown_lines = if has_history { 4 } else { 3 };
        let height = lines[..shown_lines].iter().map(|g| g.size().y).sum::<f32>()
            + gaps[..shown_lines - 1].iter().sum::<f32>()
            + 2.0 * pad.y;
        let card = Rect::from_center_size(board.center(), vec2(width, height));

        let painter = ui.painter();
        painter.rect_filled(card.translate(vec2(0.0, 8.0)).expand(4.0), 14.0, Color32::from_black_alpha(90));
        painter.rect_filled(card, 10.0, syn.screen);
        painter.rect_stroke(card, 10.0, Stroke::new(1.0_f32, syn.border), StrokeKind::Inside);

        let mut y = card.top() + pad.y;
        for (i, galley) in lines.into_iter().take(shown_lines).enumerate() {
            let size = galley.size();
            // The prompt sits at the left like a terminal; the rest is centred.
            let x = if i == 0 { card.left() + pad.x } else { card.center().x - size.x / 2.0 };
            if i == 0 && (now * 1.6).fract() < 0.55 {
                let cursor = Rect::from_min_size(pos2(x + size.x + 4.0, y + 2.0), vec2(small.size * 0.55, size.y - 4.0));
                painter.rect_filled(cursor, 0.0, syn.keyword);
            }
            painter.galley(pos2(x, y), galley, Color32::PLACEHOLDER);
            y += size.y + gaps.get(i).copied().unwrap_or(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fretwise_core::Note;

    fn chord(name: &str) -> Option<JamHeard> {
        Some(JamHeard { classes: 0, bass: None, name: JamName::Chord(name.into()), cents: None })
    }

    #[test]
    fn steadies_changes_and_keeps_history() {
        let mut v = JamView::default();
        v.update(0.0, chord("Am"));
        assert_eq!(v.shown.as_ref().unwrap().name, JamName::Chord("Am".into()));
        // A flicker to something else doesn't take over...
        v.update(0.05, chord("Am7"));
        v.update(0.06, chord("Am"));
        assert_eq!(v.shown.as_ref().unwrap().name, JamName::Chord("Am".into()));
        // ...a change that holds does, and Am goes into the history.
        v.update(0.1, chord("C"));
        v.update(0.3, chord("C"));
        assert_eq!(v.shown.as_ref().unwrap().name, JamName::Chord("C".into()));
        assert_eq!(v.history, ["Am"]);
        let note = JamHeard { classes: 1, bass: Some(Note(57)), name: JamName::Note(Note(57)), cents: None };
        v.update(0.4, Some(note.clone()));
        v.update(0.6, Some(note));
        assert_eq!(v.history, ["Am", "C"]);
    }
}
