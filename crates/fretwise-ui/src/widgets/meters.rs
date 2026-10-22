//! Read-outs: the tuner gauge, the input meter, the session bar and the
//! streak flame.

use egui::{pos2, vec2, Rect, Response, Sense, Stroke, Ui};

use crate::theme::{BAD, GOOD, HEARD};

/// Needle gauge from -50 to +50 cents.
pub fn tuner_gauge(ui: &mut Ui, cents: Option<f32>) {
    let width = ui.available_width().min(520.0);
    let (response, painter) = ui.allocate_painter(vec2(width, 60.0), Sense::hover());
    let rect = response.rect;
    let visuals = ui.visuals();
    let mid_y = rect.center().y;
    let x_of = |c: f32| rect.left() + (c.clamp(-50.0, 50.0) + 50.0) / 100.0 * rect.width();

    painter.rect_filled(
        Rect::from_min_max(pos2(x_of(-5.0), rect.top()), pos2(x_of(5.0), rect.bottom())),
        3.0,
        GOOD.gamma_multiply(0.25),
    );
    for c in (-50..=50).step_by(10) {
        let x = x_of(c as f32);
        let h = if c == 0 { 22.0 } else { 10.0 };
        painter.line_segment(
            [pos2(x, mid_y - h), pos2(x, mid_y + h)],
            Stroke::new(if c == 0 { 2.0_f32 } else { 1.0 }, visuals.weak_text_color()),
        );
    }
    if let Some(c) = cents {
        let color = if c.abs() < 5.0 { GOOD } else { HEARD };
        let x = x_of(c);
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(4.0_f32, color));
    }
}

/// LED-style input meter, -60..0 dBFS: green, then yellow above -12, red
/// above -3.
pub fn level_meter(ui: &mut Ui, level_db: f32) -> Response {
    let segments = 16;
    let (rect, response) = ui.allocate_exact_size(vec2(96.0, 12.0), Sense::hover());
    let painter = ui.painter();
    let gap = 1.5;
    let w = (rect.width() - gap * (segments - 1) as f32) / segments as f32;
    let lit = ((level_db + 60.0) / 60.0).clamp(0.0, 1.0) * segments as f32;
    for i in 0..segments {
        let db = -60.0 + 60.0 * (i as f32 + 1.0) / segments as f32;
        // Healthy signal runs through the flag colours; yellow and red still
        // warn near clipping.
        let color = if db > -3.0 {
            BAD
        } else if db > -12.0 {
            HEARD
        } else {
            crate::theme::pride_gradient((db + 60.0) / 48.0)
        };
        let on = (i as f32) < lit.round();
        let x = rect.left() + i as f32 * (w + gap);
        let seg = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
        painter.rect_filled(seg, 1.5, if on { color } else { ui.visuals().extreme_bg_color });
    }
    response.on_hover_text(format!("Input {level_db:.0} dBFS"))
}

/// The streak flame: Lucide's "flame" in pink, `h` points tall.
pub fn flame(painter: &egui::Painter, c: egui::Pos2, h: f32) {
    let rect = Rect::from_center_size(c, vec2(h, h));
    crate::icons::paint(painter, rect, &crate::icons::FLAME, crate::theme::TRANS_PINK);
}

/// Today's session as one bar: done over done + still to go, filled with
/// the flag gradient.
pub fn session_bar(ui: &mut Ui, done: usize, to_go: usize) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(220.0, 12.0), Sense::hover());
    let total = done + to_go;
    let t = if total == 0 { 1.0 } else { done as f32 / total as f32 };
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
    if t > 0.0 {
        // Clip a full-width gradient to the filled part, so the colours
        // stay put as the bar grows.
        let filled = Rect::from_min_size(rect.min, vec2(rect.width() * t, rect.height()));
        let clipped = painter.with_clip_rect(filled.intersect(painter.clip_rect()));
        let steps = 32;
        for i in 0..steps {
            let x0 = rect.left() + rect.width() * i as f32 / steps as f32;
            let x1 = rect.left() + rect.width() * (i + 1) as f32 / steps as f32;
            let seg = Rect::from_min_max(pos2(x0, rect.top()), pos2(x1 + 0.5, rect.bottom()));
            clipped.rect_filled(seg, 0.0, crate::theme::pride_gradient(i as f32 / steps as f32));
        }
    }
    response.on_hover_text(format!("{done} done this session, {to_go} still to review or new"))
}
