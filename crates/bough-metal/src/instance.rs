//! The 48-byte GPU instance and the pool layout shared with `shader.metal`.

pub const W: i32 = 1920;
pub const H: i32 = 1440;

// The instance and its kinds live in bough-frame (round 7, platform lane); re-exported under the old paths.
pub use bough_frame::instance::{rgba, Instance, KIND_BORDER, KIND_GLYPH, KIND_RRECT, KIND_SOLID};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Hdr {
    pub viewport: [f32; 2],
    pub spaces_off: u32,
    pub clips_off: u32,
    pub rects_off: u32,
    pub inst_off: u32,
    pub list_off: u32,
    /// instance count (read by the compute repair kernel, which culls the whole array itself)
    pub n_inst: u32,
    /// 16-px damage plane for the vertex-clipped stream: 90 rows x 16 B
    pub plane_off: u32,
    /// per-64-instance page ink unions (int4 x0,y0,x1,y1; empty = x0 > x1): the compute kernel's first-level cull
    pub pages_off: u32,
    /// bough-core mirror (INTERFACE.md): directory page ids, directory page-union NRects, entry count, page pool
    pub dir_page_off: u32,
    pub dir_ink_off: u32,
    pub n_dir: u32,
    pub pagepool_off: u32,
    /// kernel diagnostics: 0 normal, 1 cull only, 2 count staged records into the stats words
    pub debug: u32,
    pub stats_off: u32,
    /// bough-text GPU format (INTERFACE.md): glyph pool columns, run headers, strike entry table, space -> clip
    pub gx_off: u32,
    pub gent_off: u32,
    pub runs_off: u32,
    pub entries_off: u32,
    pub space_clip_off: u32,
}

pub const MAX_INST: usize = 4096;
pub const MAX_SPACES: usize = 256;
pub const MAX_CLIPS: usize = 16;
pub const MAX_RECTS: usize = 512;
pub const MAX_LIST: usize = 256 * 1024;

pub const SPACES_OFF: usize = 256;
pub const CLIPS_OFF: usize = SPACES_OFF + MAX_SPACES * 8;
pub const RECTS_OFF: usize = CLIPS_OFF + MAX_CLIPS * 16;
pub const PLANE_OFF: usize = 12288;
pub const PAGES_OFF: usize = 13824;
pub const DIR_PAGE_OFF: usize = 14848;
pub const DIR_INK_OFF: usize = 15104;
pub const MAX_DIR: usize = MAX_INST / 64;
pub const HDR_N_DIR_OFF: usize = 48;
pub const HDR_DEBUG_OFF: usize = 56;
pub const STATS_OFF: usize = 15616;
pub const INST_OFF: usize = 16384;
pub const LIST_OFF: usize = INST_OFF + MAX_INST * 48;
/// Frame slots for the compute repair: 32-B header (indirect args, target, ntiles) + tile list (u32 (ty<<16)|tx).
pub const FRAME_OFF: usize = LIST_OFF + MAX_LIST * 4;
pub const SLOT_BYTES: usize = 64 * 1024;
pub const N_SLOTS: usize = 3; // two frame slots (parity) + one for the full-redraw reference
/// bough-core `Page` layout (1344 B, 64-aligned): depth u16[64] @0, ink NRect[64] @128, gink NRect[8] @640,
/// kind u8[64] @704, payload u32[64] @768, handle u32[64] @1024, version/id/dir @1280.
pub const PG_BYTES: usize = 1344;
pub const PG_INK: usize = 128;
pub const PG_GINK: usize = 640;
pub const PG_KIND: usize = 704;
pub const PG_PAYLOAD: usize = 768;
pub const PAGEPOOL_OFF: usize = (FRAME_OFF + N_SLOTS * SLOT_BYTES + 63) & !63;
/// Text (bough-text iteration 2 format): x: i32 per glyph slot (pen, font units), ent: u16 per slot (strike entry
/// id = token), RunHeader 32 B per run (a window: base, len, x0), Entry 32 B per (strike, token).
pub const MAX_GLYPHS: usize = 8192;
pub const MAX_RUNS: usize = 1024;
pub const N_ENTRIES: usize = 256;
pub const RUN_BIT: u32 = 1 << 31;
pub const KIND_RUN: u8 = 4;
pub const GX_OFF: usize = PAGEPOOL_OFF + MAX_DIR * PG_BYTES;
pub const GENT_OFF: usize = GX_OFF + MAX_GLYPHS * 4;
pub const RUNS_OFF: usize = GENT_OFF + MAX_GLYPHS * 2;
pub const ENTRIES_OFF: usize = RUNS_OFF + MAX_RUNS * 32;
pub const SPACE_CLIP_OFF: usize = ENTRIES_OFF + N_ENTRIES * 32;
pub const POOL_BYTES: usize = SPACE_CLIP_OFF + MAX_SPACES * 4;

/// bough-text `RunHeader` (32 B): a window onto shaped glyph slots.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunHeader {
    /// pen origin (baseline-relative top here) in pixels relative to the space; origin[1] integral
    pub origin: [f32; 2],
    /// pixels per font unit
    pub scale: f32,
    pub color: u32,
    pub space: u16,
    pub strike: u16,
    pub base: u32,
    pub len: u32,
    pub x0: i32,
}
const _: () = assert!(core::mem::size_of::<RunHeader>() == 32);

/// bough-text strike `Entry` (32 B): quad offset/size from the pen and the atlas rect.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Entry {
    /// [left, -top, w, h] px
    pub geo: [f32; 4],
    pub uv: [u16; 4],
    pub state: u32,
    pub pad: u32,
}
const _: () = assert!(core::mem::size_of::<Entry>() == 32);
pub const HDR_N_INST_OFF: usize = 28;
const _: () = assert!(RECTS_OFF + MAX_RECTS * 16 <= PLANE_OFF);
const _: () = assert!(PLANE_OFF + 90 * 16 <= PAGES_OFF);
const _: () = assert!(PAGES_OFF + (MAX_INST / 64) * 16 <= DIR_PAGE_OFF);
const _: () = assert!(DIR_PAGE_OFF + MAX_DIR * 4 <= DIR_INK_OFF);
const _: () = assert!(DIR_INK_OFF + MAX_DIR * 8 <= INST_OFF);

/// Integer pixel rectangle, half-open [x0,x1) x [y0,y1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}
impl IRect {
    pub const EMPTY: IRect = IRect { x0: i32::MAX, y0: i32::MAX, x1: i32::MIN, y1: i32::MIN };
    pub const FULL: IRect = IRect { x0: 0, y0: 0, x1: W, y1: H };
    #[inline]
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        IRect { x0, y0, x1, y1 }
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    #[inline]
    pub fn union(&self, o: &IRect) -> IRect {
        IRect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    #[inline]
    pub fn intersect(&self, o: &IRect) -> IRect {
        IRect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    #[inline]
    pub fn overlaps(&self, o: &IRect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
    pub fn area(&self) -> i64 {
        if self.is_empty() { 0 } else { (self.x1 - self.x0) as i64 * (self.y1 - self.y0) as i64 }
    }
}
