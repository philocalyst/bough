//! The tile pool: one anonymous mapping, reserved as address space, holding a prefix (the legacy image atlas of
//! icons and gradient ramps, `prefix` bytes) and then slots of [`crate::SLOT_BYTES`] each. A slot's pages become
//! resident when it is first written and are given back to the system when it is freed, so what the pool costs
//! is the slots in use, whatever its reservation.
//!
//! The kernel reads the pool as one image atlas: texel index `t` is bytes `4 t .. 4 t + 4`; slot `s` starts at
//! texel [`Pool::texel0`]`(s)` and is 64 texels wide (`stride` 64), so each slot is a contiguous 64 x 64 image.
//!
//! All writes go through `&mut self` (the owner's thread), so [`Pool::bytes`] can hand out a plain slice.

use crate::{SLOT_BYTES, SLOT_TEXELS};
use std::ptr::NonNull;

pub struct Pool {
    base: NonNull<u8>,
    /// Bytes reserved (prefix + slot capacity).
    reserved: usize,
    prefix: usize,
    /// Slots ever handed out (the high-water mark); slots below it are in use or free.
    hwm: u32,
    cap: u32,
    free: Vec<u32>,
    /// Slots given back to the system (their pages are not resident until written again).
    pub released: u64,
    /// Bytes of the prefix that were ever written (its high-water mark).
    prefix_hwm: usize,
}

// SAFETY: the mapping is owned by the pool and reached only through `&self` (reads) and `&mut self` (writes).
unsafe impl Send for Pool {}

#[cfg(unix)]
fn map(len: usize) -> NonNull<u8> {
    #[cfg(target_os = "linux")]
    let flags = libc::MAP_ANON | libc::MAP_PRIVATE | libc::MAP_NORESERVE;
    #[cfg(not(target_os = "linux"))]
    let flags = libc::MAP_ANON | libc::MAP_PRIVATE;
    // SAFETY: a fresh anonymous private mapping; the kernel picks the (page-aligned) address.
    let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, flags, -1, 0) };
    assert!(p != libc::MAP_FAILED, "bough-media: reserving {len} bytes of address space failed");
    NonNull::new(p as *mut u8).expect("mapping")
}

impl Pool {
    /// A pool of `prefix` bytes (rounded up to a slot) and up to `slots` slots.
    pub fn new(prefix: usize, slots: u32) -> Pool {
        let prefix = prefix.next_multiple_of(SLOT_BYTES);
        let reserved = prefix + slots as usize * SLOT_BYTES;
        Pool { base: map(reserved.max(SLOT_BYTES)), reserved, prefix, hwm: 0, cap: slots, free: Vec::new(), released: 0, prefix_hwm: 0 }
    }

    /// The pool as the kernel's image atlas: the prefix and every slot up to the high-water mark.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: the mapping is `reserved` bytes, readable, zero until written; `len <= reserved`; writes happen
        // only through `&mut self`, so no write aliases this borrow.
        unsafe { std::slice::from_raw_parts(self.base.as_ptr(), self.prefix + self.hwm as usize * SLOT_BYTES) }
    }

    /// The prefix (the legacy image atlas), writable.
    pub fn prefix_mut(&mut self) -> &mut [u8] {
        self.prefix_hwm = self.prefix;
        // SAFETY: the first `prefix` bytes of the mapping, uniquely borrowed through `&mut self`.
        unsafe { std::slice::from_raw_parts_mut(self.base.as_ptr(), self.prefix) }
    }
    pub fn prefix(&self) -> &[u8] {
        &self.bytes()[..self.prefix]
    }
    pub fn prefix_len(&self) -> usize {
        self.prefix
    }

    /// The first texel of slot `s` in the kernel's image atlas (64 texels per row).
    #[inline]
    pub fn texel0(&self, s: u32) -> u32 {
        (self.prefix / 4) as u32 + s * SLOT_TEXELS
    }

    /// A slot, or `None` when every slot is in use.
    pub fn alloc(&mut self) -> Option<u32> {
        if let Some(s) = self.free.pop() {
            return Some(s);
        }
        if self.hwm == self.cap {
            return None;
        }
        self.hwm += 1;
        Some(self.hwm - 1)
    }

    /// Slot `s`'s bytes, writable.
    pub fn slot_mut(&mut self, s: u32) -> &mut [u8; SLOT_BYTES] {
        assert!(s < self.hwm, "slot {s} not allocated");
        // SAFETY: slot `s` lies inside the mapping (s < hwm <= cap) and is uniquely borrowed through `&mut self`.
        unsafe { &mut *(self.base.as_ptr().add(self.prefix + s as usize * SLOT_BYTES) as *mut [u8; SLOT_BYTES]) }
    }

    /// Return slot `s` and its pages: it reads as zero (or its old bytes) until written again.
    pub fn free(&mut self, s: u32) {
        assert!(s < self.hwm, "slot {s} not allocated");
        // SAFETY: the slot's pages lie inside the mapping; nothing borrows them (`&mut self`).
        unsafe { Self::give_back(self.base.as_ptr().add(self.prefix + s as usize * SLOT_BYTES), SLOT_BYTES) };
        self.released += 1;
        self.free.push(s);
    }

    /// Hand pages back to the system. macOS: `MADV_FREE_REUSABLE` (the pages leave the footprint at once, as
    /// malloc does for freed blocks); Linux: `MADV_DONTNEED` (freed at once, zero on the next touch).
    ///
    /// # Safety
    /// `p .. p + len` is page-aligned memory of this pool that nothing borrows.
    unsafe fn give_back(p: *mut u8, len: usize) {
        #[cfg(target_os = "macos")]
        const ADVICE: i32 = 7; // MADV_FREE_REUSABLE
        #[cfg(not(target_os = "macos"))]
        const ADVICE: i32 = libc::MADV_DONTNEED;
        // SAFETY: the caller's contract.
        unsafe { libc::madvise(p as *mut libc::c_void, len, ADVICE) };
    }

    /// Slots in use (allocated and not free).
    pub fn in_use(&self) -> u32 {
        self.hwm - self.free.len() as u32
    }
    pub fn high_water(&self) -> u32 {
        self.hwm
    }
    pub fn capacity(&self) -> u32 {
        self.cap
    }
    pub fn reserved(&self) -> usize {
        self.reserved
    }
    /// Resident and dirty bytes of the pool's touched range (`mincore`; dirty is not measurable this way on
    /// every platform, so it is the resident bytes).
    pub fn residency(&self) -> usize {
        let len = self.prefix + self.hwm as usize * SLOT_BYTES;
        if len == 0 {
            return 0;
        }
        resident(self.base.as_ptr(), len)
    }
    /// The prefix's written high-water mark (0 until something was written into it).
    pub fn prefix_used(&self) -> usize {
        self.prefix_hwm
    }
}

/// Resident bytes of `[p, p + len)` by `mincore`.
fn resident(p: *const u8, len: usize) -> usize {
    let page = page_size();
    let n = len.div_ceil(page);
    let mut v = vec![0u8; n];
    #[cfg(target_os = "macos")]
    // SAFETY: `p` is page aligned, `v` holds one byte per page of the range.
    let rc = unsafe { libc::mincore(p as *const libc::c_void, len, v.as_mut_ptr() as *mut libc::c_char) };
    #[cfg(not(target_os = "macos"))]
    // SAFETY: as above.
    let rc = unsafe { libc::mincore(p as *mut libc::c_void, len, v.as_mut_ptr()) };
    if rc != 0 {
        return 0;
    }
    v.iter().filter(|&&b| b & 1 != 0).count() * page
}

fn page_size() -> usize {
    // SAFETY: plain libc call.
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

impl Drop for Pool {
    fn drop(&mut self) {
        // SAFETY: mapped in `new` with exactly this length; no borrow outlives the pool.
        unsafe { libc::munmap(self.base.as_ptr() as *mut libc::c_void, self.reserved.max(SLOT_BYTES)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slots_are_contiguous_pages_and_free_gives_them_back() {
        let mut p = Pool::new(SLOT_BYTES, 64);
        let a = p.alloc().unwrap();
        let b = p.alloc().unwrap();
        assert_eq!((a, b), (0, 1));
        assert_eq!(p.texel0(1) - p.texel0(0), SLOT_TEXELS);
        p.slot_mut(a).fill(7);
        p.slot_mut(b).fill(9);
        let bytes = p.bytes();
        assert_eq!(bytes.len(), 3 * SLOT_BYTES);
        assert_eq!(bytes[SLOT_BYTES], 7);
        assert_eq!(bytes[2 * SLOT_BYTES], 9);
        assert!(p.residency() >= 2 * SLOT_BYTES);
        p.free(a);
        assert_eq!(p.in_use(), 1);
        assert_eq!(p.alloc(), Some(a));
    }
}
