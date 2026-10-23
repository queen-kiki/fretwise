# <img src="assets/logo/fretwise-logo.svg" width="36" height="36" align="top" alt="" /> Fretwise

[![Release build](https://img.shields.io/github/actions/workflow/status/queen-kiki/fretwise/release.yml?label=release%20build)](https://github.com/queen-kiki/fretwise/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/queen-kiki/fretwise)](https://github.com/queen-kiki/fretwise/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/queen-kiki/fretwise/total)](https://github.com/queen-kiki/fretwise/releases)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-blue)
![Made with Rust](https://img.shields.io/badge/made%20with-Rust-dea584?logo=rust)
[![License: MIT](https://img.shields.io/github/license/queen-kiki/fretwise)](LICENSE)

> [!WARNING]
> ## ⚠️ AI SLOP WARNING ⚠️
>
> This entire project was written by an AI (Claude) in a single chat session, with only light
> human review. The pitch and chord detection are heuristic, and the music theory, scale shapes,
> chord voicings, and fingerings have **not** been independently checked by a guitar teacher or
> anyone who knows what they're doing.
>
> **If a shape feels wrong or hurts your hand:** stop and look up a real chord chart or ask an
> actual teacher before drilling bad habits into muscle memory.

A hands-free practice app for electric guitar. Plug into an audio interface and Fretwise listens while you play—quizzing you on scales, chords, and fretboard notes without making you take your hands off the neck.

## Getting started

### Install

Grab the latest standalone binary from [**Releases**](https://github.com/queen-kiki/fretwise/releases), or build from source with [Rust](https://rustup.rs/):

```sh
cargo run --release
```

### Audio setup

1. Click **⚙** in the top-right corner and select your **audio interface** and the **input channel** your guitar is plugged into.
2. Pluck a string and watch the **IN** meter in the header.
3. Adjust the **Gate** if needed (marked by a tick on the input meter; anything quieter turns grey and is ignored). Lower the gate if quiet notes aren't picked up, or raise it if hum and string noise trigger false notes.

Fretwise only listens to your dry guitar signal and plays a short chime when you hit the right note—use your interface's direct monitoring or an amp sim to hear your guitar.

## What's inside

- **Learn** — Spaced-repetition flashcards (like Anki, but hands-free) for scale shapes and chords. Pick a deck, play what's asked, and cards are scheduled automatically based on your speed and accuracy. Once a card matures, the fretboard diagram hides so you have to recall it from memory.
  - Press `1`–`4` during the pause after a card to override its grade (`Again` / `Hard` / `Good` / `Easy`).
  - Click **Cards** to browse the deck or inspect individual shapes.
- **Free practice**:
  - **Notes** — Drills individual notes or full scale runs in any key and scale, weighting the prompt toward positions you miss or play slowly. Drag the highlighted strip of fret numbers under the neck to lock drills to a specific fret range, or click string numbers on the left (1 = high E, 6 = low E) to mute strings out of the drill.
  - **Chords** — Drills chords by family, single chord, or Learn deck, recognizing both full strums and picked arpeggios. With **Diagram** set to *Auto*, shapes are shown while you learn them and hidden once you play them cleanly a few times in a row.
- **Jam** — Play freely through the amp: the effects rack opens and a screen over the darkened neck names what you play — a single note with its cents, an interval, or a chord (including slash chords and extended voicings), with the last few chords listed underneath.
- **Tuner** — Chromatic tuner with cents gauge and string indicators.
- **Stats** — Fretboard heatmap of your first-try accuracy and average response time across every string and fret.

Press **Space** at any time during a drill to skip the current prompt. **F1**–**F5** switch between Learn, Free practice, Jam, Tuner and Stats; the menu shows each mode's key.

## Custom scales, chords & decks

Fretwise loads user JSON files on startup from `%APPDATA%\Fretwise\`:

- `%APPDATA%\Fretwise\scales\`
- `%APPDATA%\Fretwise\chords\`
- `%APPDATA%\Fretwise\decks\`

See the built-in JSON files in [`crates/fretwise-core/`](crates/fretwise-core/) for examples of each format.

## Credits

- Prompt font: [**Monocraft**](https://github.com/IdreesInc/Monocraft) v4.2.1 by Idrees Hassan (SIL Open Font License 1.1)
- Icons: [**Lucide**](https://lucide.dev) v1.61.0 (ISC License)
