//! Surfaces the CPU executor draws into (see the module docs of `backend`).

/// A ring of same-sized BGRA8 surfaces (premultiplied; byte order B, G, R, A, i.e. Wayland's `ARGB8888` /
/// `XRGB8888` and Metal's `BGRA8Unorm`), which surface to draw being the façade's policy (`App::pick`: the ring is
/// content-addressed and repairs by buffer age).
///
/// # Safety
/// An implementation promises that `pixels(i)` returns memory valid for reads and writes of `size().1` rows of
/// `bpr >= 4 * size().0` bytes, that nothing else (no compositor, no other thread) reads or writes surface `i`
/// while `in_use(i)` is false, and that the pointer stays valid until the next `resize`.
pub unsafe trait Surfaces {
    /// Surfaces in the ring (constant for the target's life).
    fn count(&self) -> usize;
    /// The surfaces' size in device pixels.
    fn size(&self) -> (u32, u32);
    /// The compositor still holds surface `i` (it may read it): it must not be drawn now.
    fn in_use(&self, i: usize) -> bool;
    /// Surface `i`'s pixels: (base, bytes per row).
    fn pixels(&mut self, i: usize) -> (*mut u8, usize);
    /// Show surface `i`. `damage`: the 32 px tile rows (bit `tx` of `damage[ty]`) where it differs from what is
    /// on screen now; `full`: everything.
    fn present(&mut self, i: usize, damage: &[u128], full: bool);
    /// The window is now `w` x `h` device pixels: every surface follows (contents undefined until redrawn).
    fn resize(&mut self, w: u32, h: u32);
}

/// Offscreen surfaces in plain memory: what `App::new(.., gpu: true)` draws into where no GPU presenter exists, so
/// pixel tests (`presented_pixels` against `reference_pixels`) run on every platform. Never in use.
pub struct MemSurfaces {
    w: u32,
    h: u32,
    bufs: Vec<Vec<u8>>,
    /// Frames presented, and the damaged tiles they named (diagnostics).
    pub presents: u64,
    pub damaged_tiles: u64,
}

impl MemSurfaces {
    pub fn new(w: u32, h: u32, n: usize) -> MemSurfaces {
        MemSurfaces { w, h, bufs: (0..n).map(|_| Vec::new()).collect(), presents: 0, damaged_tiles: 0 }
    }
    fn ensure(&mut self, i: usize) {
        let n = self.w as usize * 4 * self.h as usize;
        if self.bufs[i].len() != n {
            // a surface owns no memory until first drawn (as an IOSurface)
            self.bufs[i] = vec![0; n];
        }
    }
}

// SAFETY: each surface is its own heap buffer of `h` rows of `4 w` bytes, owned here and reached only through
// `pixels`; it is reallocated only by `resize` (and lazily at first use).
unsafe impl Surfaces for MemSurfaces {
    fn count(&self) -> usize {
        self.bufs.len()
    }
    fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }
    fn in_use(&self, _: usize) -> bool {
        false
    }
    fn pixels(&mut self, i: usize) -> (*mut u8, usize) {
        self.ensure(i);
        (self.bufs[i].as_mut_ptr(), self.w as usize * 4)
    }
    fn present(&mut self, _: usize, damage: &[u128], full: bool) {
        self.presents += 1;
        self.damaged_tiles += if full { (self.w.div_ceil(32) * self.h.div_ceil(32)) as u64 } else { damage.iter().map(|r| r.count_ones() as u64).sum() };
    }
    fn resize(&mut self, w: u32, h: u32) {
        (self.w, self.h) = (w, h);
        for b in &mut self.bufs {
            *b = Vec::new();
        }
    }
}

/// Scroll by copy on the CPU (the CPU twin of the `fs_copy` kernel): the pixels of `copy.rect` (target px) come from
/// `src` at `p - (dx, dy)`. Tiles the repair rewrites are copied too (it overwrites them; the kernel skips them).
/// `src` may be `dst` (the surface on screen repaired in place): rows are then moved in the order that reads each
/// source row before it is overwritten, each row with a memmove.
///
/// # Safety
/// `src` and `dst` are surfaces of the same size (or the same one, with `sbpr == dbpr`), valid for `rect`'s rows at
/// their `bpr`, and `rect - (dx, dy)` lies inside the source (the copy plan intersects the clip with its translate).
pub unsafe fn copy_rect(src: *const u8, sbpr: usize, dst: *mut u8, dbpr: usize, copy: &bough_frame::SlotCopy) {
    let [x0, y0, x1, y1] = copy.rect;
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let n = (x1 - x0) as usize * 4;
    let sx = (x0 - copy.dx) as usize;
    let row = |y: i32| {
        let sy = (y - copy.dy) as usize;
        // SAFETY: the caller's contract: both rows lie inside their surfaces; `copy` is a memmove, so a row of the
        // same surface overlapping itself (a horizontal move) is moved correctly.
        unsafe { std::ptr::copy(src.add(sy * sbpr + sx * 4), dst.add(y as usize * dbpr + x0 as usize * 4), n) };
    };
    // moving down (dy > 0) in one surface: bottom row first, so every source row is read before it is written
    if copy.dy > 0 {
        (y0..y1).rev().for_each(row);
    } else {
        (y0..y1).for_each(row);
    }
}
