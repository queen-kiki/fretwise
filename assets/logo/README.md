# Fretwise logo

A guitar pick striped like the trans flag (blue, pink, white, pink, blue),
with a sticker edge: a plum outline outside and a thin white ring inside, so
it stands out on both the light and the dark theme. Hand-written SVG, no fonts.

## Colours

| Use          | Hex       |
|--------------|-----------|
| Blue stripes | `#5BCEFA` (`TRANS_BLUE`) |
| Pink stripes | `#F5A9B8` (`TRANS_PINK`) |
| White stripe, inner ring | `#FFFFFF` |
| Outer edge   | `#2A1F28` (the dark theme's plum) |

## Files

- `fretwise-logo.svg`: the icon mark (64×64 viewBox).
- `fretwise-logo-{16,32,64,256,512}.png`: transparent PNG exports.
- `fretwise.ico`: Windows icon with 16, 24, 32, 48, 64, 128 and 256 px.
- `candidates/`: every round of the design, with `roundN-compare.html` pages
  showing each candidate at several sizes on light and dark.

The PNGs were rendered from the SVG at 1024 px with headless Edge and scaled
down with Pillow (Lanczos). There is no wordmark yet; if one is added, convert
the text to paths or use a font whose license allows embedding.
