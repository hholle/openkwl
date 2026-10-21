//! Fixed-window moving average.
//!
//! The stock firmware smooths every ADC channel and both tacho signals with an 8-sample
//! ring buffer and divides the running sum by 8, including during warm-up.

/// Moving average over the last `N` samples. Like the stock firmware, the sum is always
/// divided by `N`, so the first `N - 1` results ramp up from zero.
#[derive(Debug, Clone)]
pub struct MovingAverage<const N: usize> {
    buf: [u32; N],
    idx: usize,
    sum: u32,
}

impl<const N: usize> MovingAverage<N> {
    pub const fn new() -> Self {
        assert!(N > 0, "window must not be empty");
        Self { buf: [0; N], idx: 0, sum: 0 }
    }

    /// Adds a sample and returns the new average.
    pub fn push(&mut self, sample: u32) -> u32 {
        self.sum = self.sum - self.buf[self.idx] + sample;
        self.buf[self.idx] = sample;
        self.idx = (self.idx + 1) % N;
        self.average()
    }

    pub fn average(&self) -> u32 {
        self.sum / N as u32
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl<const N: usize> Default for MovingAverage<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_up_then_tracks_window() {
        let mut avg = MovingAverage::<8>::new();
        assert_eq!(avg.push(800), 100);
        for _ in 0..7 {
            avg.push(800);
        }
        assert_eq!(avg.average(), 800);
        for _ in 0..8 {
            avg.push(0);
        }
        assert_eq!(avg.average(), 0);
    }
}
