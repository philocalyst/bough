//! The runtime tables behind the façade, laid out in ONE region (`bough_mem::Region`): on a GPU it is the
//! fused presenter's shared buffer, headless it is heap memory with the same layout. Every column the GPU
//! reads is carved from it once and written in place by the CPU (bough-metal `INTERFACE.md`, fused layout):
//!
//! | column | owner | GPU use |
//! |---|---|---|
//! | header, 3 frame slots | presenter | offsets, tile lists, row spans |
//! | quad instances (48 B) | here | `payload` = an instance, or a run of them (bars, glyph-instance text) |
//! | pages, directory ids/inks, clips | bough-core | hierarchical cull, record clip |
//! | glyph slots `x`/`ent`, run headers, spaces | bough-text | text runs expanded by the bin pass |
//! | strike token tables | bough-text | glyph geometry and atlas rects |
//!
//! Nothing is mirrored: a colour write is 4 bytes into an instance, a digit change is one `u16` per glyph
//! slot, a scroll is one space entry plus bough-core's page offsets.
//!
//! Damage has two rings, both bough-core `DamageRing`s advanced in lockstep: the Bough's own (every
//! geometry, structure and text mark) and the **paint** ring (paint-only writes: colours, glyph alphas).
//! The split exists for content addressing: a paint write's pixels are a function of the record and its
//! new state alone, so each epoch keeps a short log of its paint writes as (record, old state hash, new
//! state hash). A surface drawn at epoch `e` holds the current pixels iff no epoch since `e` had non-paint
//! damage and every record's composed change since `e` is the identity (`App::pick`): O(1) per paint
//! write, no per-tile state.
use crate::text::TextSys;
use bough_core::bough::Columns;
use bough_core::damage::DamageRing;
use bough_core::page::Page;
use bough_core::{Bough, Handle, NRect, RecordInit};
use bough_mem::{Col, Region};
use bough_metal::fused::{Layout, Masks, HDR_BYTES};
use bough_metal::instance::Instance;
use bough_text::atlas::TokStrike;
use bough_text::pool::{GlyphPool, PoolColumns, RunHeader};
use std::collections::VecDeque;
use std::ops::Range;

/// The geometry grid: quad and bar edges are multiples of 1/256 px. Every sum the kernels form from them
/// (position + integral space offset + size, ± integral margins, clip edges, radii on the grid) is then exact
/// in f32 for |v| < 2^15 px, whatever the compiler's association: an edge that did not move is bitwise the
/// same edge on the GPU, which is what makes edge-band damage exact. (1/256 px is far below the colour
/// accumulation's half precision.)
#[inline(always)]
pub fn snap(v: f32) -> f32 {
    (v * 256.0).round() * (1.0 / 256.0)
}

/// Damage band of a box primitive's edge (`Bough::damage_edges`): how far from an edge its pixels can
/// depend on it (solid: the pixel-centre test, 1/2 px; rounded: the radius and the 1 px anti-aliasing;
/// border: radius or border width), plus the ink's margin and 1 px of rounding slack.
#[inline]
pub fn edge_band(kind: u8, radius: f32, bw: f32) -> i16 {
    match kind {
        KIND_SOLID => 2,
        KIND_RRECT => (radius.max(0.5) + 1.0).ceil() as i16 + 2,
        _ => (radius.max(0.5 + bw) + 1.0).ceil() as i16 + 2,
    }
}

/// A static record's band under a scroll copy (`Bough::scroll_run_copy`): a single quad of a box kind has
/// `edge_band`; text, instance runs and every other kind (images, gradients, shadows) are not translation
/// invariant anywhere (-1: their whole visible ink).
#[inline]
fn scroll_band(inst: &Col<Instance>, kind: u8, payload: u32) -> i16 {
    if kind != REC_QUAD || payload & (bough_metal::fused::PAY_RUN | bough_metal::fused::PAY_IRUN) != 0 {
        return -1;
    }
    if payload as usize >= inst.len() {
        return -1;
    }
    let q = &inst[payload as usize];
    match q.kind {
        KIND_SOLID | KIND_RRECT | KIND_BORDER => edge_band(q.kind, q.radius, q.bw),
        _ => -1,
    }
}

/// [`edge_band`] for any instance, the decoration kinds included (deco.rs): an image or a gradient changes
/// everywhere when its box moves or resizes (its content is box-relative), so its band is the whole box; a
/// shadow's pixels depend on an edge only within its corner radius plus 4 sigma (the kernel's erf saturates
/// there and its y window is 3 sigma), plus the pixel-centre slack.
#[inline]
pub fn edge_band_of(p: &Instance) -> i16 {
    match p.kind {
        crate::deco::KIND_IMAGE | crate::deco::KIND_GRAD => (p.size[0].max(p.size[1]) + p.flags.max(1) as f32).ceil() as i16 + 2,
        crate::deco::KIND_SHADOW => (p.radius.min(0.5 * p.size[0].min(p.size[1])).max(0.0) + 4.0 * p.bw + 1.0).ceil() as i16 + 2,
        k => edge_band(k, p.radius, p.bw),
    }
}

pub const KIND_SOLID: u8 = 0;
pub const KIND_RRECT: u8 = 1;
pub const KIND_BORDER: u8 = 2;
pub const KIND_GLYPH: u8 = 3;

/// Record kinds (bough-core's `kind` column; the GPU dispatches on the payload, not on this).
pub const REC_QUAD: u8 = 1;
pub const REC_TEXT: u8 = 2;
/// A paint-less layout box (its record carries the layout structure; empty ink, no payload).
pub const REC_BOX: u8 = 3;

/// Node kinds (`Node::kind`).
pub const NK_QUAD: u8 = 1;
pub const NK_TEXT: u8 = 2;
pub const NK_BORDER: u8 = 3;
pub const NK_BARS: u8 = 4;
/// Layout-owned text (measured, wrapped and placed by the layout flush).
pub const NK_LTEXT: u8 = 5;
pub const NK_GALPHA: u8 = 6;
/// Flag on `Node::kind`: the record shows its hover colour (input.rs); a colour wire then stores the base
/// colour without writing the instance.
pub const NK_HOVERED: u8 = 0x80;

pub const NONE: u32 = u32::MAX;

/// The largest window side (device px): the damage planes' 128 tile rows.
pub const MAX_WINDOW: u32 = 4096;

/// Capacities of the region's columns (fixed for the window's life: a column never moves).
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub instances: usize,
    pub pages: usize,
    pub glyphs: usize,
    pub runs: usize,
    pub spaces: usize,
    pub clips: usize,
    pub strikes: usize,
}

impl Caps {
    /// Sized for the comparison scenes with headroom (heavy: ~2.6 K records / 2.3 K glyphs; vlist: ~6 K
    /// glyphs mounted). Untouched capacity costs no memory until written.
    pub const DEFAULT: Caps = Caps { instances: 8192, pages: 512, glyphs: 1 << 16, runs: 8192, spaces: 2048, clips: 512, strikes: 16 };

    /// Every capacity rounded up to a power of two: the GPU masks indices with `capacity - 1`, and the
    /// rounding is the phantom padding that keeps a masked index inside its own column (zero-filled: an
    /// all-zero instance, page, glyph or run header draws nothing).
    pub fn pow2(&self) -> Caps {
        let p = |n: usize| n.max(1).next_power_of_two();
        Caps { instances: p(self.instances), pages: p(self.pages), glyphs: p(self.glyphs), runs: p(self.runs), spaces: p(self.spaces), clips: p(self.clips), strikes: p(self.strikes) }
    }

    pub fn region_bytes(&self, w: u32, h: u32) -> usize {
        let caps = self.pow2();
        // frame slots sized for the tallest window (resize never moves them; 4.3 KB each)
        let sb = Layout::slot_bytes(w, h.max(MAX_WINDOW));
        let a = |n: usize| n.next_multiple_of(256) + 256;
        HDR_BYTES
            + 3 * a(sb)
            + a(caps.instances * size_of::<Instance>())
            + a(caps.pages * size_of::<Page>())
            + 2 * a(caps.pages * 8)
            + a(caps.clips * 8)
            + a(caps.spaces * 8)
            + a(caps.spaces * 32)
            + a(caps.glyphs * 4)
            + a(caps.glyphs * 2)
            + a(caps.runs * size_of::<RunHeader>())
            + a(caps.strikes * size_of::<TokStrike>())
            + 4096
    }
}

#[derive(Clone, Copy, Default)]
pub struct Run {
    pub first: u32,
    pub len: u32,
    /// log2 of the capacity class.
    pub class: u8,
    pub live: bool,
}

/// Mutable per-element state that wires patch. Quads and text share the struct (cold fields are small).
pub struct Node {
    pub handle: Handle,
    /// Instance run (quads, bars, glyph-instance text), or `NONE`.
    pub run: u32,
    pub space: u16,
    /// bough-core clip id of the element's record.
    pub clip: u16,
    pub kind: u8,
    pub live: bool,
    /// Quad: local px x, y(origin), w, h.
    pub x: f32,
    pub y: f32,
    /// Quad: space-local px y of the element's origin (the `span` operand is relative to it).
    pub y0: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
    pub color: u32,
    pub margin: f32,
    pub size_px: f32,
    pub line_h: f32,
    /// Series node (`bars`): memoised samples and the key they were computed for.
    pub series: Vec<f32>,
    pub series_key: u64,
    pub series_ok: bool,
    /// Text state (`Rt::texts` index), or `NONE`.
    pub text: u32,
    /// Layout node (index into `Rt::lay`) of an element laid out by cephal, else `NONE`.
    pub lnode: u32,
}

impl Node {
    fn blank() -> Node {
        Node {
            handle: Handle { idx: 0, generation: 0 },
            run: NONE,
            space: 0,
            clip: 0,
            kind: 0,
            live: false,
            x: 0.0,
            y: 0.0,
            y0: 0.0,
            w: 0.0,
            h: 0.0,
            radius: 0.0,
            color: 0,
            margin: 0.0,
            size_px: 0.0,
            line_h: 0.0,
            series: Vec::new(),
            series_key: 0,
            series_ok: false,
            text: NONE,
            lnode: NONE,
        }
    }
}

/// A wire's patch: `(runtime, wire, signal address, owner address)`.
///
/// # Safety
/// `sig` must point at a live `Signal<T>` of exactly the type the patch was monomorphised for (guaranteed by
/// `Wired`: the address is `owner + offset_of!(V, path)` and the field's type is checked at the `sig!` site);
/// `owner` must point at the live owner `V`.
pub type PatchFn = unsafe fn(&mut Rt, &Wire, *const u8, *const u8);

#[derive(Clone, Copy)]
pub struct Wire {
    /// Next wire of the same signal (0 = end).
    pub next: u32,
    pub dead: bool,
    pub node: u32,
    pub patch: PatchFn,
    pub recipe: [u8; 16],
}

unsafe fn nop(_: &mut Rt, _: &Wire, _: *const u8, _: *const u8) {}

/// Everything created while mounting one subtree that must be released when it unmounts.
#[derive(Default)]
pub struct Scope {
    pub nodes: Vec<u32>,
    pub wires: Vec<u32>,
    pub runs: Vec<u32>,
    /// Layout roots (groups) created while mounting; removed from the layout engine on release.
    pub lroots: Vec<u32>,
}

pub struct RowInfo {
    pub nrec: u32,
    pub space: u16,
    pub scope: Scope,
}

impl VRegion {
    /// The rows a scroll offset (points) mounts.
    #[inline]
    pub fn range_at(&self, scroll_pt: f32) -> Range<usize> {
        if self.count == 0 {
            return (self.visible)(scroll_pt);
        }
        let row_pt = self.row_h / self.scale_pt;
        let first = (scroll_pt / row_pt) as usize;
        let end = ((scroll_pt + self.port_h) / row_pt).ceil() as usize;
        first.saturating_sub(2)..end.saturating_add(2).min(self.count)
    }
}

pub type RowMountFn = unsafe fn(&mut Rt, *const u8, usize, usize, &mut Vec<RecordInit>) -> RowInfo;

pub struct VRegion {
    pub row_h: f32,
    pub vp_x: f32,
    pub vp_y: f32,
    pub clip: u16,
    /// The viewport (absolute device px): the scroll marks are clipped to it.
    pub vp: NRect,
    /// The clip the port lies in (a moved port is cut to it).
    pub parent_clip: u16,
    pub depth: u16,
    pub base_pos: usize,
    pub range: Range<usize>,
    pub rows: VecDeque<RowInfo>,
    pub scroll_px: f32,
    pub visible: fn(f32) -> Range<usize>,
    /// Port lists (`template::port_list`): row count and port height (points); the range is the rows
    /// meeting the port, two rows of overscan each side. 0: `visible` decides.
    pub count: usize,
    pub port_h: f32,
    /// Device px per point (`row_h` is in device px).
    pub scale_pt: f32,
    pub(crate) mount_row: RowMountFn,
    /// The user's `fn(&V, usize, &mut Mounter<V>)`, type-erased (transmuted back by `mount_row`).
    pub(crate) row_fn: usize,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Counters {
    pub wires_run: u64,
    pub text_set: u64,
    /// Line sets: lines shaped and lines whose block was kept (text round 3).
    pub lines_shaped: u64,
    pub lines_kept: u64,
    pub rows_mounted: u64,
    pub rows_unmounted: u64,
}

/// One epoch's paint writes: `(record key, old state hash, new state hash)`; past `PAINT_LOG` entries the
/// epoch is not content-addressable (`overflow`): continuous animation pays one compare per write.
pub const PAINT_LOG: usize = 16;
#[derive(Clone, Copy, Default)]
pub struct PaintLog {
    pub n: u8,
    pub overflow: bool,
    pub e: [(u64, u64, u64); PAINT_LOG],
}
impl PaintLog {
    #[inline]
    pub fn clear(&mut self) {
        self.n = 0;
        self.overflow = false;
    }
}

/// `bough_core::Plane` as bough-text's tile-mark sink: labels OR their damage rows straight into the
/// current epoch's plane (the row band stays exact).
pub struct CoreMarks<'a>(pub &'a mut bough_core::Plane);
impl bough_text::pool::Marks for CoreMarks<'_> {
    #[inline(always)]
    fn rows(&self) -> i32 {
        self.0.n as i32
    }
    #[inline(always)]
    fn or_rows(&mut self, r0: i32, r1: i32, m: u128) {
        self.0.or_rows(r0, r1, m)
    }
}

pub struct Rt {
    pub bough: Bough,
    pub glyphs: GlyphPool,
    pub text: TextSys,
    /// Quad (and glyph-instance) instances, the whole capacity addressable (zeroed memory is valid).
    pub inst: Col<Instance>,
    pub layout: Layout,
    pub caps: Caps,
    pub scale: f32,
    pub win_w: i32,
    pub win_h: i32,
    next_fresh: u32,
    free_blocks: [Vec<u32>; 16],
    pub runs: Vec<Run>,
    run_free: Vec<u32>,
    pub nodes: Vec<Node>,
    node_free: Vec<u32>,
    /// The wire arena and the owner offsets of wired signals: crate-private, because `settle` trusts them
    /// (every offset is an `offset_of!`-proven `Signal<_>` field, every recipe the closure its patch reads).
    pub(crate) wires: Vec<Wire>,
    wire_free: Vec<u32>,
    pub(crate) wired: Vec<u32>,
    pub texts: Vec<crate::text::TextNode>,
    text_free: Vec<u32>,
    /// Virtual-list regions: crate-private (each holds a type-erased row function its `mount_row` trusts).
    pub(crate) regions: Vec<VRegion>,
    clip_dedupe: Vec<(NRect, u16)>,
    space_free: Vec<u16>,
    next_space: u16,
    /// Per space: `[a-1, b, c, d-1, pivot x, pivot y, tx, ty]`, what the kernels apply to every instance and
    /// glyph run in the space (see `set_space_xform`).
    pub xforms: Col<[f32; 8]>,
    /// Paint-only damage, advanced with the Bough's ring.
    pub paint: DamageRing,
    /// Per epoch (`epoch % RING`): its paint writes, see the module docs.
    pub paint_log: [PaintLog; bough_core::damage::RING],
    pub tiles_x: usize,
    pub tiles_y: usize,
    pub ctr: Counters,
    /// The layout bridge (cephal over the mounted records) and the measured lists.
    pub lay: crate::layout::Lay,
    pub mregions: Vec<crate::mlist::MRegion>,
    /// Pointer, hover, focus, routes (input.rs).
    pub input: crate::input::Input,
    /// Running transitions and the frame clock (transition.rs).
    pub transitions: crate::transition::Transitions,
    /// Icons, rotated images and gradient ramps (deco.rs), uploaded before a frame that wrote them.
    pub images: crate::deco::Images,
    /// The mounted owner (boxed by `App`, never moves): what a layout placement hook needs to mount the rows
    /// of a list whose port it moved.
    pub(crate) owner: *const u8,
    /// Keeps a heap region alive (the GPU region is kept alive by the columns' reference count too).
    _region: Region,
}

impl Rt {
    /// The runtime over `region` (the presenter's shared buffer, or heap memory of the same size).
    pub fn new(win_w: i32, win_h: i32, scale: f32, font: &[u8], mut region: Region, caps: Caps) -> Rt {
        let caps = caps.pow2();
        let sb = Layout::slot_bytes(win_w as u32, (win_h as u32).max(MAX_WINDOW));
        let hdr = region.reserve(HDR_BYTES, 64);
        assert_eq!(hdr, 0, "the header is at offset 0");
        let mut slot = [0u32; 3];
        for s in &mut slot {
            *s = region.reserve(sb, 256) as u32;
        }
        let mut inst: Col<Instance> = region.carve("instances", caps.instances);
        // SAFETY: carved memory is zeroed (fresh buffer / alloc_zeroed) and all-zero is a valid Instance.
        unsafe { inst.set_len(caps.instances) };
        let cols = Columns { pages: region.carve("pages", caps.pages), dir_id: region.carve("dir_id", caps.pages), dir_ink: region.carve("dir_ink", caps.pages), clips: region.carve("clips", caps.clips) };
        let pcols = PoolColumns { x: region.carve("glyph_x", caps.glyphs), ent: region.carve("glyph_ent", caps.glyphs), runs: region.carve("runs", caps.runs), spaces: region.carve("spaces", caps.spaces) };
        let toks: Col<TokStrike> = region.carve("tok_strikes", caps.strikes);
        // per space: the affine of a transformed subtree (zeroed: the identity; `set_space_xform`)
        let mut xforms: Col<[f32; 8]> = region.carve("xforms", caps.spaces);
        // SAFETY: carved memory is zeroed and all-zero is the identity transform.
        unsafe { xforms.set_len(caps.spaces) };
        let bough = Bough::with_columns(win_w, win_h, cols);
        let mut glyphs = GlyphPool::with_columns(pcols);
        glyphs.set_space((caps.spaces - 1) as u16, [0, 0]); // the whole space column is addressable
        let text = TextSys::new(font, toks);
        let o = bough.gpu_offsets().expect("carved columns");
        let layout = Layout {
            w: win_w as u32,
            h: win_h as u32,
            inst_off: inst.region_offset().unwrap() as u32,
            spaces_off: glyphs.spaces.region_offset().unwrap() as u32,
            clips_off: o[3] as u32,
            dir_id_off: o[1] as u32,
            dir_ink_off: o[2] as u32,
            pages_off: o[0] as u32,
            gx_off: glyphs.x.region_offset().unwrap() as u32,
            gent_off: glyphs.ent.region_offset().unwrap() as u32,
            runs_off: glyphs.runs.region_offset().unwrap() as u32,
            toks_off: text.cache.tok_offset().unwrap() as u32,
            tok_stride: size_of::<TokStrike>() as u32,
            xforms_off: xforms.region_offset().unwrap() as u32,
            slot,
            masks: Masks {
                inst: caps.instances as u32 - 1,
                page: caps.pages as u32 - 1,
                glyph: caps.glyphs as u32 - 1,
                run: caps.runs as u32 - 1,
                space: caps.spaces as u32 - 1,
                clip: caps.clips as u32 - 1,
                strike: caps.strikes as u32 - 1,
            },
        };
        let (tx, ty) = (layout.tiles_x() as usize, layout.tiles_y() as usize);
        Rt {
            bough,
            glyphs,
            text,
            inst,
            layout,
            caps,
            scale,
            win_w,
            win_h,
            next_fresh: 0,
            free_blocks: Default::default(),
            runs: vec![Run::default()],
            run_free: Vec::new(),
            nodes: vec![Node::blank()],
            node_free: Vec::new(),
            wires: vec![Wire { next: 0, dead: true, node: 0, patch: nop, recipe: [0; 16] }],
            wire_free: Vec::new(),
            wired: Vec::new(),
            texts: Vec::new(),
            text_free: Vec::new(),
            regions: Vec::new(),
            clip_dedupe: Vec::new(),
            space_free: Vec::new(),
            next_space: 1,
            xforms,
            paint: DamageRing::new(win_w, win_h),
            paint_log: Default::default(),
            tiles_x: tx,
            tiles_y: ty,
            ctr: Counters::default(),
            lay: crate::layout::Lay::new(scale),
            mregions: Vec::new(),
            input: Default::default(),
            transitions: Default::default(),
            images: Default::default(),
            owner: std::ptr::null(),
            _region: region,
        }
    }

    /// The region's first byte (header and frame slots are addressed from it).
    pub fn region_base(&self) -> *mut u8 {
        self._region.base()
    }

    // ------------------------------------------------------------------ instances
    #[inline(always)]
    pub fn inst_mut(&mut self, i: u32) -> &mut Instance {
        &mut self.inst[i as usize]
    }

    /// Scroll by copy (bough-metal RESULTS §15): move records `[start, start + len)` by an integral `(0, dy)`
    /// inside viewport `vp` with `Bough::scroll_run_copy` (damage: the exposed strip, the static content's
    /// bands, this epoch's earlier marks also at their moved place) and the paint ring's earlier marks too.
    /// Positions are integral device px by construction: spaces are `[i32; 2]` and every scroll offset is
    /// rounded to device px before the delta is taken, so the copied pixels are the moved content exactly.
    pub fn scroll_copy(&mut self, start: usize, len: usize, dy: i32, vp: NRect) {
        self.paint.shift_within(vp, 0, dy);
        let inst = &self.inst;
        self.bough.scroll_run_copy(start, len, 0, dy, vp, |kind, payload| scroll_band(inst, kind, payload));
    }

    // ------------------------------------------------------------------ spaces and clips
    /// Set space `s`'s translation (device px; rounded: spaces are integral, text pens snap to pixels).
    #[inline]
    pub fn set_space(&mut self, s: u16, v: [f32; 2]) {
        self.glyphs.spaces[s as usize] = [v[0].round() as i32, v[1].round() as i32];
    }
    #[inline]
    pub fn space(&self, s: u16) -> [f32; 2] {
        let v = self.glyphs.spaces[s as usize];
        [v[0] as f32, v[1] as f32]
    }
    /// A clip id for an absolute device-px rect (deduplicated); 0 is the whole domain.
    pub fn clip_rect(&mut self, r: [f32; 4]) -> u16 {
        let n = NRect::new(r[0].floor() as i32, r[1].floor() as i32, r[2].ceil() as i32, r[3].ceil() as i32);
        if let Some(&(_, id)) = self.clip_dedupe.iter().find(|(c, _)| *c == n) {
            return id;
        }
        assert!(self.bough_clip_count() < self.caps.clips, "clip column full: raise Caps::clips");
        let id = self.bough.new_clip(n);
        self.clip_dedupe.push((n, id));
        id
    }
    fn bough_clip_count(&self) -> usize {
        self.clip_dedupe.len() + 1
    }
    pub fn alloc_space(&mut self) -> u16 {
        if let Some(s) = self.space_free.pop() {
            return s;
        }
        assert!((self.next_space as usize) < self.caps.spaces - 1, "space column full: raise Caps::spaces");
        self.next_space += 1;
        self.next_space - 1
    }
    /// The per-instance half of a transform: every instance and glyph run in space `s` is drawn at
    /// `T + c + m (p - c) + t` (T the space's translation, `m` row-major [a, b, c, d], pivot `c` space-local
    /// px). The record half is `Bough::set_run_xform` on the subtree's records (`transform_subtree`).
    pub fn set_space_xform(&mut self, s: u16, m: [f32; 4], c: [f32; 2], t: [f32; 2]) {
        self.xforms[s as usize] = [m[0] - 1.0, m[1], m[2], m[3] - 1.0, c[0], c[1], t[0], t[1]];
    }

    /// Transform the subtree of the record `h` (its records in space `s`): its records' inks become their
    /// transformed covers in the core (damage, hit testing, culling) and its instances are transformed by
    /// the kernels, both by the same affine about `c` (space-local px). A scroll of an enclosing view moves
    /// both afterwards (page offsets, the space's translation).
    pub fn transform_subtree(&mut self, h: Handle, s: u16, m: [f32; 4], c: [f32; 2], t: [f32; 2]) {
        self.set_space_xform(s, m, c, t);
        let Some(pos) = self.bough.pos_of(h) else { return };
        let end = self.bough.subtree_end(pos);
        let o = self.space(s);
        self.bough.set_run_xform(pos, end - pos, m, [o[0] + c[0], o[1] + c[1]], t);
    }

    pub fn free_space(&mut self, s: u16) {
        self.space_free.push(s);
    }

    // ------------------------------------------------------------------ instance runs
    /// A block of `1 << class` instances: a free block of that class, else a split of a larger free block, else
    /// fresh column (aligned to its size, so buddies pair up). Blocks are buddy-coalesced when freed, so a long
    /// run of rows of varying sizes (a list jumping about) cannot fragment the column away.
    fn alloc_block(&mut self, class: u8) -> u32 {
        if let Some(b) = self.free_blocks[class as usize].pop() {
            return b;
        }
        for c in class as usize + 1..16 {
            if let Some(b) = self.free_blocks[c].pop() {
                for lower in (class as usize..c).rev() {
                    self.free_blocks[lower].push(b + (1 << lower));
                }
                return b;
            }
        }
        let size = 1u32 << class;
        let b = (self.next_fresh + size - 1) & !(size - 1);
        assert!((b + size) as usize <= self.caps.instances, "instance column full ({}): raise Caps::instances", self.caps.instances);
        // the alignment gap becomes free blocks (largest aligned pieces that fit)
        let mut a = self.next_fresh;
        while a < b {
            let mut c = a.trailing_zeros().min(15);
            while a + (1 << c) > b {
                c -= 1;
            }
            self.free_blocks[c as usize].push(a);
            a += 1 << c;
        }
        self.next_fresh = b + size;
        b
    }
    fn free_block(&mut self, first: u32, class: u8) {
        let (mut b, mut c) = (first, class as usize);
        while c < 15 {
            let buddy = b ^ (1 << c);
            match self.free_blocks[c].iter().position(|&x| x == buddy) {
                Some(i) => {
                    self.free_blocks[c].swap_remove(i);
                    b = b.min(buddy);
                    c += 1;
                }
                None => break,
            }
        }
        self.free_blocks[c].push(b);
    }
    /// A run with capacity for at least `n` instances (power-of-two size classes, buddy blocks).
    pub fn alloc_run(&mut self, n: usize) -> u32 {
        let class = (n.max(1).next_power_of_two().trailing_zeros()) as u8;
        let first = self.alloc_block(class);
        let r = Run { first, len: 0, class, live: true };
        if let Some(i) = self.run_free.pop() {
            self.runs[i as usize] = r;
            i
        } else {
            self.runs.push(r);
            (self.runs.len() - 1) as u32
        }
    }
    pub fn free_run(&mut self, i: u32) {
        let r = self.runs[i as usize];
        if r.live {
            self.free_block(r.first, r.class);
            self.runs[i as usize].live = false;
            self.run_free.push(i);
        }
    }
    // ------------------------------------------------------------------ nodes and wires
    pub fn alloc_node(&mut self) -> u32 {
        let mut n = Node::blank();
        n.live = true;
        if let Some(i) = self.node_free.pop() {
            n.series = std::mem::take(&mut self.nodes[i as usize].series);
            self.nodes[i as usize] = n;
            i
        } else {
            self.nodes.push(n);
            (self.nodes.len() - 1) as u32
        }
    }
    pub fn free_node(&mut self, i: u32) {
        let t = std::mem::replace(&mut self.nodes[i as usize].text, NONE);
        if t != NONE {
            self.free_text(t);
        }
        self.nodes[i as usize].live = false;
        self.node_free.push(i);
    }
    pub(crate) fn alloc_text(&mut self, t: crate::text::TextNode) -> u32 {
        if let Some(i) = self.text_free.pop() {
            self.texts[i as usize] = t;
            i
        } else {
            self.texts.push(t);
            (self.texts.len() - 1) as u32
        }
    }
    fn free_text(&mut self, t: u32) {
        let tn = std::mem::take(&mut self.texts[t as usize]);
        crate::text::release(self, tn);
        self.text_free.push(t);
    }

    /// Install a wire on the signal whose head is `head` (the signal lives at `owner + off`).
    ///
    /// # Safety
    /// `off` must be the `offset_of!`-proven offset of a `Signal<T>` field of the owner type, `head` that
    /// signal's head, and `patch` monomorphised for that `T` (and `recipe` holding what it reads).
    pub(crate) unsafe fn add_wire(&mut self, head: &std::cell::Cell<u32>, off: u32, patch: PatchFn, node: u32, recipe: [u8; 16]) -> u32 {
        let h = head.get();
        let first = h & !crate::signal::QUEUED;
        if first == 0 {
            self.wired.push(off);
        }
        let w = Wire { next: first, dead: false, node, patch, recipe };
        let idx = if let Some(i) = self.wire_free.pop() {
            self.wires[i as usize] = w;
            i
        } else {
            self.wires.push(w);
            (self.wires.len() - 1) as u32
        };
        head.set(idx | (h & crate::signal::QUEUED));
        idx
    }

    /// Run every queued signal's wires. Dead wires are unlinked as they are met.
    ///
    /// # Safety
    /// `owner` must point at the live owner whose wired signals were installed through `Wired` (every
    /// offset in `wired` is an `offset_of!`-proven `Signal<_>` field of it; the head is the first field of
    /// every `Signal`, `#[repr(C)]`).
    pub unsafe fn settle(&mut self, owner: *const u8) {
        let mut k = 0;
        while k < self.wired.len() {
            let off = self.wired[k] as usize;
            k += 1;
            // SAFETY: see the function contract: a Signal<_> lives at owner + off, its head first.
            let sig = unsafe { owner.add(off) };
            let head = unsafe { &*(sig as *const std::cell::Cell<u32>) };
            let h = head.get();
            if h & crate::signal::QUEUED == 0 {
                continue;
            }
            head.set(h & !crate::signal::QUEUED);
            let mut i = h & !crate::signal::QUEUED;
            let mut prev = 0u32;
            while i != 0 {
                let w = self.wires[i as usize];
                if w.dead {
                    // unlink
                    if prev == 0 {
                        head.set(w.next | (head.get() & crate::signal::QUEUED));
                    } else {
                        self.wires[prev as usize].next = w.next;
                    }
                    self.wire_free.push(i);
                } else {
                    self.ctr.wires_run += 1;
                    // SAFETY: the wire was installed for this signal's type and the owner's type.
                    unsafe { (w.patch)(self, &w, sig, owner) };
                    prev = i;
                }
                i = w.next;
            }
        }
    }

    // ------------------------------------------------------------------ geometry helpers
    /// Absolute ink (device px, outward rounded, with `margin`) of a space-local rect.
    #[inline]
    pub fn ink_rect(&self, space: u16, x: f32, y: f32, w: f32, h: f32, margin: f32) -> NRect {
        let o = self.space(space);
        let x0 = (o[0] + x - margin).floor() as i32;
        let y0 = (o[1] + y - margin).floor() as i32;
        let x1 = (o[0] + x + w + margin).ceil() as i32;
        let y1 = (o[1] + y + h + margin).ceil() as i32;
        NRect::new(x0, y0, x1, y1)
    }

    /// Move a quad node's box to `(x, y, w, h)` (space-local px; snapped to the geometry grid here): node,
    /// instance and record ink are written, and the damage is only the bands around the edges that moved
    /// (`Bough::set_ink_handle_edges`), not old ∪ new. Edges are compared exactly: on the grid `x + w` is
    /// exact, so an edge that did not move is bitwise the edge the GPU computes (bough-metal RESULTS Part IV).
    #[inline]
    pub fn quad_geom(&mut self, node: u32, x: f32, y: f32, w: f32, h: f32) {
        let (x, y, w, h) = (snap(x), snap(y), snap(w), snap(h));
        let n = &self.nodes[node as usize];
        let moved = (n.x != x) as u8 | ((n.y != y) as u8) << 1 | ((n.x + n.w != x + w) as u8) << 2 | ((n.y + n.h != y + h) as u8) << 3;
        if moved == 0 {
            return;
        }
        let (inst, margin, hd, space) = (self.runs[n.run as usize].first, n.margin, n.handle, n.space);
        let n = &mut self.nodes[node as usize];
        (n.x, n.y, n.w, n.h) = (x, y, w, h);
        let p = self.inst_mut(inst);
        p.pos = [x, y];
        p.size = [w, h];
        let band = edge_band_of(p);
        let ink = self.ink_rect(space, x, y, w, h, margin);
        self.bough.set_ink_handle_edges(hd, ink, moved, band);
    }

    /// A paint-only write to `h`'s record (colour, glyph alphas) from state hash `old` to `new`: its visible
    /// ink goes to the paint ring and the write to the epoch's paint log.
    #[inline]
    pub fn paint_write(&mut self, h: Handle, old: u64, new: u64) {
        let Some(ink) = self.bough.visible_ink_handle(h) else { return };
        let ink = ink.intersect(self.bough.window()).canon();
        if ink.is_empty() {
            return;
        }
        self.paint.add_inside(ink);
        let l = &mut self.paint_log[self.paint.epoch as usize % bough_core::damage::RING];
        if (l.n as usize) < PAINT_LOG {
            l.e[l.n as usize] = ((h.idx as u64) << 32 | h.generation as u64, old, new);
            l.n += 1;
        } else {
            l.overflow = true;
        }
    }

    /// The text-mark sink of the current epoch.
    #[inline(always)]
    pub fn marks(&mut self) -> CoreMarks<'_> {
        CoreMarks(self.bough.plane_mut())
    }

    /// What the GPU draws for a record payload, expanded on the CPU with the shader's arithmetic (tests: a
    /// retained state compared with a fresh mount; glyph-pool placement and atlas positions differ between
    /// the two, so this is the comparable form). Each item: `(kind, x, y, w, h, color, color2, radius, bw,
    /// uv w, uv h)` with positions absolute (space added).
    pub fn expand_payload(&self, payload: u32) -> Vec<(u8, [f32; 4], u32, u32, f32, f32, [u16; 2])> {
        use bough_metal::fused::{PAY_IRUN, PAY_RUN};
        let mut out = Vec::new();
        let quad = |i: &Instance, out: &mut Vec<_>| {
            let sp = self.glyphs.spaces[i.space as usize];
            out.push((i.kind, [i.pos[0] + sp[0] as f32, i.pos[1] + sp[1] as f32, i.size[0], i.size[1]], i.color, i.color2, i.radius, i.bw, [i.uv[2], i.uv[3]]));
        };
        if payload & PAY_RUN != 0 {
            let (r0, rn) = (payload & 0xF_FFFF, ((payload >> 20) & 0x7FF) + 1);
            for r in r0..r0 + rn {
                let h = self.glyphs.runs[r as usize];
                let sp = self.glyphs.spaces[h.space as usize];
                let tok = self.text.cache.tok(h.strike as usize);
                for k in h.base..h.base + h.len {
                    let e = tok.entry[self.glyphs.ent[k as usize] as usize];
                    if e.uv[2] == 0 {
                        continue;
                    }
                    let px = (self.glyphs.x[k as usize].wrapping_sub(h.x0) as f32).mul_add(h.scale, h.origin[0] + 0.5).floor() + e.geo[0];
                    let p = [px + sp[0] as f32, h.origin[1] + e.geo[1] + sp[1] as f32, e.geo[2], e.geo[3]];
                    out.push((KIND_GLYPH, p, h.color, 0, 0.0, 0.0, [e.uv[2], e.uv[3]]));
                }
            }
        } else if payload & PAY_IRUN != 0 {
            let (i0, n) = (payload & 0x3_FFFF, ((payload >> 18) & 0xFFF) + 1);
            for i in i0..i0 + n {
                quad(&self.inst[i as usize], &mut out);
            }
        } else {
            quad(&self.inst[payload as usize], &mut out);
        }
        out
    }

    // ------------------------------------------------------------------ release
    pub fn release_scope(&mut self, s: Scope) {
        for w in s.wires {
            self.wires[w as usize].dead = true;
        }
        for n in s.nodes {
            self.free_node(n);
        }
        for r in s.runs {
            self.free_run(r);
        }
        for r in s.lroots {
            self.lay_remove_root(r);
        }
    }

    // ------------------------------------------------------------------ virtual-list regions
    fn region_remove(&mut self, reg: usize, front: bool, count: usize) {
        let mut nrec = 0usize;
        for _ in 0..count {
            let row = if front { self.regions[reg].rows.pop_front() } else { self.regions[reg].rows.pop_back() }.unwrap();
            nrec += row.nrec as usize;
            self.free_space(row.space);
            self.release_scope(row.scope);
            self.ctr.rows_unmounted += 1;
        }
        let r = &self.regions[reg];
        let start = if front { r.base_pos } else { r.base_pos + r.rows.iter().map(|x| x.nrec as usize).sum::<usize>() };
        self.bough.remove_run(start, nrec);
    }

    /// Mount rows `range` (ascending) and insert them at the front or the back of the region.
    unsafe fn region_mount(&mut self, reg: usize, owner: *const u8, range: Range<usize>, front: bool) {
        if range.is_empty() {
            return;
        }
        let mut recs: Vec<RecordInit> = Vec::with_capacity(range.len() * 64);
        let f = self.regions[reg].mount_row;
        let mut built: Vec<RowInfo> = Vec::with_capacity(range.len());
        for row in range {
            // SAFETY: `mount_row` was monomorphised for the owner's type at mount (`mount_row::<V>`).
            built.push(unsafe { f(self, owner, reg, row, &mut recs) });
            self.ctr.rows_mounted += 1;
        }
        let r = &mut self.regions[reg];
        let pos = if front { r.base_pos } else { r.base_pos + r.rows.iter().map(|x| x.nrec as usize).sum::<usize>() };
        if front {
            for b in built.into_iter().rev() {
                r.rows.push_front(b);
            }
        } else {
            r.rows.extend(built);
        }
        self.bough.insert_run(pos, &recs);
    }

    /// Initial rows of a region at mount time: rows are built into `recs` (the caller inserts them).
    ///
    /// # Safety
    /// `owner` must point at the live owner the region's row function was monomorphised for.
    pub unsafe fn region_initial(&mut self, reg: usize, owner: *const u8, scroll_pt: f32, recs: &mut Vec<RecordInit>) {
        let r = &mut self.regions[reg];
        r.scroll_px = (scroll_pt * self.scale).round();
        let range = r.range_at(scroll_pt);
        r.range = range.clone();
        let f = r.mount_row;
        for row in range {
            // SAFETY: as in `region_mount`.
            let info = unsafe { f(self, owner, reg, row, recs) };
            self.ctr.rows_mounted += 1;
            self.regions[reg].rows.push_back(info);
        }
    }

    pub fn region_row_space_origin(&self, reg: usize, row: usize) -> [f32; 2] {
        let r = &self.regions[reg];
        [r.vp_x, r.vp_y + row as f32 * r.row_h - r.scroll_px]
    }

    /// Scroll a region: unmount rows that left the range, move the survivors through their spaces (not
    /// rewritten) and bough-core's page offsets (`scroll_copy`: damage = the exposed strip and the static bands;
    /// the presenter copies the rest from the previous frame),
    /// mount rows that entered.
    ///
    /// # Safety
    /// As `region_initial`.
    pub unsafe fn region_scroll(&mut self, reg: usize, owner: *const u8, scroll_pt: f32) {
        let new_px = (scroll_pt * self.scale).round();
        let new_range = self.regions[reg].range_at(scroll_pt);
        let (old_px, old_range) = (self.regions[reg].scroll_px, self.regions[reg].range.clone());
        if new_px == old_px && new_range == old_range {
            return;
        }
        let disjoint = new_range.start >= old_range.end || new_range.end <= old_range.start;
        if disjoint {
            let n = self.regions[reg].rows.len();
            self.region_remove(reg, true, n);
            let r = &mut self.regions[reg];
            r.scroll_px = new_px;
            r.range = new_range.clone();
            unsafe { self.region_mount(reg, owner, new_range, false) };
            return;
        }
        let drop_front = new_range.start.saturating_sub(old_range.start);
        let drop_back = old_range.end.saturating_sub(new_range.end);
        if drop_front > 0 {
            self.region_remove(reg, true, drop_front);
        }
        if drop_back > 0 {
            self.region_remove(reg, false, drop_back);
        }
        let dy = (new_px - old_px) as i32;
        self.regions[reg].scroll_px = new_px;
        let (base, nrec, vp) = {
            let r = &self.regions[reg];
            (r.base_pos, r.rows.iter().map(|x| x.nrec as usize).sum::<usize>(), r.vp)
        };
        for i in 0..self.regions[reg].rows.len() {
            let s = self.regions[reg].rows[i].space as usize;
            self.glyphs.spaces[s][1] -= dy;
        }
        self.scroll_copy(base, nrec, -dy, vp);
        let surv_start = old_range.start.max(new_range.start);
        let surv_end = old_range.end.min(new_range.end);
        self.regions[reg].range = new_range.clone();
        unsafe {
            self.region_mount(reg, owner, new_range.start..surv_start, true);
            self.region_mount(reg, owner, surv_end..new_range.end, false);
        }
    }
}
