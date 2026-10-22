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

Pick the interface and the input channel your guitar is plugged into in the
top bar. Watch the level meter: it should move when you play.

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
