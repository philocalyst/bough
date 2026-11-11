//! Tiny xorshift64* RNG (no dependencies).

#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        ((self.next_u64() >> 32) * n) >> 32
    }
    #[inline]
    pub fn usize_below(&mut self, n: usize) -> usize {
        self.below(n as u64) as usize
    }
    /// Uniform in lo..=hi.
    #[inline]
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u64) as i32
    }
    #[inline]
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }
}
