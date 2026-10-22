//! Custom-painted widgets, all plain egui: the fretboard with its fret-range
//! strip, the stepped knob, buttons and pickers, and the meters.

mod controls;
mod fretboard;
mod knob;
mod meters;

use egui::{Color32, Ui};

pub use controls::{dropdown, gear_button, icon_button, plain_button, segmented, skip_button, toggle, Icon};
pub use fretboard::{fretboard, neck_point, sparkles, Marker, NeckControls};
pub use knob::knob;
pub use meters::{flame, level_meter, session_bar, tuner_gauge};

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
