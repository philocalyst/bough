//! Table-driven stream compaction: the cull's output is a table lookup, not a branchy loop.
//!
//! A group of 8 records produces an 8-bit hit mask. `COMPACT[mask]` is the byte shuffle that moves
//! the payloads of the set lanes to the front of a 32-byte vector, so emission is two `tbl`
//! (table lookups over the 32 payload bytes), two 16-byte stores and a pointer bump by `popcount`,
//! with no per-record branch. The stores may write up to 8 slots past the new end, so the caller
//! keeps `capacity - len >= 8` (pages reserve 64 up front).

/// `COMPACT[m]`: for the j-th set bit `i` of `m`, bytes `[4j, 4j+4)` are `[4i, 4i+1, 4i+2, 4i+3]`;
/// the rest are 0xFF (out of range for `tbl`, which yields 0 there).
pub static COMPACT: [[u8; 32]; 256] = build();

const fn build() -> [[u8; 32]; 256] {
    let mut t = [[0xFFu8; 32]; 256];
    let mut m = 0;
    while m < 256 {
        let mut j = 0;
        let mut i = 0;
        while i < 8 {
            if m & (1 << i) != 0 {
                let mut b = 0;
                while b < 4 {
                    t[m][4 * j + b] = (4 * i + b) as u8;
                    b += 1;
                }
                j += 1;
            }
            i += 1;
        }
        m += 1;
    }
    t
}

/// Append `src[i]` for every set bit `i` of the 8-bit `mask`, in order; returns the new end.
///
/// # Safety
/// `src` valid for reads of 8 u32; `out` valid for writes of 8 u32 (the full vector is stored).
#[inline(always)]
pub unsafe fn compact8(src: *const u32, mask: u32, out: *mut u32) -> *mut u32 {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        let p = vld1q_u8_x2(src as *const u8);
        let ix = vld1q_u8_x2(COMPACT.get_unchecked(mask as usize).as_ptr());
        vst1q_u8(out as *mut u8, vqtbl2q_u8(p, ix.0));
        vst1q_u8((out as *mut u8).add(16), vqtbl2q_u8(p, ix.1));
        out.add(mask.count_ones() as usize)
    }
    #[cfg(not(target_arch = "aarch64"))]
    unsafe {
        let mut o = out;
        let mut m = mask;
        while m != 0 {
            let i = m.trailing_zeros() as usize;
            m &= m - 1;
            *o = *src.add(i);
            o = o.add(1);
        }
        o
    }
}
