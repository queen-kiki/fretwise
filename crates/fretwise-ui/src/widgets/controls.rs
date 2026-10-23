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

/// Header and menu icons, all Lucide.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Free practice: Lucide "eighth-note".
    Practice,
    /// Tuner: Lucide "gauge".
    Tuner,
    /// Learn, home: Lucide "graduation-cap".
    Learn,
    /// Jam: Lucide "guitar".
    Jam,
    /// Stats: Lucide "chart-no-axes-column".
    Stats,
    /// Back to Learn: Lucide "chevron-left".
    Back,
    /// Practice amp drawer: Lucide "speaker".
    Amp,
    /// Sandwich menu: Lucide "menu".
    Menu,
    /// Settings gear: Lucide "settings".
    Settings,
}

impl Icon {
    fn glyph(self) -> &'static crate::icons::Svg {
        match self {
            Icon::Practice => &crate::icons::EIGHTH_NOTE,
            Icon::Tuner => &crate::icons::GAUGE,
            Icon::Learn => &crate::icons::GRADUATION_CAP,
            Icon::Jam => &crate::icons::GUITAR,
            Icon::Stats => &crate::icons::CHART,
            Icon::Back => &crate::icons::CHEVRON_LEFT,
            Icon::Amp => &crate::icons::SPEAKER,
            Icon::Menu => &crate::icons::MENU,
            Icon::Settings => &crate::icons::SETTINGS,
        }
    }
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
    crate::icons::paint(ui.painter(), rect, icon.glyph(), color);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// The amp drawer toggle icon: turns muted grey when the amp Wippschalter is disabled.
pub fn amp_button(ui: &mut Ui, open: bool, enabled: bool) -> Response {
    let size = 24.0;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let visuals = ui.visuals();
    let color = if !enabled {
        visuals.weak_text_color().gamma_multiply(if response.hovered() { 0.65 } else { 0.40 })
    } else if open {
        accent(ui)
    } else if response.hovered() {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    crate::icons::paint(ui.painter(), rect, Icon::Amp.glyph(), color);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Horizontal stylized glowing "0 | I" Wippschalter (hardware rocker switch).
pub fn wippschalter(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(46.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool(response.id, *on);
    let visuals = ui.visuals();
    let painter = ui.painter();

    // Soft outer neon glow when switched to 'I'
    if t > 0.01 {
        let glow_col = crate::theme::TRANS_PINK.gamma_multiply(0.28 * t);
        painter.rect_filled(rect.expand(2.5), 6.5, glow_col);
    }

    // Outer recessed housing bezel
    let bezel_stroke = lerp_color(
        visuals.widgets.noninteractive.bg_stroke.color,
        crate::theme::TRANS_PINK,
        t * 0.75,
    );
    painter.rect_filled(rect, 4.5, visuals.extreme_bg_color);
    painter.rect_stroke(rect, 4.5, Stroke::new(1.0_f32, bezel_stroke), StrokeKind::Inside);

    let inner = rect.shrink(2.0);
    let mid_x = inner.center().x;
    let left_half = Rect::from_min_max(inner.min, pos2(mid_x, inner.bottom()));
    let right_half = Rect::from_min_max(pos2(mid_x, inner.top()), inner.max);

    // Left rocker face ('0'): sunken when off, raised when on.
    let left_fill = lerp_color(
        visuals.extreme_bg_color,
        visuals.widgets.inactive.bg_fill,
        t,
    );
    painter.rect_filled(left_half, 3.0, left_fill);

    // Right rocker face ('I'): raised matte when off, pressed & glowing when on.
    let lit_fill = accent(ui);
    let right_fill = lerp_color(visuals.widgets.inactive.bg_fill, lit_fill, t);
    painter.rect_filled(right_half, 3.0, right_fill);
    if t > 0.05 {
        let sheen = crate::theme::TRANS_PINK.gamma_multiply(0.35 * t);
        painter.rect_filled(right_half.shrink(1.5), 2.0, sheen);
    }

    // Center pivot ridge line
    painter.line_segment(
        [pos2(mid_x, inner.top() + 1.0), pos2(mid_x, inner.bottom() - 1.0)],
        Stroke::new(1.2_f32, Color32::from_black_alpha(110)),
    );

    // '0' and 'I' markings
    let zero_col = lerp_color(
        visuals.text_color().gamma_multiply(0.85),
        visuals.weak_text_color().gamma_multiply(0.55),
        t,
    );
    let one_col = lerp_color(
        visuals.weak_text_color().gamma_multiply(0.55),
        Color32::WHITE,
        t,
    );
    painter.text(
        left_half.center(),
        egui::Align2::CENTER_CENTER,
        "0",
        FontId::proportional(11.5),
        zero_col,
    );
    painter.text(
        right_half.center(),
        egui::Align2::CENTER_CENTER,
        "I",
        FontId::proportional(12.0),
        one_col,
    );

    response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(if *on { "Amp power: I (On)" } else { "Amp power: 0 (Off)" })
}

/// Width of an item in the header's sandwich menu.
pub const MENU_WIDTH: f32 = 190.0;

/// An item inside the header's sandwich menu, keeping its Lucide icon alongside the label,
/// and its keyboard shortcut (e.g. "F3") as a dark key cap on the right.
pub fn menu_item(ui: &mut Ui, icon: Icon, label: &str, shortcut: Option<&str>, active: bool) -> bool {
    let size = vec2(MENU_WIDTH, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals().clone();
    let accent = accent(ui);
    let hot = response.hovered();
    let fill = if active {
        accent.gamma_multiply(0.22)
    } else if hot {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);

    let icon_color = if active {
        accent
    } else if hot {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    let text_color = if active || hot {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };

    let icon_rect = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(18.0, 18.0));
    crate::icons::paint(ui.painter(), icon_rect, icon.glyph(), icon_color);
    ui.painter().text(
        pos2(icon_rect.right() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        text_color,
    );
    if let Some(key) = shortcut {
        let font = FontId::monospace(11.0);
        let galley = ui.painter().layout_no_wrap(key.to_string(), font, visuals.text_color().gamma_multiply(0.8));
        let cap = Rect::from_center_size(
            pos2(rect.right() - 8.0 - (galley.size().x + 12.0) / 2.0, rect.center().y),
            vec2(galley.size().x + 12.0, 20.0),
        );
        let shade = if visuals.dark_mode { 120 } else { 28 };
        ui.painter().rect_filled(cap, 5.0, Color32::from_black_alpha(shade));
        ui.painter().rect_stroke(cap, 5.0, Stroke::new(1.0_f32, Color32::from_black_alpha(shade / 2)), StrokeKind::Inside);
        ui.painter().galley(cap.center() - galley.size() / 2.0, galley, Color32::PLACEHOLDER);
    }
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}
