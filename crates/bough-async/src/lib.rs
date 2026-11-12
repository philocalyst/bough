#![feature(async_iterator)]
//! bough's async core, with no platform and no GUI in it: what a question, an action and a flight are, how the
//! questions one turn asks are published together, the clock, and work for other cores. The façade (`bough`)
//! binds it to cells, wires and descriptions; the model checker (`tests/model.rs`) binds it to plain integers
//! and checks the publication rules over random and exhaustive schedules.
pub mod combin;
pub mod compute;
pub mod engine;
pub mod time;

/// Maps keyed by addresses (cells, heads): one multiply per lookup instead of SipHash. The keys are not attacker
/// controlled (they are this process's own pointers), so a keyed hash buys nothing here.
pub mod addr {
    use std::hash::{BuildHasherDefault, Hasher};

    #[derive(Default, Clone, Copy)]
    pub struct AddrHasher(u64);
    impl Hasher for AddrHasher {
        #[inline]
        fn finish(&self) -> u64 {
            self.0
        }
        #[inline]
        fn write(&mut self, b: &[u8]) {
            for &x in b {
                self.0 = (self.0.rotate_left(5) ^ x as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
            }
        }
        #[inline]
        fn write_usize(&mut self, n: usize) {
            // Fibonacci hashing: the high bits of the product (hashbrown takes its 7-bit tag from the top)
            self.0 = (n as u64 ^ self.0).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        #[inline]
        fn write_u64(&mut self, n: u64) {
            self.write_usize(n as usize)
        }
        #[inline]
        fn write_u32(&mut self, n: u32) {
            self.write_usize(n as usize)
        }
    }
    pub type AddrMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<AddrHasher>>;
}

pub use combin::{race, yield_now};
pub use compute::{compute, compute_stream, Cancel, Report};
pub use engine::{AId, Engine, Landing, Out, QId, QOpts, Question, Step, Work};
pub use time::{now, sleep, timeout};
