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
//! Damage is one ring of tile planes (bough-core `DamageRing`) fed from two forms. Geometry, structure and
//! text mark tiles as they happen. **Paint-only** writes (colours, glyph alphas) are record-space: a bit per
//! record in the epoch's paint plane (`Bough::mark_paint`), projected onto the epoch's tile plane once,
//! before the frame reads it (`paint_resolve`). The frame asks "did this epoch have damage other than paint"
//! before that projection, which is what content addressing needs: a paint write's pixels are a function of
//! the record and its new state alone, so each epoch keeps a short log of its paint writes as (record, old
//! state hash, new state hash). A surface drawn at epoch `e` holds the current pixels iff no epoch since `e`
//! had non-paint damage and every record's composed change since `e` is the identity (`App::pick`).
use crate::text::TextSys;
use bough_core::bough::Columns;
use bough_core::page::Page;
use bough_core::nrect::WRect;
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

/// Capacities of the façade's own (CPU-only) tables, derived from [`Caps`]: every table is a column carved
/// from one zeroed block (`Rt::new`), so it never moves and never copies what was mounted before (round 6,
/// lane `store`: type-stable storage). Untouched capacity is not resident.
#[derive(Clone, Copy, Debug)]
pub struct StoreCaps {
    /// Element ids (node id = layout id = record handle index).
    pub ids: usize,
    pub texts: usize,
    pub wires: usize,
    pub runs: usize,
    /// Words of the span arena.
    pub spans: usize,
}

impl StoreCaps {
    pub fn of(caps: &Caps) -> StoreCaps {
        // an id per record the pages can hold, twice over (handles of records not inserted yet, of static
        // texts, and of removed records whose ids were not reissued yet)
        let ids = (caps.pages * crate::rt::PAGE_SLOTS * 2).next_power_of_two();
        StoreCaps { ids, texts: ids, wires: ids * 2, runs: caps.instances, spans: ids * 16 }
    }
    fn bytes(&self) -> usize {
        let a = |n: usize| n.next_multiple_of(256) + 256;
        a(self.ids * size_of::<Node>())
            + a(self.ids * 16)
            + a(self.runs * size_of::<Run>())
            + a(self.texts * size_of::<crate::text::TextNode>())
            + a(self.wires * size_of::<Wire>())
            + a(self.ids * size_of::<crate::layout::LInfo>())
            + a(self.ids * 16)
            + a(self.spans * 4)
            + crate::lay_deep::Deep::column_bytes(self.ids, self.texts)
            + 4096
    }
}

/// Records per core page.
pub const PAGE_SLOTS: usize = 64;

/// A GPU-read column's offset in the region: the kernels address the region with 32-bit offsets (`FHdr`), so
/// every column the GPU reads must start below 4 GiB (the façade's CPU-only store block may be larger).
fn gpu_off(o: usize) -> u32 {
    u32::try_from(o).expect("GPU region offsets are u32: the kernels address the region with 32-bit offsets")
}

impl Caps {
    /// Sized for the comparison scenes with headroom (heavy: ~2.6 K records / 2.3 K glyphs; vlist: ~6 K
    /// glyphs mounted). With the direct-Metal presenter these are only minimums: see [`Caps::reserve`].
    pub const DEFAULT: Caps = Caps { instances: 8192, pages: 512, glyphs: 1 << 16, runs: 8192, spaces: 2048, clips: 512, strikes: 16 };

    /// The record format's own limits (round 6, lane memory): an instance run's first instance is 18 bits
    /// (`pay_irun`), a text record's first run header 20 bits (`pay_runs`); 2^15 pages hold two million
    /// records, 2^22 glyph slots four million glyphs, and a run header's strike is masked to 8 bits here. With
    /// column views (the direct-Metal presenter) a column's capacity is address space, so the region reserves
    /// these and a mount only ever reaches the format's limits, never a configuration's.
    pub const RESERVE: Caps = Caps { instances: 1 << 18, pages: 1 << 15, glyphs: 1 << 22, runs: 1 << 20, spaces: 1 << 12, clips: 1 << 12, strikes: 1 << 8 };

    /// The capacities a region with column views reserves: the larger of `self` and [`Caps::RESERVE`] for the
    /// columns that grow with content (spaces and clips keep `self`'s: their ids are 16 bits, and the space
    /// column is written whole at start).
    pub fn reserve(&self) -> Caps {
        let r = Caps::RESERVE;
        Caps {
            instances: self.instances.max(r.instances),
            pages: self.pages.max(r.pages),
            glyphs: self.glyphs.max(r.glyphs),
            runs: self.runs.max(r.runs),
            spaces: self.spaces,
            clips: self.clips,
            strikes: self.strikes.max(r.strikes),
        }
    }

    /// Every capacity rounded up to a power of two: the GPU masks indices with `capacity - 1`, and the
    /// rounding is the phantom padding that keeps a masked index inside its own column (zero-filled: an
    /// all-zero instance, page, glyph or run header draws nothing).
    pub fn pow2(&self) -> Caps {
        let p = |n: usize| n.max(1).next_power_of_two();
        Caps { instances: p(self.instances), pages: p(self.pages), glyphs: p(self.glyphs), runs: p(self.runs), spaces: p(self.spaces), clips: p(self.clips), strikes: p(self.strikes) }
    }

    pub fn region_bytes(&self, w: u32, h: u32) -> usize {
        let caps = self.pow2();
        // one instance per element id after the runs' region (`Rt::ibase`)
        let instances = (caps.instances + StoreCaps::of(&caps).ids).next_power_of_two();
        // frame slots sized for the tallest window (resize never moves them; 4.3 KB each)
        let sb = Layout::slot_bytes(w, h.max(MAX_WINDOW));
        // every column on a page boundary (`Region::carve_paged`: a column view is whole pages)
        let a = |n: usize| n.next_multiple_of(256) + 256 + bough_mem::vm_page();
        HDR_BYTES
            + 3 * a(sb)
            + a(instances * size_of::<Instance>())
            + a(caps.pages * size_of::<Page>())
            // the directory's page ids and inks: twice the page capacity (blocks of 64 with holes)
            + a(2 * caps.pages * 4)
            + a(2 * caps.pages * 8)
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

/// Per-element state, by element id (round 6, lane `store`).
///
/// **One index space**: an element's id is its record handle's index, which is also its layout node id
/// (`Lay::info`, the engine's nodes) and, for an element that draws one quad, the index of its GPU instance
/// (`Rt::inst_of`). **Columns by the pass that reads them**: what every wire and every walk reads first is
/// eight bytes (`Hot`); the handle's generation is its own column; a quad's geometry and colour are the
/// instance's own fields (the instance is the state: nothing mirrors it); what few elements have (a paint
/// offset, an opacity, a bar series) is a cold record (`Node`) the hot entry links to.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Hot {
    /// `NK_*`, with `NK_HOVERED`. 0: no element state at this id.
    pub kind: u8,
    /// `HF_*`.
    pub flags: u8,
    /// bough-core clip id of the element's record.
    pub clip: u16,
    /// A text element: its text state (`Rt::texts`). With `HF_COLD`: its cold record (`Rt::cold`).
    pub link: u32,
}
const _: () = assert!(size_of::<Hot>() == 8);

/// `Hot::link` is a cold record (bars, lanes, a quad under a paint offset / opacity / hover).
pub const HF_COLD: u8 = 1;
/// A quad whose drawn colour or position is not its described one: an opacity or a paint offset is
/// installed, or it is hovered. Its cold record holds the described colour and position.
pub const HF_MOD: u8 = 2;

/// The cold record of an element that needs one (`HF_COLD`): bars and lanes (their series and geometry
/// constants), and a quad under a paint modifier (`HF_MOD`: its described colour, opacity, paint offset and
/// untranslated position).
pub struct Node {
    /// The first instance of a run (bars, lanes).
    pub inst: u32,
    /// Its instance run (`Rt::runs`), or `NONE`.
    pub run: u32,
    pub space: u16,
    /// Quad: local px x, y(origin), w, h.
    pub x: f32,
    pub y: f32,
    /// Bars: space-local px y of the element's origin.
    pub y0: f32,
    pub w: f32,
    pub h: f32,
    pub color: u32,
    pub margin: f32,
    pub size_px: f32,
    pub line_h: f32,
    /// Series node (`bars`): its memoised samples (a span of `Rt::spans`, `series_n` floats) and the key
    /// they were computed for.
    pub series: u32,
    pub series_n: u32,
    pub series_key: u64,
    pub series_ok: bool,
    /// Box operands (`boxops.rs`): paint offset (device px) and opacity.
    pub tx: f32,
    pub ty: f32,
    pub alpha: f32,
}

impl Node {
    fn blank() -> Node {
        Node { inst: NONE, run: NONE, space: 0, x: 0.0, y: 0.0, y0: 0.0, w: 0.0, h: 0.0, color: 0, margin: 0.0, size_px: 0.0, line_h: 0.0, series: 0, series_n: 0, series_key: 0, series_ok: false, tx: 0.0, ty: 0.0, alpha: 1.0 }
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
    /// Runs while its source is held by a pending cohort (`latest(..)` operands, reader pending wires).
    pub latest: bool,
    pub node: u32,
    /// Address of the head the wire was linked into. `settle` runs a wire only from that exact head: a
    /// head moved elsewhere (a `Signal` swapped between owners or apps, a list row taken out and put back)
    /// carries indices that are not its own, and the chain is cut there instead of being run.
    pub head: usize,
    /// Owner class of the owner the wire's head lives in (`changed` is shown to that owner).
    pub cls: u32,
    pub patch: PatchFn,
    pub recipe: [u8; 16],
}

impl Drop for Rt {
    fn drop(&mut self) {
        crate::signal::LOG.with(|l| {
            if l as *const _ as usize == self.log_home {
                l.live.set(l.live.get() - 1);
            }
        })
    }
}

/// `View::changed` of an owner class, type-erased: `(owner, publications, committed readers)`.
pub type ChangedFn = unsafe fn(*mut u8, &crate::changed::Pubs, &[usize]);

/// # Safety
/// None needed: reads nothing (a patch for a wire whose cell has nothing to apply).
unsafe fn nop(_: &mut Rt, _: &Wire, _: *const u8, _: *const u8) {}

/// Index bits of a head (the rest are flags: `QUEUED`, `HELD`).
pub const HEAD_IDX: u32 = (1 << 30) - 1;

/// Everything created while mounting one subtree that must be released when it unmounts.
#[derive(Default)]
pub struct Scope {
    pub wires: Vec<u32>,
    /// Text states (`Rt::texts`) of the subtree's text elements.
    pub texts: Vec<u32>,
    /// Single instances (a quad's) and instance runs (`Rt::runs`: bars, lanes, glyph-instance text).
    pub insts: Vec<u32>,
    pub runs: Vec<u32>,
    /// Bar series' sample spans (`Rt::spans` offset, count).
    pub series: Vec<(u32, u32)>,
    /// Layout roots (groups) created while mounting; removed from the layout engine on release.
    pub lroots: Vec<u32>,
    /// Dynamic parts of a re-describing owner (`describe.rs`), released with their items.
    pub dyns: Vec<u32>,
    /// List regions mounted in this scope (`listview.rs`), released with their rows.
    pub lregs: Vec<u32>,
    /// Async producers declared in this scope (`reader.rs`), revoked with it.
    pub prods: Vec<u32>,
}

impl Scope {
    /// Heap bytes of the scope's lists (memory ledger).
    pub fn heap_bytes(&self) -> usize {
        (self.wires.capacity() + self.texts.capacity() + self.insts.capacity() + self.runs.capacity() + self.lroots.capacity() + self.dyns.capacity() + self.lregs.capacity() + self.prods.capacity()) * 4 + self.series.capacity() * 8
    }
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
    free_bits: [Vec<u64>; 16],
    pub runs: Col<Run>,
    run_free: Vec<u32>,
    /// Per element id: the hot entry, the handle generation, and one float only some kinds use (a raw
    /// quad's origin y, a decoration's ink margin). See [`Hot`].
    pub hot: Col<Hot>,
    pub hgen: Col<u32>,
    pub aux: Col<f32>,
    /// Cold records (`HF_COLD`), and the free ones.
    pub cold: Col<Node>,
    cold_free: Vec<u32>,
    /// Index of element id 0's instance: the instance column is `[runs and static quads | one per id]`.
    pub ibase: u32,
    /// The highest element id handed out (the per-id columns are touched up to it).
    max_id: u32,
    /// Handles of nodes that were allocated and given back before their record was made (an element that
    /// turned out static): the next `alloc_node` takes one.
    spare: Vec<Handle>,
    /// Variable-length element state (bar series, tapes, line windows): one word arena (`store.rs`).
    pub spans: crate::store::Spans,
    /// The wire arena and the owner offsets of wired signals: crate-private, because `settle` trusts them
    /// (every offset is an `offset_of!`-proven `Signal<_>` field, every recipe the closure its patch reads).
    pub(crate) wires: Col<Wire>,
    pub(crate) wire_free: Vec<u32>,
    /// Per owner class: the offsets of its wired cells (class 0 is the app's root owner; every list
    /// region adds one class shared by all its rows, `list.rs`).
    pub(crate) classes: Vec<Vec<u32>>,
    /// Per class, indexed by `offset / 4`: 1 + the cell's position in `classes[cls]`, or 0 where no wired
    /// cell starts. It answers "is this offset a wired cell" in one load (a wire being added; an address
    /// from the write log) and gives the log's entries the order a scan would run them in.
    pub(crate) class_rank: Vec<Vec<u32>>,
    /// The thread's write log this runtime was made on, and the log epoch it last scanned its owner at.
    log_home: usize,
    log_epoch: u32,
    /// `BOUGH_SETTLE_AUDIT`: after settling from the log, scan the owner and panic on a queued head.
    settle_audit: bool,
    /// Per owner class: its `View::changed` (rows and entity views of a `View` type; class 0 is the app's).
    pub(crate) class_changed: Vec<Option<ChangedFn>>,
    pub texts: Col<crate::text::TextNode>,
    text_free: Vec<u32>,
    /// Virtual-list regions: crate-private (each holds a type-erased row function its `mount_row` trusts).
    pub(crate) regions: Vec<VRegion>,
    clip_dedupe: Vec<(NRect, u16)>,
    space_free: Vec<u16>,
    next_space: u16,
    /// Per space: `[a-1, b, c, d-1, pivot x, pivot y, tx, ty]`, what the kernels apply to every instance and
    /// glyph run in the space (see `set_space_xform`).
    pub xforms: Col<[f32; 8]>,
    /// Nested spaces: a space allocated under another (`alloc_space_in`) moves with it (`set_space` /
    /// `shift_space` carry the delta to its descendants and to the list regions placed in them).
    space_parent: Vec<u16>,
    space_kids: Vec<Vec<u16>>,
    pub(crate) space_regs: Vec<Vec<u32>>,
    /// Per epoch (`epoch % RING`): its paint writes, see the module docs.
    pub paint_log: [PaintLog; bough_core::damage::RING],
    pub tiles_x: usize,
    pub tiles_y: usize,
    pub ctr: Counters,
    /// The layout bridge (cephal over the mounted records).
    pub lay: crate::layout::Lay,
    /// List regions (`listview.rs`).
    pub lists: crate::listview::Lists,
    /// Reflection (`reflect.rs`): wired fields' descriptors, element type names by record handle.
    pub reflect: crate::reflect::Reflect,
    /// Released subtrees whose resources return to the free lists at the next frames (`reclaim`).
    pub(crate) reclaim_q: Vec<Scope>,
    /// Dynamic parts of re-describing owners (`describe.rs`).
    pub dyns: crate::describe::Dyns,
    /// Entity views (`entity.rs`), async producers / cohort / executor (`reader.rs`), and this turn's
    /// publications for `View::changed` (`changed.rs`).
    pub(crate) eviews: Vec<crate::entity::EView>,
    pub asy: crate::reader::Async,
    pub(crate) pubs: crate::changed::Pubs,
    pub(crate) selects: crate::select::Selects,
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
    /// The block the façade's own tables are carved from (`StoreCaps`).
    _store: Region,
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
        // every GPU-read column starts on a page boundary: a presenter with column views wraps each one's used
        // prefix in its own buffer (bough-metal `Fused::new_views`)
        // the instance column: the runs' region (buddy blocks: bars, lanes, glyph text, static quads), then
        // one instance per element id (lane store: `Rt::ibase`)
        let icap = (caps.instances + StoreCaps::of(&caps).ids).next_power_of_two();
        let mut inst: Col<Instance> = region.carve_paged("instances", icap);
        // SAFETY: carved memory is zeroed (fresh mapping / buffer) and all-zero is a valid Instance.
        unsafe { inst.set_len(icap) };
        // the directory columns hold twice the page capacity: the core's directory is blocks of 64 entries with
        // holes after the live pages (32 to 64 live per block), so its length runs ahead of the page count; the
        // kernels clamp the directory scan to `2 * (m_page + 1)` entries
        let cols = Columns { pages: region.carve_paged("pages", caps.pages), dir_id: region.carve_paged("dir_id", 2 * caps.pages), dir_ink: region.carve_paged("dir_ink", 2 * caps.pages), clips: region.carve_paged("clips", caps.clips) };
        let pcols = PoolColumns { x: region.carve_paged("glyph_x", caps.glyphs), ent: region.carve_paged("glyph_ent", caps.glyphs), runs: region.carve_paged("runs", caps.runs), spaces: region.carve_paged("spaces", caps.spaces) };
        let toks: Col<TokStrike> = region.carve_paged("tok_strikes", caps.strikes);
        // per space: the affine of a transformed subtree (zeroed: the identity; `set_space_xform`)
        let mut xforms: Col<[f32; 8]> = region.carve_paged("xforms", caps.spaces);
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
            inst_off: gpu_off(inst.region_offset().unwrap()),
            spaces_off: gpu_off(glyphs.spaces.region_offset().unwrap()),
            clips_off: gpu_off(o[3]),
            dir_id_off: gpu_off(o[1]),
            dir_ink_off: gpu_off(o[2]),
            pages_off: gpu_off(o[0]),
            gx_off: gpu_off(glyphs.x.region_offset().unwrap()),
            gent_off: gpu_off(glyphs.ent.region_offset().unwrap()),
            runs_off: gpu_off(glyphs.runs.region_offset().unwrap()),
            toks_off: gpu_off(text.cache.tok_offset().unwrap()),
            tok_stride: size_of::<TokStrike>() as u32,
            xforms_off: gpu_off(xforms.region_offset().unwrap()),
            slot,
            masks: Masks {
                inst: icap as u32 - 1,
                page: caps.pages as u32 - 1,
                glyph: caps.glyphs as u32 - 1,
                run: caps.runs as u32 - 1,
                space: caps.spaces as u32 - 1,
                clip: caps.clips as u32 - 1,
                strike: caps.strikes as u32 - 1,
            },
            cols: Default::default(),
        };
        let mut layout = layout;
        {
            use bough_metal::fused::*;
            let d = |off: u32, elem: usize, cap: usize, group: usize| ColumnDesc { off, elem: elem as u32, reserve: u32::try_from(elem * cap).expect("a GPU column's reservation fits 32 bits"), group: group as u8 };
            let l = &mut layout;
            l.cols[COL_INST] = d(l.inst_off, size_of::<Instance>(), icap, G_INST);
            l.cols[COL_SPACES] = d(l.spaces_off, 8, caps.spaces, G_SPACE);
            l.cols[COL_CLIPS] = d(l.clips_off, 8, caps.clips, G_CLIP);
            // a page-group element of a directory column is a pair of entries (the columns hold 2x pages), so
            // a view of the page group's capacity covers the `2 * (m_page + 1)` entries the kernels may scan
            l.cols[COL_DIR_ID] = d(l.dir_id_off, 2 * 4, caps.pages, G_PAGE);
            l.cols[COL_DIR_INK] = d(l.dir_ink_off, 2 * 8, caps.pages, G_PAGE);
            l.cols[COL_PAGES] = d(l.pages_off, size_of::<Page>(), caps.pages, G_PAGE);
            l.cols[COL_GX] = d(l.gx_off, 4, caps.glyphs, G_GLYPH);
            l.cols[COL_GENT] = d(l.gent_off, 2, caps.glyphs, G_GLYPH);
            l.cols[COL_RUNS] = d(l.runs_off, size_of::<RunHeader>(), caps.runs, G_RUN);
            l.cols[COL_TOKS] = d(l.toks_off, size_of::<TokStrike>(), caps.strikes, G_STRIKE);
            l.cols[COL_XFORMS] = d(l.xforms_off, 32, caps.spaces, G_SPACE);
        }
        let (tx, ty) = (layout.tiles_x() as usize, layout.tiles_y() as usize);
        let sc = StoreCaps::of(&caps);
        let mut store = Region::heap(sc.bytes());
        let mut runs: Col<Run> = store.carve("runs", sc.runs);
        runs.push(Run::default());
        let mut hot: Col<Hot> = store.carve("element hot", sc.ids);
        let mut hgen: Col<u32> = store.carve("element generations", sc.ids);
        let mut aux: Col<f32> = store.carve("element aux", sc.ids);
        // SAFETY: mapped memory is zero and all-zero is a valid `Hot` / `u32` / `f32`: the whole id range is
        // addressable, an untouched id reads as "no element".
        unsafe {
            hot.set_len(sc.ids);
            hgen.set_len(sc.ids);
            aux.set_len(sc.ids);
        }
        let mut cold: Col<Node> = store.carve("element cold", sc.ids);
        cold.push(Node::blank());
        let mut wires: Col<Wire> = store.carve("wires", sc.wires);
        wires.push(Wire { next: 0, dead: true, latest: false, node: 0, head: 0, cls: 0, patch: nop, recipe: [0; 16] });
        let texts = store.carve("texts", sc.texts);
        let spans = crate::store::Spans::new(store.carve("spans", sc.spans));
        let lay = crate::layout::Lay::new(scale, &mut store, &sc);
        let reflect = crate::reflect::Reflect { types: store.carve("element types", sc.ids) };
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
            free_bits: Default::default(),
            runs,
            run_free: Vec::new(),
            hot,
            hgen,
            aux,
            cold,
            cold_free: Vec::new(),
            ibase: caps.instances as u32,
            max_id: 0,
            spare: Vec::new(),
            spans,
            wires,
            wire_free: Vec::new(),
            classes: vec![Vec::new()],
            class_rank: vec![Vec::new()],
            log_home: crate::signal::LOG.with(|l| {
                l.live.set(l.live.get() + 1);
                l as *const _ as usize
            }),
            // one behind: the first settle scans
            log_epoch: crate::signal::LOG.with(|l| l.epoch.get().wrapping_sub(1)),
            settle_audit: std::env::var_os("BOUGH_SETTLE_AUDIT").is_some(),
            class_changed: vec![None],
            texts,
            text_free: Vec::new(),
            regions: Vec::new(),
            clip_dedupe: Vec::new(),
            space_free: Vec::new(),
            next_space: 1,
            xforms,
            space_parent: Vec::new(),
            space_kids: Vec::new(),
            space_regs: Vec::new(),
            paint_log: Default::default(),
            tiles_x: tx,
            tiles_y: ty,
            ctr: Counters::default(),
            lay,
            lists: Default::default(),
            reclaim_q: Vec::new(),
            reflect,
            dyns: Default::default(),
            eviews: Vec::new(),
            asy: Default::default(),
            pubs: Default::default(),
            selects: Default::default(),
            input: Default::default(),
            transitions: Default::default(),
            images: Default::default(),
            owner: std::ptr::null(),
            _region: region,
            _store: store,
        }
    }

    /// The region's first byte (header and frame slots are addressed from it).
    pub fn region_base(&self) -> *mut u8 {
        self._region.base()
    }
    /// The region's memory and a keep-alive share of it (a presenter wraps it, or views of it).
    pub(crate) fn region_mem(&self) -> (*mut u8, usize, Box<dyn std::any::Any>) {
        (self._region.base(), self._region.len(), self._region.keeper())
    }

    /// Per mask group (bough-metal `G_*`), the elements the GPU may be pointed at: each GPU-read column's
    /// high-water mark. A presenter with column views must cover them before a frame
    /// (`Fused::ensure_columns`); loads and compares only.
    #[inline]
    pub fn column_needs(&self) -> [usize; bough_metal::fused::NGROUPS] {
        let (pages, dir, clips) = self.bough.gpu_extent();
        [
            // the runs' region's high-water mark, and the per-element instances after it (`ibase + id`)
            (self.next_fresh as usize).max(if self.max_id == 0 { 0 } else { (self.ibase + self.max_id) as usize + 1 }),
            pages.max(dir.div_ceil(2)),
            self.glyphs.x.len().max(self.glyphs.ent.len()),
            self.glyphs.runs.len(),
            self.next_space as usize + 1,
            clips,
            self.text.cache.strike_tables(),
        ]
    }

    /// Every table the runtime holds, as memory-ledger lines (diagnostics: walks the tables; never on a
    /// frame path). Region columns report their resident pages; heap blocks their capacities (mincore
    /// for the large ones).
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::{Content, Window};
        const G: &str = "runtime";
        let (rr, rd) = self._region.residency();
        l.push("region", "whole region (header, slots, every column)", bough_mem::Space::Region, Content, self._region.used(), self._region.len(), Some(rr), Some(rd));
        let slots = 3 * Layout::slot_bytes(self.layout.w, self.layout.h.max(MAX_WINDOW));
        l.push("region", "header + 3 frame slots", bough_mem::Space::Region, Window, HDR_BYTES + slots, HDR_BYTES + slots, None, None);
        // the instance and transform columns are addressable whole (`set_len(cap)`): what is in use is the
        // allocators' high-water marks
        // instances: the runs' region's high-water mark, then one per element id (`ibase + id`, lane store)
        let m = self.inst.mem();
        let ids = if self.max_id == 0 { 0 } else { self.max_id as usize + 1 };
        l.push(G, "instances", bough_mem::Space::Region, Content, (self.next_fresh as usize + ids) * m.elem, m.cap_bytes(), Some(m.resident), Some(m.dirty));
        let m = self.xforms.mem();
        l.push(G, "xforms", bough_mem::Space::Region, Content, self.next_space as usize * m.elem, m.cap_bytes(), Some(m.resident), Some(m.dirty));
        self.bough.ledger(l);
        self.glyphs.ledger(l);
        self.text.ledger(l);
        let sb = Scope::heap_bytes;
        // per-element state: columns at the element id, carved from the façade's store block (lane store)
        l.col(G, &self.hot, Content);
        l.col(G, &self.hgen, Content);
        l.col(G, &self.aux, Content);
        l.col(G, &self.cold, Content);
        l.vec(G, "cold record free list", &self.cold_free, self.cold_free.capacity(), Content);
        l.vec(G, "spare handles", &self.spare, self.spare.capacity(), Content);
        l.col(G, self.spans.column(), Content);
        l.col(G, &self.wires, Content);
        l.vec(G, "wire free list", &self.wire_free, self.wire_free.capacity(), Content);
        let cl: usize = self.classes.iter().map(|c| c.capacity() * 4).sum::<usize>() + self.classes.capacity() * 24 + self.class_changed.capacity() * 8;
        l.many(G, "owner classes", Content, cl, cl);
        l.col(G, &self.runs, Content);
        let fb: usize = self.free_blocks.iter().map(|v| v.capacity() * 4).sum::<usize>() + self.free_bits.iter().map(|v| v.capacity() * 8).sum::<usize>() + self.run_free.capacity() * 4;
        l.many(G, "instance free lists", Content, fb, fb);
        l.col(G, &self.texts, Content);
        l.vec(G, "text node free list", &self.text_free, self.text_free.capacity(), Content);
        let (mut strings, mut shapes) = (0usize, 0usize);
        for t in self.texts.iter() {
            strings += t.text.capacity();
            shapes += match &t.shape {
                crate::text::TShape::None | crate::text::TShape::Glyphs { .. } => 0,
                crate::text::TShape::Label(_) => size_of::<bough_text::pool::Label>(),
                crate::text::TShape::Set(s) => size_of::<bough_text::lines::LineSet>() + s.heap_bytes(),
                crate::text::TShape::Lines { widths, spans, .. } => widths.capacity() * 4 + spans.capacity() * 8,
            };
        }
        l.many(G, "text node strings", Content, strings, strings);
        l.many(G, "text node shapes (labels, line sets, line tables)", Content, shapes, shapes);
        let rg: usize = self.regions.iter().map(|r| r.rows.capacity() * size_of::<RowInfo>() + r.rows.iter().map(|x| sb(&x.scope)).sum::<usize>()).sum::<usize>() + self.regions.capacity() * size_of::<VRegion>();
        l.many(G, "virtual-list regions (raw)", Content, rg, rg);
        let sp: usize = (self.space_free.capacity() + self.space_parent.capacity()) * 2 + self.space_kids.iter().map(|k| k.capacity() * 2 + 24).sum::<usize>() + self.space_regs.iter().map(|k| k.capacity() * 4 + 24).sum::<usize>() + self.clip_dedupe.capacity() * 12;
        l.many(G, "spaces and clip dedupe", Content, sp, sp);
        let rq: usize = self.reclaim_q.iter().map(sb).sum::<usize>() + self.reclaim_q.capacity() * size_of::<Scope>();
        l.many(G, "reclaim queue", Content, rq, rq);
        self.lay.ledger(l);
        self.lists.ledger(l);
        self.dyns.ledger(l);
        l.col(G, &self.reflect.types, Content);
        self.images.ledger(l);
        self.input.ledger(l);
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
        let inst = &self.inst;
        self.bough.scroll_run_copy(start, len, 0, dy, vp, |kind, payload| scroll_band(inst, kind, payload));
    }

    /// A record's ink as the Bough holds it now (moves translate records, not the elements' cached inks).
    #[inline]
    pub(crate) fn cur_ink(&self, h: bough_core::Handle) -> NRect {
        self.bough.pos_of(h).map_or(NRect::EMPTY, |p| self.bough.ink_at(p))
    }

    // ------------------------------------------------------------------ spaces and clips
    /// Set space `s`'s translation (device px; rounded: spaces are integral, text pens snap to pixels). The
    /// spaces nested in it move with it.
    #[inline]
    pub fn set_space(&mut self, s: u16, v: [f32; 2]) {
        let new = [v[0].round() as i32, v[1].round() as i32];
        let old = self.glyphs.spaces[s as usize];
        self.glyphs.spaces[s as usize] = new;
        if new != old && (self.space_kids.get(s as usize).is_some_and(|k| !k.is_empty()) || self.space_regs.get(s as usize).is_some_and(|r| !r.is_empty())) {
            self.carry_space(s, new[0] - old[0], new[1] - old[1]);
        }
    }
    /// Move space `s` (and what is nested in it) by `(dx, dy)` device px.
    #[inline]
    pub fn shift_space(&mut self, s: u16, dx: i32, dy: i32) {
        let sp = &mut self.glyphs.spaces[s as usize];
        sp[0] += dx;
        sp[1] += dy;
        if (dx != 0 || dy != 0) && (self.space_kids.get(s as usize).is_some_and(|k| !k.is_empty()) || self.space_regs.get(s as usize).is_some_and(|r| !r.is_empty())) {
            self.carry_space(s, dx, dy);
        }
    }
    /// `s` moved by `(dx, dy)`: so do its descendants; the list regions placed in any of them keep their rows
    /// where they are (their spaces moved, and the records with the move's own `scroll_run`) and re-window
    /// (what they show may have changed: an exposure).
    fn carry_space(&mut self, s: u16, dx: i32, dy: i32) {
        let mut stack = vec![s];
        while let Some(x) = stack.pop() {
            if x != s {
                let sp = &mut self.glyphs.spaces[x as usize];
                sp[0] += dx;
                sp[1] += dy;
            }
            if let Some(regs) = self.space_regs.get(x as usize) {
                for &r in regs {
                    let reg = &mut self.lists.regs[r as usize];
                    reg.placed_at[0] += dx as i64;
                    reg.placed_at[1] += dy as i64;
                    reg.dirty = true;
                }
            }
            if let Some(k) = self.space_kids.get(x as usize) {
                stack.extend_from_slice(k);
            }
        }
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

    /// A space nested in `parent` (it moves with it).
    pub fn alloc_space_in(&mut self, parent: u16) -> u16 {
        let s = self.alloc_space();
        let n = s.max(parent) as usize + 1;
        if self.space_parent.len() < n {
            self.space_parent.resize(n, u16::MAX);
            self.space_kids.resize_with(n, Vec::new);
        }
        self.space_parent[s as usize] = parent;
        self.space_kids[parent as usize].push(s);
        s
    }
    pub fn free_space(&mut self, s: u16) {
        if let Some(&p) = self.space_parent.get(s as usize) {
            if p != u16::MAX {
                let k = &mut self.space_kids[p as usize];
                if let Some(i) = k.iter().rposition(|&x| x == s) {
                    k.swap_remove(i);
                }
                self.space_parent[s as usize] = u16::MAX;
            }
            for c in std::mem::take(&mut self.space_kids[s as usize]) {
                self.space_parent[c as usize] = u16::MAX;
            }
        }
        self.space_free.push(s);
    }
    /// List region `reg` is placed in space `s` (it re-windows when `s` moves).
    pub(crate) fn space_reg(&mut self, s: u16, reg: u32, on: bool) {
        if self.space_regs.len() <= s as usize {
            self.space_regs.resize_with(s as usize + 1, Vec::new);
        }
        let v = &mut self.space_regs[s as usize];
        if on {
            v.push(reg);
        } else if let Some(i) = v.iter().position(|&x| x == reg) {
            v.swap_remove(i);
        }
    }

    // ------------------------------------------------------------------ instance runs
    #[inline]
    fn bfree(&self, c: usize, b: u32) -> bool {
        let i = (b >> c) as usize;
        self.free_bits[c].get(i / 64).is_some_and(|w| w >> (i % 64) & 1 != 0)
    }
    #[inline]
    fn set_bfree(&mut self, c: usize, b: u32, v: bool) {
        let i = (b >> c) as usize;
        let w = &mut self.free_bits[c];
        if w.len() <= i / 64 {
            w.resize(i / 64 + 1, 0);
        }
        if v {
            w[i / 64] |= 1 << (i % 64);
        } else {
            w[i / 64] &= !(1 << (i % 64));
        }
    }
    fn push_free(&mut self, c: usize, b: u32) {
        self.set_bfree(c, b, true);
        self.free_blocks[c].push(b);
    }
    /// Pop a free block of class `c` (the stacks keep stale entries of blocks merged since: the bitmap is
    /// the truth, so a buddy lookup is one bit test instead of a free-list scan).
    fn pop_free(&mut self, c: usize) -> Option<u32> {
        while let Some(b) = self.free_blocks[c].pop() {
            if self.bfree(c, b) {
                self.set_bfree(c, b, false);
                return Some(b);
            }
        }
        None
    }
    /// A block of `1 << class` instances: a free block of that class, else a split of a larger free block, else
    /// fresh column (aligned to its size, so buddies pair up). Blocks are buddy-coalesced when freed, so a long
    /// run of rows of varying sizes (a list jumping about) cannot fragment the column away.
    fn alloc_block(&mut self, class: u8) -> u32 {
        if let Some(b) = self.pop_free(class as usize) {
            return b;
        }
        for c in class as usize + 1..16 {
            if let Some(b) = self.pop_free(c) {
                for lower in (class as usize..c).rev() {
                    self.push_free(lower, b + (1 << lower));
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
            self.push_free(c as usize, a);
            a += 1 << c;
        }
        self.next_fresh = b + size;
        b
    }
    fn free_block(&mut self, first: u32, class: u8) {
        let (mut b, mut c) = (first, class as usize);
        while c < 15 {
            let buddy = b ^ (1 << c);
            if !self.bfree(c, buddy) {
                break;
            }
            self.set_bfree(c, buddy, false);
            b = b.min(buddy);
            c += 1;
        }
        self.push_free(c, b);
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
    /// A new element: a record handle, and blank state at its index (the element id).
    pub fn alloc_node(&mut self) -> u32 {
        let h = match self.spare.pop() {
            Some(h) => h,
            None => self.bough.new_handle(),
        };
        self.node_at(h)
    }
    /// Blank state for the element whose record handle is `h` (the element id is `h.idx`).
    #[inline]
    pub fn node_at(&mut self, h: Handle) -> u32 {
        let i = h.idx as usize;
        assert!(i < self.hot.len(), "element ids exhausted ({}): raise Caps::pages", self.hot.len());
        // the id's previous element is gone (its handle was released): so is its cold record
        self.drop_cold(i);
        self.hot[i] = Hot::default();
        self.hgen[i] = h.generation;
        self.max_id = self.max_id.max(h.idx);
        h.idx
    }
    /// Give back an element whose record will carry no handle (it turned out static): the handle is kept
    /// for the next `alloc_node`.
    pub fn drop_node(&mut self, i: u32) {
        self.drop_cold(i as usize);
        self.hot[i as usize] = Hot::default();
        self.spare.push(Handle { idx: i, generation: self.hgen[i as usize] });
    }
    #[inline]
    fn drop_cold(&mut self, i: usize) {
        let old = self.hot[i];
        if old.flags & HF_COLD != 0 {
            self.cold_free.push(old.link);
        }
    }
    /// The record handle of element `i`.
    #[inline(always)]
    pub fn handle(&self, i: u32) -> Handle {
        Handle { idx: i, generation: self.hgen[i as usize] }
    }
    /// Is the state at id `i` that of a current element (its handle is the live handle of index `i`)?
    #[inline]
    pub fn node_live(&self, i: u32) -> bool {
        i != 0 && self.hot.get(i as usize).is_some_and(|n| n.kind != 0) && self.bough.handle_live(self.handle(i))
    }
    /// `node_live` for an element being mounted (its handle is allocated, its record not inserted yet).
    #[inline]
    pub(crate) fn node_pending(&self, i: u32) -> bool {
        self.node_live(i)
    }
    /// The instance of element `i` that draws one quad: its index in the instance column is its id.
    #[inline(always)]
    pub fn inst_of(&self, i: u32) -> u32 {
        self.ibase + i
    }
    /// The text state of text element `i`.
    #[inline(always)]
    pub fn text_of(&self, i: u32) -> u32 {
        self.hot[i as usize].link
    }
    /// A cold record for element `i` (`HF_COLD`), blank.
    pub fn alloc_cold(&mut self, i: u32) -> &mut Node {
        let c = match self.cold_free.pop() {
            Some(c) => {
                self.cold[c as usize] = Node::blank();
                c
            }
            None => {
                self.cold.push(Node::blank());
                (self.cold.len() - 1) as u32
            }
        };
        let h = &mut self.hot[i as usize];
        h.flags |= HF_COLD;
        h.link = c;
        &mut self.cold[c as usize]
    }
    /// The cold record of element `i` (which has one).
    #[inline]
    pub fn cold_of(&self, i: u32) -> &Node {
        let h = self.hot[i as usize];
        debug_assert!(h.flags & HF_COLD != 0);
        &self.cold[h.link as usize]
    }
    #[inline]
    pub fn cold_mut(&mut self, i: u32) -> &mut Node {
        let h = self.hot[i as usize];
        debug_assert!(h.flags & HF_COLD != 0);
        &mut self.cold[h.link as usize]
    }
    /// The cold record of quad `i` under a paint modifier, made on first use from what the instance holds
    /// (the instance was the state until now): described colour, position, no offset, full opacity.
    pub fn mod_of(&mut self, i: u32) -> &mut Node {
        if self.hot[i as usize].flags & HF_MOD == 0 {
            let p = self.inst[self.inst_of(i) as usize];
            let base = if p.kind == KIND_BORDER { p.color2 } else { p.color };
            let c = self.alloc_cold(i);
            (c.color, c.x, c.y) = (base, p.pos[0], p.pos[1]);
            self.hot[i as usize].flags |= HF_MOD;
        }
        self.cold_mut(i)
    }
    /// The described colour of quad `i` (its fill): the instance's, or under a paint modifier the cold
    /// record's.
    pub fn quad_color(&self, i: u32) -> u32 {
        let h = self.hot[i as usize];
        if h.flags & HF_MOD != 0 {
            return self.cold_of(i).color;
        }
        let p = &self.inst[self.inst_of(i) as usize];
        if h.kind & !NK_HOVERED == NK_BORDER { p.color2 } else { p.color }
    }
    /// The ink margin of quad `i`: a decoration's own (`aux`), else what its instance's kind draws outside
    /// its box (anti-aliasing of a rounded or bordered edge: 1 px; a solid: none).
    #[inline(always)]
    pub fn margin_of(&self, i: u32, p: &Instance) -> f32 {
        if self.hot[i as usize].kind & !NK_HOVERED == crate::deco::NK_DECO {
            self.aux[i as usize]
        } else if p.kind == KIND_SOLID {
            0.0
        } else {
            1.0
        }
    }
    /// One instance of a static quad (no element state): a block of the instance column.
    #[inline]
    pub fn alloc_inst(&mut self) -> u32 {
        self.alloc_block(0)
    }
    #[inline]
    pub fn free_inst(&mut self, i: u32) {
        self.free_block(i, 0);
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

    /// Install a wire on the cell whose head is `head` (the cell lives at `owner + off`, owner class `cls`).
    ///
    /// # Safety
    /// `off` must be the `offset_of!`-proven offset of a headed cell field of the class's owner type, `head`
    /// that cell's head, and `patch` monomorphised for the cell's type (and `recipe` holding what it reads).
    pub(crate) unsafe fn add_wire(&mut self, head: &std::cell::Cell<u32>, off: u32, cls: u32, patch: PatchFn, node: u32, recipe: [u8; 16]) -> u32 {
        let addr = head as *const _ as usize;
        let h = head.get();
        let mut first = h & HEAD_IDX;
        // a chain that is not this head's own (moved in from elsewhere) is dropped, never extended
        if first != 0 && self.wires.get(first as usize).is_none_or(|w| w.head != addr) {
            first = 0;
        }
        let (c, r) = (&mut self.classes[cls as usize], &mut self.class_rank[cls as usize]);
        debug_assert!(off % 4 == 0);
        let k = (off / 4) as usize;
        if k >= r.len() {
            r.resize(k + 1, 0);
        }
        if r[k] == 0 {
            c.push(off);
            r[k] = c.len() as u32;
        }
        // `latest` wires (producers, `latest(..)` operands) stay at the front of the chain: they run before a
        // held source's other wires are skipped (`run_head`), whatever the declaration order
        let mut after = 0u32;
        let mut p = first;
        while p != 0 && self.wires[p as usize].latest && self.wires[p as usize].head == addr {
            after = p;
            p = self.wires[p as usize].next;
        }
        let w = Wire { next: if after == 0 { first } else { p }, dead: false, latest: false, node, head: addr, cls, patch, recipe };
        let idx = if let Some(i) = self.wire_free.pop() {
            self.wires[i as usize] = w;
            i
        } else {
            self.wires.push(w);
            (self.wires.len() - 1) as u32
        };
        assert!(idx <= HEAD_IDX, "wire arena full");
        if after == 0 {
            head.set(idx | (h & !HEAD_IDX));
        } else {
            self.wires[after as usize].next = idx;
        }
        idx
    }

    /// A new owner class (list rows, entities): its wired offsets start empty.
    pub(crate) fn new_class(&mut self) -> u32 {
        self.classes.push(Vec::new());
        self.class_rank.push(Vec::new());
        self.class_changed.push(None);
        (self.classes.len() - 1) as u32
    }

    /// Has `settle` nothing to do? True iff the thread's write log is this runtime's, is empty and has not
    /// overflowed since the last settle, no producer asked for another scan and no list waits for its end of
    /// turn: exactly the state in which `settle` runs its loop once over nothing. (The audit always settles.)
    #[inline(always)]
    pub(crate) fn settled(&self) -> bool {
        !self.settle_audit
            && !self.asy.rescan
            && self.lists.ends.is_empty()
            && crate::signal::LOG.with(|l| l as *const _ as usize == self.log_home && l.n.get() == 0 && l.epoch.get() == self.log_epoch)
    }

    /// Run every queued cell's wires of the root owner. Dead wires are unlinked as they are met.
    ///
    /// # Safety
    /// `owner` must point at the live root owner whose wired cells were installed through `sig!` proofs
    /// (every offset in class 0 is an `offset_of!`-proven headed field of it; the head is the first field of
    /// every cell, `#[repr(C)]`), and be derived from a unique borrow (list patches write their cell).
    pub unsafe fn settle(&mut self, owner: *const u8) {
        // waves: a wire may queue further cells of the owner (bounded: a cycle is left for the next turn)
        for _ in 0..64 {
            // SAFETY: this fn's contract (`owner` is the live root owner).
            let more = unsafe { self.settle_logged(owner) };
            if !std::mem::take(&mut self.asy.rescan) && !more {
                break;
            }
            // the wave ran wires: another only if they queued something (the log says so)
            if !self.settle_audit && self.lists.ends.is_empty() && self.settled() {
                break;
            }
        }
        if self.settle_audit {
            for &off in &self.classes[0] {
                let h = unsafe { &*(owner.add(off as usize) as *const std::cell::Cell<u32>) }.get();
                assert!(h & crate::signal::QUEUED == 0 || h & crate::signal::HELD != 0, "settle left a queued head at offset {off}");
            }
        }
        self.lists_end_turn();
    }

    /// One wave of the root owner's settle, from the thread's write log (`signal.rs`): the logged heads that
    /// are wired cells of `owner` run in the order a scan would meet them. Returns whether any ran. A scan of
    /// the whole class replaces it when the log cannot be trusted to hold every write: it overflowed since
    /// this runtime last scanned, or the runtime is settling on another thread than it was made on.
    ///
    /// # Safety
    /// As `settle`.
    unsafe fn settle_logged(&mut self, owner: *const u8) -> bool {
        use crate::signal::{LOG, LOG_CAP};
        // (rank, offset) of the logged cells that are this owner's; only `run[..n]` is ever read
        let mut run = [const { std::mem::MaybeUninit::<(u32, u32)>::uninit() }; LOG_CAP];
        let mut n = 0;
        let rank = &self.class_rank[0];
        let mut away = false;
        let scan = LOG.with(|l| {
            if l as *const _ as usize != self.log_home {
                away = true;
                return true;
            }
            let scan = l.epoch.get() != self.log_epoch;
            self.log_epoch = l.epoch.get();
            let (len, mut keep) = (l.n.get() as usize, 0);
            for i in 0..len {
                let a = l.at[i].get();
                let off = a.wrapping_sub(owner as usize);
                // a wired cell of this owner (an address is only ever dereferenced through this test)
                let r = if off % 4 == 0 { rank.get(off / 4).copied().unwrap_or(0) } else { 0 };
                if r != 0 {
                    run[n].write((r, off as u32));
                    n += 1;
                } else if l.live.get() > 1 {
                    // another runtime's on this thread: its to take
                    l.at[keep].set(a);
                    keep += 1;
                }
            }
            l.n.set(keep as u32);
            scan
        });
        if scan {
            unsafe { self.settle_cls(owner, 0) };
            // what the scan's wires queued is in the log (not when away: the next turn's scan finds it)
            return !away;
        }
        // SAFETY: the first `n` entries were written above.
        let run = unsafe { &mut *(&mut run[..n] as *mut [std::mem::MaybeUninit<(u32, u32)>] as *mut [(u32, u32)]) };
        // scan order (insertion sort: a handful of entries)
        for i in 1..n {
            let mut j = i;
            while j > 0 && run[j - 1].0 > run[j].0 {
                run.swap(j - 1, j);
                j -= 1;
            }
        }
        for &(_, off) in run.iter() {
            let sig = unsafe { owner.add(off as usize) };
            let head = unsafe { &*(sig as *const std::cell::Cell<u32>) };
            if head.get() & crate::signal::QUEUED != 0 {
                crate::span!(2 + crate::attr::NEST, unsafe { self.run_head(head, sig, owner) });
            }
        }
        n != 0
    }

    /// `settle` for the owner at `owner` of class `cls` (a list row, an entity).
    ///
    /// # Safety
    /// As `settle`, for the class's owner type.
    pub(crate) unsafe fn settle_cls(&mut self, owner: *const u8, cls: u32) {
        let mut k = 0;
        while k < self.classes[cls as usize].len() {
            let off = self.classes[cls as usize][k] as usize;
            k += 1;
            // SAFETY: see the function contract: a headed cell lives at owner + off, its head first.
            let sig = unsafe { owner.add(off) };
            let head = unsafe { &*(sig as *const std::cell::Cell<u32>) };
            let h = head.get();
            if h & crate::signal::QUEUED == 0 {
                continue;
            }
            // SAFETY: the contract again: `head` is the head of the cell at `sig`, in the live owner `owner`.
            unsafe { self.run_head(head, sig, owner) };
        }
    }

    /// Run the wires of one queued head (clears QUEUED; a HELD head runs only its `latest` wires).
    ///
    /// # Safety
    /// `sig` is the cell whose head is `head`, inside the live owner `owner` its wires were installed for.
    pub(crate) unsafe fn run_head(&mut self, head: &std::cell::Cell<u32>, sig: *const u8, owner: *const u8) {
        let addr = head as *const _ as usize;
        let h = head.get();
        head.set(h & !crate::signal::QUEUED);
        let mut i = h & HEAD_IDX;
        let mut prev = 0u32;
        while i != 0 {
            let w = match self.wires.get(i as usize) {
                Some(w) if w.head == addr => *w,
                // not this head's chain (see `Wire::head`): cut it here
                _ => {
                    if prev == 0 {
                        head.set(head.get() & !HEAD_IDX);
                    } else {
                        self.wires[prev as usize].next = 0;
                    }
                    break;
                }
            };
            if w.dead {
                // unlink
                if prev == 0 {
                    head.set(w.next | (head.get() & !HEAD_IDX));
                } else {
                    self.wires[prev as usize].next = w.next;
                }
                self.wires[i as usize].head = 0;
                self.wire_free.push(i);
            } else {
                // re-read: a producer running first may have just put this source on hold
                if head.get() & crate::signal::HELD == 0 || w.latest {
                    self.ctr.wires_run += 1;
                    // SAFETY: the wire was installed for this cell's type and the owner's type.
                    crate::span!(3 + crate::attr::NEST, unsafe { (w.patch)(self, &w, sig, owner) });
                }
                prev = i;
            }
            i = w.next;
        }
    }

    // ------------------------------------------------------------------ geometry helpers
    /// Absolute ink (device px, outward rounded, with `margin`) of a space-local rect.
    #[inline]
    ///
    /// An exact rect (`WRect`): content far from its scroll origin lies outside the 16-bit window domain
    /// (`NRect`), and a clamped ink there could not be scrolled back into view (round 6, finding 3).
    pub fn ink_rect(&self, space: u16, x: f32, y: f32, w: f32, h: f32, margin: f32) -> WRect {
        let o = self.space(space);
        let x0 = (o[0] + x - margin).floor() as i32;
        let y0 = (o[1] + y - margin).floor() as i32;
        let x1 = (o[0] + x + w + margin).ceil() as i32;
        let y1 = (o[1] + y + h + margin).ceil() as i32;
        WRect::new(x0, y0, x1, y1)
    }

    /// Move quad `node`'s box to `(x, y, w, h)` (space-local px; snapped to the geometry grid here): the
    /// instance and the record ink are written, and the damage is only the bands around the edges that moved
    /// (`Bough::set_ink_handle_edges`), not old ∪ new. The instance is compared with, not a mirror of it:
    /// on the grid `x + w` is exact, so an edge that did not move is bitwise the edge the GPU computes
    /// (bough-metal RESULTS Part IV).
    #[inline]
    pub fn quad_geom(&mut self, node: u32, x: f32, y: f32, w: f32, h: f32) {
        let (x, y, w, h) = (snap(x), snap(y), snap(w), snap(h));
        let i = self.inst_of(node) as usize;
        // a box translation (boxops' `translate` operand) offsets the drawn box, not the laid-out one
        let (ox, oy, tx, ty) = if self.hot[node as usize].flags & HF_MOD != 0 {
            let c = self.cold_of(node);
            (c.x, c.y, c.tx, c.ty)
        } else {
            let p = &self.inst[i];
            (p.pos[0], p.pos[1], 0.0, 0.0)
        };
        let p = &self.inst[i];
        let (ow, oh) = (p.size[0], p.size[1]);
        let moved = (ox != x) as u8 | ((oy != y) as u8) << 1 | ((ox + ow != x + w) as u8) << 2 | ((oy + oh != y + h) as u8) << 3;
        if moved == 0 {
            return;
        }
        if self.hot[node as usize].flags & HF_MOD != 0 {
            let c = self.cold_mut(node);
            (c.x, c.y) = (x, y);
        }
        let p = &mut self.inst[i];
        p.pos = [x + tx, y + ty];
        p.size = [w, h];
        let (band, space, p) = (edge_band_of(p), p.space, *p);
        let ink = self.ink_rect(space, x + tx, y + ty, w, h, self.margin_of(node, &p));
        self.bough.set_ink_handle_edges_wide(self.handle(node), ink, moved, band);
    }

    /// Re-derive quad `node`'s ink from its instance and write it through: the old and the new ink are
    /// damaged even when the rounded rects are equal (a sub-pixel change of an edge still changes its
    /// anti-aliased pixels).
    #[inline]
    pub fn quad_ink(&mut self, node: u32) {
        let p = self.inst[self.inst_of(node) as usize];
        let ink = self.ink_rect(p.space, p.pos[0], p.pos[1], p.size[0], p.size[1], self.margin_of(node, &p));
        let h = self.handle(node);
        self.bough.touch_handle(h);
        self.bough.set_ink_handle_wide(h, ink);
    }

    /// A paint-only write to `h`'s record (colour, glyph alphas) from state hash `old` to `new`: its visible
    /// ink goes to the paint ring and the write to the epoch's paint log.
    ///
    /// Round 7: damage is record-space first. The write appends to the epoch's paint log (content
    /// addressing) and sets the record's bit in the core's paint plane; nothing is projected to tiles here.
    /// The tile marks are derived once, before a frame reads the paint ring (`paint_resolve`), at the place
    /// each record has then.
    #[inline(always)]
    pub fn paint_write(&mut self, h: Handle, old: u64, new: u64) {
        let l = &mut self.paint_log[self.bough.epoch() as usize % bough_core::damage::RING];
        if (l.n as usize) < PAINT_LOG {
            l.e[l.n as usize] = ((h.idx as u64) << 32 | h.generation as u64, old, new);
            l.n += 1;
        } else {
            l.overflow = true;
        }
        self.bough.mark_paint(h.idx);
    }

    /// Project the epoch's record-space paint plane onto the damage ring's current tile plane
    /// (`Bough::project_paint`); returns whether paint damaged any tile. There is one damage ring: whether
    /// the epoch had damage other than paint is asked before this is called (`App::frame_as`).
    #[inline(always)]
    pub fn paint_resolve(&mut self) -> bool {
        self.bough.paint_pending() && self.bough.project_paint()
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
    /// Release what a subtree held. Two phases (`reclaim`): **now**, everything that could still reach the
    /// subtree is cut (its wires die, its layout groups leave the engine, its list regions stop), which is
    /// O(wires + layout nodes) of flag writes; **later**, at the next frames' `reclaim`, its nodes, texts,
    /// instance runs, glyph blocks and dynamic items return to their free lists. The removal frame pays
    /// only the cut and bough-core's O(pages) run removal.
    pub fn release_scope(&mut self, mut s: Scope) {
        self.cut_scope(&mut s);
        if !(s.texts.is_empty() && s.insts.is_empty() && s.series.is_empty() && s.runs.is_empty() && s.dyns.is_empty()) {
            self.reclaim_q.push(s);
        }
    }

    fn cut_scope(&mut self, s: &mut Scope) {
        for w in s.wires.drain(..) {
            self.wires[w as usize].dead = true;
        }
        for r in s.lroots.drain(..) {
            self.lay_remove_root(r);
        }
        for r in s.lregs.drain(..) {
            self.lr_release_region(r as usize);
        }
        for p in s.prods.drain(..) {
            self.revoke_producer(p);
        }
        // dynamic items are cut now too (their wires may sit on live signals)
        for &d in &s.dyns {
            let mut items = std::mem::take(&mut self.dyns.parts[d as usize]);
            for it in &mut items {
                self.cut_scope(&mut it.scope);
            }
            self.dyns.parts[d as usize] = items;
        }
    }

    /// Return up to about `budget` released nodes to the free lists (see `release_scope`); returns whether
    /// anything is still waiting.
    pub fn reclaim(&mut self, mut budget: usize) -> bool {
        while budget > 0 {
            let Some(mut s) = self.reclaim_q.pop() else { break };
            while budget > 0 {
                if let Some(t) = s.texts.pop() {
                    self.free_text(t);
                } else if let Some(i) = s.insts.pop() {
                    self.free_inst(i);
                } else if let Some((o, n)) = s.series.pop() {
                    self.spans.free(o, crate::store::class_of(n as usize));
                } else if let Some(r) = s.runs.pop() {
                    self.free_run(r);
                } else if let Some(d) = s.dyns.pop() {
                    self.release_dyn(d);
                } else {
                    break;
                }
                budget -= 1;
            }
            if !(s.texts.is_empty() && s.insts.is_empty() && s.series.is_empty() && s.runs.is_empty() && s.dyns.is_empty()) {
                self.reclaim_q.push(s);
            }
        }
        // layout leaves' glyph blocks; their states are kept for reuse
        while budget > 0 && self.lay.eng.reclaim_orphan(&mut self.glyphs) {
            budget -= 1;
        }
        !self.reclaim_q.is_empty() || !self.lay.eng.orphans.is_empty()
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
    ///
    /// # Safety
    /// As `region_initial`: `owner` points at the live owner the region's row function was monomorphised for.
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
            // SAFETY: `region_scroll`'s contract is `region_mount`'s.
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
            let s = self.regions[reg].rows[i].space;
            self.shift_space(s, 0, -dy);
        }
        self.scroll_copy(base, nrec, -dy, vp);
        let surv_start = old_range.start.max(new_range.start);
        let surv_end = old_range.end.min(new_range.end);
        self.regions[reg].range = new_range.clone();
        // SAFETY: `region_scroll`'s contract is `region_mount`'s.
        unsafe {
            self.region_mount(reg, owner, new_range.start..surv_start, true);
            self.region_mount(reg, owner, surv_end..new_range.end, false);
        }
    }

    // ------------------------------------------------------------------ the storage ledger
    /// The façade's tables as `(name, entries, bytes)`: `size_of` x capacity of each table plus the heap
    /// capacity of every `Vec` / `Box` reachable from its entries (lane `store`: what one mounted element
    /// costs beside its 23 B core record). GPU-region columns (instances, pages, glyph slots) are not heap
    /// and are not listed. Names starting with `col ` are columns carved from the reserved block (not the
    /// allocator's: the harness adds them to the allocator's live bytes).
    pub fn store_ledger(&self) -> Vec<(&'static str, usize, usize)> {
        use std::mem::size_of;
        // a carved column's bytes are its high-water length (what was touched), not its reservation
        let mut v: Vec<(&'static str, usize, usize)> = Vec::new();
        v.push(("core: bough (pages' directory, prefix, htab, planes)", self.bough.len(), self.bough.heap_bytes()));
        // the per-id columns are addressable whole; what is touched is up to the highest id handed out
        let ids = self.max_id as usize + 1;
        v.push(("col rt.hot + hgen (12 B per element id)", ids, ids * 12));
        v.push(("col rt.cold (bars, lanes, modified quads)", self.cold.len(), self.cold.len() * size_of::<Node>()));
        v.push(("col rt.spans (series, tapes, line windows)", self.spans.touched(), self.spans.touched() * 4));
        v.push(("col rt.runs", self.runs.len(), self.runs.len() * size_of::<Run>() + self.run_free.capacity() * 4));
        v.push(("col rt.wires", self.wires.len(), self.wires.len() * size_of::<Wire>() + self.wire_free.capacity() * 4));
        let text_heap = |t: &crate::text::TextNode| {
            t.text.capacity()
                + match &t.shape {
                    crate::text::TShape::Label(lb) => size_of::<bough_text::pool::Label>() + lb.text().len().next_power_of_two().max(16) + 16,
                    crate::text::TShape::Set(_) => size_of::<bough_text::lines::LineSet>(),
                    crate::text::TShape::Lines { widths, spans, .. } => widths.capacity() * 4 + spans.capacity() * 8,
                    _ => 0,
                }
        };
        v.push(("col rt.texts", self.texts.len(), self.texts.len() * size_of::<crate::text::TextNode>() + self.texts.iter().map(text_heap).sum::<usize>() + self.text_free.capacity() * 4));
        v.push(("col lay.info", self.lay.info.len(), self.lay.info.len() * size_of::<crate::layout::LInfo>()));
        v.extend(self.lay.eng.store_ledger());
        fn scope_heap(s: &Scope) -> usize {
            (s.texts.capacity() + s.insts.capacity() + s.wires.capacity() + s.runs.capacity() + s.lroots.capacity() + s.dyns.capacity() + s.lregs.capacity() + s.prods.capacity()) * 4
        }
        let (mut items, mut bytes) = (0usize, 0usize);
        for p in &self.dyns.parts {
            items += p.len();
            bytes += p.capacity() * size_of::<crate::describe::DynItem>();
            bytes += p.iter().map(|i| i.tape.capacity() * 4 + scope_heap(&i.scope)).sum::<usize>();
        }
        v.push(("dyns.parts (items, tapes, scopes)", items, bytes));
        v.push(("dyns.seals", self.dyns.seals.len(), self.dyns.seals.iter().map(|s| s.bytes.capacity()).sum::<usize>()));
        v.push(("col reflect.types", self.reflect.types.len(), self.reflect.types.len() * 16));
        v.push(("reclaim_q", self.reclaim_q.len(), self.reclaim_q.capacity() * size_of::<Scope>() + self.reclaim_q.iter().map(scope_heap).sum::<usize>()));
        v
    }
}
