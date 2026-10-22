//! Buttons and pickers: segmented switches, the toggle, the dropdown, and
//! the painted and icon buttons.

use std::hash::Hash;

use egui::{
    pos2, vec2, Color32, CursorIcon, FontId, PopupCloseBehavior, Rect, Response, Sense, Stroke, StrokeKind, Ui,
};

use super::{accent, lerp_color};

/// A row of buttons that act as one switch, like a hardware mode selector.
/// Returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)], text_size: f32) -> bool {
    let before = *value;
    let font = FontId::proportional(text_size);
    let pad = vec2(text_size * 0.9, text_size * 0.45);
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label)| ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::PLACEHOLDER))
        .collect();
    let height = text_size + 2.0 * pad.y;
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 2.0 * pad.x).collect();
    let (rect, base) = ui.allocate_exact_size(vec2(widths.iter().sum::<f32>() + 4.0, height + 4.0), Sense::hover());

    let visuals = ui.visuals().clone();
    let accent = accent(ui);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 8.0, visuals.extreme_bg_color);
    let mut x = rect.left() + 2.0;
    for (i, ((opt, galley), w)) in options.iter().zip(galleys).zip(widths).enumerate() {
        let item = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(w, height));
        x += w;
        let r = ui.interact(item, base.id.with(i), Sense::click());
        if r.clicked() {
            *value = opt.0;
        }
        let selected = *value == opt.0;
        let fill = if selected {
            accent
        } else if r.hovered() {
            visuals.widgets.hovered.weak_bg_fill
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(item, 6.0, fill);
        let color = if selected {
            visuals.selection.stroke.color
        } else if r.hovered() {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        painter.galley(item.center() - galley.size() / 2.0, galley, color);
        r.on_hover_cursor(CursorIcon::PointingHand);
    }
    *value != before
}

/// On/off switch for settings.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(36.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool(response.id, *on);
    let visuals = ui.visuals();
    let track = lerp_color(visuals.widgets.inactive.bg_fill, accent(ui), t);
    ui.painter().rect_filled(rect, 10.0, track);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 7.5, visuals.strong_text_color());
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A drop-down picker in the segmented buttons' style: the current choice in
/// a sunken box, and a list with a caption over each group. `width` is fixed
/// so the panel doesn't shift as the choice changes.
pub fn dropdown<T: PartialEq + Copy>(
    ui: &mut Ui,
    id_salt: impl Hash,
    value: &mut T,
    shown: &str,
    groups: &[(&str, Vec<(T, &str)>)],
    width: f32,
    text_size: f32,
) -> bool {
    let before = *value;
    let height = text_size * 1.9 + 4.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let popup_id = ui.id().with(id_salt).with("list");
    let open = ui.memory(|m| m.is_popup_open(popup_id));
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let visuals = ui.visuals().clone();
    let painter = ui.painter();
    let hot = response.hovered() || open;
    painter.rect_filled(rect, 8.0, visuals.extreme_bg_color);
    if hot {
        painter.rect_stroke(rect.shrink(1.0), 7.0, Stroke::new(1.5_f32, accent(ui)), StrokeKind::Inside);
    }
    let pad = text_size * 0.9;
    let text_color = if hot { visuals.strong_text_color() } else { visuals.text_color() };
    let chevron = 4.5;
    let galley = painter.layout(
        shown.to_string(),
        FontId::proportional(text_size),
        text_color,
        width - 3.0 * pad - 2.0 * chevron,
    );
    // One line: names too long for the box are cut, with the full name on hover.
    let clipped = painter.with_clip_rect(rect.shrink2(vec2(pad, 0.0)).intersect(painter.clip_rect()));
    clipped.galley(pos2(rect.left() + pad, rect.center().y - text_size * 0.6), galley, text_color);
    let c = pos2(rect.right() - pad - chevron, rect.center().y);
    let tip = if open { -chevron * 0.6 } else { chevron * 0.6 };
    painter.add(egui::Shape::convex_polygon(
        vec![c + vec2(-chevron, -tip), c + vec2(chevron, -tip), c + vec2(0.0, tip)],
        if hot { accent(ui) } else { visuals.weak_text_color() },
        Stroke::NONE,
    ));
    let response = response.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(shown);

    // Opens upwards: the front panel sits at the bottom of the window.
    let room = (rect.top() - ui.ctx().screen_rect().top() - 40.0).max(160.0);
    let above = egui::AboveOrBelow::Above;
    egui::popup_above_or_below_widget(ui, popup_id, &response, above, PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(width - 8.0);
        // The popup's own area remembers its last size, so ask for the
        // height outright rather than taking what's available.
        egui::ScrollArea::vertical().max_height(room).min_scrolled_height(room).show(ui, |ui| {
            for (i, (group, items)) in groups.iter().enumerate() {
                if i > 0 {
                    ui.add_space(6.0);
                }
                ui.label(egui::RichText::new(group.to_uppercase()).size(11.0).weak());
                for (item, label) in items {
                    let selected = *item == *value;
                    let r = ui.selectable_label(selected, egui::RichText::new(*label).size(text_size - 1.0));
                    // Show the current choice when the list opens.
                    if selected && !open {
                        r.scroll_to_me(None);
                    }
                    if r.clicked() {
                        *value = *item;
                    }
                }
            }
        });
    });
    *value != before
}

/// The trainer's main action: a square accent button with Lucide's
/// skip-forward icon.
pub fn skip_button(ui: &mut Ui, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let hot = response.hovered();
    let fill = if response.is_pointer_button_down_on() {
        accent(ui).gamma_multiply(0.8)
    } else if hot {
        crate::theme::lerp(accent(ui), Color32::WHITE, 0.15)
    } else {
        accent(ui)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, fill);
    let ink = ui.visuals().selection.stroke.color;
    crate::icons::paint(painter, rect.shrink(size * 0.27), &crate::icons::SKIP_FORWARD, ink);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// The settings gear (Lucide), brighter when hovered, accent when open.
pub fn gear_button(ui: &mut Ui, on: bool) -> Response {
    let size = 26.0;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let visuals = ui.visuals();
    let color = if on {
        accent(ui)
    } else if response.hovered() {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    crate::icons::paint(ui.painter(), rect, &crate::icons::SETTINGS, color);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A quiet painted button in the dropdown's sunken style.
pub fn plain_button(ui: &mut Ui, text: &str, text_size: f32) -> Response {
    let font = FontId::proportional(text_size);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, Color32::PLACEHOLDER);
    let size = galley.size() + vec2(text_size * 1.8, text_size * 0.9);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let hot = response.hovered();
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, visuals.extreme_bg_color);
    if hot {
        painter.rect_stroke(rect.shrink(1.0), 7.0, Stroke::new(1.5_f32, accent(ui)), StrokeKind::Inside);
    }
    let color = if hot { visuals.strong_text_color() } else { visuals.text_color() };
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Header tools, all Lucide icons like the gear.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Free practice: Lucide "eighth-note".
    Practice,
    /// Tuner: Lucide "gauge".
    Tuner,
    /// Stats: Lucide "chart-no-axes-column".
    Stats,
    /// Back to Learn: Lucide "chevron-left".
    Back,
}

pub fn icon_button(ui: &mut Ui, icon: Icon, on: bool) -> Response {
    let size = 26.0;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let visuals = ui.visuals();
    let color = if on {
        accent(ui)
    } else if response.hovered() {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    let glyph = match icon {
        Icon::Practice => &crate::icons::EIGHTH_NOTE,
        Icon::Tuner => &crate::icons::GAUGE,
        Icon::Stats => &crate::icons::CHART,
        Icon::Back => &crate::icons::CHEVRON_LEFT,
    };
    crate::icons::paint(ui.painter(), rect, glyph, color);
    response.on_hover_cursor(CursorIcon::PointingHand)
}
