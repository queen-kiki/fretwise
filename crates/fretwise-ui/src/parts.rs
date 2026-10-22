//! Pieces the screens share: the neck-and-terminal layout, the terminal,
//! markers for scales and chords, and small labelled controls.

use egui::text::LayoutJob;
use egui::{Color32, RichText};
use fretwise_core::theory::pitch_class_name;
use fretwise_core::{ChordVoicing, Position};

use crate::theme::{self, rack_fill, GOOD, HEARD, TARGET};
use crate::widgets::{toggle, Marker};

/// Frets drawn on every neck, and the highest a practice range can reach.
pub(crate) const NECK_FRETS: u8 = 15;

/// Size of a dot that carries a finger number, in chords and scales alike.
pub(crate) const FINGER_DOT: f32 = 0.8;

/// Space between the neck and the terminal under it.
pub(crate) const STAGE_GAP: f32 = 8.0;

/// The faceplate the controls sit on at the bottom of a screen.
pub(crate) fn rack_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new().fill(rack_fill(ui)).inner_margin(egui::Margin::symmetric(16, 10)).corner_radius(8)
}

/// Centres the neck and the terminal under it between header and rack:
/// adds the space above and returns the neck's height. The terminal's
/// height is last frame's, stored under `terminal_id` after drawing it.
pub(crate) fn center_stage(ui: &mut egui::Ui, terminal_id: egui::Id) -> f32 {
    let terminal_h = ui.data(|d| d.get_temp::<f32>(terminal_id)).unwrap_or(100.0);
    let height = (ui.available_height() - terminal_h - STAGE_GAP - 12.0).clamp(220.0, 360.0);
    let spare = ui.available_height() - height - STAGE_GAP - terminal_h;
    ui.add_space((spare / 2.0).max(0.0));
    height
}

/// The terminal under the neck: the command line `cmd` with a blinking
/// block cursor, and the output line `out`. Returns the height it took.
pub(crate) fn terminal_screen(ui: &mut egui::Ui, cmd: LayoutJob, out: LayoutJob, cmd_font: &egui::FontId, now: f64) -> f32 {
    let syn = &theme::SYNTAX;
    egui::Frame::new()
        .fill(syn.screen)
        .stroke(egui::Stroke::new(1.0_f32, syn.border))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(18, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            let line = ui.label(cmd).rect;
            if (now * 1.6).fract() < 0.55 {
                let cursor = egui::Rect::from_min_size(
                    egui::pos2(line.right() + 4.0, line.top() + 2.0),
                    egui::vec2(cmd_font.size * 0.55, line.height() - 4.0),
                );
                ui.painter().rect_filled(cursor, 0.0, syn.keyword);
            }
            ui.label(out);
        })
        .response
        .rect
        .height()
}

/// A dot of the scale pattern, small and unlabelled so the shape reads at a
/// glance: a bright tonic, a slightly bigger 5th, the rest smallest.
/// `interval` is semitones above the root.
pub(crate) fn pattern_dot(pos: Position, interval: u8, root_fill: Color32, other_fill: Color32) -> Marker {
    let (color, size) = match interval {
        0 => (root_fill, 0.55),
        7 => (other_fill, 0.42),
        _ => (other_fill, 0.32),
    };
    Marker::dot(pos, color, size)
}

/// Where the note being played sits on the neck.
pub(crate) fn heard_dot(pos: Position) -> Marker {
    Marker::dot(pos, HEARD, 0.42)
}

/// The pitch classes set in `mask`, by name, each followed by a space.
pub(crate) fn pitch_class_tokens(job: &mut LayoutJob, mask: u16, font: &egui::FontId, color: Color32) {
    for pc in (0..12u8).filter(|pc| mask & (1 << pc) != 0) {
        token(job, pitch_class_name(pc), font, color);
        token(job, " ", font, theme::SYNTAX.text);
    }
}

/// The note to play next: a blue dot with the finger number when the
/// fingering is known (the same look as a chord shape), otherwise a ring
/// around whatever dot is already there. Open strings are always a ring.
pub(crate) fn target_marker(markers: &[Marker], pos: Position, finger: Option<u8>, color: Color32) -> Marker {
    match finger.filter(|&f| f > 0) {
        Some(f) => {
            let label = Some(f.to_string());
            Marker { pos, color, size: FINGER_DOT, ring: false, label, text: Color32::BLACK }
        }
        None => {
            let under = markers.iter().filter(|m| m.pos == pos).map(|m| m.size).fold(0.32, f32::max);
            Marker { pos, color, size: under + 0.28, ring: true, label: None, text: Color32::BLACK }
        }
    }
}

/// A chord shape on the neck, the same in Learn and free practice: a blue
/// dot with the finger number per fretted note (green once heard), a ring
/// for each open string and an x on muted ones. With `show` off, only what
/// has been heard.
pub(crate) fn chord_markers(
    v: &ChordVoicing,
    heard_strings: u8,
    resolved: bool,
    show: bool,
    fingers: bool,
    visuals: &egui::Visuals,
) -> Vec<Marker> {
    let mut markers = Vec::new();
    for string in 0..6 {
        let heard = heard_strings & (1 << string) != 0;
        let color = if resolved || heard { GOOD } else { TARGET };
        match v.frets[string] {
            None if show => markers.push(Marker {
                pos: Position { string, fret: 0 },
                color: Color32::TRANSPARENT,
                size: 0.9,
                ring: false,
                label: Some("×".into()),
                text: visuals.text_color(),
            }),
            None => {}
            Some(0) if show || heard => markers.push(Marker {
                pos: Position { string, fret: 0 },
                color,
                size: 0.6,
                ring: !(resolved || heard),
                label: None,
                text: Color32::BLACK,
            }),
            Some(fret) if show || heard => {
                let finger = v.fingers[string];
                let label = (show && fingers && finger > 0).then(|| finger.to_string());
                let size = if show { FINGER_DOT } else { 0.65 };
                let pos = Position { string, fret };
                markers.push(Marker { pos, color, size, ring: false, label, text: Color32::BLACK });
            }
            Some(_) => {}
        }
    }
    markers
}


/// Append `text` to a terminal line in one colour.
pub(crate) fn token(job: &mut LayoutJob, text: &str, font: &egui::FontId, color: Color32) {
    job.append(text, 0.0, egui::TextFormat::simple(font.clone(), color));
}

/// A small caps caption over a control, as printed on hardware.
pub(crate) fn rack_group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(title.to_uppercase()).size(11.0).weak());
        add(ui);
    });
}

/// A number with a caption under it, for the score.
pub(crate) fn readout(ui: &mut egui::Ui, caption: &str, value: u32, color: Color32) {
    let layout = egui::Layout::top_down(egui::Align::Center);
    ui.allocate_ui_with_layout(egui::vec2(48.0, 40.0), layout, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(value.to_string()).size(20.0).strong().color(color));
        ui.label(RichText::new(caption.to_uppercase()).size(11.0).color(caption_color(ui)));
    });
}

/// Small print on the faceplates: dimmer than text, brighter than egui's weak.
pub(crate) fn caption_color(ui: &egui::Ui) -> Color32 {
    ui.visuals().text_color().gamma_multiply(0.75)
}

pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(title.to_uppercase()).size(11.0).weak());
    ui.separator();
}

pub(crate) fn setting_switch(ui: &mut egui::Ui, on: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        toggle(ui, on);
        ui.label(label);
    });
}
