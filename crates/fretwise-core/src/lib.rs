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
pub mod deck;
pub mod fft;
pub mod fingering;
pub mod learn;
pub mod pitch;
pub mod poly;
pub mod scale;
pub mod srs;
pub mod stabilizer;
pub mod theory;
pub mod trainer;

pub use analyzer::{Analyzer, AnalyzerConfig, PitchReading, SharedPitch};
pub use chime::Chime;
pub use chord::{
    ChordChallenge, ChordFeedback, ChordKind, ChordOrder, ChordProgress, ChordTrainer, ChordTrainerConfig,
    ChordVoicing,
};
pub use deck::{DeckDef, DeckKind};
pub use fingering::fingering;
pub use learn::{ActiveCard, GuideMode, LearnCard, LearnConfig, LearnFeedback, LearnProgress, LearnTrainer};
pub use pitch::{PitchEstimate, Yin};
pub use scale::{Scale, ScaleKind};
pub use srs::{format_interval, unix_seconds, Grade, SrsCard};
pub use stabilizer::{NoteStabilizer, StableNote};
pub use theory::{Note, NoteReading, Position, Tuning};
pub use trainer::{
    Challenge, Feedback, Labels, Order, PositionStats, Progress, PromptStyle, Trainer, TrainerConfig,
};
