# Fretwise

Guitar learning app for electric guitar through an audio interface.

- **Fretboard trainer**: asks for a note on a string (or shows a dot), listens
  for it, and asks more often about the positions you're slow on or miss.
- **Tuner**: note, cents gauge, and which string to turn which way.
- **Progress**: per-position heatmap of first-try accuracy and average time.

## Run

```
cargo run --release
```

Open settings with the ⚙ button (top right), and pick the interface and the
input your guitar is plugged into. Watch the IN meter next to it: it should
move when you play.

## Using the trainer

- **Key** and **Scale** knobs: drag up/down, scroll, or click for a list.
- **Order** and **Labels**: Random / Up / Up & down, and note names or scale
  degrees.
- **Fret range**: drag the lit strip of fret numbers under the neck. Drag an
  end to resize it, drag the middle to move it, or click a fret outside it.
- **Strings**: click a string name left of the neck to leave it out.
- Everything else (audio device, tuning, A4, prompt style, scale pattern,
  chime, theme) is in the ⚙ settings panel. Esc closes it.
- The colours are soft pinks in light and dark mode, with the trans pride
  blue, pink and white as accents. The flag next to "About" in settings opens
  the About window (version and build date).

## UI widgets

The knobs, segmented buttons, switches, LED meter and fretboard are painted
with plain egui in `crates/fretwise-ui/src/widgets.rs`; no image assets are
bundled. Existing egui knob crates were considered but target newer egui
than the 0.31 this workspace pins (to match `nih_plug_egui`): `egui_knob`
0.6 needs egui 0.36, `armas-audio` 0.2 needs 0.33.

### ASIO (optional)

WASAPI is the default. This app only listens and doesn't play audio back, so
the interface's direct monitoring handles what you hear. Most of the display
delay comes from the pitch-detection window (~15–30 ms), so ASIO doesn't change
much yet. To build with ASIO anyway (needs LLVM installed; cpal's build script
downloads the Steinberg ASIO SDK unless `CPAL_ASIO_DIR` points at one):

```
cargo run --release -p fretwise-app --features asio
```

## Architecture

```
fretwise-core   no I/O, no GUI
  theory        notes, tunings, positions
  pitch         YIN pitch detector, allocation-free
  analyzer      Analyzer::push(sample) -> SharedPitch (atomics)  <- audio thread
  stabilizer    jittery readings -> "holding note X"             <- GUI thread
  trainer       challenges, judging, stats, weighted selection   <- GUI thread

fretwise-ui     plain egui views: FretwiseUi::show(ui, PitchReading)

fretwise-app    cpal input stream + eframe window (the only host-specific crate)
```

The seam is the same one a plugin uses: something owns the audio callback and
calls `Analyzer::push`, and something owns an egui context and calls
`FretwiseUi::show` with `SharedPitch::load()`. A VST3/CLAP build would be a
fourth crate, `fretwise-plugin`, using [nih-plug](https://github.com/robbert-vdh/nih-plug):

- `Plugin::process`: downmix the buffer to mono, `analyzer.process(&mono)`,
  pass audio through unchanged.
- `Plugin::editor`: `nih_plug_egui::create_egui_editor`, calling
  `FretwiseUi::show` inside a `CentralPanel`.

One constraint: `fretwise-ui`'s egui version has to match the one
`nih_plug_egui` pins, so check that before adding the plugin crate.
