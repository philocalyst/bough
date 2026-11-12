//! Pointer and focus queries over the paint preorder (round 3: input).
//!
//! The Bough already is the hit index: records are in paint preorder, inks are negated-max `NRect`s, and
//! pages, groups and chunks carry union summaries. So a hit test is a range query: the answer is the
//! **last** record in preorder whose visible ink (`ink ∩ clip`) contains the pixel, found by the same
//! chunk -> page -> group -> record SIMD descent the cull uses, scanned from the top. Nothing is built
//! for it and nothing is ever rebuilt (a scroll moves page offsets, which the threshold absorbs).
//!
//! Interaction state lives in the `kind` column's top three bits (the façade's record kinds use the low
//! five), so every flag moves with its record through splices and page splits for free:
//!
//! * [`KF_INTERACT`]: a dispatch target (it has routes: hover, click, keys, drag, wheel);
//! * [`KF_FOCUS`]: a tab stop (tab order is preorder);
//! * [`KF_NOHIT`]: pointer-transparent (carets, focus rings, tooltips, drag previews).
//!
//! **The hit cell.** Between two pointer events the topmost record under the pointer is constant inside
//! a rectangle we can compute: its visible ink minus every later (higher) record that overlaps it. The
//! cell is valid until geometry changes inside it, and every geometry change marks its old and new
//! support in the core damage ring, so validity is read off the damage planes (no flag, no hook).
use super::*;

/// A dispatch target: the record has routes (the façade's `Act` table, by handle).
pub const KF_INTERACT: u8 = 0x80;
/// A tab stop.
pub const KF_FOCUS: u8 = 0x40;
/// Pointer-transparent: never the topmost record of a hit test.
pub const KF_NOHIT: u8 = 0x20;
/// The flag bits (the rest of the byte is the record kind).
pub const KF_MASK: u8 = 0xE0;

/// Bit `i` of the result = bit `bit` of byte `i` of `kind[8g .. 8g + 8]` (8 bytes gathered by one
/// multiply: byte i's bit lands at bit 56 + i, the other partial products stay below bit 56 or wrap).
#[inline(always)]
fn flag8(kind: &[u8; CAP], g: usize, bit: u32) -> u32 {
    let k = u64::from_le_bytes(kind[g * GROUP..g * GROUP + GROUP].try_into().unwrap());
    let b = (k >> bit) & 0x0101_0101_0101_0101;
    (b.wrapping_mul(0x0102_0408_1020_4080) >> 56) as u32
}

/// Lanes of a page's kind column with any bit of `flag` set (bit i = slot i).
#[inline(always)]
fn page_flags(kind: &[u8; CAP], flag: u8) -> u64 {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // SAFETY: 4 x 16 bytes of a 64-byte array.
        use core::arch::aarch64::*;
        let f = vdupq_n_u8(flag);
        const W: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
        let w = vld1q_u8(W.as_ptr());
        let p = kind.as_ptr();
        let m0 = vandq_u8(vtstq_u8(vld1q_u8(p), f), w);
        let m1 = vandq_u8(vtstq_u8(vld1q_u8(p.add(16)), f), w);
        let m2 = vandq_u8(vtstq_u8(vld1q_u8(p.add(32)), f), w);
        let m3 = vandq_u8(vtstq_u8(vld1q_u8(p.add(48)), f), w);
        let x = vpaddq_u8(m0, m1);
        let y = vpaddq_u8(m2, m3);
        let z = vpaddq_u8(x, y);
        let r = vpaddq_u8(z, z);
        vgetq_lane_u64::<0>(vreinterpretq_u64_u8(r))
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut m = 0u64;
        for (i, &k) in kind.iter().enumerate() {
            m |= ((k & flag != 0) as u64) << i;
        }
        m
    }
}

/// Shrink `c` (which contains (x, y)) so it misses `o` (which does not) by the one cut that keeps the
/// most area. Each candidate keeps the point: a cut at `o`'s right edge exists only if that edge is at or
/// left of x, and so on.
#[inline]
fn cut(c: NRect, o: NRect, x: i32, y: i32) -> NRect {
    let [cx0, cy0, cx1, cy1] = c.to_xyxy();
    let [ox0, oy0, ox1, oy1] = o.to_xyxy();
    let mut best = [x, y, x + 1, y + 1];
    let mut area = 1i64;
    let mut take = |r: [i32; 4]| {
        let a = (r[2] - r[0]) as i64 * (r[3] - r[1]) as i64;
        if a > area {
            area = a;
            best = r;
        }
    };
    if ox1 <= x {
        take([ox1.max(cx0), cy0, cx1, cy1]);
    }
    if ox0 > x {
        take([cx0, cy0, ox0.min(cx1), cy1]);
    }
    if oy1 <= y {
        take([cx0, oy1.max(cy0), cx1, cy1]);
    }
    if oy0 > y {
        take([cx0, cy0, cx1, oy0.min(cy1)]);
    }
    NRect::new(best[0], best[1], best[2], best[3])
}

/// The most obstacles a cell computation looks at before settling for the one-pixel cell (always valid).
pub const CELL_BUDGET: u32 = 256;

impl Bough {
    /// Topmost (last in preorder) record whose visible ink contains pixel (x, y), skipping records flagged
    /// [`KF_NOHIT`]. The descent of [`Bough::hit`] with one extra mask per candidate group: the group's
    /// eight NOHIT bits, gathered from the kind column by one multiply.
    pub fn hit_top(&self, x: i32, y: i32) -> Option<usize> {
        let t = Thresh::of_point(x, y)?;
        let nc = self.c_ink.len();
        let mut chi = nc;
        while chi > 0 {
            let cb = (chi - 1) / 64 * 64;
            // SAFETY: cb + (chi - cb) <= c_ink.len(), <= 64 rects.
            let mut cm = unsafe { lt_mask(self.c_ink.as_ptr().add(cb), chi - cb, t) };
            while cm != 0 {
                let c = cb + 63 - cm.leading_zeros() as usize;
                cm &= !(1u64 << (c - cb));
                let b = c * CHUNK;
                // the block's live pages (its holes are EMPTY and would fail anyway: a short block is a
                // short scan)
                // SAFETY: c is a block; its live entries are in bounds; cnt <= 64.
                let cnt = unsafe { *self.r_len.get_unchecked(c) } as usize;
                let mut pm = unsafe { lt_mask(self.d_ink.as_ptr().add(b), cnt, t) };
                while pm != 0 {
                    let bit = 63 - pm.leading_zeros() as usize;
                    pm &= !(1u64 << bit);
                    let d = b + bit;
                    let p = self.page(d);
                    if p.is_xf() {
                        // a transformed page: the inverse of the point against the untransformed inks
                        if let Some(slot) = self.xf_hit_slot(p, self.d_len[d] as usize, x, y, KF_NOHIT) {
                            return Some(self.start_of(d) + slot);
                        }
                        continue;
                    }
                    let tp = t.shift_lanes(p.offl);
                    // SAFETY: gink is 8 rects; each group is 8 rects of the ink column.
                    let mut gm = unsafe { lt8(p.gink.as_ptr(), tp) };
                    while gm != 0 {
                        let g = 31 - gm.leading_zeros() as usize;
                        gm &= !(1u32 << g);
                        let rm = if p.cgroups & (1 << g) != 0 {
                            let v = self.group_visible(p, g);
                            unsafe { lt8(v.as_ptr(), tp) }
                        } else {
                            unsafe { lt8(p.ink.as_ptr().add(g * GROUP), tp) }
                        };
                        let rm = rm & !flag8(&p.kind, g, 5);
                        if rm != 0 {
                            let slot = g * GROUP + 31 - rm.leading_zeros() as usize;
                            return Some(self.start_of(d) + slot);
                        }
                    }
                }
            }
            chi = cb;
        }
        None
    }

    /// Ancestors-or-self of `pos` whose kind has any bit of `flag`, nearest first (the dispatch chain:
    /// `out[0]` is the target, the rest is where an event bubbles). One backward pass over the depth
    /// column, as [`Bough::ancestors`], reading the kind byte of each ancestor it lands on.
    pub fn flagged_chain(&self, pos: usize, flag: u8, out: &mut Vec<usize>) {
        out.clear();
        let (mut d, mut s, mut base) = self.locate_base(pos);
        let p = self.page(d);
        if p.kind[s] & flag != 0 {
            out.push(pos);
        }
        let mut dpt = p.depth[s];
        while dpt > 0 {
            let p = self.page(d);
            if let Some(j) = page_find_last_lt(&p.depth, s, dpt) {
                if p.kind[j] & flag != 0 {
                    out.push(base + j);
                }
                (s, dpt) = (j, p.depth[j]);
                continue;
            }
            let Some(d2) = self.depth_prev_lt(d, dpt) else { break };
            (d, s, base) = (d2, CAP, self.start_of(d2));
        }
    }

    /// The hit cell of the topmost record `top` at (x, y) (from [`Bough::hit_top`]): a rectangle holding
    /// the pixel inside which `hit_top` answers `top` while the tree is unchanged. It is `top`'s visible
    /// ink (window-clipped) minus every later record (not NOHIT) whose visible ink meets it, each removed by
    /// the single cut that keeps the most area. Later records are found by the page summaries (64 pages
    /// per mask), group summaries and record inks, as the cull does, starting at `top`'s own slot. Past
    /// [`CELL_BUDGET`] obstacles, and when nothing is under the pointer, the cell is the pixel itself.
    pub fn hit_cell(&self, x: i32, y: i32, top: Option<usize>) -> NRect {
        let px = NRect::new(x, y, x + 1, y + 1);
        let Some(r) = top else { return px };
        let (d0, s0) = self.locate(r);
        if self.page(d0).is_xf() {
            // a transformed record's hit region is not a rectangle: the pixel is its cell
            return px;
        }
        let mut c = self.visible(d0, s0).intersect(self.ring.window).canon();
        if !c.contains_point(x, y) {
            return px;
        }
        let n = self.dir_len();
        let mut budget = CELL_BUDGET;
        let mut b = d0;
        while b < n {
            let t = Thresh::of_rect(c);
            let cnt = (n - b).min(64);
            // SAFETY: b + cnt <= n = d_ink.len(); cnt <= 64.
            let mut pm = unsafe { lt_mask(self.d_ink.as_ptr().add(b), cnt, t) };
            while pm != 0 {
                let d = b + pm.trailing_zeros() as usize;
                pm &= pm - 1;
                let p = self.page(d);
                // the first page starts after `top`'s slot
                let from = if d == d0 { s0 + 1 } else { 0 };
                if p.is_xf() {
                    // a transformed page: its records' transformed bounding boxes are the obstacles
                    for s in from..self.d_len[d] as usize {
                        if p.kind[s] & KF_NOHIT != 0 {
                            continue;
                        }
                        let o = self.visible_in(p, s);
                        if o.intersects(c) {
                            if budget == 0 {
                                return px;
                            }
                            budget -= 1;
                            c = cut(c, o, x, y);
                        }
                    }
                    continue;
                }
                let tp = Thresh::of_rect(c).shift_lanes(p.offl);
                // SAFETY: gink is 8 rects.
                let mut gm = unsafe { lt8(p.gink.as_ptr(), tp) } & (!0u32 << (from / GROUP));
                while gm != 0 {
                    let g = gm.trailing_zeros() as usize;
                    gm &= gm - 1;
                    let vis;
                    let ink: *const NRect = if p.cgroups & (1 << g) != 0 {
                        vis = self.group_visible(p, g);
                        vis.as_ptr()
                    } else {
                        // SAFETY: group g is 8 rects of the ink column.
                        unsafe { p.ink.as_ptr().add(g * GROUP) }
                    };
                    // SAFETY: 8 rects.
                    let mut rm = unsafe { lt8(ink, tp) } & !flag8(&p.kind, g, 5);
                    if g == from / GROUP {
                        rm &= !0u32 << (from % GROUP);
                    }
                    while rm != 0 {
                        let i = rm.trailing_zeros() as usize;
                        rm &= rm - 1;
                        // SAFETY: i < 8.
                        let o = p.abs(unsafe { *ink.add(i) });
                        if !o.intersects(c) {
                            continue; // met the cell before an earlier cut shrank it
                        }
                        if budget == 0 {
                            return px;
                        }
                        budget -= 1;
                        c = cut(c, o, x, y);
                    }
                }
            }
            b += 64;
        }
        c
    }

    /// Is a hit cell computed at epoch `since` still valid? Only if no core damage since then (that
    /// epoch included: conservative) touches its tiles. Every geometry, structure, clip and scroll change
    /// marks its old and new support in the ring, so this is the whole invalidation.
    pub fn cell_valid(&self, cell: NRect, since: u32) -> bool {
        let age = self.ring.epoch.wrapping_sub(since);
        if age as usize >= RING {
            return false;
        }
        // A scroll by copy moves records without marking the tiles the copy supplies, so the planes alone
        // do not see it: an epoch whose scroll record meets the cell voids it too (round 6).
        (0..=age).all(|b| !self.ring.plane(b).hits(cell, self.ring.window) && self.ring.scroll(b).is_none_or(|r| r.clip.intersect(cell).is_empty()))
    }

    /// The first record at or after `pos` whose kind has a bit of `flag`: a scan of the kind column, 64
    /// lanes per page in four NEON compares (padding has kind 0, so it never matches).
    pub fn first_flagged_from(&self, pos: usize, flag: u8) -> Option<usize> {
        if pos >= self.total {
            return None;
        }
        let (mut d, s, mut base) = self.locate_base(pos);
        let mut m = page_flags(&self.page(d).kind, flag) & (!0u64 << s);
        loop {
            if m != 0 {
                return Some(base + m.trailing_zeros() as usize);
            }
            base += self.d_len[d] as usize;
            d = self.next_dir(d);
            if d >= self.dir_len() {
                return None;
            }
            m = page_flags(&self.page(d).kind, flag);
        }
    }

    /// The last record before `pos` (exclusive) whose kind has a bit of `flag`.
    pub fn last_flagged_before(&self, pos: usize, flag: u8) -> Option<usize> {
        if pos == 0 || self.total == 0 {
            return None;
        }
        let (mut d, s, mut base) = self.locate_base(pos.min(self.total) - 1);
        let keep = if s == 63 { !0u64 } else { (1u64 << (s + 1)) - 1 };
        let mut m = page_flags(&self.page(d).kind, flag) & keep;
        loop {
            if m != 0 {
                return Some(base + 63 - m.leading_zeros() as usize);
            }
            d = self.prev_dir(d)?;
            base -= self.d_len[d] as usize;
            m = page_flags(&self.page(d).kind, flag);
        }
    }

    /// Tab order: the next record after `after` (or the first) with `flag`, wrapping around; `back` walks
    /// the other way. A single stop returns itself.
    pub fn next_flagged(&self, after: Option<usize>, flag: u8, back: bool) -> Option<usize> {
        if back {
            let from = after.unwrap_or(self.total);
            self.last_flagged_before(from, flag).or_else(|| self.last_flagged_before(self.total, flag))
        } else {
            let from = after.map_or(0, |a| a + 1);
            self.first_flagged_from(from, flag).or_else(|| self.first_flagged_from(0, flag))
        }
    }

    /// The handle bound to the record at `pos`, if any.
    pub fn handle_at(&self, pos: usize) -> Option<Handle> {
        let (d, s) = self.locate(pos);
        let idx = self.page(d).handle[s];
        if idx == 0 {
            return None;
        }
        let e = &self.htab[idx as usize];
        e.live.then_some(Handle { idx, generation: e.generation })
    }

    /// The live handle with index `idx` (its current generation).
    pub fn handle_gen(&self, idx: u32) -> Handle {
        Handle { idx, generation: self.htab[idx as usize].generation }
    }

    /// Absolute visible ink (ink ∩ clip) of the record at `pos`.
    pub fn visible_at(&self, pos: usize) -> NRect {
        let (d, s) = self.locate(pos);
        self.visible(d, s)
    }

    /// Set or clear flag bits in the kind byte of a handle's record (no damage: flags are not paint).
    pub fn set_kind_flags(&mut self, h: Handle, set: u8, clear: u8) -> bool {
        match self.handle_ptr(h) {
            Some((p, s)) => {
                // SAFETY: bound handles point at live pages; s < CAP; &mut self is exclusive.
                let pg = unsafe { &mut *p };
                pg.wgen = pg.wgen.wrapping_add(1);
                self.mgen = self.mgen.wrapping_add(1);
                pg.kind[s] = (pg.kind[s] | set) & !clear;
                true
            }
            None => false,
        }
    }

    /// Resize the window: the damage ring's window, tile columns and rows (the caller repaints).
    pub fn set_window(&mut self, w: i32, h: i32) {
        self.ring.set_window(w, h);
    }
}
