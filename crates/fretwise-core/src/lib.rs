//! Fretwise core: realtime-safe pitch analysis and host-agnostic training logic.
//!
//! Nothing in this crate touches audio devices or a GUI toolkit. The audio side
//! ([`Analyzer`]) is fed samples by whoever owns the audio callback — the
//! standalone app's cpal stream today, a plugin's `process()` tomorrow — and
//! publishes results through lock-free [`SharedPitch`]. The training side
//! ([`Trainer`], [`NoteStabilizer`]) runs on the GUI thread.

pub mod analyzer;
pub mod pitch;
pub mod stabilizer;
pub mod theory;
pub mod trainer;

pub use analyzer::{Analyzer, AnalyzerConfig, PitchReading, SharedPitch};
pub use pitch::{PitchEstimate, Yin};
pub use stabilizer::{NoteStabilizer, StableNote};
pub use theory::{Note, NoteReading, Position, Tuning};
pub use trainer::{Challenge, Feedback, PositionStats, Progress, PromptStyle, Trainer, TrainerConfig};
