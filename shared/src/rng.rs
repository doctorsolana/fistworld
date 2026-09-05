//! Deterministic RNG shared by simulation code.

/// Tiny deterministic RNG (xorshift64*, no external deps).
///
/// Seeded results keep simulation fixtures and procedural choices reproducible.
/// Multiplayer decisions still belong to the authoritative server.
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
}
