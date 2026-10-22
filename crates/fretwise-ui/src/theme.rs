//! Fretwise's look: soft pinks in both light and dark mode, with the trans
//! pride colours (blue, pink, white) as the accents.

use egui::{pos2, vec2, Color32, Rect, Stroke, Visuals};

pub const TRANS_BLUE: Color32 = Color32::from_rgb(91, 206, 250);
pub const TRANS_PINK: Color32 = Color32::from_rgb(245, 169, 184);
pub const TRANS_WHITE: Color32 = Color32::WHITE;

/// Install the pink visuals for both themes; egui picks one per the
/// user's preference. Cheap enough to call every frame, but only does
/// work once per context.
pub fn apply(ctx: &egui::Context) {
    let id = egui::Id::new("fretwise_theme_applied");
    if ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return;
    }
    ctx.set_visuals_of(egui::Theme::Dark, dark());
    ctx.set_visuals_of(egui::Theme::Light, light());
    ctx.add_font(egui::epaint::text::FontInsert::new(
        "Monocraft",
        egui::FontData::from_static(include_bytes!("../assets/fonts/Monocraft.ttf")),
        vec![egui::epaint::text::InsertFontFamily {
            family: pixel_family(),
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    ));
    ctx.data_mut(|d| d.insert_temp(id, true));
}

/// Monocraft, a Minecraft-style monospace font (SIL OFL 1.1), for the
/// terminal under the neck.
pub fn pixel_family() -> egui::FontFamily {
    egui::FontFamily::Name("pixel".into())
}

/// The pixel font at `size`, or egui's monospace until it's loaded (fonts
/// added this frame only arrive on the next).
pub fn pixel_font(ui: &egui::Ui, size: f32) -> egui::FontId {
    let family = pixel_family();
    if ui.fonts(|f| f.families().contains(&family)) {
        egui::FontId::new(size, family)
    } else {
        egui::FontId::monospace(size)
    }
}

/// Token colours for the terminal: a dark screen in either theme, with the
/// flag colours for what matters.
pub struct Syntax {
    pub screen: Color32,
    pub border: Color32,
    pub text: Color32,
    pub comment: Color32,
    pub keyword: Color32,
    pub note: Color32,
    pub number: Color32,
}

pub const SYNTAX: Syntax = Syntax {
    screen: Color32::from_rgb(22, 14, 21),
    border: Color32::from_rgb(84, 58, 78),
    text: Color32::from_rgb(226, 210, 220),
    comment: Color32::from_rgb(140, 112, 132),
    keyword: TRANS_PINK,
    note: TRANS_BLUE,
    number: TRANS_WHITE,
};

fn dark() -> Visuals {
    let mut v = Visuals::dark();
    let plum = |r, g, b| Color32::from_rgb(r, g, b);
    v.panel_fill = plum(30, 22, 29);
    v.window_fill = plum(44, 32, 42);
    v.extreme_bg_color = plum(17, 12, 17);
    v.faint_bg_color = plum(38, 28, 36);
    v.code_bg_color = plum(44, 32, 42);
    v.window_stroke = Stroke::new(1.0_f32, plum(84, 58, 78));
    v.hyperlink_color = TRANS_PINK;
    v.selection.bg_fill = plum(208, 92, 140);
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = v.panel_fill;
    w.noninteractive.weak_bg_fill = v.panel_fill;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, plum(70, 50, 66));
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, plum(214, 196, 208));
    w.inactive.bg_fill = plum(62, 44, 58);
    w.inactive.weak_bg_fill = plum(56, 40, 53);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, plum(232, 214, 226));
    w.hovered.bg_fill = plum(92, 58, 82);
    w.hovered.weak_bg_fill = plum(84, 54, 76);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, TRANS_PINK);
    w.active.bg_fill = plum(120, 66, 100);
    w.active.weak_bg_fill = plum(120, 66, 100);
    w.active.bg_stroke = Stroke::new(1.0_f32, TRANS_PINK);
    w.open.bg_fill = plum(62, 44, 58);
    w.open.weak_bg_fill = plum(62, 44, 58);
    v
}

fn light() -> Visuals {
    let mut v = Visuals::light();
    let rose = |r, g, b| Color32::from_rgb(r, g, b);
    v.panel_fill = rose(255, 246, 249);
    v.window_fill = rose(255, 238, 244);
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = rose(252, 236, 242);
    v.code_bg_color = rose(252, 232, 240);
    v.window_stroke = Stroke::new(1.0_f32, rose(236, 196, 212));
    v.hyperlink_color = rose(200, 70, 120);
    v.selection.bg_fill = TRANS_PINK;
    v.selection.stroke = Stroke::new(1.0_f32, rose(74, 20, 48));

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = v.panel_fill;
    w.noninteractive.weak_bg_fill = v.panel_fill;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, rose(236, 204, 216));
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, rose(92, 60, 78));
    w.inactive.bg_fill = rose(246, 222, 232);
    w.inactive.weak_bg_fill = rose(246, 222, 232);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, rose(70, 40, 58));
    w.hovered.bg_fill = rose(250, 208, 222);
    w.hovered.weak_bg_fill = rose(250, 208, 222);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, rose(220, 120, 160));
    w.active.bg_fill = TRANS_PINK;
    w.active.weak_bg_fill = TRANS_PINK;
    w.active.bg_stroke = Stroke::new(1.0_f32, rose(200, 70, 120));
    w.open.bg_fill = rose(246, 222, 232);
    w.open.weak_bg_fill = rose(246, 222, 232);
    v
}

/// Background for the control rack and panels: a shade off the window,
/// like a plug-in's faceplate.
pub fn rack_fill(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(42, 31, 40)
    } else {
        Color32::from_rgb(252, 234, 241)
    }
}

/// Blue → pink → white, the flag read from the edge to the middle; for
/// gradients that nod to it without shouting.
pub fn pride_gradient(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        lerp(TRANS_BLUE, TRANS_PINK, t * 2.0)
    } else {
        lerp(TRANS_PINK, TRANS_WHITE, (t - 0.5) * 2.0)
    }
}

/// The Fretwise logo, a guitar pick striped like the flag with a sticker
/// edge; drawn to match `assets/logo/fretwise-logo.svg` (a 64×64 box).
pub fn paint_pick(painter: &egui::Painter, rect: Rect) {
    let s = rect.width().min(rect.height()) / 64.0;
    let at = |x: f32, y: f32| rect.center() + vec2(x - 32.0, y - 32.0) * s;
    // The SVG outline: four cubics, sampled. The shape is convex.
    let curves = [
        [(32.0, 60.0), (24.0, 52.0), (6.0, 35.0), (6.0, 20.0)],
        [(6.0, 20.0), (6.0, 9.0), (17.0, 4.0), (32.0, 4.0)],
        [(32.0, 4.0), (47.0, 4.0), (58.0, 9.0), (58.0, 20.0)],
        [(58.0, 20.0), (58.0, 35.0), (40.0, 52.0), (32.0, 60.0)],
    ];
    let mut outline = Vec::new();
    for [p0, p1, p2, p3] in curves {
        for i in 0..16 {
            let t = i as f32 / 16.0;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            outline.push((a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1));
        }
    }
    let points = |pts: &[(f32, f32)]| pts.iter().map(|&(x, y)| at(x, y)).collect::<Vec<_>>();
    let plum = Color32::from_rgb(42, 31, 40);

    // The white ring is a centred stroke over the stripes' edge, so the plum
    // one is wider by as much to keep the same width showing outside it.
    painter.add(egui::Shape::closed_line(points(&outline), Stroke::new(9.0 * s, plum)));
    let stripes = [(0.0, 16.0, TRANS_BLUE), (16.0, 27.0, TRANS_PINK), (27.0, 37.0, TRANS_WHITE), (37.0, 46.0, TRANS_PINK), (46.0, 64.0, TRANS_BLUE)];
    for (top, bottom, color) in stripes {
        let band = clip_band(&outline, top, bottom);
        if band.len() >= 3 {
            painter.add(egui::Shape::convex_polygon(points(&band), color, Stroke::NONE));
        }
    }
    painter.add(egui::Shape::closed_line(points(&outline), Stroke::new(3.0 * s, TRANS_WHITE)));
}

/// The part of a convex polygon between two horizontal lines.
fn clip_band(poly: &[(f32, f32)], top: f32, bottom: f32) -> Vec<(f32, f32)> {
    let clip = |poly: Vec<(f32, f32)>, y: f32, keep_below: bool| {
        let inside = |p: (f32, f32)| if keep_below { p.1 >= y } else { p.1 <= y };
        let mut out = Vec::new();
        for (i, &p) in poly.iter().enumerate() {
            let q = poly[(i + 1) % poly.len()];
            if inside(p) {
                out.push(p);
            }
            if inside(p) != inside(q) {
                let t = (y - p.1) / (q.1 - p.1);
                out.push((p.0 + (q.0 - p.0) * t, y));
            }
        }
        out
    };
    clip(clip(poly.to_vec(), top, true), bottom, false)
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// The trans pride flag: five stripes, blue, pink, white, pink, blue.
pub fn paint_flag(painter: &egui::Painter, rect: Rect, rounding: f32) {
    let stripes = [TRANS_BLUE, TRANS_PINK, TRANS_WHITE, TRANS_PINK, TRANS_BLUE];
    let h = rect.height() / stripes.len() as f32;
    painter.rect_filled(rect, rounding, TRANS_BLUE);
    // Inner stripes are square-cornered; the outer blue ones carry the rounding.
    for (i, color) in stripes.iter().enumerate().skip(1).take(3) {
        let r = Rect::from_min_size(pos2(rect.left(), rect.top() + i as f32 * h), vec2(rect.width(), h));
        painter.rect_filled(r, 0.0, *color);
    }
}
