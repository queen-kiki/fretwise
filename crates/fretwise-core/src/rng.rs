//! A small seeded random number generator and weighted picking, shared by
//! the trainers to choose what to ask next.

/// xorshift64*: fast, seedable, and plenty for picking prompts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // The state must never be zero.
        Self(seed | 1)
    }

    /// Uniform in `0.0..1.0`.
    pub fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    /// An index into `weights`, each chosen in proportion to its weight;
    /// `None` if there are none. Draws one number even then.
    pub fn pick_weighted(&mut self, weights: &[f64]) -> Option<usize> {
        let total: f64 = weights.iter().sum();
        let mut pick = self.next_f64() * total;
        for (i, &w) in weights.iter().enumerate() {
            if pick < w {
                return Some(i);
            }
            pick -= w;
        }
        weights.len().checked_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_in_range_and_repeats_per_seed() {
        let (mut a, mut b) = (Rng::new(42), Rng::new(42));
        for _ in 0..1000 {
            let x = a.next_f64();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x, b.next_f64());
        }
        // Zero is a valid seed.
        let mut zero = Rng::new(0);
        assert_ne!(zero.next_f64(), zero.next_f64());
    }

    #[test]
    fn weighted_picks_follow_the_weights() {
        let mut rng = Rng::new(7);
        assert_eq!(rng.pick_weighted(&[]), None);
        assert_eq!(rng.pick_weighted(&[0.0, 2.0, 0.0]), Some(1));
        let mut counts = [0; 2];
        for _ in 0..10_000 {
            counts[rng.pick_weighted(&[1.0, 3.0]).unwrap()] += 1;
        }
        // About a quarter and three quarters.
        assert!((2_200..2_800).contains(&counts[0]), "{counts:?}");
    }
}
