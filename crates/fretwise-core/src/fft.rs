//! A small in-place radix-2 FFT, enough for the chord detector. Tables are
//! built once in [`Fft::new`]; [`Fft::process`] doesn't allocate, so it's
//! safe on the audio thread.

use std::f32::consts::TAU;

pub struct Fft {
    n: usize,
    /// `e^(-2πik/n)` for `k` in `0..n/2`, as (re, im).
    twiddles: Vec<(f32, f32)>,
    /// Bit-reversed index of each position.
    reversed: Vec<usize>,
}

impl Fft {
    /// An FFT of `n` points; `n` must be a power of two.
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && n >= 2, "FFT size must be a power of two");
        let bits = n.trailing_zeros();
        let twiddles = (0..n / 2)
            .map(|k| {
                let a = -TAU * k as f32 / n as f32;
                (a.cos(), a.sin())
            })
            .collect();
        let reversed = (0..n).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect();
        Self { n, twiddles, reversed }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Transform `re` + i·`im` in place. Both must be [`Self::len`] long.
    pub fn process(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.n;
        assert!(re.len() == n && im.len() == n);
        for i in 0..n {
            let j = self.reversed[i];
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = self.twiddles[k * step];
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_lands_in_its_bin() {
        let n = 256;
        let fft = Fft::new(n);
        let mut re: Vec<f32> = (0..n).map(|i| (TAU * 10.0 * i as f32 / n as f32).cos()).collect();
        let mut im = vec![0.0; n];
        fft.process(&mut re, &mut im);
        let mag: Vec<f32> = re.iter().zip(&im).map(|(r, i)| (r * r + i * i).sqrt()).collect();
        assert!((mag[10] - n as f32 / 2.0).abs() < 1e-2, "{}", mag[10]);
        assert!((mag[n - 10] - n as f32 / 2.0).abs() < 1e-2);
        for (k, m) in mag.iter().enumerate() {
            if k != 10 && k != n - 10 {
                assert!(*m < 1e-2, "bin {k}: {m}");
            }
        }
    }

    #[test]
    fn matches_a_direct_dft() {
        let n = 64;
        let fft = Fft::new(n);
        let x: Vec<f32> = (0..n).map(|i| ((i * 7 + 3) % 11) as f32 - 5.0).collect();
        let mut re = x.clone();
        let mut im = vec![0.0; n];
        fft.process(&mut re, &mut im);
        for k in 0..n {
            let (mut dr, mut di) = (0.0f32, 0.0f32);
            for (t, &v) in x.iter().enumerate() {
                let a = -TAU * (k * t) as f32 / n as f32;
                dr += v * a.cos();
                di += v * a.sin();
            }
            assert!((re[k] - dr).abs() < 1e-3 && (im[k] - di).abs() < 1e-3, "bin {k}");
        }
    }
}
