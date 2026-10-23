# Adding a plugin to the effects rack

How a new device (an effect, or a meter/visualiser like the oscilloscope) gets
into the rack drawer, end to end. It touches three crates; nothing is
discovered at runtime, so each step below is a small edit in a known place.

```
input callback ──► SampleRing ──► EffectChain (output callback) ──► out gain ──► device
 (analyzer too)                    │
                                   ├─ mono:   NoiseGate → CleanAmp → Cabinet
                                   └─ stereo: VintageChorus → ScopeTap ──► SharedScope ──► GUI
GUI knobs ──► AmpConfig ──► SharedAmp (atomics, stored every frame) ──► effects read it per block
```

## 1. Write the DSP in `fretwise-core`

Pick the trait by what the device does to the channel count:

| Trait | Signature | Use for |
|---|---|---|
| `Effect` | `process(&mut self, samples: &mut [f32])` | mono in, mono out (amp, cab, gate, drive, EQ, compressor) |
| `StereoEffect` | `process(&mut self, left: &mut [f32], right: &mut [f32])` | anything that widens or works on L/R (chorus, stereo delay, reverb), and taps/meters |

Both live in [`fx.rs`](../crates/fretwise-core/src/fx.rs). The chain runs every
mono effect, copies the result to both sides, then runs the stereo effects.
A stereo effect therefore always sits after every mono one. A mono effect
can't go after the chorus.

Put a new device in its own module (`chorus.rs`, `gate.rs`, `scope.rs` are the
examples), add `pub mod` and a `pub use` in
[`lib.rs`](../crates/fretwise-core/src/lib.rs). `Biquad` is `pub(crate)` in
`fx.rs` for filters; `rng::Rng` is there for noise.

### Rules for `process` (it runs on the audio callback)

- **No allocation, locking, blocking or I/O.** Size buffers (delay lines etc.)
  in `prepare`, which runs when the chain is built, not per block.
- **Read parameters once per block** with `self.shared.load()` (a handful of
  relaxed atomic loads), not per sample.
- **Recompute coefficients only on change.** `CleanAmp::update_tone` and
  `Cabinet::update_mic` cache the last value and compare.
- **Bypass without clicks.** Either reset state when going disabled
  (`Cabinet`), or better, glide a wet amount to 0 and skip work once it's
  there (`VintageChorus`). Smooth any knob that's audible while turning it
  (one-pole, ~20 ms).
- **Keep output in `-1.0..=1.0`** (`clamp`) and finite.
- `prepare(sample_rate)` can be called again with a new rate; recompute
  everything rate-dependent there.

## 2. Add its settings

Settings the **audio thread** needs go in `AmpConfig` / `SharedAmp`
([`fx.rs`](../crates/fretwise-core/src/fx.rs)). For each one:

1. A field on `AmpConfig` (with a doc comment giving the range) and a default
   in `impl Default for AmpConfig`. The struct is `#[serde(default)]`, so
   older save files simply get the default.
2. A `pub const …_RANGE: RangeInclusive<i32>` if it's a stepped knob.
3. A line in `AmpConfig::sanitized` so a hand-edited save can't load an
   out-of-range value.
4. An atomic on `SharedAmp` (`AtomicBool`, or `AtomicU32` holding
   `f32::to_bits`), plus its line in `Default`, `store` and `load`.
5. Export the range constants from `lib.rs`.

Settings that **only the GUI** uses (the scope's mode/timebase/sensitivity)
don't belong there. Give them their own `Serialize`/`Deserialize` struct with
`#[serde(default)]` and a `sanitized()`, hold it on `FretwiseUi`, and add it to
`UiState`, `from_state` and `state()` in
[`fretwise-ui/src/lib.rs`](../crates/fretwise-ui/src/lib.rs). `ScopeConfig`
is the example.

## 3. Put it in the chain

In `AudioEngine::start_output` in
[`fretwise-app/src/audio.rs`](../crates/fretwise-app/src/audio.rs):

```rust
chain.push(Box::new(MyMonoEffect::new(sr, self.amp.clone())));
chain.push_stereo(Box::new(MyStereoEffect::new(sr, self.amp.clone())));
```

Order of the `push` calls is signal order. The chain is built when the
output stream opens, so a device is always present and is switched with its
`*_enabled` flag rather than added or removed live. The output callback then
sends left/right to the chosen channel pair (or their average to a lone
channel) after the master `out_gain_db`.

A plugin host would call `EffectChain::process_from_ring` (or the effects
directly) from its own `process()`; nothing in core knows about cpal.

## 4. Draw its front plate

In [`fretwise-ui/src/fx_rack.rs`](../crates/fretwise-ui/src/fx_rack.rs):

1. Add the plate to `fx_pane` between two `paint_chain_arrow` calls. An
   arrow is lit when signal flows through it.
2. Write `fn my_plate(&mut self, ui: &mut Ui)` following `chorus_plate`:
   - `allocate_exact_size(vec2(width, CARD_HEIGHT), …)`. Every device is
     `CARD_HEIGHT` tall (236, the square scope's size). Widths so far are
     236 (one knob or the scope) and 412 (art + three knobs).
     `FX_PANE_SPACE` (how much the window grows) follows `CARD_HEIGHT`.
     Centre art and knobs on the body's vertical centre rather than pinning
     them to the top, so they stay balanced if the height changes again.
   - `ShellColors` for dark and light themes, then
     `paint_device_shell(ui, rect, "id_power", active, (TITLE, SUBTITLE), colors)`.
     It returns true when the power button was clicked. Devices after the
     amp only toggle while the amp is on, because the amp head is what
     un-mutes the chain.
   - The art window on the left (88 px wide, inset 14/12 px). Paint it with
     the `Painter`; animate from `ui.input(|i| i.time)`. The app already
     repaints every frame.
   - Knob slots 98 px apart, each a `new_child` with a 92×108 max rect,
     wrapped in `add_enabled_ui(active, …)`. Use `stepped_knob` for an `f32`
     setting over an integer range, `knob` for a `usize` index into labels.
3. If the drawer outgrows the window, it scrolls horizontally.

Knobs aren't the only option. The scope has none: its settings are phosphor
read-outs on the screen that you drag or scroll (`read_out` + `step_by_drag`
in `scope.rs`), plus trace markers you drag to move each trace (double-click
resets one). These are plain `ui.interact` rects over painted text. Give each
a hover state, a cursor and a tooltip, because nothing else tells the player
they can drag it.

**Knob read-outs are a 14-segment display** (`segment14_mask` in
`widgets/plate.rs`): digits, A–Z, `+ - / \ _ # * %`, `d`/`b` for dB/flat, and
`.` as a decimal point on the previous character. Any other character renders
blank, so check a label renders before relying on it.

## 5. Visualisers and meters (GUI reads audio)

The scope shows the pattern for anything the GUI must draw from the audio
thread:

- A lock-free buffer in core (`SharedScope`: atomics, one writer, a write
  counter published with `Release`). The GUI can read a slightly torn
  snapshot, which is fine for display.
- A pass-through `StereoEffect` (`ScopeTap`) at the point in the chain you
  want to watch, so it is placed like any other device.
- The engine owns the `Arc` (`AudioEngine::scope`), and the app hands it to
  the UI once (`FretwiseUi::set_scope` in `App::new`). The UI must cope with
  `None` and with no data yet (the scope free-runs over silence).
- For a single number (a level), an atomic on `SharedAmp` is enough
  (`out_level_db`).

## 6. Test it

Core effects are plain structs; test them without audio devices (see the
tests at the bottom of `fx.rs`, `chorus.rs`, `gate.rs`, `scope.rs`):

- bypassed or disabled means the signal is untouched or silent, as intended
- the DSP does its job on a synthetic sine (cab rolls off 10 kHz, chorus
  makes L ≠ R, gate mutes −66 dBFS hum and passes a −20 dBFS note)
- output stays finite and within ±1

`monitoring_survives_callback_jitter` covers the ring/resampler and needn't
change. GUI-side logic that doesn't need egui (the scope's trigger and
trace capture) can be unit-tested in the UI crate too.

Then run the app and look at it: `cargo run --bin fretwise`, open the rack
with the amp icon in the header.

## Gotchas found along the way

- `Stroke::new(1.0, …)` with a bare float now warns
  (`float_literal_f32_fallback`); the codebase writes `1.0_f32`.
- Additive glow: `color.gamma_multiply(a).additive()` adds light instead of
  covering. The CRT beam and phosphor use it.
- The input gate (`SharedPitch::gate_db`, set on the IN meter) is shared by
  pitch detection and the `NoiseGate` at the head of the chain. Read it
  from `SharedPitch` rather than duplicating it in `AmpConfig`.
- `CleanAmp` outputs silence while the amp is off, so every device after it
  sees zeros then. Its UI shows it as inactive.

## Checklist

- [ ] DSP struct implementing `Effect` or `StereoEffect`, in its own core module, exported
- [ ] Settings: `AmpConfig` field + default + range + `sanitized` + `SharedAmp` atomic (or a UI-only config in `UiState`)
- [ ] `chain.push` / `push_stereo` in `start_output`, in signal order
- [ ] Front plate in `fx_rack.rs`, wired into `fx_pane` with arrows
- [ ] Unit tests for bypass, the effect itself, and bounds
- [ ] `cargo test --workspace`, then a look in the running app (light and dark)
