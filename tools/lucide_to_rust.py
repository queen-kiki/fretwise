"""Turn Lucide SVG icons into Rust point lists for egui.

egui can't draw SVG without extra crates, so each icon's strokes are
flattened to polylines in its 24x24 box once, here, and written to
crates/fretwise-ui/src/lucide.rs.

Usage: python tools/lucide_to_rust.py <dir with .svg files> NAME=file ...
e.g.   python tools/lucide_to_rust.py lucide FLAME=flame SETTINGS=settings

Icons: Lucide (https://lucide.dev), ISC licence; see
crates/fretwise-ui/assets/icons/Lucide-LICENSE.txt.
"""

import math
import os
import re
import sys

NUM = re.compile(r"[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?")
TOKEN = re.compile(r"[MmLlHhVvCcSsQqTtAaZz]|" + NUM.pattern)


def quad(p0, c, p1, n=10):
    return [
        ((1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * c[0] + t * t * p1[0],
         (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * c[1] + t * t * p1[1])
        for t in (i / n for i in range(1, n + 1))
    ]


def cubic(p0, c1, c2, p1, n=12):
    out = []
    for i in range(1, n + 1):
        t = i / n
        m = 1 - t
        out.append((m**3 * p0[0] + 3 * m * m * t * c1[0] + 3 * m * t * t * c2[0] + t**3 * p1[0],
                    m**3 * p0[1] + 3 * m * m * t * c1[1] + 3 * m * t * t * c2[1] + t**3 * p1[1]))
    return out


def arc(p0, rx, ry, large, sweep, p1):
    """SVG endpoint arc (no rotation in Lucide's icons) as points."""
    (x1, y1), (x2, y2) = p0, p1
    if rx == 0 or ry == 0 or p0 == p1:
        return [p1]
    dx, dy = (x1 - x2) / 2, (y1 - y2) / 2
    lam = dx * dx / (rx * rx) + dy * dy / (ry * ry)
    if lam > 1:
        rx, ry = rx * math.sqrt(lam), ry * math.sqrt(lam)
    num = rx * rx * ry * ry - rx * rx * dy * dy - ry * ry * dx * dx
    den = rx * rx * dy * dy + ry * ry * dx * dx
    k = math.sqrt(max(0.0, num / den)) if den else 0.0
    if large == sweep:
        k = -k
    cxp, cyp = k * rx * dy / ry, -k * ry * dx / rx
    cx, cy = cxp + (x1 + x2) / 2, cyp + (y1 + y2) / 2

    def ang(ux, uy, vx, vy):
        return math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)

    t1 = ang(1, 0, (dx - cxp) / rx, (dy - cyp) / ry)
    dt = ang((dx - cxp) / rx, (dy - cyp) / ry, (-dx - cxp) / rx, (-dy - cyp) / ry)
    if not sweep and dt > 0:
        dt -= 2 * math.pi
    elif sweep and dt < 0:
        dt += 2 * math.pi
    n = max(4, int(abs(dt) * max(rx, ry) * 1.5))
    return [(cx + rx * math.cos(t1 + dt * i / n), cy + ry * math.sin(t1 + dt * i / n)) for i in range(1, n + 1)]


def path(d):
    """Polylines (points, closed) for one SVG path."""
    toks = TOKEN.findall(d)
    i = 0
    strokes, pts = [], []
    cur = start = (0.0, 0.0)
    cmd = None
    last_c = last_q = None

    def num():
        nonlocal i
        v = float(toks[i])
        i += 1
        return v

    def flush(closed=False):
        nonlocal pts
        if len(pts) > 1:
            strokes.append((pts, closed))
        pts = []

    while i < len(toks):
        if re.fullmatch(r"[A-Za-z]", toks[i]):
            cmd = toks[i]
            i += 1
            if cmd in "Zz":
                flush(closed=True)
                cur = start
                pts = [cur]
                continue
        rel = cmd.islower()
        c = cmd.upper()

        def pt():
            x, y = num(), num()
            return (cur[0] + x, cur[1] + y) if rel else (x, y)

        prev_c, prev_q = last_c, last_q
        last_c = last_q = None
        if c == "M":
            flush()
            cur = start = pt()
            pts = [cur]
            cmd = "l" if rel else "L"  # extra pairs after M are lines
        elif c == "L":
            cur = pt()
            pts.append(cur)
        elif c == "H":
            x = num()
            cur = (cur[0] + x if rel else x, cur[1])
            pts.append(cur)
        elif c == "V":
            y = num()
            cur = (cur[0], cur[1] + y if rel else y)
            pts.append(cur)
        elif c == "C":
            c1, c2, e = pt(), pt(), pt()
            pts += cubic(cur, c1, c2, e)
            cur, last_c = e, c2
        elif c == "S":
            c1 = (2 * cur[0] - prev_c[0], 2 * cur[1] - prev_c[1]) if prev_c else cur
            c2, e = pt(), pt()
            pts += cubic(cur, c1, c2, e)
            cur, last_c = e, c2
        elif c == "Q":
            q, e = pt(), pt()
            pts += quad(cur, q, e)
            cur, last_q = e, q
        elif c == "T":
            q = (2 * cur[0] - prev_q[0], 2 * cur[1] - prev_q[1]) if prev_q else cur
            e = pt()
            pts += quad(cur, q, e)
            cur, last_q = e, q
        elif c == "A":
            rx, ry, _rot, large, sweep = num(), num(), num(), num(), num()
            e = pt()
            pts += arc(cur, rx, ry, int(large), int(sweep), e)
            cur = e
        else:
            raise ValueError(f"unsupported command {cmd}")
    flush()
    return strokes


def circle(cx, cy, r):
    n = max(12, int(r * 6))
    return [([(cx + r * math.cos(2 * math.pi * k / n), cy + r * math.sin(2 * math.pi * k / n)) for k in range(n)], True)]


def icon(svg):
    strokes = []
    for m in re.finditer(r"<path\s+d=\"([^\"]+)\"", svg):
        strokes += path(m.group(1))
    for m in re.finditer(r"<circle\s+cx=\"([^\"]+)\"\s+cy=\"([^\"]+)\"\s+r=\"([^\"]+)\"", svg):
        strokes += circle(*map(float, m.groups()))
    return strokes


def rust(name, source, strokes):
    lines = [f"/// Lucide \"{source}\".", f"pub const {name}: Icon = &["]
    for pts, closed in strokes:
        coords = ", ".join(f"({x:.2f}, {y:.2f})" for x, y in pts)
        body = f"    (&[{coords}], {str(closed).lower()}),"
        # Wrap long point lists at ~110 columns.
        while len(body) > 116:
            cut = body.rfind("), ", 0, 116) + 2
            lines.append(body[:cut].rstrip())
            body = "        " + body[cut:].lstrip()
        lines.append(body)
    lines.append("];")
    return "\n".join(lines)


HEADER = """//! Lucide icons (https://lucide.dev, ISC licence; see
//! assets/icons/Lucide-LICENSE.txt), flattened to polylines in their 24x24
//! box by tools/lucide_to_rust.py. Generated: edit the script, not this file.

use egui::{vec2, Color32, Rect, Stroke};

/// An icon's strokes: each a polyline and whether it closes.
pub type Icon = &'static [(&'static [(f32, f32)], bool)];

/// Stroke `icon` into the square centred in `rect`, at Lucide's 2-unit line
/// weight scaled to fit.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let scale = side / 24.0;
    let origin = rect.center() - vec2(side, side) / 2.0;
    let stroke = Stroke::new(2.0 * scale, color);
    for (points, closed) in icon {
        let pts: Vec<egui::Pos2> = points.iter().map(|&(x, y)| origin + vec2(x, y) * scale).collect();
        if *closed {
            painter.add(egui::Shape::closed_line(pts, stroke));
        } else {
            painter.add(egui::Shape::line(pts, stroke));
        }
        // Round caps and joins, as Lucide draws them.
        for &(x, y) in points.iter() {
            painter.circle_filled(origin + vec2(x, y) * scale, stroke.width / 2.0, color);
        }
    }
}
"""


def main():
    src_dir, specs = sys.argv[1], sys.argv[2:]
    out = [HEADER]
    for spec in specs:
        name, file = spec.split("=")
        svg = open(os.path.join(src_dir, file + ".svg"), encoding="utf-8").read()
        out.append(rust(name, file, icon(svg)))
    dest = os.path.join(os.path.dirname(__file__), "..", "crates", "fretwise-ui", "src", "lucide.rs")
    open(dest, "w", encoding="utf-8", newline="\n").write("\n\n".join(out) + "\n")
    print("wrote", os.path.normpath(dest))


if __name__ == "__main__":
    main()
