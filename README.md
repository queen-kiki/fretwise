# Fretwise

Guitar learning app for electric guitar through an audio interface.

- **Learn** (the home screen, Anki-style spaced repetition): SM-2 flashcards
  for whole scale patterns and chords, picked from a deck. Cards are graded
  hands-free from speed and accuracy; in the pause after a card, keys `1`–`4`
  override the grade as `Again` / `Hard` / `Good` / `Easy`. Once a card
  matures, the shape on the neck is hidden so you recall it from memory. The
  **Cards** button opens the deck browser.
- **Free practice**, from the eighth-note button:
  - **Notes**: asks for a note on a string (or shows a dot), listens for it,
    and asks more often about the positions you're slow on or miss.
  - **Chords**: a chord family, a single chord, or a Learn deck's chords,
    recognizing both full strums (via FFT polyphonic detection) and slow
    strums / arpeggios. **Diagram** on Auto shows a chord's shape until
    you've played it cleanly and quickly a few times, then hides it.
- **Tuner**: note, cents gauge, and which string to turn which way.
- **Stats**: per-position heatmap of the note trainer's first-try accuracy
  and average time.

The space bar skips, like the skip button, on the Learn, notes and chords
screens.

## Run

```
cargo run --release
```

Open settings with the ⚙ button (top right), and pick the interface and the
input your guitar is plugged into. Watch the IN meter next to it: it should
move when you play. The tick on the meter is the **Gate** (in settings,
-55 dB to start): quieter input is ignored, and the meter turns grey below
it. If your notes aren't heard, lower the gate; if hum or string noise
counts as notes, raise it.

## Using free practice (notes)

- **Key** knob: drag up/down, scroll, or click for a list.
- **Scale** menu: click it for the list, sorted into groups (basic, modes,
  minor, exotic). The groups come from the scale files, not the code.
- **Order** and **Labels**: Random / Up / Up & down, and note names or scale
  degrees.
- **Fret range**: drag the lit strip of fret numbers under the neck. Drag an
  end to resize it, drag the middle to move it, or click a fret outside it.
  When at least two roots of the key fit in the range, notes below the lowest
  root and above the highest root are left out, so the pattern runs from
  octave to octave.
- **Strings**: go by number, 1 = high E to 6 = low E, on the neck and in the
  prompt. Click a string number left of the neck to leave it out.
- **Fingering**: when the fret range fits one hand position (up to 5 frets,
  e.g. 5 to 8), the prompt says which finger to use: "with finger 3", 1 index
  to 4 pinky, or "open string". It follows the standard "one finger per
  fret" rule: the index sits on the first fret of the range, and the pinky
  stretches to a fifth fret. Wider ranges hold several overlapping positions
  with no single right answer, so they show no numbers. You can turn this off
  in settings.
- **Chromatic**: every note is in the scale, so only the key's root is marked.
- Everything else (audio device, input gate, tuning, A4, prompt style,
  scale pattern, chime, theme) is in the ⚙ settings panel. Esc or a click outside closes it.
- The colours are soft pinks in light and dark mode, with the trans pride
  blue, pink and white as accents. A small "about" link at the very bottom of
  settings opens the About window (version and build date).

## Adding a scale

Each scale is a JSON file. Built-in ones are in `crates/fretwise-core/scales/`
and are compiled into the app (and a future plugin), so nothing has to sit
next to the exe. Add a file there and rebuild, or drop one into
`%APPDATA%\Fretwise\scales\` and restart the app. A bad file there is
skipped, with the reason printed to the console.

```json
{
  "id": "lydian",
  "name": "Lydian",
  "group": "modes",
  "order": 103,
  "intervals": [0, 2, 4, 6, 7, 9, 11],
  "degrees": ["1", "2", "3", "#4", "5", "6", "7"]
}
```

- `id`: unique and stable. Saved settings store it, so don't rename it.
- `name`: shown after the key, e.g. "A Lydian".
- `group`: any text. The menu has one section per group.
- `intervals`: semitones above the root. Start at 0, go up, stay below 12.
- `order` (optional, default 1000): lower comes first. Groups are sorted by
  their lowest `order`, then by name. Scales within a group are sorted by
  `order`, then by name. The built-ins use 0–99 for basic, 100s for modes,
  200s for minor and 300s for exotic.
- `degrees` (optional): one name per interval, spelled with `b` and `#`. Without
  it, degrees fall back to 1 b2 2 b3 3 4 b5 5 b6 6 b7 7.

## Adding chords and decks

Individual chord cards live in `crates/fretwise-core/chords/` (compiled in) and
`%APPDATA%\Fretwise\chords\` (loaded at startup):

```json
{
  "id": "em-open",
  "name": "Em",
  "frets": [0, 2, 2, 0, 0, 0],
  "fingers": [0, 2, 3, 0, 0, 0]
}
```

- `frets`: 6 strings from low E (string 6) to high E (string 1). `null` is a
  muted string, `0` is open, `1..=15` is a fret.
- `fingers` (optional): `0` for muted/open, `1` (index) to `4` (pinky).
- `barre` (optional): fret barred by the index finger.

Decks live in `crates/fretwise-core/decks/` and `%APPDATA%\Fretwise\decks\` and
reference scale ids, chord ids, and/or sub-deck ids. When a deck combines
overlapping sub-decks, each card is included once and shares its SM-2 progress
across all decks:

```json
{
  "id": "rock-metal",
  "name": "rock & metal",
  "group": "collections",
  "order": 204,
  "decks": ["power-chords", "barre-chords"],
  "scales": ["minor-pentatonic", "blues", "natural-minor", "harmonic-minor", "phrygian", "phrygian-dominant"]
}
```

## UI widgets

The knobs, segmented buttons, switches, LED meter and fretboard are painted
with plain egui in `crates/fretwise-ui/src/widgets/`; only the icons and the
logo are bundled, as SVG. Existing egui knob crates were considered but target newer egui
than the 0.31 this workspace pins (to match `nih_plug_egui`): `egui_knob`
0.6 needs egui 0.36, `armas-audio` 0.2 needs 0.33.

The prompt terminal under the neck uses **Monocraft** v4.2.1, a
Minecraft-style monospace font by Idrees Hassan
(https://github.com/IdreesInc/Monocraft), licensed under the SIL Open Font
License 1.1. Only the regular `Monocraft.ttf` is bundled, unmodified, in
`crates/fretwise-ui/assets/fonts/` next to its licence (`Monocraft-OFL.txt`).

The icons (flame, eighth note, gauge, chart, settings, chevron, skip) are
from **Lucide** (https://lucide.dev), ISC licence; the licence is in
`crates/fretwise-ui/assets/icons/Lucide-LICENSE.txt`. Their SVG files sit
unmodified next to it, taken from `lucide-static` 1.61.0, and are compiled
in and drawn by `egui_extras`' SVG loader (resvg), as is the logo
(`assets/logo/fretwise-logo.svg`) in the About window.

### ASIO (optional)

WASAPI is the default. This app doesn't pass your guitar through (it only
plays a short chime on the default output), so the interface's direct
monitoring handles what you hear. Most of the display
delay comes from the pitch-detection window (~15–30 ms), so ASIO doesn't change
much yet. To build with ASIO anyway (needs LLVM installed; cpal's build script
downloads the Steinberg ASIO SDK unless `CPAL_ASIO_DIR` points at one):

```
cargo run --release -p fretwise-app --features asio
```

## Architecture

```
fretwise-core   no audio I/O, no GUI
  theory        notes, tunings, positions
  pitch         YIN pitch detector, allocation-free          <- audio thread
  fft, poly     FFT and polyphonic (chord) detector          <- audio thread
  analyzer      Analyzer::push(sample) -> SharedPitch (atomics)  <- audio thread
  chime         the correct-answer ding, allocation-free     <- audio thread
  stabilizer    jittery readings -> "holding note X"         <- GUI thread
  scale, chord, deck
                registries built from scales/, chords/ and decks/*.json
                (compiled in, plus user folders); registry has the shared loader
  trainer       note trainer: challenges, judging, stats     <- GUI thread
  fingering     which finger plays each spot of a pattern
  chord         also the chord drill and ChordListener (strums, arpeggios)
  srs, learn    SM-2 cards and the Learn trainer             <- GUI thread

fretwise-ui     plain egui views: FretwiseUi::show(ui, PitchReading)
  lib           state, saving, the frame loop
  learn, notes, chords, tuner, stats, header, settings, browser
                one module per screen; parts has what they share
  widgets/      custom-painted fretboard, knob, controls, meters
  theme, icons  colours, fonts, SVG icons

fretwise-app    cpal input and chime output, eframe window, state.json
                (the only host-specific crate)
```

The seam is the same one a plugin uses: something owns the audio callback and
calls `Analyzer::push`, and something owns an egui context and calls
`FretwiseUi::show` with `SharedPitch::load()`. A VST3/CLAP build would be a
fourth crate, `fretwise-plugin`, using [nih-plug](https://github.com/robbert-vdh/nih-plug):

- `Plugin::process`: downmix the buffer to mono, `analyzer.process(&mono)`,
  pass audio through unchanged.
- `Plugin::editor`: `nih_plug_egui::create_egui_editor`, calling
  `FretwiseUi::show` inside a `CentralPanel`, and passing
  `FretwiseUi::gate_db()` to `SharedPitch::set_gate_db` each frame, as the
  app does.

One constraint: `fretwise-ui`'s egui version has to match the one
`nih_plug_egui` pins, so check that before adding the plugin crate.
