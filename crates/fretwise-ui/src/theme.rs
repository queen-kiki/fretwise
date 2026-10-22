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
    ctx.data_mut(|d| d.insert_temp(id, true));
}

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
