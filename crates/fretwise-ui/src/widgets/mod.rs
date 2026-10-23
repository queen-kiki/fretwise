//! Custom-painted widgets, all plain egui: the fretboard with its fret-range
//! strip, the stepped knob, buttons and pickers, the meters, and the
//! nameplates and segment read-outs they wear.

mod controls;
mod fretboard;
mod knob;
mod meters;
mod plate;

use egui::{Color32, Ui};

pub use controls::{
    amp_button, dropdown, icon_button, menu_item, plain_button, MENU_WIDTH, segmented, skip_button, toggle, wippschalter, Icon,
};
pub use fretboard::{fretboard, neck_point, sparkles, Marker, NeckControls};
pub use knob::{knob, stepped_knob};
pub use meters::{flame, level_meter, output_meter, session_bar, tuner_gauge};
pub use plate::paint_embossed_plate;
use plate::{paint_embossed_label, paint_segment_screen};

/// The colour the controls light up in: the theme's selection colour, so
/// knobs, buttons and the fret range match egui's own widgets.
pub fn accent(ui: &Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

/// Mixes `a` towards `b` by `t`, truncating each channel (unlike
/// [`crate::theme::lerp`], which rounds).
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}
