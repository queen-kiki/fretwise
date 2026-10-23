//! Fretwise core: realtime-safe pitch analysis and host-agnostic training logic.
//!
//! Nothing in this crate touches audio devices or a GUI toolkit. The audio side
//! ([`Analyzer`]) is fed samples by whoever owns the audio callback — the
//! standalone app's cpal stream today, a plugin's `process()` tomorrow — and
//! publishes results through lock-free [`SharedPitch`]. The training side
//! ([`Trainer`], [`NoteStabilizer`]) runs on the GUI thread.

pub mod analyzer;
pub mod chime;
pub mod chord;
pub mod chorus;
pub mod deck;
pub mod fft;
pub mod fingering;
pub mod fx;
pub mod gate;
pub mod jam;
pub mod learn;
pub mod pitch;
pub mod poly;
mod registry;
mod rng;
pub mod scale;
pub mod scope;
pub mod srs;
pub mod stabilizer;
pub mod theory;
pub mod trainer;

pub use analyzer::{Analyzer, AnalyzerConfig, PitchReading, SharedPitch, DEFAULT_GATE_DB, GATE_RANGE_DB};
pub use chime::Chime;
pub use chorus::VintageChorus;
pub use chord::{
    ChordChallenge, ChordFeedback, ChordKind, ChordOrder, ChordProgress, ChordTrainer, ChordTrainerConfig,
    ChordFamily, ChordPick, ChordVoicing,
};
pub use deck::{DeckDef, DeckKind};
pub use fingering::fingering;
pub use gate::NoiseGate;
pub use jam::{name_chord, JamHeard, JamListener, JamName};
pub use fx::{
    AmpConfig, Cabinet, CleanAmp, Effect, EffectChain, SampleRing, SharedAmp, StereoEffect, AMP_GAIN_RANGE_DB,
    AMP_LEVEL_RANGE_DB, AMP_TONE_RANGE, CAB_MIC_RANGE, CHORUS_DEPTH_RANGE, CHORUS_MIX_RANGE, CHORUS_RATE_RANGE,
    OUT_GAIN_RANGE_DB,
};
pub use learn::{ActiveCard, GuideMode, LearnCard, LearnConfig, LearnFeedback, LearnProgress, LearnTrainer};
pub use pitch::{PitchEstimate, Yin};
pub use scale::{Scale, ScaleKind};
pub use scope::{ScopeTap, SharedScope};
pub use srs::{format_interval, unix_seconds, Grade, SrsCard};
pub use stabilizer::{NoteStabilizer, StableNote};
pub use theory::{Note, NoteReading, Position, Tuning};
pub use trainer::{
    Challenge, Feedback, Labels, Order, PositionStats, Progress, PromptStyle, Trainer, TrainerConfig,
};
