//! Custom-painted widgets: fretboard, tuner gauge, level meter.

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};
use fretwise_core::{Position, Tuning};

pub struct Marker {
    pub pos: Position,
    pub color: Color32,
    pub label: Option<String>,
}

/// Horizontal fretboard, high E on top like tab. Frets are evenly spaced
/// (easier to read than real scale length at this size).
pub fn fretboard(ui: &mut egui::Ui, tuning: &Tuning, frets: u8, markers: &[Marker]) {
    let width = ui.available_width();
    let height = 230.0;
    let (response, painter) = ui.allocate_painter(vec2(width, height), Sense::hover());
    let rect = response.rect;
    let visuals = ui.visuals();
    let line = visuals.weak_text_color();
    let text = visuals.text_color();

    let label_w = 34.0;
    let open_w = 40.0;
    let top = rect.top() + 14.0;
    let bottom = rect.bottom() - 24.0;
    let nut_x = rect.left() + label_w + open_w;
    let fret_w = (rect.right() - 8.0 - nut_x) / frets as f32;
    let spacing = (bottom - top) / 5.0;

    let string_y = |s: usize| top + (5 - s) as f32 * spacing;
    let fret_center = |f: u8| {
        if f == 0 {
            nut_x - open_w / 2.0
        } else {
            nut_x + (f as f32 - 0.5) * fret_w
        }
    };

    // Board
    painter.rect_filled(
        Rect::from_min_max(pos2(nut_x, top - 8.0), pos2(nut_x + frets as f32 * fret_w, bottom + 8.0)),
        4.0,
        visuals.extreme_bg_color,
    );

    // Inlays and fret numbers
    for f in 1..=frets {
        let x = fret_center(f);
        let mid = (top + bottom) / 2.0;
        let inlay = visuals.faint_bg_color.gamma_multiply(3.0);
        match f % 12 {
            3 | 5 | 7 | 9 => {
                painter.circle_filled(pos2(x, mid), 6.0, inlay);
            }
            0 => {
                painter.circle_filled(pos2(x, mid - spacing), 6.0, inlay);
                painter.circle_filled(pos2(x, mid + spacing), 6.0, inlay);
            }
            _ => {}
        }
        painter.text(pos2(x, bottom + 18.0), Align2::CENTER_CENTER, f.to_string(), FontId::proportional(12.0), line);
    }

    // Frets and nut
    for f in 1..=frets {
        let x = nut_x + f as f32 * fret_w;
        painter.line_segment([pos2(x, top - 8.0), pos2(x, bottom + 8.0)], Stroke::new(2.0_f32, line));
    }
    painter.line_segment([pos2(nut_x, top - 8.0), pos2(nut_x, bottom + 8.0)], Stroke::new(6.0_f32, text));

    // Strings (thicker for lower strings) and their names
    for s in 0..6 {
        let y = string_y(s);
        let thickness = 1.0 + (5 - s) as f32 * 0.4;
        painter.line_segment([pos2(nut_x - open_w, y), pos2(rect.right() - 8.0, y)], Stroke::new(thickness, line));
        painter.text(
            pos2(rect.left() + label_w / 2.0, y),
            Align2::CENTER_CENTER,
            tuning.strings[s].to_string(),
            FontId::proportional(14.0),
            text,
        );
    }

    // Markers
    let radius = (spacing * 0.42).min(fret_w * 0.42);
    for m in markers.iter().filter(|m| m.pos.fret <= frets) {
        let c = pos2(fret_center(m.pos.fret), string_y(m.pos.string));
        painter.circle_filled(c, radius, m.color);
        if let Some(label) = &m.label {
            painter.text(c, Align2::CENTER_CENTER, label, FontId::proportional(radius * 0.9), Color32::BLACK);
        }
    }
}

/// Needle gauge from -50 to +50 cents.
pub fn tuner_gauge(ui: &mut egui::Ui, cents: Option<f32>) {
    let width = ui.available_width().min(520.0);
    let (response, painter) = ui.allocate_painter(vec2(width, 60.0), Sense::hover());
    let rect = response.rect;
    let visuals = ui.visuals();
    let mid_y = rect.center().y;
    let x_of = |c: f32| rect.left() + (c.clamp(-50.0, 50.0) + 50.0) / 100.0 * rect.width();

    painter.rect_filled(
        Rect::from_min_max(pos2(x_of(-5.0), rect.top()), pos2(x_of(5.0), rect.bottom())),
        3.0,
        Color32::from_rgb(80, 200, 120).gamma_multiply(0.25),
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
        let color = if c.abs() < 5.0 {
            Color32::from_rgb(80, 200, 120)
        } else {
            Color32::from_rgb(240, 190, 70)
        };
        let x = x_of(c);
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(4.0_f32, color));
    }
}

/// Small horizontal input meter, -80..0 dBFS.
pub fn level_meter(ui: &mut egui::Ui, level_db: f32) {
    let (response, painter) = ui.allocate_painter(vec2(120.0, 12.0), Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let t = ((level_db + 80.0) / 80.0).clamp(0.0, 1.0);
    let color = if level_db > -3.0 {
        Color32::from_rgb(230, 90, 80)
    } else {
        Color32::from_rgb(80, 200, 120)
    };
    painter.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width() * t, rect.height())), 2.0, color);
    response.on_hover_text(format!("{level_db:.0} dBFS"));
}
