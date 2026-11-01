//! The 48-byte GPU instance and the pool layout shared with `shader.metal`.

pub const W: i32 = 1920;
pub const H: i32 = 1440;

pub const KIND_SOLID: u8 = 0;
pub const KIND_RRECT: u8 = 1;
pub const KIND_BORDER: u8 = 2;
pub const KIND_GLYPH: u8 = 3;

#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct Instance {
    pub pos: [f32; 2],  //  0
    pub size: [f32; 2], //  8
    pub color: u32,     // 16 RGBA8, r in low byte
    pub color2: u32,    // 20
    pub radius: f32,    // 24
    pub bw: f32,        // 28
    pub uv: [u16; 4],   // 32
    pub space: u16,     // 40
    pub kind: u8,       // 42
    pub flags: u8,      // 43
    pub ext: u32,       // 44 clip index
}
const _: () = assert!(core::mem::size_of::<Instance>() == 48);
const _: () = assert!(core::mem::align_of::<Instance>() == 8);

#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (r as u32) | (g as u32) << 8 | (b as u32) << 16 | (a as u32) << 24
}

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
pub const INST_OFF: usize = 16384;
pub const LIST_OFF: usize = INST_OFF + MAX_INST * 48;
/// Frame slots for the compute repair: 32-B header (indirect args, target, ntiles) + tile list (u32 (ty<<16)|tx).
pub const FRAME_OFF: usize = LIST_OFF + MAX_LIST * 4;
pub const SLOT_BYTES: usize = 64 * 1024;
pub const N_SLOTS: usize = 3; // two frame slots (parity) + one for the full-redraw reference
pub const POOL_BYTES: usize = FRAME_OFF + N_SLOTS * SLOT_BYTES;
pub const HDR_N_INST_OFF: usize = 28;
const _: () = assert!(RECTS_OFF + MAX_RECTS * 16 <= PLANE_OFF);
const _: () = assert!(PLANE_OFF + 90 * 16 <= PAGES_OFF);
const _: () = assert!(PAGES_OFF + (MAX_INST / 64) * 16 <= INST_OFF);

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
