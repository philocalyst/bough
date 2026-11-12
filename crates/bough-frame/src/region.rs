//! The shared region as the kernels read it: column offsets ([`Layout`]), capacity masks, the record payload
//! encodings, and the per-frame slot of damaged tile rows ([`write_slot`]). The region itself is bough-mem's; a
//! GPU presenter wraps it (or views of its columns) in device buffers, the CPU executor reads it in place.

/// Surfaces a presenter may draw into (the façade's ring).
pub const MAX_SURFACES: usize = 4;
/// Payload bit of a text record: runs `[p & 0xFFFFF, + ((p >> 20) & 0x7FF) + 1)`.
pub const PAY_RUN: u32 = 0x8000_0000;
/// Payload bit of an instance-run record: instances `[p & 0x3FFFF, + ((p >> 18) & 0xFFF) + 1)`.
pub const PAY_IRUN: u32 = 0x4000_0000;

#[inline]
pub const fn pay_runs(first: u32, n: u32) -> u32 {
    debug_assert!(first < (1 << 20) && n >= 1 && n <= 2048);
    PAY_RUN | (n - 1) << 20 | first
}
#[inline]
pub const fn pay_irun(first: u32, n: u32) -> u32 {
    debug_assert!(first < (1 << 18) && n >= 1 && n <= 4096);
    PAY_IRUN | (n - 1) << 18 | first
}

/// Region offsets and sizes the kernels read (written into the header once; `n_dir` per frame).
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout {
    pub w: u32,
    pub h: u32,
    pub inst_off: u32,
    pub spaces_off: u32,
    pub clips_off: u32,
    pub dir_id_off: u32,
    pub dir_ink_off: u32,
    pub pages_off: u32,
    pub gx_off: u32,
    pub gent_off: u32,
    pub runs_off: u32,
    pub toks_off: u32,
    pub tok_stride: u32,
    /// Per space: the affine of a transformed subtree (`[f32; 8]`: M - I, pivot, translation; all zero is
    /// the identity), indexed like `spaces` (round 3c, input agent).
    pub xforms_off: u32,
    /// Three frame slots (two in flight, one for the reference render): header + damaged-row records.
    pub slot: [u32; 3],
    /// Capacity masks (capacity - 1, every column's capacity a power of two): the kernels AND every index
    /// they read from region data with its column's mask, so no value in the region can make the GPU read
    /// outside the column (bough-metal RESULTS Part IV). `Layout::default()` (probes: empty scene) masks to 0.
    /// With column views (`Fused::new_views`) the masks the kernels see are the views' committed capacities.
    pub masks: Masks,
    /// The GPU-read columns (order `COL_*`): where each starts, its element size, its mask group and the bytes
    /// the region reserves for it. A presenter with column views wraps each one's used prefix.
    pub cols: [ColumnDesc; NCOLS],
}

/// One GPU-read column of the region (`Layout::cols`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnDesc {
    /// Byte offset in the region (a VM page boundary when the presenter makes views).
    pub off: u32,
    pub elem: u32,
    /// Bytes reserved for the column (its carved capacity).
    pub reserve: u32,
    /// Mask group (`G_*`): the columns indexed by one index share one mask and so one capacity.
    pub group: u8,
}

/// The column table's order (`FCols` in shader.metal).
pub const COL_INST: usize = 0;
pub const COL_SPACES: usize = 1;
pub const COL_CLIPS: usize = 2;
pub const COL_DIR_ID: usize = 3;
pub const COL_DIR_INK: usize = 4;
pub const COL_PAGES: usize = 5;
pub const COL_GX: usize = 6;
pub const COL_GENT: usize = 7;
pub const COL_RUNS: usize = 8;
pub const COL_TOKS: usize = 9;
pub const COL_XFORMS: usize = 10;
pub const NCOLS: usize = 11;
/// Mask groups, in `Masks` order: an index read from region data is ANDed with its group's mask, so every
/// column of a group must hold the group's capacity.
pub const G_INST: usize = 0;
pub const G_PAGE: usize = 1;
pub const G_GLYPH: usize = 2;
pub const G_RUN: usize = 3;
pub const G_SPACE: usize = 4;
pub const G_CLIP: usize = 5;
pub const G_STRIKE: usize = 6;
pub const NGROUPS: usize = 7;

/// Power-of-two capacity minus one, per GPU-read column.
#[derive(Clone, Copy, Debug, Default)]
pub struct Masks {
    pub inst: u32,
    pub page: u32,
    pub glyph: u32,
    pub run: u32,
    pub space: u32,
    pub clip: u32,
    pub strike: u32,
}

impl Layout {
    pub fn tiles_x(&self) -> u32 {
        self.w.div_ceil(32)
    }
    pub fn tiles_y(&self) -> u32 {
        self.h.div_ceil(32)
    }
    /// Bytes of one frame slot for a `w`-by-`h` window: a 64-byte header (`FSlot`) and one 32-byte record per
    /// tile row (`FRow`, at +64).
    pub fn slot_bytes(_w: u32, h: u32) -> usize {
        (SLOT_HDR + 32 * h.div_ceil(32) as usize).next_multiple_of(256)
    }
}

/// Bytes of a frame slot's header (`FSlot`): the three dispatches' indirect args, target, rows, the copy.
pub const SLOT_HDR: usize = 64;

/// Scroll by copy (RESULTS §15): this frame first copies `rect` (target px, `[x0, y0, x1, y1)`, empty = no
/// copy) from surface `src` at `p - (dx, dy)` into the target, skipping the tiles the repair writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotCopy {
    pub src: u32,
    pub rect: [i32; 4],
    pub dx: i32,
    pub dy: i32,
}
impl SlotCopy {
    pub const NONE: SlotCopy = SlotCopy { src: 0, rect: [0; 4], dx: 0, dy: 0 };
    pub fn is_none(&self) -> bool {
        self.rect[0] >= self.rect[2] || self.rect[1] >= self.rect[3]
    }
}

/// Bytes reserved for the header at the start of the region: `FHdr` @0, the bind table @`FBIND_OFF`, the
/// column table @`FCOL_OFF`.
pub const HDR_BYTES: usize = 512;
/// Byte offset of the column table (`FCols` in shader.metal): each GPU-read column's GPU address. The
/// kernels read every column through it (round 6, lane memory): a column is either a range of the one region
/// buffer (`Fused::new`, `new_on`) or its own no-copy view of the region's memory (`new_views`).
pub const FCOL_OFF: usize = 256;
const _: () = assert!(FCOL_OFF + 8 * NCOLS <= HDR_BYTES);
/// Byte offset of the bind table in the region header (`FBind` in shader.metal).
pub const FBIND_OFF: usize = 128;

/// The image atlas's size (premultiplied RGBA8 texels).
pub const IMG_W: u32 = 1024;
pub const IMG_H: u32 = 1024;

/// Copy the OR-ed tile rows (`rows[ty]`, bit tx = tile) into frame slot `k` as damaged-row records (see the
/// module docs): O(damaged rows). `full`: every tile of the window. Returns (tiles, rows). This is the whole
/// CPU side of a frame's raster input (headless runs call it too, so RENDER=0 measures it).
///
/// # Safety
/// `region` must be a region laid out by `l` (slots reserved with `Layout::slot_bytes` for its window), not
/// read by the GPU while this writes.
pub unsafe fn write_slot(region: *mut u8, l: &Layout, k: usize, target: u32, rows: &[u128], full: bool, copy: &SlotCopy) -> (u32, u32) {
    let (tx, ty) = (l.tiles_x(), l.tiles_y());
    let all = if tx >= 128 { u128::MAX } else { (1u128 << tx) - 1 };
    // SAFETY: see the function contract.
    unsafe {
        let s = region.add(l.slot[k] as usize) as *mut u32;
        // the scroll copy: one threadgroup per tile of the destination's tile span (row-major), its origin and
        // width in `cspan`; the destination rect and the move as u16 / i16 pairs
        let c = if full { &SlotCopy::NONE } else { copy };
        if c.is_none() {
            *s.add(8) = 0;
        } else {
            let [x0, y0, x1, y1] = c.rect;
            let (cx0, cy0) = ((x0 >> 5) as u32, (y0 >> 5) as u32);
            let (cw, ch) = (((x1 + 31) >> 5) as u32 - cx0, ((y1 + 31) >> 5) as u32 - cy0);
            *s.add(8) = cw * ch;
            *s.add(11) = c.src;
            *s.add(12) = x0 as u32 | (y0 as u32) << 16;
            *s.add(13) = x1 as u32 | (y1 as u32) << 16;
            *s.add(14) = c.dx as i16 as u16 as u32 | (c.dy as i16 as u16 as u32) << 16;
            *s.add(15) = cx0 | cy0 << 8 | cw << 16;
        }
        *s.add(9) = 1;
        *s.add(10) = 1;
        let rec = s.add(SLOT_HDR / 4);
        let (mut nt, mut nr) = (0u32, 0u32);
        for y in 0..ty as usize {
            let row = if full { all } else { rows.get(y).copied().unwrap_or(0) & all };
            if row == 0 {
                continue;
            }
            let r = rec.add(nr as usize * 8);
            *r = y as u32;
            *r.add(1) = row.trailing_zeros() | (128 - row.leading_zeros()) << 8;
            *r.add(2) = nt;
            (r.add(4) as *mut [u32; 4]).write_unaligned(std::mem::transmute::<u128, [u32; 4]>(row));
            nt += row.count_ones();
            nr += 1;
        }
        *s = nr;
        *s.add(1) = 1;
        *s.add(2) = 1;
        *s.add(3) = nt;
        *s.add(4) = 1;
        *s.add(5) = 1;
        *s.add(6) = target;
        *s.add(7) = nr;
        (nt, nr)
    }
}
