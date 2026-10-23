//! The rack's stereo oscilloscope: a simulated electron-tube CRT with a
//! green phosphor that blooms, lingers and needs a moment to warm up.
//!
//! The audio side is [`fretwise_core::SharedScope`]; this module turns its
//! latest frames into beam traces and paints the tube. There are no knobs:
//! the settings are read-outs on the screen itself, dragged or scrolled to
//! change, and each trace's marker on the left edge drags it up or down.

use std::collections::VecDeque;

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Id, Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};
use serde::{Deserialize, Serialize};

use fretwise_core::SharedScope;

/// Horizontal timebase choices, in milliseconds per division.
const TIME_DIVS_MS: [f32; 6] = [0.5, 1.0, 2.0, 5.0, 10.0, 20.0];
const TIME_LABELS: [&str; 6] = ["0.5ms/div", "1ms/div", "2ms/div", "5ms/div", "10ms/div", "20ms/div"];
/// Vertical sensitivity choices, in full-scale units per division.
const VOLT_DIVS: [f32; 6] = [0.5, 0.2, 0.1, 0.05, 0.02, 0.01];
const VOLT_LABELS: [&str; 6] = ["500mV/div", "200mV/div", "100mV/div", "50mV/div", "20mV/div", "10mV/div"];
const MODE_LABELS: [&str; 2] = ["DUAL", "X-Y"];

/// Graticule size in divisions: a square screen.
const DIVS_X: f32 = 8.0;
const DIVS_Y: f32 = 8.0;
/// How far a trace's marker can be dragged from the centre, in divisions.
const MAX_POSITION: f32 = 3.5;
/// Drag distance per step of a stepped read-out, in points.
const DRAG_STEP: f32 = 18.0;
/// Beam points per trace; plenty for a screen this size.
const TRACE_POINTS: usize = 480;
/// Earlier traces kept glowing on the phosphor.
const PERSISTENCE: usize = 5;
/// Heater warm-up after power-on, and the collapsing dot after power-off, in seconds.
const WARM_UP_S: f64 = 1.6;
const COLLAPSE_S: f64 = 0.35;

/// P31 phosphor green, and the near-white core of the beam.
const PHOSPHOR: Color32 = Color32::from_rgb(70, 255, 130);
const BEAM_CORE: Color32 = Color32::from_rgb(210, 255, 225);

/// The scope's front-panel settings, kept between runs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ScopeConfig {
    pub enabled: bool,
    /// Index into [`MODE_LABELS`]: 0 draws left and right as two traces, 1 plots left against right.
    pub mode: usize,
    /// Index into [`TIME_DIVS_MS`].
    pub time: usize,
    /// Index into [`VOLT_DIVS`].
    pub volts: usize,
    /// Where left and right sit in Dual mode, in divisions above the centre.
    pub positions: [f32; 2],
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self { enabled: true, mode: 0, time: 2, volts: 1, positions: [2.0, -2.0] }
    }
}

impl ScopeConfig {
    /// Replaces anything out of range (a hand-edited or older save) with its default.
    pub fn sanitized(self) -> Self {
        let d = Self::default();
        Self {
            mode: if self.mode < MODE_LABELS.len() { self.mode } else { d.mode },
            time: if self.time < TIME_DIVS_MS.len() { self.time } else { d.time },
            volts: if self.volts < VOLT_DIVS.len() { self.volts } else { d.volts },
            positions: if self.positions.iter().all(|p| p.abs() <= MAX_POSITION) { self.positions } else { d.positions },
            ..self
        }
    }
}

/// What the scope remembers between frames: the phosphor's afterglow and
/// when the tube was switched on or off.
#[derive(Default)]
pub(crate) struct ScopeScreen {
    frames: Vec<(f32, f32)>,
    /// Recent traces, newest last, in graticule divisions (centre is 0, 0).
    trails: VecDeque<Vec<Vec<Pos2>>>,
    /// When power last changed, and whether it went on.
    switched: Option<(f64, bool)>,
    last_mode: usize,
}

impl ScopeScreen {
    /// Note a power switch at `now`, for the warm-up glow or the collapsing dot.
    pub fn power_changed(&mut self, on: bool, now: f64) {
        self.switched = Some((now, on));
        self.trails.clear();
    }

    /// Take the latest traces from `scope` into the afterglow.
    fn capture(&mut self, scope: &SharedScope, cfg: &ScopeConfig) {
        if cfg.mode != self.last_mode {
            self.trails.clear();
            self.last_mode = cfg.mode;
        }
        let rate = scope.sample_rate().max(1) as f32;
        let span = ((TIME_DIVS_MS[cfg.time] * DIVS_X * 0.001 * rate) as usize).max(TRACE_POINTS);
        let per_div = VOLT_DIVS[cfg.volts];
        let traces = if cfg.mode == 0 {
            // Twice the span, so a trigger point can be found with a full sweep after it.
            scope.latest(span * 2, &mut self.frames);
            // Before there's enough audio (or with none), the beam free-runs
            // over silence rather than shrinking to a dot.
            if self.frames.len() < span * 2 {
                let missing = span * 2 - self.frames.len();
                self.frames.splice(0..0, std::iter::repeat_n((0.0, 0.0), missing));
            }
            let start = trigger(&self.frames, span);
            let sweep = &self.frames[start..(start + span).min(self.frames.len())];
            let x = |i: usize| i as f32 / span as f32 * DIVS_X - DIVS_X / 2.0;
            let lane = |pick: fn(&(f32, f32)) -> f32, centre: f32| -> Vec<Pos2> {
                let stride = (sweep.len() / TRACE_POINTS).max(1);
                (0..sweep.len())
                    .step_by(stride)
                    .map(|i| pos2(x(i), (centre + pick(&sweep[i]) / per_div).clamp(-DIVS_Y, DIVS_Y)))
                    .collect()
            };
            vec![lane(|f| f.0, cfg.positions[0]), lane(|f| f.1, cfg.positions[1])]
        } else {
            // X-Y: a fixed ~20 ms of beam, independent of the timebase.
            scope.latest((0.02 * rate) as usize, &mut self.frames);
            let stride = (self.frames.len() / (TRACE_POINTS * 2)).max(1);
            let half = (DIVS_X / 2.0, DIVS_Y / 2.0);
            let trace = self
                .frames
                .iter()
                .step_by(stride)
                .map(|&(l, r)| pos2((l / per_div).clamp(-half.0, half.0), (r / per_div).clamp(-half.1, half.1)))
                .collect();
            vec![trace]
        };
        self.trails.push_back(traces);
        while self.trails.len() > PERSISTENCE {
            self.trails.pop_front();
        }
    }
}

/// Where a sweep of `span` frames should start: the latest rising zero
/// crossing of the summed channels that leaves a whole sweep after it, so a
/// steady note stands still. Free-runs from the last full sweep if there's none.
fn trigger(frames: &[(f32, f32)], span: usize) -> usize {
    let latest = frames.len().saturating_sub(span);
    let mono = |i: usize| frames[i].0 + frames[i].1;
    (8..=latest)
        .rev()
        .find(|&i| mono(i - 1) < 0.0 && mono(i) >= 0.0 && mono(i) - mono(i - 8) > 0.002)
        .unwrap_or(latest)
}

/// The CRT in `rect` (bezel included): graticule, beam and afterglow, with
/// its settings as draggable read-outs on the glass.
pub(crate) fn crt(
    ui: &mut Ui,
    rect: Rect,
    scope: Option<&SharedScope>,
    cfg: &mut ScopeConfig,
    screen: &mut ScopeScreen,
    now: f64,
) {
    let painter = ui.painter().clone();
    // Bezel: a dark moulded surround with a lighter lip.
    painter.rect_filled(rect, 10.0, Color32::from_rgb(14, 16, 15));
    painter.rect_stroke(rect, 10.0, Stroke::new(1.0_f32, Color32::from_white_alpha(40)), StrokeKind::Inside);
    let tube = rect.shrink(6.0);
    let since = screen.switched.map_or(f64::INFINITY, |(t, _)| now - t);

    // Faceplate glass, darker toward the curved edges.
    let glow = if cfg.enabled { (since / WARM_UP_S).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let centre_col = crate::theme::lerp(Color32::from_rgb(8, 13, 10), Color32::from_rgb(14, 34, 22), glow);
    paint_radial(&painter, tube, centre_col, Color32::from_rgb(3, 6, 4));

    let screen_rect = tube.shrink2(vec2(4.0, 4.0));
    let to_screen = |p: Pos2| {
        pos2(
            screen_rect.center().x + p.x / DIVS_X * screen_rect.width(),
            screen_rect.center().y - p.y / DIVS_Y * screen_rect.height(),
        )
    };
    let clipped = painter.with_clip_rect(tube);

    // Graticule: etched lines, a ruled centre cross with minor ticks.
    let grid = Color32::from_rgba_unmultiplied(120, 170, 140, 34);
    for i in 0..=DIVS_X as i32 {
        let x = i as f32 - DIVS_X / 2.0;
        clipped.line_segment([to_screen(pos2(x, -DIVS_Y / 2.0)), to_screen(pos2(x, DIVS_Y / 2.0))], Stroke::new(0.6_f32, grid));
    }
    for i in 0..=DIVS_Y as i32 {
        let y = i as f32 - DIVS_Y / 2.0;
        clipped.line_segment([to_screen(pos2(-DIVS_X / 2.0, y)), to_screen(pos2(DIVS_X / 2.0, y))], Stroke::new(0.6_f32, grid));
    }
    let tick = Color32::from_rgba_unmultiplied(150, 200, 170, 60);
    for i in 0..=(DIVS_X * 5.0) as i32 {
        let p = to_screen(pos2(i as f32 / 5.0 - DIVS_X / 2.0, 0.0));
        clipped.line_segment([p - vec2(0.0, 1.5), p + vec2(0.0, 1.5)], Stroke::new(0.6_f32, tick));
    }
    for i in 0..=(DIVS_Y * 5.0) as i32 {
        let p = to_screen(pos2(0.0, i as f32 / 5.0 - DIVS_Y / 2.0));
        clipped.line_segment([p - vec2(1.5, 0.0), p + vec2(1.5, 0.0)], Stroke::new(0.6_f32, tick));
    }

    if cfg.enabled {
        if let Some(scope) = scope {
            screen.capture(scope, cfg);
        }
        // Older traces fade on the phosphor; the newest is the live beam.
        let n = screen.trails.len();
        for (age, traces) in screen.trails.iter().enumerate() {
            let fresh = (age + 1) as f32 / n.max(1) as f32;
            let intensity = glow * fresh * fresh;
            let live = age + 1 == n;
            for trace in traces {
                let points: Vec<Pos2> = trace.iter().map(|&p| to_screen(p)).collect();
                paint_beam(&clipped, points, intensity, live);
            }
        }
        read_outs(ui, screen_rect, cfg, glow, &to_screen);
    } else if since < COLLAPSE_S {
        // Switching off: the raster collapses to a bright dot that fades out.
        let t = (since / COLLAPSE_S) as f32;
        let c = tube.center();
        clipped.circle_filled(c, 10.0 * (1.0 - t) + 1.0, PHOSPHOR.gamma_multiply(0.3 * (1.0 - t)).additive());
        clipped.circle_filled(c, 2.0, BEAM_CORE.gamma_multiply(1.0 - t));
    }

    // A soft reflection across the curved glass.
    let shine = Rect::from_min_size(tube.min + vec2(3.0, 2.0), vec2(tube.width() * 0.6, tube.height() * 0.3));
    for (i, alpha) in [1_u8, 1, 1].into_iter().enumerate() {
        clipped.rect_filled(shine.shrink(i as f32 * 3.0), 10.0, Color32::from_white_alpha(alpha));
    }
    painter.rect_stroke(tube, 8.0, Stroke::new(1.5_f32, Color32::from_black_alpha(160)), StrokeKind::Inside);
}

/// The on-screen read-outs, which are the scope's controls: drag (or scroll)
/// the mode, volts and timebase, and drag each trace's marker up or down.
fn read_outs(ui: &mut Ui, screen: Rect, cfg: &mut ScopeConfig, glow: f32, to_screen: &dyn Fn(Pos2) -> Pos2) {
    let id = ui.id().with("scope_read_outs");
    let inset = screen.shrink(3.0);

    let mode = read_out(ui, id.with("mode"), inset.right_top(), Align2::RIGHT_TOP, MODE_LABELS[cfg.mode], glow);
    step_by_drag(ui, &mode, id.with("mode"), &mut cfg.mode, MODE_LABELS.len(), vec2(1.0, 0.0));
    if mode.clicked() {
        cfg.mode = (cfg.mode + 1) % MODE_LABELS.len();
    }
    mode.on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Click, drag or scroll: two traces (Dual) or left against right (X-Y)");

    let volts = read_out(ui, id.with("volts"), inset.left_bottom(), Align2::LEFT_BOTTOM, VOLT_LABELS[cfg.volts], glow);
    step_by_drag(ui, &volts, id.with("volts"), &mut cfg.volts, VOLT_DIVS.len(), vec2(0.0, -1.0));
    volts.on_hover_cursor(CursorIcon::ResizeVertical).on_hover_text("Drag up or scroll up for a bigger trace");

    if cfg.mode == 0 {
        let time = read_out(ui, id.with("time"), inset.right_bottom(), Align2::RIGHT_BOTTOM, TIME_LABELS[cfg.time], glow);
        step_by_drag(ui, &time, id.with("time"), &mut cfg.time, TIME_DIVS_MS.len(), vec2(1.0, 0.0));
        time.on_hover_cursor(CursorIcon::ResizeHorizontal).on_hover_text("Drag right or scroll up to show more time");

        let per_div = screen.height() / DIVS_Y;
        for (ch, (position, default)) in cfg.positions.iter_mut().zip(ScopeConfig::default().positions).enumerate() {
            let at = to_screen(pos2(-DIVS_X / 2.0, *position));
            let marker = trace_marker(ui, id.with(("ch", ch)), at, ch + 1, glow);
            if marker.dragged() {
                *position = (*position - marker.drag_delta().y / per_div).clamp(-MAX_POSITION, MAX_POSITION);
            }
            if marker.double_clicked() {
                *position = default;
            }
            let side = if ch == 0 { "Left" } else { "Right" };
            marker
                .on_hover_cursor(CursorIcon::ResizeVertical)
                .on_hover_text(format!("{side} channel: drag to move the trace, double-click to put it back"));
        }
    } else {
        read_out(ui, id.with("axes"), inset.right_bottom(), Align2::RIGHT_BOTTOM, "X:L Y:R", glow * 0.6);
    }
}

/// A phosphor read-out on the glass that lights up under the pointer.
fn read_out(ui: &mut Ui, id: Id, anchor: Pos2, align: Align2, text: &str, glow: f32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::monospace(10.0), PHOSPHOR);
    let rect = align.anchor_size(anchor, galley.size() + vec2(8.0, 4.0));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let hot = response.hovered() || response.dragged();
    if hot {
        ui.painter().rect_filled(rect, 3.0, PHOSPHOR.gamma_multiply(0.08 * glow).additive());
        ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0_f32, PHOSPHOR.gamma_multiply(0.45 * glow)), StrokeKind::Inside);
    }
    let col = PHOSPHOR.gamma_multiply(if hot { 1.0 } else { 0.7 } * glow);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, col);
    response
}

/// A trace's ground marker on the left edge: a pointer with its channel number.
fn trace_marker(ui: &mut Ui, id: Id, at: Pos2, channel: usize, glow: f32) -> Response {
    let rect = Rect::from_min_size(pos2(at.x - 2.0, at.y - 7.0), vec2(20.0, 14.0));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let hot = response.hovered() || response.dragged();
    let col = PHOSPHOR.gamma_multiply(if hot { 1.0 } else { 0.7 } * glow);
    let tip = pos2(rect.right() - 4.0, at.y);
    ui.painter().add(Shape::convex_polygon(
        vec![pos2(at.x, at.y - 6.0), tip, pos2(at.x, at.y + 6.0)],
        col.gamma_multiply(if hot { 0.5 } else { 0.25 }),
        Stroke::new(1.0_f32, col),
    ));
    ui.painter().text(pos2(at.x + 5.0, at.y), Align2::CENTER_CENTER, channel.to_string(), FontId::monospace(8.0), col);
    response
}

/// Step `index` (one of `n`) by dragging along `dir`, [`DRAG_STEP`] points a
/// step, or by scrolling over it.
fn step_by_drag(ui: &Ui, response: &Response, id: Id, index: &mut usize, n: usize, dir: Vec2) {
    if response.drag_started() {
        ui.data_mut(|d| d.insert_temp(id, *index as f32));
    }
    if response.dragged() {
        let start = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(*index as f32);
        let acc = (start + response.drag_delta().dot(dir) / DRAG_STEP).clamp(0.0, (n - 1) as f32);
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
}

/// One beam trace: a wide faint bloom, a brighter glow, then a thin hot core,
/// all added onto the glass the way light adds.
fn paint_beam(painter: &Painter, points: Vec<Pos2>, intensity: f32, live: bool) {
    if intensity <= 0.0 || points.is_empty() {
        return;
    }
    if Rect::from_points(&points).size().max_elem() < 1.0 {
        // A stationary beam burns a dot.
        let p = points[0];
        painter.circle_filled(p, 4.0, PHOSPHOR.gamma_multiply(0.25 * intensity).additive());
        painter.circle_filled(p, 1.6, BEAM_CORE.gamma_multiply(intensity).additive());
        return;
    }
    painter.add(Shape::line(points.clone(), Stroke::new(5.0_f32, PHOSPHOR.gamma_multiply(0.10 * intensity).additive())));
    painter.add(Shape::line(points.clone(), Stroke::new(2.4_f32, PHOSPHOR.gamma_multiply(0.30 * intensity).additive())));
    let core = if live { BEAM_CORE } else { PHOSPHOR };
    painter.add(Shape::line(points, Stroke::new(1.0_f32, core.gamma_multiply(0.85 * intensity).additive())));
}

/// Fill `rect` with `inner` at its centre fading to `outer` at its edge.
fn paint_radial(painter: &Painter, rect: Rect, inner: Color32, outer: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.center(), inner);
    let rim = [
        rect.left_top(),
        rect.center_top(),
        rect.right_top(),
        rect.right_center(),
        rect.right_bottom(),
        rect.center_bottom(),
        rect.left_bottom(),
        rect.left_center(),
    ];
    for p in rim {
        mesh.colored_vertex(p, outer);
    }
    for i in 0..rim.len() as u32 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % rim.len() as u32);
    }
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_finds_the_latest_rising_crossing_with_a_full_sweep_after_it() {
        // A 100-frame-period sine: rising crossings at 0, 100, 200, ...
        let frames: Vec<(f32, f32)> = (0..1000)
            .map(|i| {
                let s = (std::f32::consts::TAU * i as f32 / 100.0).sin() * 0.5;
                (s, s)
            })
            .collect();
        let start = trigger(&frames, 300);
        assert!((590..=700).contains(&start), "{start}");
        assert!(frames[start].0 >= 0.0 && frames[start - 1].0 < 0.0);
        // Silence free-runs.
        assert_eq!(trigger(&[(0.0, 0.0); 1000], 300), 700);
    }

    #[test]
    fn dual_mode_draws_two_full_width_traces() {
        let cfg = ScopeConfig::default();
        let mut screen = ScopeScreen::default();

        // Nothing recorded yet: two flat lines across the screen.
        let scope = SharedScope::default();
        screen.capture(&scope, &cfg);
        let traces = screen.trails.back().unwrap();
        assert_eq!(traces.len(), 2);
        for (trace, centre) in traces.iter().zip([2.0, -2.0]) {
            assert!(trace.first().unwrap().x <= -3.9 && trace.last().unwrap().x >= 3.9);
            assert!(trace.iter().all(|p| p.y == centre));
        }

        // A 440 Hz tone at 48 kHz moves the left trace and leaves the right flat.
        scope.set_sample_rate(48_000);
        for i in 0..20_000 {
            scope.push((std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin() * 0.2, 0.0);
        }
        screen.capture(&scope, &cfg);
        let traces = screen.trails.back().unwrap();
        let (lo, hi) = traces[0].iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        assert!(lo < 1.2 && hi > 2.8, "left swing {lo}..{hi}");
        assert!(traces[1].iter().all(|p| p.y == -2.0));

        // Dragged markers move the traces.
        let cfg = ScopeConfig { positions: [0.5, -3.0], ..cfg };
        screen.capture(&scope, &cfg);
        let traces = screen.trails.back().unwrap();
        assert!(traces[1].iter().all(|p| p.y == -3.0));
        let mid = traces[0].iter().map(|p| p.y).sum::<f32>() / traces[0].len() as f32;
        assert!((mid - 0.5).abs() < 0.1, "{mid}");
    }

    #[test]
    fn out_of_range_settings_fall_back() {
        let cfg = ScopeConfig { enabled: false, mode: 9, time: 99, volts: 6, positions: [9.0, 0.0] }.sanitized();
        assert_eq!(cfg, ScopeConfig { enabled: false, ..ScopeConfig::default() });
    }
}
