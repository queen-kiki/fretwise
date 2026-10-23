//! Painted hardware details: the screwed nameplates and the 14-segment
//! read-out the knobs and the header's brand plate wear.

use egui::{pos2, vec2, Color32, FontId, Rect, Stroke, StrokeKind, Ui};

/// Draws a brushed/embossed metal hardware nameplate with bevel edges and corner or side screws.
pub fn paint_embossed_plate(ui: &Ui, rect: Rect, hot: bool, four_screws: bool) {
    let dark = ui.visuals().dark_mode;
    let painter = ui.painter();

    // Recessed drop shadow
    painter.rect_filled(rect.translate(vec2(0.0, 1.5)), 4.0, Color32::from_black_alpha(if dark { 110 } else { 35 }));

    // Plate body fill
    let plate_fill = if dark {
        if hot {
            Color32::from_rgb(48, 34, 46)
        } else {
            Color32::from_rgb(38, 26, 36)
        }
    } else if hot {
        Color32::from_rgb(245, 224, 235)
    } else {
        Color32::from_rgb(236, 212, 225)
    };
    painter.rect_filled(rect, 4.0, plate_fill);

    // Top-left specular highlight and bottom-right shadow bevel
    let hi = if dark {
        if hot {
            crate::theme::TRANS_PINK.gamma_multiply(0.55)
        } else {
            Color32::from_white_alpha(38)
        }
    } else {
        Color32::from_white_alpha(210)
    };
    let lo = if dark {
        Color32::from_black_alpha(150)
    } else {
        Color32::from_rgb(188, 152, 172)
    };
    painter.line_segment(
        [pos2(rect.left() + 3.0, rect.top() + 0.5), pos2(rect.right() - 3.0, rect.top() + 0.5)],
        Stroke::new(1.0_f32, hi),
    );
    painter.line_segment(
        [pos2(rect.left() + 3.0, rect.bottom() - 0.5), pos2(rect.right() - 3.0, rect.bottom() - 0.5)],
        Stroke::new(1.0_f32, lo),
    );
    let border_col = if hot {
        crate::theme::TRANS_PINK.gamma_multiply(0.7)
    } else if dark {
        Color32::from_rgb(76, 54, 72)
    } else {
        Color32::from_rgb(196, 160, 180)
    };
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, border_col), StrokeKind::Inside);

    // Mounting screws
    let screw_fill = if dark {
        Color32::from_rgb(88, 68, 84)
    } else {
        Color32::from_rgb(190, 162, 178)
    };
    let slot_col = if dark {
        Color32::from_rgb(22, 14, 20)
    } else {
        Color32::from_rgb(112, 86, 102)
    };
    let draw_screw = |sc: egui::Pos2, r: f32| {
        painter.circle_filled(sc + vec2(0.0, 0.6), r, Color32::from_black_alpha(80));
        painter.circle_filled(sc, r, screw_fill);
        painter.circle_stroke(sc, r, Stroke::new(0.6_f32, Color32::from_white_alpha(45)));
        let d = r * 0.58;
        painter.line_segment([sc - vec2(d, d), sc + vec2(d, d)], Stroke::new(0.85_f32, slot_col));
    };

    if four_screws {
        let inset = 5.0;
        for sx in [rect.left() + inset, rect.right() - inset] {
            for sy in [rect.top() + inset, rect.bottom() - inset] {
                draw_screw(pos2(sx, sy), 2.1);
            }
        }
    } else {
        let inset_x = 5.5;
        let cy = rect.center().y;
        draw_screw(pos2(rect.left() + inset_x, cy), 1.9);
        draw_screw(pos2(rect.right() - inset_x, cy), 1.9);
    }
}

/// Draws a small embossed screwed hardware label with all-caps label text, used for plugin knob titles.
pub fn paint_embossed_label(ui: &Ui, center: egui::Pos2, text: &str, hot: bool) {
    let dark = ui.visuals().dark_mode;
    let font = FontId::proportional(10.5);
    let text_col = if hot {
        if dark {
            Color32::WHITE
        } else {
            Color32::from_rgb(48, 22, 38)
        }
    } else if dark {
        Color32::from_rgb(222, 198, 214)
    } else {
        Color32::from_rgb(84, 52, 70)
    };
    let galley = ui.painter().layout_no_wrap(text.to_uppercase(), font, text_col);
    let w = (galley.size().x + 24.0).max(54.0);
    let h = 16.0;
    let rect = Rect::from_center_size(center, vec2(w, h));
    paint_embossed_plate(ui, rect, hot, false);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, text_col);
}

/// 14-segment ("starburst" / Union Jack) ASCII character map.
///
/// Bit layout:
/// - `0` (`A`):  top horizontal
/// - `1` (`B`):  upper-right vertical
/// - `2` (`C`):  lower-right vertical
/// - `3` (`D`):  bottom horizontal
/// - `4` (`E`):  lower-left vertical
/// - `5` (`F`):  upper-left vertical
/// - `6` (`G1`): middle-left horizontal
/// - `7` (`G2`): middle-right horizontal
/// - `8` (`I`):  upper-center vertical
/// - `9` (`L`):  lower-center vertical
/// - `10` (`H`): upper-left diagonal (`\`)
/// - `11` (`J`): upper-right diagonal (`/`)
/// - `12` (`K`): lower-right diagonal (`\`)
/// - `13` (`M`): lower-left diagonal (`/`)
fn segment14_mask(ch: char) -> u16 {
    const A: u16 = 1 << 0;
    const B: u16 = 1 << 1;
    const C: u16 = 1 << 2;
    const D: u16 = 1 << 3;
    const E: u16 = 1 << 4;
    const F: u16 = 1 << 5;
    const G1: u16 = 1 << 6;
    const G2: u16 = 1 << 7;
    const I: u16 = 1 << 8;
    const L: u16 = 1 << 9;
    const H: u16 = 1 << 10;
    const J: u16 = 1 << 11;
    const K: u16 = 1 << 12;
    const M: u16 = 1 << 13;

    match ch {
        '0' => A | B | C | D | E | F,
        '1' => B | C,
        '2' => A | B | G1 | G2 | E | D,
        '3' => A | B | G2 | C | D,
        '4' => F | G1 | G2 | B | C,
        '5' => A | F | G1 | G2 | C | D,
        '6' => A | F | E | D | C | G1 | G2,
        '7' => A | B | C,
        '8' => A | B | C | D | E | F | G1 | G2,
        '9' => A | B | C | D | F | G1 | G2,
        '+' => G1 | G2 | I | L,
        '-' => G1 | G2,
        '/' => J | M,
        '\\' => H | K,
        '_' => D,
        '%' => F | C | J | M,
        '#' | '♯' => G1 | G2 | I | L | B | E,
        '*' => G1 | G2 | I | L | H | J | K | M,
        // Keep lowercase 'd' and 'b' distinct for "dB" / flat readouts, uppercase for the rest
        'd' => B | C | D | E | G1 | G2,
        'b' | '♭' => C | D | E | F | G1 | G2,
        _ => match ch.to_ascii_uppercase() {
            'A' => A | B | C | E | F | G1 | G2,
            'B' => A | B | C | D | G2 | I | L,
            'C' => A | D | E | F,
            'D' => A | B | C | D | I | L,
            'E' => A | D | E | F | G1 | G2,
            'F' => A | E | F | G1 | G2,
            'G' => A | C | D | E | F | G2,
            'H' => B | C | E | F | G1 | G2,
            'I' => A | D | I | L,
            'J' => B | C | D | E,
            'K' => E | F | G1 | J | K,
            'L' => D | E | F,
            'M' => B | C | E | F | H | J,
            'N' => B | C | E | F | H | K,
            'O' => A | B | C | D | E | F,
            'P' => A | B | E | F | G1 | G2,
            'Q' => A | B | C | D | E | F | K,
            'R' => A | B | E | F | G1 | G2 | K,
            'S' => A | F | G1 | G2 | C | D,
            'T' => A | I | L,
            'U' => B | C | D | E | F,
            'V' => E | F | J | M,
            'W' => B | C | E | F | K | M,
            'X' => H | J | K | M,
            'Y' => H | J | L,
            'Z' => A | D | J | M,
            _ => 0,
        },
    }
}

/// Draws a recessed 14-segment LED screen readout with a dark background,
/// slightly lighter unlit segments, and a glowing pink active readout.
pub fn paint_segment_screen(ui: &Ui, center: egui::Pos2, text: &str, hot: bool) {
    let painter = ui.painter();
    // A '.' lights the decimal point after the character before it, as on
    // real LED read-outs, rather than taking a cell of its own.
    let mut chars: Vec<char> = Vec::new();
    let mut points: Vec<bool> = Vec::new();
    for c in text.chars() {
        match (c, points.last_mut()) {
            ('.', Some(dp)) if !*dp => *dp = true,
            _ => {
                chars.push(c);
                points.push(false);
            }
        }
    }
    let cells = chars.len().max(6);
    let pad_left = (cells - chars.len()) / 2;

    let cw = 6.4_f32;
    let ch = 10.0_f32;
    let gap = 2.8_f32;
    let total_chars_w = cells as f32 * cw + (cells.saturating_sub(1)) as f32 * gap;
    let screen_w = (total_chars_w + 12.0).max(58.0);
    let screen_h = 17.0;
    let rect = Rect::from_center_size(center, vec2(screen_w, screen_h));

    // Sunken dark glass housing
    let bg = Color32::from_rgb(13, 8, 13);
    painter.rect_filled(rect, 3.0, bg);
    painter.line_segment(
        [pos2(rect.left() + 2.0, rect.top() + 0.5), pos2(rect.right() - 2.0, rect.top() + 0.5)],
        Stroke::new(1.0_f32, Color32::from_black_alpha(180)),
    );
    painter.line_segment(
        [pos2(rect.left() + 2.0, rect.bottom() - 0.5), pos2(rect.right() - 2.0, rect.bottom() - 0.5)],
        Stroke::new(1.0_f32, Color32::from_white_alpha(28)),
    );
    let border_col = if hot {
        crate::theme::TRANS_PINK.gamma_multiply(0.55)
    } else {
        Color32::from_rgb(56, 38, 52)
    };
    painter.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, border_col), StrokeKind::Inside);

    // Unoccupied ghost segment color vs active pink LED glow + core
    let ghost_col = Color32::from_rgb(35, 22, 33);
    let glow_col = crate::theme::TRANS_PINK.gamma_multiply(if hot { 0.36 } else { 0.26 });
    let lit_col = if hot {
        Color32::from_rgb(255, 200, 222)
    } else {
        crate::theme::TRANS_PINK
    };

    let start_x = rect.center().x - total_chars_w * 0.5;
    let y0 = rect.center().y - ch * 0.5;
    let y1 = rect.center().y + ch * 0.5;
    let ym = rect.center().y;

    let seg_inset = |a: egui::Pos2, b: egui::Pos2| -> [egui::Pos2; 2] {
        let d = b - a;
        [a + d * 0.12, b - d * 0.12]
    };

    for idx in 0..cells {
        let (mask, dp) = if idx >= pad_left && idx < pad_left + chars.len() {
            (segment14_mask(chars[idx - pad_left]), points[idx - pad_left])
        } else {
            (0, false)
        };
        let x0 = start_x + idx as f32 * (cw + gap);
        let x1 = x0 + cw;
        let xm = (x0 + x1) * 0.5;

        let segs: [(egui::Pos2, egui::Pos2); 14] = [
            (pos2(x0, y0), pos2(x1, y0)),               // 0: A (top)
            (pos2(x1, y0), pos2(x1, ym)),               // 1: B (upper right)
            (pos2(x1, ym), pos2(x1, y1)),               // 2: C (lower right)
            (pos2(x0, y1), pos2(x1, y1)),               // 3: D (bottom)
            (pos2(x0, ym), pos2(x0, y1)),               // 4: E (lower left)
            (pos2(x0, y0), pos2(x0, ym)),               // 5: F (upper left)
            (pos2(x0, ym), pos2(xm, ym)),               // 6: G1 (mid left)
            (pos2(xm, ym), pos2(x1, ym)),               // 7: G2 (mid right)
            (pos2(xm, y0 + ch * 0.14), pos2(xm, ym)),   // 8: I (upper center)
            (pos2(xm, ym), pos2(xm, y1 - ch * 0.14)),   // 9: L (lower center)
            (pos2(x0, y0), pos2(xm, ym)),               // 10: H (top-left diag)
            (pos2(x1, y0), pos2(xm, ym)),               // 11: J (top-right diag)
            (pos2(xm, ym), pos2(x1, y1)),               // 12: K (bottom-right diag)
            (pos2(xm, ym), pos2(x0, y1)),               // 13: M (bottom-left diag)
        ];

        // 1. Pass 1: draw all 14 unoccupied ghost segments
        for (bit, &(p0, p1)) in segs.iter().enumerate() {
            if (mask & (1 << bit)) == 0 {
                painter.line_segment(seg_inset(p0, p1), Stroke::new(0.85_f32, ghost_col));
            }
        }
        // 2. Pass 2: draw soft pink LED bloom + crisp lit segment core
        for (bit, &(p0, p1)) in segs.iter().enumerate() {
            if (mask & (1 << bit)) != 0 {
                let pts = seg_inset(p0, p1);
                painter.line_segment(pts, Stroke::new(3.2_f32, glow_col));
                painter.line_segment(pts, Stroke::new(1.5_f32, lit_col));
            }
        }
        // 3. Decimal point, in the gap after the character
        let dp_pos = pos2(x1 + gap * 0.5, y1);
        if dp {
            painter.circle_filled(dp_pos, 1.6, glow_col);
            painter.circle_filled(dp_pos, 0.9, lit_col);
        } else {
            painter.circle_filled(dp_pos, 0.6, ghost_col);
        }
    }
}
