//! The stepped rotary knob.

use std::f32::consts::PI;
use std::hash::Hash;
use std::ops::RangeInclusive;

use egui::{pos2, vec2, Color32, CursorIcon, PopupCloseBehavior, Sense, Stroke, Ui};

use super::lerp_color;

/// A [`knob`] over whole numbers in `range`, for a setting kept as `f32`:
/// one step per number, each shown through `label`. A value off the grid
/// shows at the nearest step and stays until the knob is turned.
pub fn stepped_knob(
    ui: &mut Ui,
    id_salt: impl Hash,
    title: &str,
    value: &mut f32,
    range: RangeInclusive<i32>,
    label: impl Fn(i32) -> String,
) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let labels: Vec<String> = range.map(label).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut i = ((value.round() as i32).clamp(lo, hi) - lo) as usize;
    let changed = knob(ui, id_salt, title, &mut i, &labels);
    if changed {
        *value = (lo + i as i32) as f32;
    }
    changed
}

/// Stepped rotary knob, plug-in style. Drag up/down or scroll to turn it,
/// click for a list of the choices. Returns true when the choice changed.
pub fn knob(ui: &mut Ui, id_salt: impl Hash, title: &str, index: &mut usize, choices: &[&str]) -> bool {
    let n = choices.len().max(1);
    let before = *index;
    let size = vec2(92.0, 108.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let id = ui.id().with(id_salt);
    let popup_id = id.with("choices");

    // Drag: 16 px per step, either direction; up/right turns clockwise.
    if response.drag_started() {
        ui.data_mut(|d| d.insert_temp(id, *index as f32));
    }
    if response.dragged() {
        let delta = response.drag_delta();
        let acc = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(*index as f32) + (delta.x - delta.y) / 16.0;
        let acc = acc.clamp(0.0, (n - 1) as f32);
        ui.data_mut(|d| d.insert_temp(id, acc));
        *index = acc.round() as usize;
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll > 0.0 {
            *index = (*index + 1).min(n - 1);
        } else if scroll < 0.0 {
            *index = index.saturating_sub(1);
        }
    }
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let response = response.on_hover_cursor(CursorIcon::ResizeVertical);
    let side = if rect.bottom() + 180.0 > ui.ctx().screen_rect().bottom() {
        egui::AboveOrBelow::Above
    } else {
        egui::AboveOrBelow::Below
    };
    egui::popup_above_or_below_widget(ui, popup_id, &response, side, PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(140.0);
        for (i, c) in choices.iter().enumerate() {
            if ui.selectable_label(i == *index, *c).clicked() {
                *index = i;
            }
        }
    });

    let visuals = ui.visuals().clone();
    let hot = response.hovered() || response.dragged();
    let c = pos2(rect.center().x, rect.center().y + 5.5);
    super::paint_embossed_label(ui, pos2(c.x, rect.top() + 8.0), title, hot);
    let painter = ui.painter();
    let r = 21.0;
    let t = if n > 1 { *index as f32 / (n - 1) as f32 } else { 0.0 };
    // 0 sits at 7 o'clock, full at 5 o'clock, clockwise.
    let angle = |t: f32| 0.75 * PI + t * 1.5 * PI;
    let at = |a: f32, radius: f32| c + vec2(a.cos(), a.sin()) * radius;
    let arc = |from: f32, to: f32, radius: f32| -> Vec<egui::Pos2> {
        let steps = 40;
        (0..=steps).map(|i| at(angle(from + (to - from) * i as f32 / steps as f32), radius)).collect()
    };

    // Track and value arc, and a dot per step.
    painter.add(egui::Shape::line(arc(0.0, 1.0, r + 7.0), Stroke::new(3.0_f32, visuals.extreme_bg_color)));
    // The value arc runs blue → pink → white, the trans flag from edge to middle.
    // Nothing to draw at the first step; zero-length segments still show a blip.
    let steps = if t > 0.0 { 24 } else { 0 };
    for i in 0..steps {
        let (a, b) = (t * i as f32 / steps as f32, t * (i + 1) as f32 / steps as f32);
        let color = crate::theme::pride_gradient((a + b) / 2.0);
        painter.line_segment([at(angle(a), r + 7.0), at(angle(b), r + 7.0)], Stroke::new(3.0_f32, color));
    }
    let dot_step = if n > 25 {
        3
    } else if n > 16 {
        2
    } else {
        1
    };
    for i in (0..n).step_by(dot_step) {
        let ti = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
        let color = if i == *index {
            visuals.strong_text_color()
        } else {
            visuals.weak_text_color().gamma_multiply(0.6)
        };
        painter.circle_filled(at(angle(ti), r + 13.0), 1.5, color);
    }
    if !(*index).is_multiple_of(dot_step) {
        painter.circle_filled(at(angle(t), r + 13.0), 1.5, visuals.strong_text_color());
    }

    // Body: shadow and concentric bevelled discs centered at `c`.
    let (edge, face) = if visuals.dark_mode {
        (Color32::from_rgb(26, 18, 24), Color32::from_rgb(if hot { 98 } else { 84 }, 66, 82))
    } else {
        (Color32::from_rgb(214, 170, 188), Color32::from_rgb(255, if hot { 250 } else { 244 }, 248))
    };
    painter.circle_filled(c + vec2(0.0, 2.0), r + 1.0, Color32::from_black_alpha(70));
    let layers = 8;
    for i in 0..layers {
        let k = i as f32 / (layers - 1) as f32;
        let color = lerp_color(edge, face, k);
        painter.circle_filled(c, r * (1.0 - k * 0.22), color);
    }
    painter.circle_stroke(c, r, Stroke::new(1.0_f32, Color32::from_black_alpha(90)));

    // Pointer
    let a = angle(t);
    let pointer = if visuals.dark_mode { Color32::WHITE } else { Color32::from_gray(40) };
    painter.line_segment([at(a, r * 0.35), at(a, r * 0.85)], Stroke::new(3.0_f32, pointer));

    super::paint_segment_screen(
        ui,
        pos2(c.x, rect.bottom() - 8.0),
        choices.get(*index).copied().unwrap_or(""),
        hot,
    );
    *index != before
}
