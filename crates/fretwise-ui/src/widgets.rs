//! Custom-painted widgets: fretboard with a fret-range strip, stepped knob,
//! segmented buttons, toggle switch, tuner gauge, level meter.

use std::f32::consts::PI;
use std::hash::Hash;

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Id, PopupCloseBehavior, Rect, Response, Sense, Stroke,
    StrokeKind, Ui,
};
use fretwise_core::theory::string_number;
use fretwise_core::Position;

pub struct Marker {
    pub pos: Position,
    pub color: Color32,
    /// Radius relative to a full-size dot.
    pub size: f32,
    /// Draw an outline around the spot instead of a filled dot.
    pub ring: bool,
    pub label: Option<String>,
    pub text: Color32,
}

/// The colour the controls light up in: the theme's selection colour, so
/// knobs, buttons and the fret range match egui's own widgets.
pub fn accent(ui: &Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

/// What the trainer lets you pick straight on the neck. The progress view
/// passes `None` for a plain board.
pub struct NeckControls<'a> {
    pub min_fret: &'a mut u8,
    pub max_fret: &'a mut u8,
    /// Strings to quiz, lowest first; toggled by clicking their names.
    /// `None` leaves the names as plain labels.
    pub strings: Option<&'a mut [bool; 6]>,
}

/// Horizontal fretboard, high E on top like tab. Frets are evenly spaced
/// (easier to read than real scale length at this size). With `controls`,
/// the fret numbers become a draggable range and the string numbers toggle
/// strings; returns true if either changed.
pub fn fretboard(
    ui: &mut Ui,
    frets: u8,
    height: f32,
    markers: &[Marker],
    controls: Option<NeckControls>,
) -> bool {
    let width = ui.available_width();
    let (response, painter) = ui.allocate_painter(vec2(width, height), Sense::hover());
    let id = response.id;
    let rect = response.rect;
    let visuals = ui.visuals().clone();
    let line = visuals.weak_text_color();
    let text = visuals.text_color();
    let accent = accent(ui);

    // Dots stay a readable size however big the window gets, and leave room
    // for the string names, the open strings and the fret strip on top.
    let max_radius = 18.0;
    let pad = max_radius + 4.0;
    let strip_h = 24.0;
    let label_w = 40.0;
    let open_w = 2.0 * max_radius + 8.0;
    let top = rect.top() + strip_h + pad;
    let bottom = rect.bottom() - pad;
    let nut_x = rect.left() + label_w + open_w;
    let fret_w = (rect.right() - nut_x) / frets as f32;
    let spacing = (bottom - top) / 5.0;
    let board_end = nut_x + frets as f32 * fret_w;
    let radius = (spacing * 0.42).min(fret_w * 0.42).min(max_radius);
    let geometry = NeckGeometry { nut_x, open_w, fret_w, top, spacing };
    ui.ctx().data_mut(|d| d.insert_temp(Id::new("fretwise_neck_geometry"), geometry));

    let string_y = |s: usize| top + (5 - s) as f32 * spacing;
    let fret_center = |f: u8| {
        if f == 0 {
            nut_x - open_w / 2.0
        } else {
            nut_x + (f as f32 - 0.5) * fret_w
        }
    };
    // Horizontal extent of the space a fret's notes sit in.
    let cell = |f: u8| {
        if f == 0 {
            (nut_x - open_w, nut_x)
        } else {
            (nut_x + (f - 1) as f32 * fret_w, nut_x + f as f32 * fret_w)
        }
    };
    let fret_at = |x: f32| {
        if x < nut_x {
            0
        } else {
            (((x - nut_x) / fret_w).floor() as i32 + 1).clamp(1, frets as i32) as u8
        }
    };

    // Board
    painter.rect_filled(
        Rect::from_min_max(pos2(nut_x, top - 8.0), pos2(board_end, bottom + 8.0)),
        4.0,
        visuals.extreme_bg_color,
    );

    // Inlays
    for f in 1..=frets {
        let x = fret_center(f);
        let mid = (top + bottom) / 2.0;
        // Same colour as the nut, so the inlays read as part of the neck.
        // Thin rings, so they read as markings on the wood, not as notes.
        let inlay = Stroke::new(1.5_f32, text);
        match f % 12 {
            3 | 5 | 7 | 9 => {
                painter.circle_stroke(pos2(x, mid), 5.0, inlay);
            }
            0 => {
                painter.circle_stroke(pos2(x, mid - spacing), 5.0, inlay);
                painter.circle_stroke(pos2(x, mid + spacing), 5.0, inlay);
            }
            _ => {}
        }
    }

    // Frets and nut
    for f in 1..=frets {
        let x = nut_x + f as f32 * fret_w;
        painter.line_segment([pos2(x, top - 8.0), pos2(x, bottom + 8.0)], Stroke::new(2.0_f32, line));
    }
    painter.line_segment([pos2(nut_x, top - 8.0), pos2(nut_x, bottom + 8.0)], Stroke::new(6.0_f32, text));

    let mut changed = false;
    let (range, strings) = match controls {
        Some(c) => (Some((c.min_fret, c.max_fret)), c.strings),
        None => (None, None),
    };

    // Strings (thicker for lower strings) and their names, which toggle them.
    let mut strings = strings;
    for s in 0..6 {
        let y = string_y(s);
        let on = strings.as_ref().is_none_or(|st| st[s]);
        let thickness = 1.0 + (5 - s) as f32 * 0.4;
        let color = if on { line } else { line.gamma_multiply(0.3) };
        painter.line_segment([pos2(nut_x - open_w, y), pos2(rect.right(), y)], Stroke::new(thickness, color));

        let name_rect = Rect::from_center_size(pos2(rect.left() + label_w / 2.0, y), vec2(label_w - 4.0, 24.0));
        let mut name_color = text;
        if let Some(st) = strings.as_deref_mut() {
            let r = ui
                .interact(name_rect, id.with(("string", s)), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(if st[s] { "Click to leave this string out" } else { "Click to practise this string" });
            if r.clicked() {
                st[s] = !st[s];
                changed = true;
            }
            let fill = match (st[s], r.hovered()) {
                (true, _) => accent.gamma_multiply(if r.hovered() { 0.8 } else { 0.5 }),
                (false, true) => visuals.widgets.hovered.weak_bg_fill,
                (false, false) => Color32::TRANSPARENT,
            };
            painter.rect_filled(name_rect, 12.0, fill);
            if !st[s] {
                painter.rect_stroke(name_rect, 12.0, Stroke::new(1.0_f32, line), StrokeKind::Inside);
                name_color = line;
            }
        }
        // Strings go by number, 1 = high E, as in the prompt.
        let name = string_number(s).to_string();
        painter.text(name_rect.center(), Align2::CENTER_CENTER, name, FontId::proportional(14.0), name_color);
    }

    // Fret range: shade the frets outside it, then the strip of fret numbers above.
    let strip = Rect::from_min_max(pos2(nut_x - open_w, rect.top()), pos2(board_end, rect.top() + strip_h));
    match range {
        Some((min, max)) => {
            let shade = visuals.panel_fill.gamma_multiply(0.65);
            if *min > 0 {
                let r = Rect::from_min_max(pos2(nut_x - open_w, top - 10.0), pos2(cell(*min).0, bottom + 10.0));
                painter.rect_filled(r, 0.0, shade);
            }
            if *max < frets {
                let r = Rect::from_min_max(pos2(cell(*max).1, top - 10.0), pos2(board_end + 1.0, bottom + 10.0));
                painter.rect_filled(r, 0.0, shade);
            }
            changed |= fret_range_strip(ui, &painter, id.with("range"), strip, frets, min, max, &cell, &fret_at);
        }
        None => {
            for f in 1..=frets {
                painter.text(
                    pos2(fret_center(f), strip.center().y),
                    Align2::CENTER_CENTER,
                    f.to_string(),
                    FontId::proportional(12.0),
                    line,
                );
            }
        }
    }

    // Markers
    for m in markers.iter().filter(|m| m.pos.fret <= frets) {
        let c = pos2(fret_center(m.pos.fret), string_y(m.pos.string));
        if m.ring {
            painter.circle_stroke(c, radius * m.size, Stroke::new(3.0_f32, m.color));
        } else {
            painter.circle_filled(c, radius * m.size, m.color);
        }
        if let Some(label) = &m.label {
            // Finger numbers and other short labels get bigger digits.
            let scale = if label.chars().count() <= 2 { 1.1 } else { 0.8 };
            painter.text(c, Align2::CENTER_CENTER, label, FontId::proportional(radius * m.size * scale), m.text);
        }
    }
    changed
}

/// Which part of the fret range a drag holds.
#[derive(Clone, Copy)]
enum RangeGrip {
    Low,
    High,
    /// The whole range, grabbed this many frets above its low end.
    Move(u8),
}

/// Fret numbers under the neck with the practice range lit up. Drag an end
/// to resize it, drag the middle to slide it along the neck, or click a
/// fret outside it to stretch it there.
#[allow(clippy::too_many_arguments)]
fn fret_range_strip(
    ui: &mut Ui,
    painter: &egui::Painter,
    id: Id,
    strip: Rect,
    frets: u8,
    min: &mut u8,
    max: &mut u8,
    cell: &dyn Fn(u8) -> (f32, f32),
    fret_at: &dyn Fn(f32) -> u8,
) -> bool {
    let (lo, hi) = (*min, *max);
    let response = ui.interact(strip, id, Sense::click_and_drag());
    let grip_at = |x: f32| {
        let (left, right) = (cell(*min).0, cell(*max).1);
        if (x - left).abs() < 10.0 || x < left {
            RangeGrip::Low
        } else if (x - right).abs() < 10.0 || x > right {
            RangeGrip::High
        } else {
            RangeGrip::Move(fret_at(x) - *min)
        }
    };

    if let Some(p) = response.hover_pos() {
        let (left, right) = (cell(*min).0, cell(*max).1);
        let cursor = if response.dragged() {
            CursorIcon::Grabbing
        } else if (p.x - left).abs() < 10.0 || (p.x - right).abs() < 10.0 {
            CursorIcon::ResizeHorizontal
        } else if p.x > left && p.x < right {
            CursorIcon::Grab
        } else {
            CursorIcon::PointingHand
        };
        ui.ctx().set_cursor_icon(cursor);
    }
    if response.drag_started() {
        if let Some(p) = response.interact_pointer_pos() {
            ui.data_mut(|d| d.insert_temp(id, grip_at(p.x)));
        }
    }
    if response.dragged() {
        let grip = ui.data(|d| d.get_temp::<RangeGrip>(id));
        if let (Some(grip), Some(p)) = (grip, response.interact_pointer_pos()) {
            let f = fret_at(p.x);
            match grip {
                RangeGrip::Low => *min = f.min(*max),
                RangeGrip::High => *max = f.max(*min),
                RangeGrip::Move(offset) => {
                    let span = *max - *min;
                    *min = f.saturating_sub(offset).min(frets - span);
                    *max = *min + span;
                }
            }
        }
    } else if response.clicked() {
        if let Some(p) = response.interact_pointer_pos() {
            let f = fret_at(p.x);
            if f < *min {
                *min = f;
            } else if f > *max {
                *max = f;
            }
        }
    }
    let response = response.on_hover_text("Drag the ends or the middle to choose which frets to practise");

    let visuals = ui.visuals();
    let accent = accent(ui);
    painter.rect_filled(strip, 6.0, visuals.extreme_bg_color);
    let band = Rect::from_min_max(pos2(cell(*min).0, strip.top()), pos2(cell(*max).1, strip.bottom()));
    let hot = response.hovered() || response.dragged();
    painter.rect_filled(band, 6.0, accent.gamma_multiply(if hot { 0.75 } else { 0.55 }));
    for x in [band.left() + 4.0, band.right() - 4.0] {
        painter.line_segment(
            [pos2(x, band.top() + 5.0), pos2(x, band.bottom() - 5.0)],
            Stroke::new(2.0_f32, visuals.strong_text_color()),
        );
    }
    for f in 0..=frets {
        let (l, r) = cell(f);
        let inside = (*min..=*max).contains(&f);
        let color = if inside { visuals.strong_text_color() } else { visuals.weak_text_color() };
        painter.text(
            pos2((l + r) / 2.0, strip.center().y),
            Align2::CENTER_CENTER,
            f.to_string(),
            FontId::proportional(if inside { 14.0 } else { 12.0 }),
            color,
        );
    }
    (*min, *max) != (lo, hi)
}

/// Stepped rotary knob, plug-in style. Drag up/down or scroll to turn it,
/// click for a list of the choices. Returns true when the choice changed.
pub fn knob(ui: &mut Ui, id_salt: impl Hash, title: &str, index: &mut usize, choices: &[&str]) -> bool {
    let n = choices.len().max(1);
    let before = *index;
    let size = vec2(92.0, 104.0);
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
    egui::popup_below_widget(ui, popup_id, &response, PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(140.0);
        for (i, c) in choices.iter().enumerate() {
            if ui.selectable_label(i == *index, *c).clicked() {
                *index = i;
            }
        }
    });

    let visuals = ui.visuals();
    let painter = ui.painter();
    let hot = response.hovered() || response.dragged();
    painter.text(
        pos2(rect.center().x, rect.top() + 6.0),
        Align2::CENTER_CENTER,
        title.to_uppercase(),
        FontId::proportional(11.0),
        visuals.weak_text_color(),
    );

    let c = pos2(rect.center().x, rect.top() + 54.0);
    let r = 22.0;
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
    if n <= 24 {
        for i in 0..n {
            let ti = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
            let color = if i == *index {
                visuals.strong_text_color()
            } else {
                visuals.weak_text_color().gamma_multiply(0.6)
            };
            painter.circle_filled(at(angle(ti), r + 13.0), 1.5, color);
        }
    }

    // Body: a shadow, then stacked discs shifted up to fake a top light.
    let (edge, face) = if visuals.dark_mode {
        (Color32::from_rgb(26, 18, 24), Color32::from_rgb(if hot { 98 } else { 84 }, 66, 82))
    } else {
        (Color32::from_rgb(214, 170, 188), Color32::from_rgb(255, if hot { 250 } else { 244 }, 248))
    };
    painter.circle_filled(c + vec2(0.0, 2.5), r + 1.0, Color32::from_black_alpha(70));
    let layers = 8;
    for i in 0..layers {
        let k = i as f32 / (layers - 1) as f32;
        let color = lerp_color(edge, face, k);
        painter.circle_filled(c - vec2(0.0, k * 2.0), r * (1.0 - k * 0.22), color);
    }
    painter.circle_stroke(c, r, Stroke::new(1.0_f32, Color32::from_black_alpha(90)));

    // Pointer
    let a = angle(t);
    let pointer = if visuals.dark_mode { Color32::WHITE } else { Color32::from_gray(40) };
    painter.line_segment([at(a, r * 0.35), at(a, r * 0.85)], Stroke::new(3.0_f32, pointer));

    painter.text(
        pos2(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        choices.get(*index).copied().unwrap_or(""),
        FontId::proportional(15.0),
        visuals.strong_text_color(),
    );
    *index != before
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

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
            Color32::from_rgb(230, 90, 80)
        } else if db > -12.0 {
            Color32::from_rgb(240, 190, 70)
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

/// Where the last-drawn neck put things, so overlays can find a position.
#[derive(Clone, Copy)]
struct NeckGeometry {
    nut_x: f32,
    open_w: f32,
    fret_w: f32,
    top: f32,
    spacing: f32,
}

fn neck_geometry(ctx: &egui::Context) -> Option<NeckGeometry> {
    ctx.data(|d| d.get_temp::<NeckGeometry>(Id::new("fretwise_neck_geometry")))
}

/// Screen point of `pos` on the neck drawn most recently.
pub fn neck_point(ctx: &egui::Context, pos: Position) -> Option<egui::Pos2> {
    let g = neck_geometry(ctx)?;
    let x = if pos.fret == 0 { g.nut_x - g.open_w / 2.0 } else { g.nut_x + (pos.fret as f32 - 0.5) * g.fret_w };
    Some(pos2(x, g.top + (5 - pos.string.min(5)) as f32 * g.spacing))
}

/// A burst of four-point sparkles in the flag colours, `age` seconds after
/// it started; done after about a second.
pub fn sparkles(painter: &egui::Painter, origin: egui::Pos2, age: f32) {
    let life = 0.9;
    if !(0.0..life).contains(&age) {
        return;
    }
    let t = age / life;
    let colors = [crate::theme::TRANS_BLUE, crate::theme::TRANS_PINK, Color32::WHITE];
    for i in 0..14 {
        // A fixed spread per index: the same burst every time, no RNG needed.
        let a = i as f32 * 2.399 + 0.3;
        let speed = 70.0 + (i * 37 % 50) as f32;
        let d = vec2(a.cos(), a.sin()) * speed * t + vec2(0.0, 60.0 * t * t);
        let size = (5.0 + (i % 3) as f32 * 2.0) * (1.0 - t);
        let color = colors[i % 3].gamma_multiply(1.0 - t * t);
        let c = origin + d;
        for (w, h) in [(size * 0.3, size), (size, size * 0.3)] {
            painter.add(egui::Shape::convex_polygon(
                vec![c + vec2(0.0, -h), c + vec2(w, 0.0), c + vec2(0.0, h), c + vec2(-w, 0.0)],
                color,
                Stroke::NONE,
            ));
        }
    }
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
