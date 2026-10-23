//! The Ableton-style horizontal effects chain pane at the bottom of the window.
//!
//! Each [`fretwise_core::Effect`] in the signal chain gets its own hardware
//! front plate laid out left-to-right with signal-flow connectors between them.

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Painter, Rect, Sense, Stroke, StrokeKind, Ui,
};
use std::ops::RangeInclusive;

use fretwise_core::{
    VintageChorus, AMP_GAIN_RANGE_DB, AMP_LEVEL_RANGE_DB, AMP_TONE_RANGE, CAB_MIC_RANGE, CHORUS_DEPTH_RANGE,
    CHORUS_MIX_RANGE, CHORUS_RATE_RANGE,
};

use crate::scope;
use crate::theme::{self, GOOD, HEARD, TRANS_BLUE, TRANS_PINK};
use crate::widgets::stepped_knob;
use crate::FretwiseUi;

/// Height added to the viewport when the effects pane is open.
pub(crate) const FX_PANE_SPACE: f32 = CARD_HEIGHT + 30.0;

/// Every device is this tall: the height of the square oscilloscope.
const CARD_HEIGHT: f32 = 236.0;
const HEADER_HEIGHT: f32 = 28.0;

impl FretwiseUi {
    pub(crate) fn fx_pane(&mut self, ui: &mut Ui) {
        egui::ScrollArea::horizontal()
            .id_salt("fretwise_fx_scroll")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(CARD_HEIGHT);
                    ui.spacing_mut().item_spacing.x = 0.0;

                    paint_io_strip(ui, "DI IN", self.amp.enabled);
                    paint_chain_arrow(ui, self.amp.enabled);

                    self.amp_head_plate(ui);
                    paint_chain_arrow(ui, self.amp.enabled && self.amp.cab_enabled);

                    self.cabinet_plate(ui);
                    paint_chain_arrow(ui, self.amp.enabled && self.amp.chorus_enabled);

                    self.chorus_plate(ui);
                    paint_chain_arrow(ui, self.amp.enabled);

                    self.scope_plate(ui);
                    paint_chain_arrow(ui, self.amp.enabled);

                    paint_io_strip(ui, "OUT", self.amp.enabled);
                });
            });
    }

    /// Front plate 1: "VELVET TWIN" clean tube preamp head.
    fn amp_head_plate(&mut self, ui: &mut Ui) {
        let width = 412.0;
        let (rect, _) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::hover());
        let dark = ui.visuals().dark_mode;
        let enabled = self.amp.enabled;

        let colors = if dark {
            ShellColors {
                body: Color32::from_rgb(38, 26, 36),
                header: Color32::from_rgb(54, 36, 50),
                border: if enabled { Color32::from_rgb(138, 86, 122) } else { Color32::from_rgb(68, 48, 64) },
                trim: TRANS_PINK,
            }
        } else {
            ShellColors {
                body: Color32::from_rgb(254, 242, 247),
                header: Color32::from_rgb(246, 220, 232),
                border: if enabled { Color32::from_rgb(214, 130, 168) } else { Color32::from_rgb(220, 192, 206) },
                trim: Color32::from_rgb(200, 70, 120),
            }
        };
        let header_clicked =
            paint_device_shell(ui, rect, "amp_head_power", enabled, ("VELVET TWIN", "TUBE PREAMP"), colors);
        if header_clicked {
            self.amp.enabled = !self.amp.enabled;
        }

        let body_rect = Rect::from_min_max(pos2(rect.left(), rect.top() + HEADER_HEIGHT), rect.max);

        // Left side of faceplate: vintage grille & glowing dual-12AX7 tube window.
        let art_rect = Rect::from_min_size(
            pos2(body_rect.left() + 14.0, body_rect.top() + 12.0),
            vec2(88.0, body_rect.height() - 24.0),
        );
        let drive = normalized(self.amp.gain_db, &AMP_GAIN_RANGE_DB);
        paint_tube_window(ui.painter(), art_rect, enabled, drive, dark);

        // Right side of faceplate: GAIN, TONE, LEVEL stepped rotary knobs,
        // each centered vertically on `body_rect.center().y` so all knob circle centers align.
        let y = body_rect.center().y - 54.0;
        let x0 = art_rect.right() + 10.0;
        let knob_slot = |ui: &mut Ui, x: f32| {
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(pos2(x, y), vec2(92.0, 108.0)))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            )
        };

        let amp = &mut self.amp;
        knob_slot(ui, x0).add_enabled_ui(enabled, |ui| {
            stepped_knob(ui, "amp_gain", "Gain", &mut amp.gain_db, AMP_GAIN_RANGE_DB, |db| format!("+{db} dB"));
        });
        knob_slot(ui, x0 + 98.0).add_enabled_ui(enabled, |ui| {
            stepped_knob(ui, "amp_tone", "Tone", &mut amp.tone, AMP_TONE_RANGE, signed);
        });
        knob_slot(ui, x0 + 196.0).add_enabled_ui(enabled, |ui| {
            let label = |db| format!("{} dB", signed(db));
            stepped_knob(ui, "amp_level", "Level", &mut amp.level_db, AMP_LEVEL_RANGE_DB, label);
        });
    }

    /// Front plate 2: "ROSECAB 1×12" open-back speaker cabinet & dynamic mic.
    fn cabinet_plate(&mut self, ui: &mut Ui) {
        let width = 236.0;
        let (rect, _) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::hover());
        let dark = ui.visuals().dark_mode;
        let active = self.amp.enabled && self.amp.cab_enabled;

        let colors = if dark {
            ShellColors {
                body: Color32::from_rgb(31, 28, 38),
                header: Color32::from_rgb(44, 39, 54),
                border: if active { Color32::from_rgb(82, 128, 162) } else { Color32::from_rgb(60, 52, 68) },
                trim: TRANS_BLUE,
            }
        } else {
            ShellColors {
                body: Color32::from_rgb(244, 248, 254),
                header: Color32::from_rgb(224, 236, 250),
                border: if active { Color32::from_rgb(110, 178, 224) } else { Color32::from_rgb(204, 212, 224) },
                trim: Color32::from_rgb(45, 145, 200),
            }
        };
        let header_clicked = paint_device_shell(ui, rect, "cab_power", active, ("ROSECAB 1×12", "SPEAKER SIM"), colors);
        if header_clicked && self.amp.enabled {
            self.amp.cab_enabled = !self.amp.cab_enabled;
        }

        let body_rect = Rect::from_min_max(pos2(rect.left(), rect.top() + HEADER_HEIGHT), rect.max);

        // Left side: 1×12" speaker cone + movable microphone illustration.
        let speaker_rect = Rect::from_min_size(
            pos2(body_rect.left() + 14.0, body_rect.top() + 12.0),
            vec2(96.0, body_rect.height() - 24.0),
        );
        let mic = normalized(self.amp.cab_mic, &CAB_MIC_RANGE);
        paint_speaker_cone(ui.painter(), speaker_rect, active, mic);

        // Right side: MIC POS rotary knob, centered vertically on `body_rect.center().y` to match amp_head_plate.
        let knob_x = speaker_rect.right() + (body_rect.right() - speaker_rect.right() - 92.0) * 0.5;
        let knob_rect = Rect::from_min_size(pos2(knob_x, body_rect.center().y - 54.0), vec2(92.0, 108.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(knob_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        child.add_enabled_ui(active, |ui| {
            let position = |m: i32| match m {
                -5 => "Edge".to_string(),
                0 => "Center".to_string(),
                5 => "Cap".to_string(),
                m => signed(m),
            };
            stepped_knob(ui, "cab_mic", "Mic Pos", &mut self.amp.cab_mic, CAB_MIC_RANGE, position);
        });
    }

    /// Front plate 3: "LILAC BRIGADE" stereo bucket-brigade chorus.
    fn chorus_plate(&mut self, ui: &mut Ui) {
        let width = 412.0;
        let (rect, _) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::hover());
        let dark = ui.visuals().dark_mode;
        let active = self.amp.enabled && self.amp.chorus_enabled;

        let colors = if dark {
            ShellColors {
                body: Color32::from_rgb(32, 28, 44),
                header: Color32::from_rgb(46, 40, 64),
                border: if active { Color32::from_rgb(132, 110, 190) } else { Color32::from_rgb(62, 56, 78) },
                trim: Color32::from_rgb(178, 150, 255),
            }
        } else {
            ShellColors {
                body: Color32::from_rgb(246, 243, 254),
                header: Color32::from_rgb(230, 222, 250),
                border: if active { Color32::from_rgb(158, 132, 222) } else { Color32::from_rgb(214, 208, 228) },
                trim: Color32::from_rgb(120, 86, 210),
            }
        };
        let header_clicked =
            paint_device_shell(ui, rect, "chorus_power", active, ("LILAC BRIGADE", "STEREO BBD CHORUS"), colors);
        if header_clicked && self.amp.enabled {
            self.amp.chorus_enabled = !self.amp.chorus_enabled;
        }

        let body_rect = Rect::from_min_max(pos2(rect.left(), rect.top() + HEADER_HEIGHT), rect.max);

        // Left side: the BBD chip with its two sweeping taps and rate LED.
        let art_rect = Rect::from_min_size(
            pos2(body_rect.left() + 14.0, body_rect.top() + 12.0),
            vec2(88.0, body_rect.height() - 24.0),
        );
        let now = ui.input(|i| i.time);
        let lfo_phase = (now * VintageChorus::rate_hz(self.amp.chorus_rate) as f64).fract() as f32;
        let depth = normalized(self.amp.chorus_depth, &CHORUS_DEPTH_RANGE);
        paint_bbd_window(ui.painter(), art_rect, active, lfo_phase, depth);

        // Right side: RATE, DEPTH, MIX, lined up with the amp head's knobs.
        let y = body_rect.center().y - 54.0;
        let x0 = art_rect.right() + 10.0;
        let knob_slot = |ui: &mut Ui, x: f32| {
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(pos2(x, y), vec2(92.0, 108.0)))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            )
        };
        let amp = &mut self.amp;
        knob_slot(ui, x0).add_enabled_ui(active, |ui| {
            let label = |r: i32| format!("{:.2} Hz", VintageChorus::rate_hz(r as f32));
            stepped_knob(ui, "chorus_rate", "Rate", &mut amp.chorus_rate, CHORUS_RATE_RANGE, label);
        });
        knob_slot(ui, x0 + 98.0).add_enabled_ui(active, |ui| {
            stepped_knob(ui, "chorus_depth", "Depth", &mut amp.chorus_depth, CHORUS_DEPTH_RANGE, |d| d.to_string());
        });
        knob_slot(ui, x0 + 196.0).add_enabled_ui(active, |ui| {
            let label = |m: i32| format!("{}%", m * 10);
            stepped_knob(ui, "chorus_mix", "Mix", &mut amp.chorus_mix, CHORUS_MIX_RANGE, label);
        });
    }

    /// Front plate 4: "PHOSPHOR 2CH" electron-tube stereo oscilloscope: a
    /// square unit that is nearly all screen, set up by dragging the read-outs
    /// on the glass. It only watches, so its power switch works with the amp
    /// off too.
    fn scope_plate(&mut self, ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(vec2(CARD_HEIGHT, CARD_HEIGHT), Sense::hover());
        let dark = ui.visuals().dark_mode;
        let on = self.scope_cfg.enabled;

        let colors = if dark {
            ShellColors {
                body: Color32::from_rgb(28, 32, 30),
                header: Color32::from_rgb(40, 46, 43),
                border: if on { Color32::from_rgb(88, 150, 112) } else { Color32::from_rgb(56, 64, 60) },
                trim: Color32::from_rgb(70, 255, 130),
            }
        } else {
            ShellColors {
                body: Color32::from_rgb(240, 245, 241),
                header: Color32::from_rgb(220, 232, 224),
                border: if on { Color32::from_rgb(110, 190, 140) } else { Color32::from_rgb(204, 214, 208) },
                trim: Color32::from_rgb(40, 170, 90),
            }
        };
        let now = ui.input(|i| i.time);
        if paint_device_shell(ui, rect, "scope_power", on, ("PHOSPHOR 2CH", "TUBE SCOPE"), colors) {
            self.scope_cfg.enabled = !on;
            self.scope_screen.power_changed(!on, now);
        }

        let body_rect = Rect::from_min_max(pos2(rect.left(), rect.top() + HEADER_HEIGHT), rect.max);
        let side = body_rect.height() - 20.0;
        let crt_rect = Rect::from_center_size(body_rect.center(), vec2(side, side));
        let scope = self.scope.as_deref();
        scope::crt(ui, crt_rect, scope, &mut self.scope_cfg, &mut self.scope_screen, now);
    }
}

/// A device's faceplate colours; `trim` lights its power button.
struct ShellColors {
    body: Color32,
    header: Color32,
    border: Color32,
    trim: Color32,
}

/// Ableton-style device shell: rounded faceplate, top title bar with power toggle button,
/// model subtitle, jewel indicator, and four corner faceplate screws.
/// Returns `true` if the device power button was clicked.
fn paint_device_shell(
    ui: &mut Ui,
    rect: Rect,
    id_salt: &str,
    enabled: bool,
    (title, subtitle): (&str, &str),
    colors: ShellColors,
) -> bool {
    let ShellColors { body: body_fill, header: header_fill, border: border_col, trim: trim_col } = colors;
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, body_fill);

    let header_rect = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_HEIGHT));
    painter.rect_filled(header_rect, 8.0, header_fill);
    // Square off the bottom corners of the header strip
    painter.rect_filled(
        Rect::from_min_size(pos2(rect.left(), header_rect.bottom() - 8.0), vec2(rect.width(), 8.0)),
        0.0,
        header_fill,
    );
    painter.line_segment(
        [pos2(rect.left(), header_rect.bottom()), pos2(rect.right(), header_rect.bottom())],
        Stroke::new(1.0_f32, border_col),
    );
    painter.rect_stroke(rect, 8.0, Stroke::new(1.2_f32, border_col), StrokeKind::Inside);

    // Corner faceplate screws on the lower body
    let screw_col = border_col.gamma_multiply(0.75);
    for pos in [
        pos2(rect.left() + 7.0, header_rect.bottom() + 7.0),
        pos2(rect.right() - 7.0, header_rect.bottom() + 7.0),
        pos2(rect.left() + 7.0, rect.bottom() - 7.0),
        pos2(rect.right() - 7.0, rect.bottom() - 7.0),
    ] {
        painter.circle_filled(pos, 2.2, screw_col);
    }

    // Ableton-style circular power button on the left of the header bar
    let btn_center = pos2(header_rect.left() + 15.0, header_rect.center().y);
    let btn_rect = Rect::from_center_size(btn_center, vec2(18.0, 18.0));
    let power_resp = ui
        .interact(btn_rect, ui.id().with(id_salt), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(if enabled { "Bypass device" } else { "Enable device" });

    let painter = ui.painter();
    let power_fill = if enabled {
        trim_col
    } else if power_resp.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        ui.visuals().extreme_bg_color
    };
    painter.circle_filled(btn_center, 7.5, power_fill);
    painter.circle_stroke(
        btn_center,
        7.5,
        Stroke::new(1.0_f32, if enabled { Color32::WHITE.gamma_multiply(0.7) } else { border_col }),
    );
    // Tiny power glyph inside the button
    let glyph_col = if enabled {
        Color32::from_rgb(20, 14, 20)
    } else {
        ui.visuals().weak_text_color()
    };
    painter.circle_stroke(btn_center, 3.6, Stroke::new(1.4_f32, glyph_col));
    painter.line_segment([btn_center - vec2(0.0, 4.5), btn_center - vec2(0.0, 0.5)], Stroke::new(1.5_f32, glyph_col));

    // Title & subtitle badge
    let title_col = if enabled {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    painter.text(
        pos2(btn_rect.right() + 7.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        title_col,
    );
    painter.text(
        pos2(header_rect.right() - 24.0, header_rect.center().y),
        Align2::RIGHT_CENTER,
        subtitle,
        FontId::proportional(10.0),
        ui.visuals().weak_text_color(),
    );

    // Glowing pilot jewel on top-right of header
    let jewel_pos = pos2(header_rect.right() - 12.0, header_rect.center().y);
    if enabled {
        painter.circle_filled(jewel_pos, 6.0, GOOD.gamma_multiply(0.28));
        painter.circle_filled(jewel_pos, 3.5, GOOD);
    } else {
        painter.circle_filled(jewel_pos, 3.5, ui.visuals().weak_text_color().gamma_multiply(0.3));
    }

    power_resp.clicked()
}

/// Custom-painted glowing vacuum tube window + woven amp grille for the Velvet Twin faceplate.
fn paint_tube_window(painter: &Painter, rect: Rect, enabled: bool, drive_norm: f32, dark: bool) {
    let bay = Color32::from_rgb(16, 11, 16);
    painter.rect_filled(rect, 6.0, bay);

    // Subtle diagonal vintage grille weave in the background
    let weave_col = if dark {
        Color32::from_white_alpha(12)
    } else {
        Color32::from_white_alpha(20)
    };
    let clipped = painter.with_clip_rect(rect.shrink(2.0));
    let mut y = rect.top() + 6.0;
    while y < rect.bottom() - 18.0 {
        clipped.line_segment([pos2(rect.left() + 4.0, y), pos2(rect.right() - 4.0, y)], Stroke::new(1.0_f32, weave_col));
        y += 6.0;
    }

    // Two 12AX7 preamp tubes side-by-side
    let glow_strength = if enabled { 0.45 + 0.55 * drive_norm } else { 0.0 };
    for offset_x in [-18.0_f32, 18.0_f32] {
        let cx = rect.center().x + offset_x;
        // Tube-shaped whatever the plate height: centred, about 70 pt tall.
        let top = (rect.center().y - 40.0).max(rect.top() + 14.0);
        let bot = (rect.center().y + 30.0).min(rect.bottom() - 24.0);
        let tube_rect = Rect::from_min_max(pos2(cx - 10.0, top), pos2(cx + 10.0, bot));

        if enabled {
            let halo = theme::lerp(HEARD, TRANS_PINK, drive_norm * 0.6)
                .gamma_multiply((0.28 + 0.35 * glow_strength).clamp(0.0, 1.0));
            clipped.circle_filled(tube_rect.center(), 18.0, halo);
        }

        // Glass envelope & metallic base
        clipped.rect_filled(tube_rect, 9.0, Color32::from_white_alpha(if enabled { 26 } else { 12 }));
        clipped.rect_stroke(
            tube_rect,
            9.0,
            Stroke::new(1.0_f32, Color32::from_white_alpha(if enabled { 90 } else { 35 })),
            StrokeKind::Inside,
        );

        // Internal anode plates + glowing heater filament
        let filament_col = if enabled {
            theme::lerp(HEARD, TRANS_PINK, drive_norm * 0.5)
        } else {
            Color32::from_gray(55)
        };
        let filament_rect = Rect::from_center_size(tube_rect.center() + vec2(0.0, 2.0), vec2(6.0, 18.0));
        clipped.rect_filled(filament_rect, 2.0, filament_col.gamma_multiply(if enabled { 0.85 } else { 0.4 }));
        if enabled {
            clipped.circle_filled(filament_rect.center(), 3.0, Color32::WHITE.gamma_multiply(0.85));
        }

        // Chrome socket base
        let base_rect = Rect::from_min_max(pos2(cx - 11.0, bot - 6.0), pos2(cx + 11.0, bot + 2.0));
        clipped.rect_filled(base_rect, 2.0, Color32::from_rgb(90, 78, 88));
    }

    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0_f32, Color32::from_white_alpha(35)),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 8.0),
        Align2::CENTER_CENTER,
        "2× 12AX7",
        FontId::proportional(9.5),
        Color32::from_gray(155),
    );
}

/// Custom-painted 1×12" speaker cone + dynamic microphone that moves from dust cap to cone edge.
fn paint_speaker_cone(painter: &Painter, rect: Rect, active: bool, mic_norm: f32) {
    let baffle = Color32::from_rgb(16, 15, 22);
    painter.rect_filled(rect, 6.0, baffle);

    let center = pos2(rect.center().x, rect.center().y - 5.0);
    let outer_r = (rect.width().min(rect.height()) * 0.36).max(22.0);

    // Speaker surround & ribbed paper cone
    let cone_col = if active {
        Color32::from_rgb(48, 52, 66)
    } else {
        Color32::from_rgb(32, 32, 40)
    };
    let ring_col = if active {
        TRANS_BLUE.gamma_multiply(0.65)
    } else {
        Color32::from_gray(70)
    };
    painter.circle_filled(center, outer_r, cone_col);
    painter.circle_stroke(center, outer_r, Stroke::new(2.0_f32, ring_col));
    painter.circle_stroke(center, outer_r * 0.72, Stroke::new(1.0_f32, Color32::from_white_alpha(22)));
    painter.circle_stroke(center, outer_r * 0.45, Stroke::new(1.0_f32, Color32::from_white_alpha(28)));

    // Center dust cap
    let cap_r = outer_r * 0.28;
    painter.circle_filled(center, cap_r, Color32::from_rgb(26, 28, 36));
    painter.circle_stroke(
        center,
        cap_r,
        Stroke::new(1.2_f32, if active { TRANS_PINK.gamma_multiply(0.8) } else { Color32::from_gray(85) }),
    );

    // Dynamic microphone Capsule: mic_norm = 1.0 is dead-center on the dust cap,
    // mic_norm = 0.0 is out at the left cone edge.
    let mic_x = egui::lerp((center.x - outer_r * 0.78)..=center.x, mic_norm);
    let mic_y = center.y + outer_r * 0.18;
    let mic_head = pos2(mic_x, mic_y);

    // Mic stand stem
    painter.line_segment(
        [pos2(mic_x, rect.bottom() - 14.0), pos2(mic_x, mic_y + 5.0)],
        Stroke::new(2.5_f32, Color32::from_gray(130)),
    );
    // Mic grille capsule
    let capsule_col = if active { TRANS_PINK } else { Color32::from_gray(150) };
    painter.circle_filled(mic_head, 5.5, capsule_col);
    painter.circle_stroke(mic_head, 5.5, Stroke::new(1.0_f32, Color32::BLACK));

    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0_f32, Color32::from_white_alpha(35)),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 8.0),
        Align2::CENTER_CENTER,
        "12\" ALNICO",
        FontId::proportional(9.5),
        Color32::from_gray(155),
    );
}

/// The chorus faceplate window: a bucket-brigade chip behind glass, its two
/// delay taps swept in opposite phase, and a rate LED pulsing with the LFO.
fn paint_bbd_window(painter: &Painter, rect: Rect, active: bool, lfo_phase: f32, depth: f32) {
    painter.rect_filled(rect, 6.0, Color32::from_rgb(15, 12, 22));
    let violet = Color32::from_rgb(178, 150, 255);

    // The BBD chip: a DIP package with its pins.
    // Chip and sweep track together, centred in the window.
    let chip = Rect::from_center_size(pos2(rect.center().x, rect.center().y - 30.0), vec2(48.0, 20.0));
    for i in 0..6 {
        let x = chip.left() + 6.0 + i as f32 * 7.2;
        for (y0, y1) in [(chip.top() - 4.0, chip.top()), (chip.bottom(), chip.bottom() + 4.0)] {
            painter.line_segment([pos2(x, y0), pos2(x, y1)], Stroke::new(1.6_f32, Color32::from_gray(120)));
        }
    }
    painter.rect_filled(chip, 2.0, Color32::from_rgb(34, 32, 38));
    painter.circle_filled(chip.left_center() + vec2(4.0, 0.0), 2.0, Color32::from_gray(20));
    painter.text(chip.center(), Align2::CENTER_CENTER, "BBD 1024", FontId::monospace(7.5), Color32::from_gray(150));

    // Sweep track: the two taps ride one triangle LFO, half a cycle apart.
    let track = Rect::from_min_max(
        pos2(rect.left() + 10.0, chip.bottom() + 14.0),
        pos2(rect.right() - 10.0, chip.bottom() + 40.0),
    );
    let swing = track.height() * 0.5 * depth.max(0.1);
    let tri = |p: f32| 1.0 - 4.0 * ((p + 0.25).fract() - 0.5).abs();
    let at = |t: f32| pos2(track.left() + t * track.width(), track.center().y - tri(t) * swing);
    let wave: Vec<_> = (0..=40).map(|i| at(i as f32 / 40.0)).collect();
    let line_col = if active { violet.gamma_multiply(0.45) } else { Color32::from_gray(60) };
    painter.add(egui::Shape::line(wave, Stroke::new(1.2_f32, line_col)));
    for (offset, col) in [(0.0, TRANS_PINK), (0.5, TRANS_BLUE)] {
        let p = at((lfo_phase + offset).fract());
        if active {
            painter.circle_filled(p, 5.0, col.gamma_multiply(0.3));
            painter.circle_filled(p, 2.6, col);
        } else {
            painter.circle_filled(p, 2.6, Color32::from_gray(70));
        }
    }
    let side_label = FontId::proportional(8.0);
    painter.text(track.left_bottom() + vec2(0.0, 6.0), Align2::LEFT_CENTER, "L", side_label.clone(), TRANS_PINK);
    painter.text(track.right_bottom() + vec2(0.0, 6.0), Align2::RIGHT_CENTER, "R", side_label, TRANS_BLUE);

    // Rate LED, brightest at the top of each LFO cycle.
    let led = pos2(rect.right() - 10.0, rect.top() + 9.0);
    if active {
        let pulse = 0.25 + 0.75 * tri(lfo_phase).max(0.0);
        painter.circle_filled(led, 5.0, violet.gamma_multiply(0.3 * pulse));
        painter.circle_filled(led, 2.6, violet.gamma_multiply(pulse));
    } else {
        painter.circle_filled(led, 2.6, Color32::from_gray(50));
    }

    painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, Color32::from_white_alpha(35)), StrokeKind::Inside);
    painter.text(
        pos2(rect.center().x, rect.bottom() - 8.0),
        Align2::CENTER_CENTER,
        "2× SWEEP",
        FontId::proportional(9.5),
        Color32::from_gray(155),
    );
}

/// Far-left signal input strip in the Ableton chain.
fn paint_io_strip(ui: &mut Ui, label: &str, active: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, CARD_HEIGHT), Sense::hover());
    let painter = ui.painter();
    let fill = ui.visuals().extreme_bg_color;
    painter.rect_filled(rect, 6.0, fill);
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
        StrokeKind::Inside,
    );
    let dot_col = if active { GOOD } else { ui.visuals().weak_text_color().gamma_multiply(0.4) };
    painter.circle_filled(pos2(rect.center().x, rect.top() + 18.0), 4.0, dot_col);
    painter.text(
        rect.center() + vec2(0.0, 6.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.5),
        ui.visuals().weak_text_color(),
    );
}

/// Patch connector arrow between adjacent plugins in the horizontal rack.
fn paint_chain_arrow(ui: &mut Ui, active: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, CARD_HEIGHT), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let color = if active {
        TRANS_BLUE
    } else {
        ui.visuals().weak_text_color().gamma_multiply(0.35)
    };
    painter.line_segment([pos2(rect.left() + 3.0, c.y), pos2(rect.right() - 6.0, c.y)], Stroke::new(2.0_f32, color));
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(rect.right() - 3.0, c.y),
            pos2(rect.right() - 9.0, c.y - 4.5),
            pos2(rect.right() - 9.0, c.y + 4.5),
        ],
        color,
        Stroke::NONE,
    ));
}

/// `v`'s place in `range`, 0..=1.
fn normalized(v: f32, range: &RangeInclusive<i32>) -> f32 {
    let (lo, hi) = (*range.start() as f32, *range.end() as f32);
    ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// `n` with an explicit `+` above zero, as amp panels print it.
fn signed(n: i32) -> String {
    if n > 0 {
        format!("+{n}")
    } else {
        n.to_string()
    }
}
