//! Read-outs: the tuner gauge, the interactive input gate meter, the
//! interactive output level/gain meter, the session bar and the streak flame.

use std::ops::RangeInclusive;

use egui::{pos2, vec2, Color32, CursorIcon, Rect, Response, Sense, Stroke, StrokeKind, Ui};
use fretwise_core::{GATE_RANGE_DB, OUT_GAIN_RANGE_DB};

use crate::theme::{BAD, GOOD, HEARD, TRANS_BLUE, TRANS_PINK};
use crate::widgets::accent;

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

/// Interactive LED input meter (-80..0 dBFS) with a draggable **Gate Bracket** (`[`).
///
/// Dragging or scrolling adjusts `gate_db` within [`GATE_RANGE_DB`] (-80..-30 dBFS).
/// Signal below the gate bracket is shaded, ignored by pitch detection and muted on the monitored DI.
pub fn level_meter(ui: &mut Ui, level_db: f32, gate_db: &mut f32) -> Response {
    let min_db = *GATE_RANGE_DB.start();
    let span_db = -min_db;
    let (rect, mut response) = ui.allocate_exact_size(vec2(100.0, 14.0), Sense::click_and_drag());

    drag_db(ui, &mut response, rect, min_db..=0.0, gate_db, GATE_RANGE_DB);

    let painter = ui.painter();
    let bar_rect = Rect::from_min_max(pos2(rect.left(), rect.top() + 1.0), pos2(rect.right(), rect.bottom() - 1.0));
    let gated = level_db < *gate_db;
    let tint = gated.then(|| ui.visuals().weak_text_color().gamma_multiply(0.65));
    paint_leds(ui, bar_rect, level_db, min_db, tint, ui.visuals().extreme_bg_color);

    // Distinct Gate Bar Indicator: shaded blocked zone on the left + '[' threshold bracket
    // whose jaws open to the right (indicating only signal above this threshold passes).
    let gate_t = ((*gate_db - min_db) / span_db).clamp(0.0, 1.0);
    let gate_x = bar_rect.left() + gate_t * bar_rect.width();
    if gate_x > bar_rect.left() + 1.0 {
        let blocked = Rect::from_min_max(bar_rect.min, pos2(gate_x, bar_rect.bottom()));
        painter.rect_filled(blocked, 1.5, Color32::from_black_alpha(75));
    }

    let hot = response.hovered() || response.dragged();
    let bracket_col = if hot {
        TRANS_PINK
    } else {
        ui.visuals().strong_text_color()
    };
    let top_y = rect.top() - 1.5;
    let bot_y = rect.bottom() + 1.5;
    let jaw = 4.5;
    painter.line_segment([pos2(gate_x, top_y), pos2(gate_x, bot_y)], Stroke::new(2.0_f32, bracket_col));
    painter.line_segment([pos2(gate_x, top_y), pos2(gate_x + jaw, top_y)], Stroke::new(1.8_f32, bracket_col));
    painter.line_segment([pos2(gate_x, bot_y), pos2(gate_x + jaw, bot_y)], Stroke::new(1.8_f32, bracket_col));

    let hint = if gated {
        format!(
            "Input {level_db:.0} dBFS (below gate at {gate_db:.0} dB)\nDrag or scroll to adjust noise gate threshold."
        )
    } else {
        format!("Input {level_db:.0} dBFS · Gate {gate_db:.0} dB\nDrag or scroll to adjust noise gate threshold.")
    };
    response.on_hover_cursor(CursorIcon::ResizeHorizontal).on_hover_text(hint)
}

/// Interactive LED output meter (-60..0 dBFS) with a draggable **Master Gain Fader**
/// riding along a bottom level rail (`-24..+6 dB`, unrelated to individual amp plugins).
///
/// When `enabled` is false (the amp Wippschalter is at `0`), the meter, rail, and fader
/// cap all turn muted grey.
pub fn output_meter(ui: &mut Ui, level_db: f32, out_gain_db: &mut f32, enabled: bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(100.0, 14.0), Sense::click_and_drag());

    let (g_lo, g_hi) = (*OUT_GAIN_RANGE_DB.start(), *OUT_GAIN_RANGE_DB.end());
    let g_span = g_hi - g_lo;

    if enabled {
        if response.double_clicked() {
            *out_gain_db = 0.0;
            response.mark_changed();
        } else {
            drag_db(ui, &mut response, rect, OUT_GAIN_RANGE_DB, out_gain_db, OUT_GAIN_RANGE_DB);
        }
    }

    let painter = ui.painter();
    // Upper portion holds the 16 LED segments; bottom 3 px holds the continuous gain rail.
    let leds_rect = Rect::from_min_max(pos2(rect.left(), rect.top()), pos2(rect.right(), rect.bottom() - 4.0));
    let disabled_fill = ui.visuals().weak_text_color().gamma_multiply(0.28);
    if enabled {
        paint_leds(ui, leds_rect, level_db, -60.0, None, ui.visuals().extreme_bg_color);
    } else {
        paint_leds(ui, leds_rect, f32::NEG_INFINITY, -60.0, None, disabled_fill);
    }

    // Distinct Level/Gain Fader Indicator: bottom gain rail + rounded studio fader thumb cap.
    let gain_t = ((*out_gain_db - g_lo) / g_span).clamp(0.0, 1.0);
    let gain_x = rect.left() + gain_t * rect.width();
    let rail_rect = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.5), pos2(rect.right(), rect.bottom()));
    painter.rect_filled(rail_rect, 1.2, if enabled { ui.visuals().extreme_bg_color } else { disabled_fill });

    let fill_rail = Rect::from_min_max(rail_rect.min, pos2(gain_x, rail_rect.bottom()));
    let rail_col = if !enabled {
        ui.visuals().weak_text_color().gamma_multiply(0.42)
    } else {
        TRANS_BLUE
    };
    painter.rect_filled(fill_rail, 1.2, rail_col);

    // Rounded fader slider cap straddling `gain_x`
    let hot = enabled && (response.hovered() || response.dragged());
    let cap_rect = Rect::from_center_size(pos2(gain_x, rect.center().y), vec2(6.5, rect.height() + 3.0));
    let cap_fill = if !enabled {
        ui.visuals().widgets.inactive.bg_fill
    } else if hot {
        accent(ui)
    } else {
        ui.visuals().widgets.hovered.bg_fill
    };
    let cap_stroke = if !enabled {
        ui.visuals().weak_text_color().gamma_multiply(0.45)
    } else if hot {
        Color32::WHITE
    } else {
        TRANS_PINK
    };
    painter.rect_filled(cap_rect, 2.5, cap_fill);
    painter.rect_stroke(cap_rect, 2.5, Stroke::new(1.2_f32, cap_stroke), StrokeKind::Inside);
    painter.line_segment(
        [pos2(gain_x, cap_rect.top() + 2.5), pos2(gain_x, cap_rect.bottom() - 2.5)],
        Stroke::new(1.0_f32, if enabled { Color32::WHITE } else { disabled_fill }),
    );

    let sign = if *out_gain_db > 0.0 { "+" } else { "" };
    let hint = if enabled {
        format!(
            "Output {level_db:.0} dBFS · Master gain {sign}{out_gain_db:.0} dB\nDrag or scroll to adjust final gain (double-click for 0 dB)."
        )
    } else {
        format!("Output disabled (Master gain {sign}{out_gain_db:.0} dB)")
    };
    let response = if enabled {
        response.on_hover_cursor(CursorIcon::ResizeHorizontal)
    } else {
        response
    };
    response.on_hover_text(hint)
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

/// Drag or click along `rect` (spanning `scale`) or scroll over it to set
/// `db` in whole dB within `limits`.
fn drag_db(
    ui: &Ui,
    response: &mut Response,
    rect: Rect,
    scale: RangeInclusive<f32>,
    db: &mut f32,
    limits: RangeInclusive<f32>,
) {
    let (lo, hi) = (*limits.start(), *limits.end());
    let before = *db;
    if response.dragged() || response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            *db = egui::lerp(scale, t).round().clamp(lo, hi);
        }
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 {
            *db = (*db + scroll.signum()).round().clamp(lo, hi);
        }
    }
    if *db != before {
        response.mark_changed();
    }
}

/// Sixteen LED segments from `min_db` to 0 dBFS, lit up to `level_db`: the
/// flag colours, then yellow above -12 and red above -3, or all `tint`.
fn paint_leds(ui: &Ui, rect: Rect, level_db: f32, min_db: f32, tint: Option<Color32>, unlit: Color32) {
    let segments = 16;
    let gap = 1.5;
    let w = (rect.width() - gap * (segments - 1) as f32) / segments as f32;
    let lit = ((level_db - min_db) / -min_db).clamp(0.0, 1.0) * segments as f32;
    let painter = ui.painter();
    for i in 0..segments {
        let db = min_db - min_db * (i as f32 + 1.0) / segments as f32;
        let color = tint.unwrap_or(if db > -3.0 {
            BAD
        } else if db > -12.0 {
            HEARD
        } else {
            crate::theme::pride_gradient((db - min_db) / (-12.0 - min_db))
        });
        let on = (i as f32) < lit.round();
        let x = rect.left() + i as f32 * (w + gap);
        let seg = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
        painter.rect_filled(seg, 1.5, if on { color } else { unlit });
    }
}
