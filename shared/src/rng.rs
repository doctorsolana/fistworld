//! Deterministic RNG shared by simulation code.

/// Tiny deterministic RNG (xorshift64*, no external deps).
///
/// Deterministic and dependency-free, which matters if the unit simulation later
/// moves to lockstep networking where every client must produce identical results.
#[derive(Clone, Copy, Debug)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub fn next_f32(&mut self) -> f32 {
        // Use 24 bits of mantissa precision (matches f32 mantissa size).
        let v = (self.next_u64() >> 40) as u32;
        (v as f32) / ((1u32 << 24) as f32)
    }
}
