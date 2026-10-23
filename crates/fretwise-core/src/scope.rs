//! A stereo oscilloscope tap: the audio thread records the rack's output and
//! the GUI reads back the latest stretch of it to draw.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::fx::StereoEffect;

/// Lock-free history of the most recent stereo frames, written by one audio
/// thread and read by any number of GUI frames. A read can race a write and
/// see a few frames from a moment later; on a scope that never shows.
pub struct SharedScope {
    left: Box<[AtomicU32]>,
    right: Box<[AtomicU32]>,
    mask: usize,
    /// Frames written so far (wrapping); the next frame goes at `write & mask`.
    write: AtomicUsize,
    /// The recording's sample rate in Hz, or 0 before the stream starts.
    sample_rate: AtomicU32,
}

impl Default for SharedScope {
    /// Room for at least 200 ms at 96 kHz.
    fn default() -> Self {
        Self::new(1 << 15)
    }
}

impl SharedScope {
    /// Keep the latest `capacity` frames (rounded up to a power of two).
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(64).next_power_of_two();
        let lane = || (0..cap).map(|_| AtomicU32::new(0)).collect::<Vec<_>>().into_boxed_slice();
        Self {
            left: lane(),
            right: lane(),
            mask: cap - 1,
            write: AtomicUsize::new(0),
            sample_rate: AtomicU32::new(0),
        }
    }

    /// Record one frame (audio thread only).
    #[inline]
    pub fn push(&self, left: f32, right: f32) {
        let w = self.write.load(Ordering::Relaxed);
        self.left[w & self.mask].store(left.to_bits(), Ordering::Relaxed);
        self.right[w & self.mask].store(right.to_bits(), Ordering::Relaxed);
        self.write.store(w.wrapping_add(1), Ordering::Release);
    }

    pub fn set_sample_rate(&self, hz: u32) {
        self.sample_rate.store(hz, Ordering::Relaxed);
    }

    /// The recording's sample rate in Hz, or 0 before anything was recorded.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// The most frames [`SharedScope::latest`] can return.
    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// Replace `out` with the latest `frames` (left, right) pairs, oldest
    /// first, and fewer if not that many were recorded yet.
    pub fn latest(&self, frames: usize, out: &mut Vec<(f32, f32)>) {
        let w = self.write.load(Ordering::Acquire);
        // Leave a little slack: the writer may be overwriting the oldest slots.
        let n = frames.min(w).min(self.capacity() - self.capacity() / 8);
        out.clear();
        out.extend((w - n..w).map(|i| {
            let i = i & self.mask;
            (
                f32::from_bits(self.left[i].load(Ordering::Relaxed)),
                f32::from_bits(self.right[i].load(Ordering::Relaxed)),
            )
        }));
    }
}

/// A pass-through [`StereoEffect`] that records what reaches it into a
/// [`SharedScope`]; put it last in the chain to watch the rack's output.
pub struct ScopeTap {
    scope: Arc<SharedScope>,
}

impl ScopeTap {
    pub fn new(sample_rate: f32, scope: Arc<SharedScope>) -> Self {
        scope.set_sample_rate(sample_rate as u32);
        Self { scope }
    }
}

impl StereoEffect for ScopeTap {
    fn name(&self) -> &str {
        "Scope Tap"
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.scope.set_sample_rate(sample_rate as u32);
    }

    fn reset(&mut self) {}

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (&l, &r) in left.iter().zip(right.iter()) {
            self.scope.push(l, r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_returns_the_latest_frames_in_order() {
        let scope = SharedScope::new(1024);
        let mut out = Vec::new();
        scope.latest(10, &mut out);
        assert!(out.is_empty());

        for i in 0..3000 {
            scope.push(i as f32, -(i as f32));
        }
        scope.latest(4, &mut out);
        assert_eq!(out, [(2996.0, -2996.0), (2997.0, -2997.0), (2998.0, -2998.0), (2999.0, -2999.0)]);
        // Never more than the ring safely holds.
        scope.latest(5000, &mut out);
        assert_eq!(out.len(), 1024 - 128);
        assert_eq!(out.last(), Some(&(2999.0, -2999.0)));
    }

    #[test]
    fn tap_passes_audio_through() {
        let scope = Arc::new(SharedScope::new(256));
        let mut tap = ScopeTap::new(48_000.0, scope.clone());
        let (mut l, mut r) = (vec![0.25, 0.5], vec![-0.25, -0.5]);
        tap.process(&mut l, &mut r);
        assert_eq!((l, r), (vec![0.25, 0.5], vec![-0.25, -0.5]));
        assert_eq!(scope.sample_rate(), 48_000);
        let mut out = Vec::new();
        scope.latest(2, &mut out);
        assert_eq!(out, [(0.25, -0.25), (0.5, -0.5)]);
    }
}
