//! The neck: strings, frets, inlays and note markers, the draggable fret
//! range, and overlays that find a spot on the last neck drawn.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, FontId, Id, Rect, Sense, Stroke, StrokeKind, Ui};
use fretwise_core::theory::string_number;
use fretwise_core::Position;

use super::accent;

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

impl Marker {
    /// A plain filled dot with no label.
    pub fn dot(pos: Position, color: Color32, size: f32) -> Self {
        Self { pos, color, size, ring: false, label: None, text: Color32::BLACK }
    }
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
