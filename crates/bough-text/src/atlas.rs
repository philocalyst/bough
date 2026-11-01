//! Glyph rasterization into a shelf-packed power-of-two atlas, and glyph instance generation.
//!
//! Rasterizer: `swash` (the rasterizer cosmic-text/zgx use), unhinted alpha coverage at
//! integer pixel positions. Its cost is paid once per (glyph, size) and is reported separately
//! in `RESULTS.md`; steady-state text never rasterizes.

use crate::shape::{GlyphRun, Shaper};
use swash::scale::{image::Content, Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::FontRef;

/// One glyph quad, 48 bytes (the uber-shader instance layout of DESIGN section 3, glyph kind).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Instance {
    /// Top-left of the quad in pixels, relative to the instance's space.
    pub xy: [f32; 2],
    /// Quad size in pixels.
    pub wh: [f32; 2],
    /// Atlas rectangle in texels: x, y, w, h.
    pub uv: [u16; 4],
    /// RGBA8.
    pub color: u32,
    pub kind: u16,
    pub space: u16,
    /// Reserved (clip / transform for the other primitive kinds).
    pub aux: [u32; 4],
}
const _: () = assert!(core::mem::size_of::<Instance>() == 48);

pub const KIND_GLYPH: u16 = 4;

/// Cached rasterization of one glyph at one size, stored the way the instance writer wants it:
/// a ready-to-add float vector and the atlas rectangle, 32 bytes.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Entry {
    /// `[left, -top, w, h]` in pixels: the quad's offset from the pen position / baseline
    /// (`top` is the bitmap's rise above the baseline) and its size.
    pub geo: [f32; 4],
    /// Atlas rectangle in texels: x, y, w, h.
    pub uv: [u16; 4],
    /// 0 = not rasterized yet, 1 = ready (bitmap possibly empty, `uv[2] == 0`).
    pub state: u32,
    pub _pad: u32,
}

struct Shelf {
    y: u32,
    h: u32,
    x: u32,
}

/// An R8 coverage atlas, power-of-two in both dimensions, packed in horizontal shelves.
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    shelves: Vec<Shelf>,
    next_y: u32,
    /// Bumped when the pixel store is reallocated/grown; the GPU copy must be re-uploaded.
    pub generation: u32,
    /// Bytes written since the last `take_dirty` (for upload accounting).
    pub dirty_bytes: usize,
}

impl Atlas {
    pub fn new(width: u32, height: u32) -> Self {
        assert!(width.is_power_of_two() && height.is_power_of_two());
        Atlas {
            width,
            height,
            pixels: vec![0; (width * height) as usize],
            shelves: vec![],
            next_y: 0,
            generation: 0,
            dirty_bytes: 0,
        }
    }

    /// Reserve a `w x h` rectangle (with a 1 px gutter on the right/bottom), growing the atlas
    /// height by doubling when full. Shelf heights are rounded up to a multiple of 4.
    pub fn alloc(&mut self, w: u32, h: u32) -> (u32, u32) {
        let (pw, ph) = (w + 1, h + 1);
        assert!(pw <= self.width, "glyph wider than the atlas");
        let sh = (ph + 3) & !3;
        // Best fit: the shelf with the least wasted height that has room.
        let mut best: Option<usize> = None;
        for (i, s) in self.shelves.iter().enumerate() {
            if s.h >= ph && s.h <= sh + 4 && s.x + pw <= self.width && best.is_none_or(|b| s.h < self.shelves[b].h) {
                best = Some(i);
            }
        }
        if let Some(i) = best {
            let s = &mut self.shelves[i];
            let p = (s.x, s.y);
            s.x += pw;
            return p;
        }
        while self.next_y + sh > self.height {
            self.height *= 2;
            self.pixels.resize((self.width * self.height) as usize, 0);
            self.generation += 1;
        }
        self.shelves.push(Shelf { y: self.next_y, h: sh, x: pw });
        let p = (0, self.next_y);
        self.next_y += sh;
        p
    }

    fn blit(&mut self, x: u32, y: u32, w: u32, h: u32, src: &[u8]) {
        for r in 0..h as usize {
            let d = ((y as usize + r) * self.width as usize) + x as usize;
            self.pixels[d..d + w as usize].copy_from_slice(&src[r * w as usize..(r + 1) * w as usize]);
        }
        self.dirty_bytes += (w * h) as usize;
    }

    /// Fraction of texels reserved.
    pub fn utilization(&self) -> f32 {
        let used: u64 = self.shelves.iter().map(|s| s.h as u64 * s.x as u64).sum();
        used as f32 / (self.width as u64 * self.height as u64) as f32
    }
}

/// All strikes of one font plus the shared atlas.
pub struct GlyphCache {
    font: Vec<u8>,
    ctx: ScaleContext,
    pub atlas: Atlas,
    strikes: Vec<Strike>,
    pub rasterized: u64,
    /// Total ns spent in the rasterizer (misses only).
    pub raster_ns: u64,
}

struct Strike {
    size: f32,
    entries: Vec<Entry>,
    /// Entries of the 128 ASCII bytes (indexed by byte, not gid), filled lazily, for the
    /// fused kernel that never materializes a `GlyphRun`.
    ascii: Box<[Entry; 128]>,
}

pub type StrikeId = usize;

impl GlyphCache {
    pub fn new(font: &[u8]) -> Self {
        GlyphCache {
            font: font.to_vec(),
            ctx: ScaleContext::new(),
            atlas: Atlas::new(1024, 512),
            strikes: vec![],
            rasterized: 0,
            raster_ns: 0,
        }
    }

    /// The strike (glyph set) for a pixel size; created on first use.
    pub fn strike(&mut self, size: f32) -> StrikeId {
        if let Some(i) = self.strikes.iter().position(|s| s.size == size) {
            return i;
        }
        self.strikes.push(Strike { size, entries: vec![], ascii: Box::new([Entry::default(); 128]) });
        self.strikes.len() - 1
    }

    #[cold]
    #[inline(never)]
    fn rasterize(&mut self, strike: StrikeId, gid: u16) -> Entry {
        let t0 = std::time::Instant::now();
        let size = self.strikes[strike].size;
        let font = FontRef::from_index(&self.font, 0).expect("font");
        let mut scaler = self.ctx.builder(font).size(size).hint(false).build();
        let img = Render::new(&[Source::Outline]).format(Format::Alpha).render(&mut scaler, gid);
        let mut e = Entry { state: 1, ..Default::default() };
        if let Some(img) = img {
            if img.placement.width > 0 && img.placement.height > 0 && img.content == Content::Mask {
                let (w, h) = (img.placement.width, img.placement.height);
                let (x, y) = self.atlas.alloc(w, h);
                self.atlas.blit(x, y, w, h, &img.data);
                e = Entry {
                    geo: [img.placement.left as f32, -(img.placement.top as f32), w as f32, h as f32],
                    uv: [x as u16, y as u16, w as u16, h as u16],
                    state: 1,
                    _pad: 0,
                };
            }
        }
        let s = &mut self.strikes[strike];
        if s.entries.len() <= gid as usize {
            s.entries.resize(gid as usize + 1, Entry::default());
        }
        s.entries[gid as usize] = e;
        self.rasterized += 1;
        self.raster_ns += t0.elapsed().as_nanos() as u64;
        e
    }

    /// Entry for a glyph, rasterizing on a miss.
    #[inline]
    pub fn entry(&mut self, strike: StrikeId, gid: u16) -> Entry {
        let s = &self.strikes[strike];
        if let Some(e) = s.entries.get(gid as usize) {
            if e.state != 0 {
                return *e;
            }
        }
        self.rasterize(strike, gid)
    }

    /// Append one 48-byte instance per visible glyph of `run`, with the pen origin at
    /// `(ox, oy)` (baseline) in pixels. Horizontal positions snap to whole pixels (`oy` should
    /// be integral). Glyphs with empty bitmaps (spaces) emit no instance. Returns the number
    /// of instances appended.
    ///
    /// Per glyph: one gid load, one 32-byte entry load, a convert/fma/floor for the pen x, one
    /// vector add, three stores. Misses rasterize on the cold path.
    pub fn instances(
        &mut self,
        strike: StrikeId,
        run: &GlyphRun,
        ox: f32,
        oy: f32,
        color: u32,
        out: &mut Vec<Instance>,
    ) -> usize {
        let n = run.len();
        out.reserve(n);
        let start = out.len();
        let mut w = start;
        let scale = run.scale();
        let oxs = ox + 0.5;
        // kind | space << 16 packed above the colour: bytes 8..16 of the second vector.
        let tail: u64 = color as u64 | ((KIND_GLYPH as u64) << 32);
        let mut i = 0;
        while i < n {
            let entries = &self.strikes[strike].entries;
            let (eptr, elen) = (entries.as_ptr(), entries.len());
            let base = out.as_mut_ptr();
            // SAFETY: i < n == run.gid.len() == run.x.len(); `g < elen` and `state != 0` are
            // checked before `eptr.add(g)` is read; the output has capacity for `n` more
            // instances (reserved above) and `w - start <= i`.
            unsafe {
                while i < n {
                    let g = *run.gid.get_unchecked(i) as usize;
                    if g >= elen || (*eptr.add(g)).state == 0 {
                        break; // miss: rasterize below, then re-read the (possibly moved) table
                    }
                    let e = &*eptr.add(g);
                    let fx = (*run.x.get_unchecked(i) as f32).mul_add(scale, oxs).floor();
                    write_instance(base.add(w), e, fx, oy, tail);
                    w += (e.uv[2] != 0) as usize;
                    i += 1;
                }
            }
            if i < n {
                let g = run.gid[i];
                self.rasterize(strike, g);
            }
        }
        // SAFETY: slots start..w were written (empty glyphs are overwritten by the next one).
        unsafe { out.set_len(w) };
        w - start
    }
}

impl GlyphCache {
    /// Shape ASCII `text` and write its glyph instances in one pass, with no intermediate
    /// `GlyphRun`: per byte one dense-table step, one 32-byte entry and one 48-byte instance.
    /// Returns the number of instances appended, or `None` (and appends nothing) if the text
    /// holds a non-ASCII byte or a trigger and needs the full path.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_instances_ascii(
        &mut self,
        shaper: &Shaper,
        strike: StrikeId,
        text: &str,
        ox: f32,
        oy: f32,
        color: u32,
        out: &mut Vec<Instance>,
    ) -> Option<usize> {
        if !text.is_ascii() {
            return None;
        }
        let s = text.as_bytes();
        let n = s.len();
        let (dense, gids) = shaper.dense_tables();
        let scale = self.strikes[strike].size / shaper.script.upem as f32;
        let oxs = ox + 0.5;
        let tail: u64 = color as u64 | ((KIND_GLYPH as u64) << 32);
        out.reserve(n);
        let start = out.len();
        let mut w = start;
        let (mut x, mut acc) = (0i32, 0u32);
        let mut i = 0usize;
        while i < n {
            let ent = self.strikes[strike].ascii.as_ptr();
            let base = out.as_mut_ptr();
            // SAFETY: s is ASCII, so `a, b < 128`: `a << 7 | b < dense.len() = 128 * 128` and
            // `ent.add(a)` is inside the 128-entry table; the output has capacity for n more
            // instances and `w - start <= i < n`.
            unsafe {
                while i < n {
                    let a = *s.get_unchecked(i) as usize;
                    let e = &*ent.add(a);
                    if e.state == 0 {
                        break;
                    }
                    let b = if i + 1 < n { *s.get_unchecked(i + 1) as usize } else { 0 };
                    let step = *dense.get_unchecked(a << 7 | b) as u32;
                    acc |= step;
                    let fx = (x as f32).mul_add(scale, oxs).floor();
                    write_instance(base.add(w), e, fx, oy, tail);
                    w += (e.uv[2] != 0) as usize;
                    x += step as i32;
                    i += 1;
                }
            }
            if i < n {
                // Miss: rasterize this byte's glyph and copy its entry into the ASCII table.
                // (An uncovered byte is rasterized as .notdef-free gid 0; the trigger check
                // below rejects the string anyway.)
                let a = s[i] as usize;
                let e = self.entry(strike, gids[a]);
                self.strikes[strike].ascii[a] = e;
            }
        }
        if acc & 0x8000 != 0 {
            return None;
        }
        // SAFETY: slots start..w were written.
        unsafe { out.set_len(w) };
        Some(w - start)
    }
}

/// Write one glyph instance at `dst` (48 bytes).
///
/// # Safety
/// `dst` must be valid for 48 bytes of writes.
#[inline(always)]
unsafe fn write_instance(dst: *mut Instance, e: &Entry, fx: f32, oy: f32, tail: u64) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        let pos = vsetq_lane_f32::<1>(oy, vsetq_lane_f32::<0>(fx, vdupq_n_f32(0.)));
        let v0 = vaddq_f32(vld1q_f32(e.geo.as_ptr()), pos);
        let v1 = vreinterpretq_u8_u64(vcombine_u64(vld1_u64(e.uv.as_ptr().cast()), vcreate_u64(tail)));
        let p = dst as *mut u8;
        vst1q_u8(p, vreinterpretq_u8_f32(v0));
        vst1q_u8(p.add(16), v1);
        vst1q_u8(p.add(32), vdupq_n_u8(0));
    }
    #[cfg(not(target_arch = "aarch64"))]
    unsafe {
        dst.write(Instance {
            xy: [fx + e.geo[0], oy + e.geo[1]],
            wh: [e.geo[2], e.geo[3]],
            uv: e.uv,
            color: tail as u32,
            kind: KIND_GLYPH,
            space: 0,
            aux: [0; 4],
        });
    }
}
