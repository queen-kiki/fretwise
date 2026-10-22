# Code review, October 2026

A read-through of all three crates at `4da0e6f`, after several sessions built
features in parallel. Line numbers refer to that commit. Confidence is how
sure I am that the finding is real: **high**, **medium** or **low**.

Each finding ends with its status: **fixed** (with the commit),
**deferred** (left for later, with why), or **left** (the user chose to
keep the current behaviour).

Baseline: `cargo test --workspace` passes (47 tests), and
`cargo clippy --workspace --all-targets` gives one warning
(`needless_range_loop` in `pitch.rs`). After this pass: 55 tests, no
clippy or rustdoc warnings, and before/after screenshots of Learn, free
practice (notes and chords), the tuner, stats, settings, About and the
deck browser are pixel-identical apart from the live input meter.

## Bugs and correctness risks

### B1. A card graded Again never counts as due again (high)

`SrsCard::is_due` (`crates/fretwise-core/src/srs.rs:109`) needs
`reps > 0`, but `review(Grade::Again)` sets `reps = 0` and `due = now + 30s`.
A lapsed card (missed, skipped, or re-graded with `1`) is then neither new
(`due == 0`) nor ever due. So:

- `LearnTrainer::deck_counts` (`learn.rs:325`) leaves it out of "to go", and
  the header bar under-counts.
- `learn_done` (`fretwise-ui/src/lib.rs:471`) can show "all caught up" while
  a skipped card is waiting 30 seconds.
- The deck browser (`lib.rs:1597`) shows it as "Learning · 30s" forever,
  and `suggest_deck` (`lib.rs:497`) ignores it.

The weighted picker (`SrsCard::weight`) still treats it as overdue, so only
the counts and screens are wrong. Proposed fix: due means "has a due time
and it has passed", `self.due > 0.0 && self.due <= unix_now`.

**Fixed** at the user's request (`e6d2d77`), with tests.

### B2. Keys 1–4 re-grade the previous card at any time (medium)

`FretwiseUi::show` (`lib.rs:240`) calls `LearnTrainer::regrade` on every
`1`–`4` press on the Learn screen, and `regrade` (`learn.rs:343`) works
whenever there is a `last_review`. The docs (`srs.rs:6`, README) say the
keys override the grade "during the post-answer pause", but a key pressed
while playing the next card silently rewrites the previous card's schedule.
After a skip, the skipped card can also be re-graded from Again to Easy.
Proposed fix: only re-grade while the trainer is resolved (the pause after a
completed card).

**Fixed** at the user's request (`9dc70a5`), with a test.

### B3. "Reset progress" leaves chord-drill stats (medium)

The Stats screen's reset (`lib.rs:1671`) clears the note trainer and Learn,
but not `ChordTrainer`'s per-chord stats, streak and best streak, which are
saved in `state.json` (`UiState.chord_progress`). Whether that's intended
is the user's call. Proposed fix: reset the chord drill too.

**Fixed** at the user's request (`9c3c2dc`), with a test.

### B4. Possible heap allocation on the audio thread (low)

`PolyDetector::analyze` (`poly.rs:240`) sorts with `sort_by`, a stable sort
that may allocate a scratch buffer for long inputs. Today the note list is
a handful of peaks, so it stays on the stack in practice, but nothing
bounds it below the allocation threshold. Proposed fix: `sort_unstable_by`,
which never allocates; only the order of exactly equal amplitudes could
differ.

**Fixed** at the user's request (`30eaea8`).

### B5. Saved-state robustness (checked, no bug)

- Fret ranges are clamped on load (`lib.rs:180`, `lib.rs:189`); string
  indices in stats are filtered (`trainer.rs:146`); unknown scale, chord
  and deck ids fall back to defaults with a message; a corrupt file is moved
  to `state.json.bad`. All with tests except the chord and deck fallbacks.
- `Scale.root` is not validated on load. Every use takes `% 12` except
  `Scale::notes_string` (`scale.rs:366`), where `root + semi` can overflow
  `u8` in a debug build. It's dead code; removing it removes the risk.
- A second corrupt file overwrites the first `state.json.bad`
  (`state.rs:486`). Deferred: harmless, and a rotation scheme is a feature.
- Realtime paths (`Analyzer::push`, `PolyDetector::push`, `Yin::detect`,
  `Fft::process`, `Chime::next_sample`, both cpal callbacks in
  `fretwise-app/src/audio.rs:373` and `:396`) don't lock or allocate, apart
  from B4. The error callbacks print, which is fine off the audio path.

**Fixed** where noted: `notes_string` was removed (`fda5fda`). The
`state.json.bad` overwrite is **deferred**.

## Structure

### S1. Chord listening is written out twice (high)

`ChordTrainer::update` (`chord.rs:555`) and `LearnTrainer::update_chord`
(`learn.rs:507`) are the same 90 lines: poly gate, per-class hold, ringing
mask, strum window, heard strings, and the constants `STRUM_WINDOW`,
`POLY_HOLD`, `CHORD_GATE_DB` (also in `learn.rs:144`). The reset of that
state is repeated in `next` and `select_*` of both.

**Fixed** (`eb14321`): `ChordListener` in `chord.rs`, with a new test for
Learn's chord cards.

### S2. Random picking and the RNG are written out three times (high)

`next_f64` (xorshift64*) is copied in `trainer.rs:409`, `chord.rs:667` and
`learn.rs:661`, and the weighted pick loop around it in all three.

**Fixed** (`783cf24`): `rng.rs`, with tests.

### S3. Loading a folder of JSON files is written out three times (high)

`scale::init`, `chord::init` and `deck::init` share the same directory
walk, `builtin_sources` and duplicate-id check; `scale.rs` and `deck.rs`
also share the group sort.

**Fixed** (`1b73b15`): `registry.rs`, with tests.

### S4. A scale's box and walk are worked out twice (high)

`LearnTrainer::scale_sequence` (`learn.rs:631`) repeats
`Trainer::fingering_positions`, `candidates` and `sequence`
(`trainer.rs:239`–`279`), minus the string filter.

**Fixed** (`bcdaff7`): `Scale::positions`, `Scale::trim_to_octaves` and
`scale::walk`, with tests.

### S5. `fretwise-ui/src/lib.rs` is 1870 lines (high)

Every screen lives in one `impl FretwiseUi`. The three terminals
(`terminal`, `chords_terminal`, `learn_terminal`) each repeat the same frame
and blinking cursor; the three tabs repeat the rack panel frame and the
neck-plus-terminal layout; the scale-pattern colours and dot sizes are
written twice (`lib.rs:757`, `lib.rs:1175`).

**Fixed**: shared helpers (`c69b723`), then one module per screen
(`a40020b`); `lib.rs` is now 350 lines.

### S6. `widgets.rs` mixes unrelated widgets (medium)

820 lines: the fretboard and its range strip, the knob, buttons, meters,
the dropdown, sparkles and icons.

**Fixed** (`c6ef07d`): `widgets/` with `fretboard`, `knob`, `controls` and
`meters`.

### S7. Colours are defined in several places (high)

`GOOD`, `BAD`, `HEARD`, `TARGET` live in `lib.rs:29`, but `widgets.rs`
repeats `GOOD` and `HEARD` as literals (`tuner_gauge`, `level_meter`), the
app repeats `BAD` (`main.rs:158`), and the pattern, heat-map and About
colours are literals in `lib.rs`. `widgets::lerp_color` and `theme::lerp`
are near-duplicates (one truncates, one rounds).

**Fixed** (`8163901`): every shared colour is a named constant or function
in `theme.rs`. The knob's own body shades stay local to it. **Deferred**:
merging the two `lerp`s, since switching the knob and toggle to rounding
would shift some pixels by one level.

### S8. Crate boundaries (checked, fine)

`fretwise-core` depends only on serde; `fretwise-ui` only on egui 0.31,
egui_extras and serde, with no eframe. Nothing is in the wrong crate.

Nothing to do.

## Cleanups

### C1. Dead code (high)

- The deck browser's chord-drill branch (`lib.rs:1468`–`1495`,
  `1514`–`1522`, `1566`, `1628`): the browser can only be opened from the
  Learn screen, so `is_chords_tab` is always false. With it,
  `DeckKind::chord_groups` (`deck.rs:273`) is unused.
- Unused API: `Scale::notes_string`, `ChordVoicing::notes`,
  `ChordTrainer::stats`, `Grade::key_number`.
- `ChordTrainerConfig.deck` no longer drives anything but must stay for
  saved state; its doc says so.

**Fixed**: the browser branch and `chord_groups` (`51e3e6b`), the unused
API (`fda5fda`). `ChordTrainerConfig.deck` is kept, since saved state needs
it; its doc says so.

### C2. Stale comments and docs (high)

- `srs.rs:3` says SM-2 state is stored in `PositionStats`; it's in
  `LearnProgress.cards`. `srs.rs:102` names `DiagramMode::Auto`, now
  `GuideMode::Auto`.
- `learn.rs:137` gives card keys as `"scale:9:minor-pentatonic"`; they're
  `"scale:minor-pentatonic"`.
- `fingering.rs:5` is one overlong line.
- The README's feature list describes Learn as `Both`/`Scales`/`Chords`
  (now decks), and its architecture sketch is missing `chord`, `deck`,
  `learn`, `srs`, `poly`, `fft`, `fingering` and `chime`.

**Fixed**: code comments (`fda5fda`), README (`3e7ffb4`).

### C3. Magic numbers (medium)

The pause after a correct answer (0.6 s, 0.75 s, 1.1 s), the 0.2 s
release in chord listening, the 35-cent tolerance and the 2 s feedback
display time are bare literals. Named where they're touched by S1/S2.

**Fixed** for the trainers' pauses and chord listening (`eb14321`,
`fda5fda`) and the neck's 15 frets (`c69b723`). **Deferred**: UI layout
sizes, which are design values best named in a design pass.

### C4. Clippy (high)

`needless_range_loop` in `pitch.rs:73`. Pedantic clippy (not enabled)
mostly flags `as` casts on audio math and `must_use`; not worth taking
wholesale.

**Fixed** (`465b959`). Pedantic **deferred**.

### C5. Saved maps aren't sorted (low)

`LearnProgress.cards` and `ChordProgress.stats` are `HashMap`s, so
`state.json` lists them in a random order each run and gets rewritten once
after every start. Note positions are already sorted for clean diffs.
Deferred: harmless, and changing the map type touches the public API.

**Deferred**, as above.

## Left for later

- B5: keep more than one `state.json.bad`.
- S7: one colour `lerp` for the theme and the widgets.
- C3: name the UI's layout sizes in a design pass.
- C4: pedantic clippy.
- C5: sort `LearnProgress.cards` and `ChordProgress.stats` in `state.json`.
- Wide-range fingering, already in `TODO.md`.
