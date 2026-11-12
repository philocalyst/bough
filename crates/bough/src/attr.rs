//! Attribution spans (feature `attr`, off by default; round 7, lane core): instructions retired inside named
//! spans of the turn, by `tinstr` deltas with the counter's own cost subtracted. An instrumented build is for
//! finding where a turn's instructions go; its totals are never reported as a result.
#[cfg(feature = "attr")]
pub mod on {
    use std::cell::Cell;
    pub const N: usize = 12;
    thread_local! {
        pub static ACC: [Cell<u64>; N] = const { [const { Cell::new(0) }; N] };
        pub static CNT: [Cell<u64>; N] = const { [const { Cell::new(0) }; N] };
    }
    /// 0: outer spans only; 1 (`BOUGH_ATTR_NEST`): nested spans too.
    pub fn level() -> usize {
        thread_local!(static L: usize = std::env::var_os("BOUGH_ATTR_NEST").is_some() as usize);
        L.with(|l| *l)
    }
    #[inline(always)]
    pub fn now() -> u64 {
        bough_metal::perf::tinstr()
    }
    #[inline(always)]
    pub fn add(i: usize, t0: u64) {
        let d = now() - t0;
        ACC.with(|a| a[i].set(a[i].get() + d));
        CNT.with(|a| a[i].set(a[i].get() + 1));
    }
    /// (instructions, spans) per slot since the last call; the caller subtracts `tinstr_overhead` per span.
    pub fn take() -> [(u64, u64); N] {
        let mut o = [(0, 0); N];
        ACC.with(|a| CNT.with(|c| {
            for i in 0..N {
                o[i] = (a[i].replace(0), c[i].replace(0));
            }
        }));
        o
    }
}

/// Added to the slot of a span nested in another span: it is read only when `BOUGH_ATTR_NEST` is set, so that
/// by default an outer span is not inflated by the counter reads of the spans inside it.
pub const NEST: usize = 100;
/// 1 while reading a whole-stage span whose inner spans are then switched off (`BOUGH_ATTR_OUTER`): unused
/// in normal builds.
pub const OUTER: usize = 1;

/// `span!(slot, expr)`: evaluate `expr`, charging its instructions to `slot` when the `attr` feature is on.
#[macro_export]
#[doc(hidden)]
macro_rules! span {
    ($i:expr, $e:expr) => {{
        #[cfg(feature = "attr")]
        let __on = $crate::attr::on::level() >= ($i) / 100;
        #[cfg(feature = "attr")]
        let __t = if __on { $crate::attr::on::now() } else { 0 };
        let __r = $e;
        #[cfg(feature = "attr")]
        if __on {
            $crate::attr::on::add(($i) % 100, __t);
        }
        __r
    }};
}
