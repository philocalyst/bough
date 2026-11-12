//! Dormant runs: a dynamic part mounts what is exposed, not what is described (round 6).
//!
//! `each(source)` over a source that can produce item `i` without producing the others is retained as a
//! sequence of **entries** covering its items in order: a *realised item* (what `describe.rs` always kept:
//! its records, its layout children, its scope, its tape, its seal) or a *dormant run* of `n` consecutive
//! items (`DynItem::run`): one paint-less `REC_BOX` record and one layout leaf whose main-axis size is the
//! run's extent. Nothing of a dormant item exists: no description, no records, no instances, no glyphs.
//!
//! # The loop (`App::expose`, after every layout flush of a turn)
//! 1. **Scan** (`Rt::dorm_scan`): per part, an early-out on an unchanged (visible rect of its clip, origin
//!    of its space, layout generation). Otherwise its entries are walked once against the layout: a run
//!    whose box meets the visible rect (± `Dorms::over` px) becomes a *demand* for the item range the rect
//!    covers; an item outside the keep margin (`Dorms::keep` px) that was not exposed for `HOLD` passes and
//!    holds no focus, hover or press becomes a *release*; the measured pitches update the part's estimate.
//! 2. **Walk**: if anything is demanded or released the owner is re-described once (`describe::redescribe`:
//!    the walk is cheap, seals step over unchanged items and dormant runs are skipped in the source in O(1)).
//!    `update_src` splits runs and mounts exactly the demanded items through `describe::mount_item`, and
//!    collapses released items into runs that remember their measured pitch.
//! 3. Layout, then **anchoring**: the first visible item of a part inside a scroll view is kept where it was
//!    on screen by moving the view's offset (a correction of sizes above the viewport is a scroll).
//! 4. Repeat until a scan asks for nothing (bounded; `DormStats::unconverged` counts the frames that hit the
//!    bound, and the tests assert it stays 0).
//!
//! Every structural change happens inside a walk: an enclosing item's record count (`DynItem::nrec`) is
//! recomputed on the way out, so nested parts (a run inside an item inside a run) need no parent links.
//!
//! # What qualifies (`qualifies`, `Dorm::eager`)
//! A run can stand for its items only if the container's layout depends on them through the sum of their
//! outer main sizes and nothing else: a flex **column**, no wrap, start-justified, content-sized on the
//! main axis, items in flow that do not grow. Anything else (rows, grids, a definite height, `justify`
//! other than start, absolute items) mounts eagerly, as do parts of owners other than the root (list rows,
//! entity views: no walk reaches them from the root), parts inside elements a re-description steps over,
//! and sources without O(1) indexing. A violation found after mount realises the whole part for good.
//!
//! # The source
//! `Source` is implemented for every iterator; it is indexable exactly when the iterator is
//! `TrustedRandomAccessNoCoerce` (std's own marker: ranges, slice iterators, and `map` / `enumerate` / `zip`
//! / `cloned` / `copied` / `skip` over them). `Iterator::nth` is not used: `Map::nth` calls the closure for
//! every skipped item.
use crate::describe::{mount_item, remove_items, repack_seals, rewrite_item, sealed_as, DynItem};
use crate::elem::{Element, Mounter};
use crate::layout::Spec;
use crate::rt::*;
use bough_core::{NRect, RecordInit};
use std::iter::TrustedRandomAccessNoCoerce;

/// Items realised at mount, before anything is laid out: they are measured to seed the estimate.
pub const SEED: u32 = 8;
/// Exposure passes an unexposed item stays realised.
pub const HOLD: u32 = 3;
/// Releasable items that force a walk of their own (a release otherwise rides on a walk that realises).
pub const CAP: usize = 24;
/// Beyond this distance from the visible rect (device px) an item is released at once, held or not: its
/// records' inks are absolute `i16` (bough-core `NRect`), so they must stay inside the domain.
pub const FAR: f32 = 12000.0;
/// Bound of the scan / walk / layout fixed point of one `App::expose`.
pub const MAX_PASSES: u32 = 12;

// =============================================================================== the source

/// An iterator as a source of items: by index when its type allows it.
pub trait Source: Iterator + Sized {
    /// Can item `i` be produced without producing the others?
    fn indexed() -> bool;
    /// The number of items (an indexed source's exact length).
    fn count_items(&self) -> usize;
    /// Item `i`.
    ///
    /// # Safety
    /// `Self::indexed()`, `i < self.count_items()`, each index is asked at most once per source value, and
    /// `next` is never called on a source `item` was called on.
    unsafe fn item(&mut self, i: usize) -> Self::Item;
}
impl<I: Iterator> Source for I {
    #[inline(always)]
    default fn indexed() -> bool {
        false
    }
    #[inline(always)]
    default fn count_items(&self) -> usize {
        0
    }
    #[inline(always)]
    default unsafe fn item(&mut self, _: usize) -> Self::Item {
        unreachable!("not an indexed source")
    }
}
impl<I: Iterator + TrustedRandomAccessNoCoerce> Source for I {
    #[inline(always)]
    fn indexed() -> bool {
        true
    }
    #[inline(always)]
    fn count_items(&self) -> usize {
        self.size()
    }
    #[inline(always)]
    unsafe fn item(&mut self, i: usize) -> Self::Item {
        // SAFETY: the function contract is `TrustedRandomAccess`'s: `i < size()`, no index twice (so `Clone`
        // is not needed), and only `size` / `__iterator_get_unchecked` / drop are used on this value after.
        unsafe { self.__iterator_get_unchecked(i) }
    }
}

// =============================================================================== state

/// What a part last scanned against.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ScanKey {
    vis: NRect,
    org: [i32; 2],
    /// `LayStats::computes` (every layout compute bumps it); `u64::MAX`: scan whatever the rest says.
    lay: u64,
}

/// The exposure state of one dormant-capable part (`Dyns::dorm`).
pub struct Dorm {
    /// Items of the part (realised or not).
    pub n: u32,
    /// The container: layout node, and the space, clip and depth its children are mounted with.
    pub(crate) lid: u32,
    pub(crate) space: u16,
    pub(crate) clip: u16,
    /// The scroll view whose content holds the part (`Input::views`), or `NONE`.
    pub(crate) view: u32,
    /// The outer main size (device px, whole) an unmeasured item counts as; `NaN` until an item was
    /// measured. (Whole: a run of estimated items then puts what follows it on the pixel grid the items
    /// themselves would.)
    pub est: f32,
    pub(crate) gap: f32,
    /// The widest measured item (device px): a run's cross size, so a content-sized container keeps its width.
    pub(crate) cross: f32,
    /// Remembered pitches of released items whose pitch is not the estimate, as run-length groups
    /// (first item, items, pitch), ascending and disjoint: a thousand released rows of one height are one
    /// group, and rows of the estimated height are none.
    pub(crate) rem: Vec<(u32, u32, f32)>,
    /// The scan's demands for the next walk: item ranges to realise, ascending and disjoint.
    pub(crate) want: Vec<(u32, u32)>,
    /// The scan's releases for the next walk: (index, measured pitch), ascending.
    pub(crate) drop: Vec<(u32, f32)>,
    /// An item kept realised whatever its exposure ("realise item i now"), or `NONE`.
    pub pin: u32,
    pub(crate) key: ScanKey,
    /// The anchor of the exposure pass `anchor_tick`: (item, its top on screen, device px).
    pub(crate) anchor: Option<(u32, f32)>,
    pub(crate) anchor_tick: u32,
    /// A run cannot stand for this part's items after all: everything is realised and stays so.
    pub eager: bool,
    /// Dormant runs among the entries.
    pub runs: u32,
    /// Run extents or the estimate changed: the leaves are re-sized at the end of the walk.
    pub(crate) resize: bool,
    cand: Vec<(u32, u32, f32, bool)>,
}

/// Counters of the exposure machinery (tests and the inspector).
#[derive(Default, Clone, Copy, Debug)]
pub struct DormStats {
    /// Parts scanned past the early-out.
    pub scans: u64,
    /// Re-descriptions run to realise or release.
    pub walks: u64,
    pub realised: u64,
    pub released: u64,
    /// `App::expose` calls that reached `MAX_PASSES` without settling.
    pub unconverged: u64,
    /// Scroll offsets moved by anchoring, and the device px moved in total.
    pub anchored: u64,
    pub anchored_px: i64,
    /// Parts that were dormant-capable and fell back to eager (a run could not stand for their items).
    pub fell_back: u64,
}

/// The gate's negative controls (`tests/dormant.rs`), from `BOUGH_DORM_MUTANT`; `None` in every normal run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    None,
    /// One item too few at the trailing edge of what is visible (and no overscan to hide it).
    Short,
    /// Nothing is ever released.
    NoRelease,
    /// Sizes corrected above the viewport move the content instead of the offset.
    NoAnchor,
}

/// The runtime's dormant parts (`Rt::dorm`).
pub struct Dorms {
    /// Parts that mount by exposure (`Dyns::dorm[d]` is `Some`), in mount order.
    pub(crate) live: Vec<u32>,
    /// `Config::eager_parts`: every part mounts everything (the oracle of the exposure gates).
    pub off: bool,
    /// Device px realised beyond the visible rect, and device px an item may be beyond it before it is
    /// releasable (the difference is the hysteresis in space).
    pub over: f32,
    pub keep: f32,
    /// Exposure passes so far (`App::expose` calls that had parts to look at).
    pub tick: u32,
    /// Elements being mounted that a re-description steps over (`describe::mount_opaque`).
    pub(crate) opaque: u32,
    /// A held item must go (beyond `FAR`): the app blurs first.
    pub(crate) blur: bool,
    pub stats: DormStats,
    pub mutant: Mutant,
    pub(crate) fix: Vec<(u32, i32)>,
    held: Vec<usize>,
}

impl Dorms {
    pub fn new(scale: f32) -> Dorms {
        static M: std::sync::OnceLock<Mutant> = std::sync::OnceLock::new();
        let mutant = *M.get_or_init(|| match std::env::var("BOUGH_DORM_MUTANT").as_deref() {
            Ok("short") => Mutant::Short,
            Ok("norelease") => Mutant::NoRelease,
            Ok("noanchor") => Mutant::NoAnchor,
            _ => Mutant::None,
        });
        Dorms { live: Vec::new(), off: false, over: 64.0 * scale, keep: 256.0 * scale, tick: 0, opaque: 0, blur: false, stats: DormStats::default(), mutant, fix: Vec::new(), held: Vec::new() }
    }
}

/// One part as the inspector and the tests see it.
#[derive(Clone, Debug, PartialEq)]
pub struct PartView {
    pub part: u32,
    /// Items described; items realised; dormant runs.
    pub items: u32,
    pub realised: u32,
    pub runs: u32,
    /// Mounts by exposure (false: an eager part, or one that fell back).
    pub dormant: bool,
    /// The item ranges realised now.
    pub ranges: Vec<(u32, u32)>,
    /// The pitch estimate (device px).
    pub est: f32,
}

impl Dorm {
    /// The remembered groups overlapping items `[a, a + c)`, each cut to the range.
    #[inline]
    fn groups(&self, a: u32, c: u32) -> impl Iterator<Item = (u32, u32, f32)> + '_ {
        let r0 = self.rem.partition_point(|g| g.0 + g.1 <= a);
        self.rem[r0..].iter().take_while(move |g| g.0 < a + c).map(move |&(s, n, pitch)| {
            let (lo, hi) = (s.max(a), (s + n).min(a + c));
            (lo, hi - lo, pitch)
        })
    }

    /// The extent of run `[a, a + c)` as its leaf is sized: the pitches of its items less one gap (the
    /// container puts a gap on each side of the leaf as it would around the items).
    fn run_size(&self, a: u32, c: u32) -> f32 {
        if self.est.is_nan() || c == 0 {
            return 0.0;
        }
        let est = self.est as f64;
        let mut s = c as f64 * est + (c - 1) as f64 * self.gap as f64;
        for (_, n, pitch) in self.groups(a, c) {
            s += n as f64 * (pitch as f64 - est);
        }
        s.max(0.0) as f32
    }

    /// The item of run `[a, a + c)` at `dy` px below the run's top, and that item's top (px below the run's
    /// top): the last item whose top is at or above `dy`, inside the run.
    fn locate(&self, a: u32, c: u32, dy: f32) -> (u32, f32) {
        let (gap, dy) = (self.gap as f64, dy as f64);
        let est = self.est as f64 + gap;
        // uniform stretch of `n` items of `pitch` starting at item `i`, top `y`: the item at `dy`, if inside
        let at = |i: u32, n: u32, pitch: f64, y: f64| -> Option<(u32, f32)> {
            (n > 0 && dy < y + n as f64 * pitch).then(|| {
                let k = if pitch > 0.0 { (((dy - y) / pitch).floor().max(0.0) as u64).min(n as u64 - 1) as u32 } else { 0 };
                (i + k, (y + k as f64 * pitch) as f32)
            })
        };
        let (mut y, mut i) = (0f64, a);
        for (s, n, pitch) in self.groups(a, c) {
            if let Some(hit) = at(i, s - i, est, y) {
                return hit;
            }
            y += (s - i) as f64 * est;
            if let Some(hit) = at(s, n, pitch as f64 + gap, y) {
                return hit;
            }
            y += n as f64 * (pitch as f64 + gap);
            i = s + n;
        }
        if let Some(hit) = at(i, a + c - i, est, y) {
            return hit;
        }
        // past the end: the last item
        let last = a + c - 1;
        (last, if c > 1 { self.run_size(a, c - 1) + self.gap } else { 0.0 })
    }

    /// Item `at` was removed (`by` = -1) or an item was inserted at `at` (`by` = 1): the remembered pitches
    /// follow their items.
    fn shift(&mut self, at: u32, by: i32) {
        if self.rem.is_empty() {
            return;
        }
        if by < 0 {
            self.forget(at, at + 1);
        } else if let Some(r) = self.rem.iter().position(|g| g.0 < at && g.0 + g.1 > at) {
            // an insertion inside a group splits it
            let (s, n, pitch) = self.rem[r];
            self.rem[r].1 = at - s;
            self.rem.insert(r + 1, (at, s + n - at, pitch));
        }
        let r0 = self.rem.partition_point(|g| g.0 < at + (by < 0) as u32);
        for g in &mut self.rem[r0..] {
            g.0 = (g.0 as i64 + by as i64) as u32;
        }
    }

    /// Forget what was remembered of items `[a, b)` (they are being realised and will be measured, or are gone).
    fn forget(&mut self, a: u32, b: u32) {
        let mut r = self.rem.partition_point(|g| g.0 + g.1 <= a);
        while r < self.rem.len() && self.rem[r].0 < b {
            let (s, n, pitch) = self.rem[r];
            let (head, tail) = (a.saturating_sub(s), (s + n).saturating_sub(b));
            match (head > 0, tail > 0) {
                (true, true) => {
                    self.rem[r].1 = head;
                    self.rem.insert(r + 1, (b, tail, pitch));
                    return;
                }
                (true, false) => {
                    self.rem[r].1 = head;
                    r += 1;
                }
                (false, true) => {
                    self.rem[r] = (b, tail, pitch);
                    return;
                }
                (false, false) => {
                    self.rem.remove(r);
                }
            }
        }
    }

    /// Item `i` was released with measured `pitch`: remembered (as part of a run-length group) unless it is
    /// what the estimate says anyway.
    fn remember(&mut self, i: u32, pitch: f32) {
        self.forget(i, i + 1);
        if pitch == self.est {
            return;
        }
        let r = self.rem.partition_point(|g| g.0 < i);
        let prev = r > 0 && self.rem[r - 1].0 + self.rem[r - 1].1 == i && self.rem[r - 1].2 == pitch;
        let next = r < self.rem.len() && self.rem[r].0 == i + 1 && self.rem[r].2 == pitch;
        match (prev, next) {
            (true, true) => {
                self.rem[r - 1].1 += 1 + self.rem[r].1;
                self.rem.remove(r);
            }
            (true, false) => self.rem[r - 1].1 += 1,
            (false, true) => {
                self.rem[r].0 = i;
                self.rem[r].1 += 1;
            }
            (false, false) => self.rem.insert(r, (i, 1, pitch)),
        }
    }
}

/// The spec of a dormant run's leaf: an in-flow leaf that does not shrink (its size is its `measure`).
const RUN_SPEC: Spec = {
    let mut s = Spec::new(false, false);
    s.shrink = 0.0;
    s
};

/// Can the container being mounted into hold dormant runs? (see the module docs)
fn container_ok(st: &cephal::Style) -> bool {
    use cephal::style::{Display, FlexDirection, FlexWrap, JustifyContent};
    st.display == Display::Flex
        && st.flex_direction == FlexDirection::Column
        && st.flex_wrap == FlexWrap::NoWrap
        && matches!(st.justify_content, None | Some(JustifyContent::START) | Some(JustifyContent::FLEX_START))
        && st.size.height.is_auto()
        && st.gap.height.is_length()
}

/// Can a run stand for this item's top-level layout child?
fn kid_ok(st: &cephal::Style) -> bool {
    use cephal::style::Position;
    matches!(st.position, Position::Static | Position::Relative) && st.flex_grow == 0.0 && st.margin.top.is_length() && st.margin.bottom.is_length()
}

/// Does a part mounted now mount by exposure? The root owner, re-describing, into a qualifying container,
/// not inside an element a re-description steps over.
#[inline]
pub(crate) fn qualifies<V>(m: &Mounter<'_, V>) -> bool {
    let lid = m.rt.lay.cur;
    m.taping() && m.own.is_root() && !m.rt.dorm.off && m.rt.dorm.opaque == 0 && lid != NONE && container_ok(m.rt.lay.eng.style(lid))
}

impl Rt {
    /// What is visible of content clipped by `clip`: its rect ∩ the window (absolute device px). The clip
    /// chain is already composed in the clip's rect (`scoped_clip`, the view and layout clip hooks).
    #[inline]
    pub(crate) fn visible_of(&self, clip: u16) -> NRect {
        let win = NRect::new(0, 0, self.win_w, self.win_h);
        self.bough.clip_rect(clip).intersect(win).canon()
    }

    /// A dormant run's leaf under `parent`: a layout node measuring `size`, registered like any box. The
    /// caller places it among the parent's children and inserts its record.
    fn dorm_leaf(&mut self, parent: u32, space: u16, clip: u16, size: [f32; 2]) -> u32 {
        let hd = self.bough.new_handle();
        let lid = self.lay.eng.add(hd, &RUN_SPEC, self.scale, None);
        self.lay.eng.set_dormant(lid, size);
        self.lay.register(lid, parent);
        let l = &mut self.lay.info[lid as usize];
        l.space = space;
        l.clip = clip;
        lid
    }

    /// Part `d` is going away (its scope was cut): it is scanned no more.
    pub(crate) fn dorm_cut(&mut self, d: u32) {
        if self.dyns.dorm.get(d as usize).is_some_and(|p| p.is_some()) {
            if let Some(i) = self.dorm.live.iter().position(|&x| x == d) {
                self.dorm.live.remove(i);
            }
        }
    }

    /// Size every run's leaf from the part's extent model; queues the container's layout if one changed.
    fn dorm_resize(&mut self, p: &mut Dorm, old: &[DynItem]) {
        p.resize = false;
        let (mut i, mut any, mut runs) = (0u32, false, 0u32);
        for e in old {
            if e.run == 0 {
                i += 1;
                continue;
            }
            runs += 1;
            let size = [p.cross, p.run_size(i, e.run)];
            any |= self.lay.eng.resize_dormant(&self.bough, e.head, size);
            i += e.run;
        }
        p.runs = runs;
        if any {
            self.queue_container(p.lid);
        }
    }

    /// The visible rect in the frame of space `space` (space-local device px: x0, y0, x1, y1), or `None` when
    /// nothing is visible. A transformed space maps the rect back through its affine (the bounding box of
    /// the four corners: a superset).
    fn dorm_local_vis(&self, vis: NRect, org: [i32; 2], space: u16) -> Option<[f32; 4]> {
        if vis.is_empty() {
            return None;
        }
        let [x0, y0, x1, y1] = vis.to_xyxy();
        let (ox, oy) = (org[0] as f32, org[1] as f32);
        let xf = self.xforms[space as usize];
        if xf == [0.0; 8] {
            return Some([x0 as f32 - ox, y0 as f32 - oy, x1 as f32 - ox, y1 as f32 - oy]);
        }
        // drawn at T + c + m (p - c) + t: p = c + m^-1 (P - T - c - t)
        let (a, b, c, d) = (xf[0] + 1.0, xf[1], xf[2], xf[3] + 1.0);
        let det = a * d - b * c;
        if det.abs() < 1e-6 {
            return None;
        }
        let mut r = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for (px, py) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
            let (qx, qy) = (px as f32 - ox - xf[4] - xf[6], py as f32 - oy - xf[5] - xf[7]);
            let (lx, ly) = (xf[4] + (d * qx - b * qy) / det, xf[5] + (a * qy - c * qx) / det);
            r = [r[0].min(lx), r[1].min(ly), r[2].max(lx), r[3].max(ly)];
        }
        Some(r)
    }

    /// Scan every dormant-capable part (see the module docs). Returns whether a walk is needed: some part
    /// has items to realise or to release. Layout may be pending afterwards too (an estimate moved).
    pub(crate) fn dorm_scan(&mut self) -> bool {
        let mut walk = false;
        let mut li = 0;
        while li < self.dorm.live.len() {
            let d = self.dorm.live[li];
            li += 1;
            let Some(p) = self.dyns.dorm[d as usize].as_deref() else { continue };
            let key = ScanKey { vis: self.visible_of(p.clip), org: self.glyphs.spaces[p.space as usize], lay: self.lay.stats.computes };
            // the per-frame cost of a part whose exposure did not change: this compare
            if key == p.key || !self.lay.info[p.lid as usize].written {
                continue;
            }
            walk |= self.dorm_scan_part(d, key);
        }
        walk
    }

    #[inline(never)]
    fn dorm_scan_part(&mut self, d: u32, key: ScanKey) -> bool {
        let mut p = self.dyns.dorm[d as usize].take().unwrap();
        let mut old = std::mem::take(&mut self.dyns.parts[d as usize]);
        let walk = self.dorm_scan_entries(&mut p, &mut old, key);
        self.dyns.parts[d as usize] = old;
        self.dyns.dorm[d as usize] = Some(p);
        walk
    }

    fn dorm_scan_entries(&mut self, p: &mut Dorm, old: &mut [DynItem], key: ScanKey) -> bool {
        self.dorm.stats.scans += 1;
        p.key = key;
        p.want.clear();
        p.drop.clear();
        p.cand.clear();
        if old.is_empty() {
            return false;
        }
        let tick = self.dorm.tick;
        let mutant = self.dorm.mutant;
        let lid = p.lid;
        if !p.eager && !container_ok(self.lay.eng.style(lid)) {
            p.eager = true;
            self.dorm.stats.fell_back += 1;
        }
        let ci = self.lay.info[lid as usize];
        let eng = &self.lay.eng;
        let kids = eng.kids(lid);
        let mut kc = kids.iter().position(|&c| c == old[0].head).unwrap_or(usize::MAX);
        if kc == usize::MAX && !p.eager {
            // the first entry does not start at a layout child of the container
            p.eager = true;
            self.dorm.stats.fell_back += 1;
        }
        if p.eager {
            if p.runs > 0 {
                p.want.push((0, p.n));
            }
            return p.runs > 0;
        }
        // the visible range along the main axis, container-local
        let (over, keep) = if mutant == Mutant::Short { (0.0, self.dorm.keep) } else { (self.dorm.over, self.dorm.keep) };
        let (v0, v1) = match self.dorm_local_vis(key.vis, key.org, p.space) {
            Some(v) if v[0] < ci.ax + ci.aw && v[2] > ci.ax => (v[1] - ci.ay, v[3] - ci.ay),
            _ => (f32::INFINITY, f32::NEG_INFINITY),
        };
        let seen_any = v0 < v1;
        let (mut sum, mut cnt, mut cross) = (0f64, 0u32, p.cross);
        let mut i = 0u32;
        let mut anchor: Option<(u32, f32)> = None;
        let mut bad = false;
        for (k, ent) in old.iter_mut().enumerate() {
            let nk = ent.nkid as usize;
            let Some(ks) = kids.get(kc..kc + nk).filter(|ks| !ks.is_empty()) else {
                bad = true;
                break;
            };
            kc += nk;
            if ent.run != 0 {
                let c = ent.run;
                let (top, bot, _) = eng.span(ks[0]);
                if !p.est.is_nan() && bot > v0 - over && top < v1 + over && bot > top {
                    let a = p.locate(i, c, (v0 - over - top).max(0.0)).0;
                    // the last item whose top is above the far edge
                    let mut b = p.locate(i, c, (v1 + over - top).min(bot - top) - 0.01).0 + 1;
                    if mutant == Mutant::Short && top > v0 {
                        b -= 1;
                    }
                    match p.want.last_mut() {
                        _ if a >= b => {}
                        Some(w) if w.1 == a => w.1 = b,
                        _ => p.want.push((a, b)),
                    }
                    if anchor.is_none() && bot > v0 && top < v1 {
                        let (ai, at) = p.locate(i, c, (v0 - top).max(0.0));
                        anchor = Some((ai, top + at));
                    }
                }
                i += c;
                continue;
            }
            let (top, _, w0) = eng.span(ks[0]);
            let (_, bot, _) = eng.span(ks[nk - 1]);
            if !ent.ok {
                if ent.head != ks[0] || !ks.iter().all(|&c| kid_ok(eng.style(c))) {
                    bad = true;
                    break;
                }
                ent.ok = true;
            }
            let pitch = bot - top;
            sum += pitch as f64;
            cnt += 1;
            cross = cross.max(w0);
            if (bot > v0 - keep && top < v1 + keep) || i == p.pin {
                ent.seen = tick;
                if anchor.is_none() && bot > v0 && top < v1 {
                    anchor = Some((i, top));
                }
            } else {
                let far = seen_any && (bot < v0 - FAR || top > v1 + FAR);
                if far || tick.wrapping_sub(ent.seen) >= HOLD {
                    p.cand.push((i, k as u32, pitch, far));
                }
            }
            i += 1;
        }
        if bad {
            p.eager = true;
            self.dorm.stats.fell_back += 1;
            p.want.clear();
            p.cand.clear();
            p.key.lay = u64::MAX;
            if p.runs > 0 {
                p.want.push((0, p.n));
            }
            return p.runs > 0;
        }
        // the estimate: the mean measured pitch, moved only when it is off by more than an eighth (every
        // move re-sizes the runs, which moves everything below them)
        let mut resize = false;
        if cnt > 0 {
            let mean = ((sum / cnt as f64) as f32).round().max(1.0);
            if p.est.is_nan() || (mean - p.est).abs() > p.est * 0.125 {
                p.est = mean;
                resize = true;
            }
        } else if p.est.is_nan() && p.runs > 0 {
            // nothing realised to measure: the first items of the first run are the seed
            let c = old.iter().find(|e| e.run != 0).map_or(0, |e| e.run);
            p.want.push((0, c.min(SEED)));
        }
        if cross != p.cross {
            p.cross = cross;
            resize = true;
        }
        if p.anchor_tick != tick {
            p.anchor_tick = tick;
            p.anchor = anchor.map(|(ai, top)| (ai, key.org[1] as f32 + ci.ay + top));
        }
        // releases: not what holds focus, hover or a press, unless it left the ink domain
        if !p.cand.is_empty() && mutant != Mutant::NoRelease {
            let mut held = std::mem::take(&mut self.dorm.held);
            held.clear();
            let focus = self.input.focus.and_then(|h| self.bough.pos_of(h));
            self.input.held_positions(&self.bough, &mut held);
            let mut blur = false;
            let b = &self.bough;
            p.cand.retain(|&(_, k, _, far)| {
                let e = &old[k as usize];
                let Some(p0) = b.pos_of(b.handle_gen(e.head)) else { return true };
                let inside = |q: usize| q >= p0 && q < p0 + e.nrec as usize;
                if focus.is_some_and(inside) {
                    blur |= far;
                    return false;
                }
                far || !held.iter().any(|&q| inside(q))
            });
            self.dorm.held = held;
            if blur {
                self.dorm.blur = true;
                p.key.lay = u64::MAX;
            }
            // a release rides on a walk that realises; alone it waits for enough of them
            if !p.want.is_empty() || p.cand.len() >= CAP || p.cand.iter().any(|c| c.3) {
                p.drop.extend(p.cand.iter().map(|c| (c.0, c.2)));
            }
        }
        if resize {
            self.dorm_resize(p, old);
        }
        !p.want.is_empty() || !p.drop.is_empty()
    }

    /// Where the anchors of this exposure pass are now: (view, device px its content moved down) for every
    /// part in a scroll view whose anchor item is no longer where it was on screen.
    pub(crate) fn dorm_anchor(&self, out: &mut Vec<(u32, i32)>) {
        if self.dorm.mutant == Mutant::NoAnchor {
            return;
        }
        for &d in &self.dorm.live {
            let Some(p) = self.dyns.dorm[d as usize].as_deref() else { continue };
            let (Some((idx, y0)), true) = (p.anchor, p.anchor_tick == self.dorm.tick && p.view != NONE) else { continue };
            if out.iter().any(|f| f.0 == p.view) {
                continue;
            }
            let old = &self.dyns.parts[d as usize];
            let Some(first) = old.first() else { continue };
            let kids = self.lay.eng.kids(p.lid);
            let Some(mut kc) = kids.iter().position(|&c| c == first.head) else { continue };
            let mut i = 0u32;
            let mut top = None;
            for e in old {
                let n = e.run.max(1);
                if idx < i + n {
                    let Some(&k0) = kids.get(kc) else { break };
                    let t = self.lay.eng.span(k0).0;
                    top = Some(if e.run != 0 { t + self.dorm_offset_in(p, i, idx) } else { t });
                    break;
                }
                i += n;
                kc += e.nkid as usize;
            }
            let Some(top) = top else { continue };
            let y1 = self.glyphs.spaces[p.space as usize][1] as f32 + self.lay.info[p.lid as usize].ay + top;
            let dy = (y1 - y0).round() as i32;
            if dy != 0 {
                out.push((p.view, dy));
            }
        }
    }

    /// Px from the top of the run starting at item `a` to the top of its item `idx`.
    fn dorm_offset_in(&self, p: &Dorm, a: u32, idx: u32) -> f32 {
        if idx <= a { 0.0 } else { p.run_size(a, idx - a) + p.gap }
    }

    /// The dynamic parts of the app's root owner (the inspector, tests): how each is mounted.
    pub fn parts(&self) -> Vec<PartView> {
        let mut out = Vec::new();
        for (d, items) in self.dyns.parts.iter().enumerate() {
            let dorm = self.dyns.dorm[d].as_deref();
            // not an unused slot, and not a part whose scope was cut (it waits to be reclaimed)
            if (items.is_empty() && dorm.is_none()) || (dorm.is_some() && !self.dorm.live.contains(&(d as u32))) {
                continue;
            }
            let (mut i, mut realised, mut runs) = (0u32, 0u32, 0u32);
            let mut ranges: Vec<(u32, u32)> = Vec::new();
            for e in items {
                if e.run != 0 {
                    runs += 1;
                    i += e.run;
                } else {
                    match ranges.last_mut() {
                        Some(r) if r.1 == i => r.1 = i + 1,
                        _ => ranges.push((i, i + 1)),
                    }
                    realised += 1;
                    i += 1;
                }
            }
            out.push(PartView { part: d as u32, items: i, realised, runs, dormant: dorm.is_some_and(|p| !p.eager), ranges, est: dorm.map_or(f32::NAN, |p| p.est) });
        }
        out
    }

    /// Realise item `i` of part `part` at the next exposure pass and keep it realised until `dorm_unpin`
    /// (focus traversal entering a run, scroll-to-item, find). `false`: no such dormant-capable part or item.
    pub fn dorm_pin(&mut self, part: u32, i: u32) -> bool {
        let Some(p) = self.dyns.dorm.get_mut(part as usize).and_then(|p| p.as_deref_mut()) else { return false };
        if i >= p.n {
            return false;
        }
        p.pin = i;
        p.key.lay = u64::MAX;
        true
    }
    pub fn dorm_unpin(&mut self, part: u32) {
        if let Some(p) = self.dyns.dorm.get_mut(part as usize).and_then(|p| p.as_deref_mut()) {
            p.pin = NONE;
            p.key.lay = u64::MAX;
        }
    }

    /// The top of item `i` of part `part` in its container's content (device px from the container's top),
    /// by the layout for a realised item and by the extent model for a dormant one.
    pub fn dorm_item_top(&self, part: u32, idx: u32) -> Option<f32> {
        let p = self.dyns.dorm.get(part as usize)?.as_deref()?;
        let old = &self.dyns.parts[part as usize];
        let kids = self.lay.eng.kids(p.lid);
        let head = old.first()?.head;
        let mut kc = kids.iter().position(|&c| c == head)?;
        let mut i = 0u32;
        for e in old {
            let n = e.run.max(1);
            if idx < i + n {
                let t = self.lay.eng.span(*kids.get(kc)?).0;
                return Some(if e.run != 0 { t + self.dorm_offset_in(p, i, idx) } else { t });
            }
            i += n;
            kc += e.nkid as usize;
        }
        None
    }

    /// The top of item `idx` of part `part` on screen (absolute device px).
    pub fn dorm_item_y(&self, part: u32, idx: u32) -> Option<f32> {
        let top = self.dorm_item_top(part, idx)?;
        let p = self.dyns.dorm[part as usize].as_deref()?;
        Some(self.glyphs.spaces[p.space as usize][1] as f32 + self.lay.info[p.lid as usize].ay + top)
    }

    /// The scroll view a part's content scrolls in (`NONE`: none).
    pub fn dorm_view(&self, part: u32) -> u32 {
        self.dyns.dorm.get(part as usize).and_then(|p| p.as_deref()).map_or(NONE, |p| p.view)
    }
}

// =============================================================================== mount

/// Mount a part that mounts by exposure: the first `SEED` items (measured by the first layout: the
/// estimate) and one dormant run for the rest.
pub(crate) fn mount_src<V, I: Source>(m: &mut Mounter<'_, V>, mut src: I)
where
    I::Item: Element<V>,
{
    let d = m.rt.dyns.alloc();
    m.tape_push(d);
    m.scope.dyns.push(d);
    let n = src.count_items() as u32;
    let seed = n.min(SEED);
    let lid = m.rt.lay.cur;
    let tick = m.rt.dorm.tick;
    let mut out = Vec::with_capacity(seed as usize + 1);
    for i in 0..seed {
        // SAFETY: an indexed source (the caller checked), `i < n`, ascending.
        let e = unsafe { src.item(i as usize) };
        let key = e.dyn_key();
        let r0 = m.recs.len();
        let mut it = mount_item(m, e, None);
        it.key = key;
        it.head = m.recs.get(r0).map_or(0, |r| r.handle);
        it.seen = tick;
        out.push(it);
    }
    let mut runs = 0;
    if n > seed {
        let leaf = m.rt.dorm_leaf(lid, m.space, m.clip, [0.0, 0.0]);
        m.rt.lay.eng.append(lid, leaf);
        m.recs.push(RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: leaf, clip: m.clip });
        out.push(DynItem { nrec: 1, nkid: 1, run: n - seed, head: leaf, ..DynItem::empty(0) });
        runs = 1;
    }
    let gap = m.rt.lay.eng.style(lid).gap.height.value();
    let view = m.rt.input.mounting_view.unwrap_or(NONE);
    let key = ScanKey { vis: NRect::EMPTY, org: [0, 0], lay: u64::MAX };
    let dorm = Dorm { n, lid, space: m.space, clip: m.clip, view, est: f32::NAN, gap, cross: 0.0, rem: Vec::new(), want: Vec::new(), drop: Vec::new(), pin: NONE, key, anchor: None, anchor_tick: 0, eager: false, runs, resize: false, cand: Vec::new() };
    m.rt.dyns.parts[d as usize] = out;
    m.rt.dyns.dorm[d as usize] = Some(Box::new(dorm));
    m.rt.dorm.live.push(d);
}

// =============================================================================== the walk

/// Mount items `[a, b)` of `src` at the walk's position (their records inserted there, their layout
/// children placed there) and return their entries. `first`: item `a`, if it was already fetched.
fn realise<V, I: Source>(m: &mut Mounter<'_, V>, src: &mut I, lid: u32, a: u32, b: u32, mut first: Option<I::Item>) -> Vec<DynItem>
where
    I::Item: Element<V>,
{
    let mut buf: Vec<RecordInit> = Vec::new();
    let mut items = Vec::with_capacity((b - a) as usize);
    let tick = m.rt.dorm.tick;
    let mut nk = 0usize;
    for idx in a..b {
        let e = match first.take() {
            Some(e) => e,
            // SAFETY: an indexed source, `idx < n`, and the walk asks indices in ascending order, once.
            None => unsafe { src.item(idx as usize) },
        };
        let key = e.dyn_key();
        let r0 = buf.len();
        let mut it = mount_item(m, e, Some(&mut buf));
        it.key = key;
        it.head = buf.get(r0).map_or(0, |r| r.handle);
        it.seen = tick;
        nk += it.nkid as usize;
        items.push(it);
    }
    m.rt.bough.insert_run(m.pos, &buf);
    if nk > 0 {
        m.rt.lay.eng.place_tail_kids(lid, m.kid, nk);
    }
    m.pos += buf.len();
    m.kid += nk;
    m.rt.dorm.stats.realised += (b - a) as u64;
    items
}

/// A new dormant run of `count` items at the walk's position: its record and its leaf.
fn new_run<V>(m: &mut Mounter<'_, V>, lid: u32, count: u32) -> DynItem {
    let leaf = m.rt.dorm_leaf(lid, m.space, m.clip, [0.0, 0.0]);
    m.rt.bough.insert_run(m.pos, &[RecordInit { depth: m.depth, ink: NRect::EMPTY, kind: REC_BOX, payload: 0, handle: leaf, clip: m.clip }]);
    m.rt.lay.eng.splice_kids(&m.rt.bough, lid, m.kid, 0, &[leaf]);
    DynItem { nrec: 1, nkid: 1, run: count, head: leaf, ..DynItem::empty(0) }
}

/// Re-describe a part that mounts by exposure. Realised items are positional: the item at index `i` is
/// updated with `src[i]` (through its seal, with the shift detection of `describe::update_dyn` between
/// neighbouring realised items); dormant runs are stepped over without asking the source for their items,
/// split where the scan demanded items, and grown where it released some. A change of the part's length is
/// applied at its end (a run grows or shrinks, entries past the end are removed).
pub(crate) fn update_src<V, I: Source>(m: &mut Mounter<'_, V>, mut src: I)
where
    I::Item: Element<V>,
{
    let d = m.tape_next();
    let mut old = std::mem::take(&mut m.rt.dyns.parts[d as usize]);
    let mut seals = std::mem::take(&mut m.rt.dyns.seals[d as usize]);
    let mut p = m.rt.dyns.dorm[d as usize].take().expect("a dormant-capable part");
    let mut scratch: Vec<u8> = Vec::new();
    let cx = (m.clip, m.space, m.depth, m.ox, m.oy);
    let lid = m.rt.lay.cur;
    let reclip = p.clip != m.clip;
    p.clip = m.clip;
    let n_new = src.count_items() as u32;
    let gap = m.rt.lay.eng.style(lid).gap.height.value();
    if gap != p.gap {
        // the container was restyled
        p.gap = gap;
        p.resize = true;
    }
    let want = std::mem::take(&mut p.want);
    let drops = std::mem::take(&mut p.drop);
    let (mut wi, mut di) = (0usize, 0usize);
    let (mut i, mut k) = (0u32, 0usize);
    // the item at index `i`, when it was fetched ahead
    let mut pending: Option<I::Item> = None;
    let mut touched = false;
    while k < old.len() && i < n_new {
        if old[k].run != 0 {
            let leaf = old[k].head;
            if reclip {
                m.rt.bough.set_clip_run(m.pos, 1, m.clip);
                m.rt.lay.info[leaf as usize].clip = m.clip;
            }
            let c = old[k].run.min(n_new - i);
            if k > 0 && old[k - 1].run != 0 {
                // two runs side by side (a release between them, a split whose items were released): one run
                old[k - 1].run += c;
                old.remove(k);
                m.rt.bough.remove_run(m.pos, 1);
                m.rt.remove_layout_subtree(leaf);
                m.rt.lay.eng.splice_kids(&m.rt.bough, lid, m.kid, 1, &[]);
                // the merged run is looked at again as one (it may hold a demand)
                k -= 1;
                m.pos -= 1;
                m.kid -= 1;
                i -= old[k].run - c;
                touched = true;
                p.resize = true;
                continue;
            }
            while wi < want.len() && want[wi].1 <= i {
                wi += 1;
            }
            let (a, b) = match want.get(wi) {
                Some(w) => (w.0.max(i), w.1.min(i + c)),
                None => (0, 0),
            };
            if a >= b {
                if c != old[k].run {
                    old[k].run = c;
                    p.resize = true;
                }
                pending = None;
                m.pos += 1;
                m.kid += 1;
                i += c;
                k += 1;
                continue;
            }
            // [i, i + c) becomes: a run [i, a), the items [a, b), a run [b, i + c)
            let (lead, trail) = (a - i, i + c - b);
            let first = if lead == 0 { pending.take() } else { None };
            pending = None;
            if lead > 0 {
                // the leaf stays as the leading run
                old[k].run = lead;
                m.pos += 1;
                m.kid += 1;
                k += 1;
            }
            p.forget(a, b);
            let items = realise(m, &mut src, lid, a, b, first);
            let ni = items.len();
            if lead > 0 {
                old.splice(k..k, items);
                k += ni;
                if trail > 0 {
                    let run = new_run(m, lid, trail);
                    old.insert(k, run);
                }
            } else if trail > 0 {
                // the leaf, now after the items, is the trailing run
                old[k].run = trail;
                old.splice(k..k, items);
                k += ni;
            } else {
                m.rt.bough.remove_run(m.pos, 1);
                m.rt.remove_layout_subtree(leaf);
                m.rt.lay.eng.splice_kids(&m.rt.bough, lid, m.kid, 1, &[]);
                old.splice(k..k + 1, items);
                k += ni;
            }
            i = b;
            touched = true;
            p.resize = true;
            continue;
        }
        // a realised item at index `i`
        if drops.get(di).is_some_and(|x| x.0 == i) {
            // released: this item and the ones after it the scan released too collapse into a run
            pending = None;
            let k1 = {
                let (mut k1, mut j, mut dj) = (k, i, di);
                while k1 < old.len() && old[k1].run == 0 && drops.get(dj).is_some_and(|x| x.0 == j) {
                    k1 += 1;
                    j += 1;
                    dj += 1;
                }
                k1
            };
            let cnt = (k1 - k) as u32;
            for x in &drops[di..di + cnt as usize] {
                p.remember(x.0, x.1);
            }
            di += cnt as usize;
            let (nrec, nkid) = old[k..k1].iter().fold((0usize, 0usize), |a, e| (a.0 + e.nrec as usize, a.1 + e.nkid as usize));
            remove_items(m, lid, nrec, nkid);
            for gone in old.drain(k..k1) {
                seals.dead += gone.seal.1;
                m.rt.release_scope(gone.scope);
            }
            m.rt.dorm.stats.released += cnt as u64;
            touched = true;
            p.resize = true;
            if k > 0 && old[k - 1].run != 0 {
                old[k - 1].run += cnt;
                i += cnt;
            } else if old.get(k).is_some_and(|e| e.run != 0) {
                // the run after them now starts at `i`
                old[k].run += cnt;
            } else {
                let run = new_run(m, lid, cnt);
                old.insert(k, run);
            }
            continue;
        }
        let e = match pending.take() {
            Some(e) => e,
            // SAFETY: an indexed source, `i < n_new`, indices asked in ascending order, once (`pending`).
            None => unsafe { src.item(i as usize) },
        };
        if old[k].key != e.dyn_key() {
            // a keyed item that changed identity: replaced in place (its run out, the new one in)
            let gone = std::mem::replace(&mut old[k], DynItem::empty(0));
            seals.dead += gone.seal.1;
            remove_items(m, lid, gone.nrec as usize, gone.nkid as usize);
            m.rt.release_scope(gone.scope);
            let mut it = realise(m, &mut src, lid, i, i + 1, Some(e));
            old[k] = it.pop().unwrap();
            touched = true;
            i += 1;
            k += 1;
            continue;
        }
        let (same, shape) = sealed_as(&old[k], &e, &seals, &m.rt.lay.descs, cx);
        if same {
            m.pos += old[k].nrec as usize;
            m.kid += old[k].nkid as usize;
            k += 1;
            i += 1;
            continue;
        }
        // edits by description (`describe::update_dyn`), between neighbouring realised items
        if !shape && old[k].seal.1 != 0 {
            if k + 1 < old.len() && old[k + 1].run == 0 && sealed_as(&old[k + 1], &e, &seals, &m.rt.lay.descs, cx).1 {
                let gone = old.remove(k);
                seals.dead += gone.seal.1;
                remove_items(m, lid, gone.nrec as usize, gone.nkid as usize);
                m.rt.release_scope(gone.scope);
                p.shift(i, -1);
                touched = true;
                pending = Some(e); // against the retained item now at `k`, still index `i`
                continue;
            }
            if i + 1 < n_new {
                // SAFETY: as above; index `i + 1` is kept in `pending` and never asked again.
                let e2 = unsafe { src.item(i as usize + 1) };
                let hit = sealed_as(&old[k], &e2, &seals, &m.rt.lay.descs, cx).1;
                pending = Some(e2);
                if hit {
                    let mut it = realise(m, &mut src, lid, i, i + 1, Some(e));
                    m.rt.dorm.stats.realised -= 1;
                    old.insert(k, it.pop().unwrap());
                    p.shift(i, 1);
                    touched = true;
                    k += 1;
                    i += 1;
                    continue;
                }
            }
        }
        rewrite_item(m, &mut old[k], e, &mut seals, &mut scratch, cx);
        k += 1;
        i += 1;
    }
    drop(pending);
    if k < old.len() {
        // the source is shorter: the entries past its end go, as one run of records
        let (nrec, nkid) = old[k..].iter().fold((0usize, 0usize), |a, e| (a.0 + e.nrec as usize, a.1 + e.nkid as usize));
        remove_items(m, lid, nrec, nkid);
        for gone in old.drain(k..) {
            seals.dead += gone.seal.1;
            m.rt.release_scope(gone.scope);
        }
        p.forget(n_new, u32::MAX);
        touched = true;
        p.resize = true;
    } else if i < n_new {
        // the source is longer than the entries cover: the rest is dormant
        let add = n_new - i;
        match old.last_mut() {
            Some(last) if last.run != 0 => last.run += add,
            _ => {
                let run = new_run(m, lid, add);
                old.push(run);
                m.pos += 1;
                m.kid += 1;
            }
        }
        touched = true;
        p.resize = true;
    }
    p.n = n_new;
    if touched {
        m.rt.lay.eng.invalidate(&m.rt.bough, lid);
        m.rt.queue_container(lid);
        p.key.lay = u64::MAX;
    }
    if p.resize {
        m.rt.dorm_resize(&mut p, &old);
    }
    repack_seals(&mut seals, &mut old);
    m.rt.dyns.seals[d as usize] = seals;
    m.rt.dyns.parts[d as usize] = old;
    m.rt.dyns.dorm[d as usize] = Some(p);
}
