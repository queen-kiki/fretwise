//! The SVG graphics, drawn straight from the files in `assets/` by
//! egui_extras' resvg loader: the Lucide icons (<https://lucide.dev>, ISC
//! licence; see assets/icons/Lucide-LICENSE.txt) and the Fretwise logo.
//! The files are compiled in, so a plugin build needs nothing on disk.

use std::borrow::Cow;
use std::sync::OnceLock;

use egui::emath::GuiRounding;
use egui::{load::Bytes, load::TexturePoll, Color32, ImageSource, Rect, Vec2};

/// An embedded SVG file.
pub struct Svg {
    uri: &'static str,
    file: &'static [u8],
    painted: OnceLock<Vec<u8>>,
}

impl Svg {
    const fn new(uri: &'static str, file: &'static [u8]) -> Self {
        Self { uri, file, painted: OnceLock::new() }
    }

    /// The file with Lucide's `currentColor` made white, which resvg would
    /// otherwise draw black: egui's tint multiplies, so white takes any
    /// colour. The file on disk stays as Lucide ships it.
    fn source(&'static self) -> ImageSource<'static> {
        let bytes = self
            .painted
            .get_or_init(|| String::from_utf8_lossy(self.file).replace("currentColor", "#fff").into_bytes());
        ImageSource::Bytes { uri: Cow::Borrowed(self.uri), bytes: Bytes::Static(bytes) }
    }
}

macro_rules! lucide {
    ($name:literal) => {
        Svg::new(
            concat!("bytes://fretwise/icons/", $name, ".svg"),
            include_bytes!(concat!("../assets/icons/", $name, ".svg")),
        )
    };
}

pub static FLAME: Svg = lucide!("flame");
pub static EIGHTH_NOTE: Svg = lucide!("eighth-note");
pub static GAUGE: Svg = lucide!("gauge");
pub static CHART: Svg = lucide!("chart-no-axes-column");
pub static SETTINGS: Svg = lucide!("settings");
pub static CHEVRON_LEFT: Svg = lucide!("chevron-left");
pub static SKIP_FORWARD: Svg = lucide!("skip-forward");

/// The guitar-pick logo, in its own colours (paint it with white).
pub static LOGO: Svg =
    Svg::new("bytes://fretwise/logo/fretwise-logo.svg", include_bytes!("../../../assets/logo/fretwise-logo.svg"));

/// Add the SVG loader to `ctx`; cheap, and does nothing once it's there.
pub fn install(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

/// Draw `svg` into the square centred in `rect`, tinted `color`, rasterised
/// at the screen's pixel size so it stays crisp at any scale.
pub fn paint(painter: &egui::Painter, rect: Rect, svg: &'static Svg, color: Color32) {
    let side = rect.width().min(rect.height());
    let rect = Rect::from_center_size(rect.center(), Vec2::splat(side)).round_to_pixels(painter.pixels_per_point());
    let image = egui::Image::new(svg.source()).tint(color).fit_to_exact_size(rect.size());
    if let Ok(TexturePoll::Ready { texture }) = image.load_for_size(painter.ctx(), rect.size()) {
        egui::widgets::paint_texture_at(painter, rect, image.image_options(), &texture);
    }
}
