//! Transitions of a dynamic part's items: `crossfade`, `slide`, `morph` (with `tag` pairs), and an item's identity
//! (`keyed`). The async lane's programs 7, 13 and 14; the design is `docs/lanes/async/DESIGN.md` section 5 and the
//! kernel contract `docs/lanes/async/TRANSITIONS.md`.
//!
//! **Leaving.** When a switch's item changes key (an arm, a `keyed` key) or stops being described, the old item
//! is not removed: its layout children are detached (the next item lays out in its place), its records stay where
//! they are as a *leaving* segment of the item's slot (pointer-transparent: `KF_NOHIT`), and its wires stay live
//! (it is still on screen). The new item mounts after the segments already in the slot. The walk steps over the
//! segments (`DynItem::lead`, `trail`); a slot's segments only change inside a walk of their part, so no record
//! count above it ever goes stale.
//!
//! **Transitions are parameters.** Every record group of both items gets a transition `(from, to, t0, duration,
//! curve)`: of its alpha (a node's paint), and for `slide` and `morph` pairs of an affine of its *space* (an item,
//! or a tagged part, is mounted in a space of its own; the kernels draw every instance and glyph run of a space
//! through that space's affine, and the record half moves the records' covers for damage and hit testing). That
//! is the compute kernel's own form of a transition (`push_anim(a, b, t0, dur, ease)` over the record's command at
//! both ends); on the Metal path the presenter evaluates it on the CPU (`App::tick` -> `step_switches`) through the
//! ordinary appliers. No handler, settle or description runs while it plays. When it ends, one description runs
//! (scheduled by the presenter) and the walk removes the leaving segments.
//!
//! **Interrupting.** A new key equal to a leaving item's key revives that item: it becomes current again (its
//! layout children come back, its records hit again), the item it replaced starts leaving, and every transition of
//! the slot is retargeted from the value it shows at that moment: the motion is continuous by construction.
use crate::boxops::{ApplyOpacity, ApplyTextColor};
use crate::color::Color;
use crate::describe::{mount_item, DynItem, Seals};
use crate::elem::{Apply, Element, Mounter};
use crate::rt::*;
use crate::transition::Curve;
use bough_core::{Handle, RecordInit, KF_FOCUS, KF_INTERACT, KF_NOHIT};

/// The key of an item that is no longer described and holds leaving records until its transition ends.
pub(crate) const GHOST: u64 = u64::MAX;
/// A slide moves the items by this fraction of their container's width.
const SLIDE: f32 = 0.25;

/// An element whose identity in a dynamic part is `key`: a new key is a new item (replaced, not updated).
pub struct Keyed<E> {
    key: u64,
    e: E,
}
pub fn keyed<E>(key: u64, e: E) -> Keyed<E> {
    Keyed { key, e }
}
impl<V, E: Element<V>> Element<V> for Keyed<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_dyn(m, std::iter::once(crate::describe::ArmItem(self.e, self.key.wrapping_add(1))));
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_dyn(m, std::iter::once(crate::describe::ArmItem(self.e, self.key.wrapping_add(1))));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Crossfade,
    Slide,
    Morph,
}

/// A dynamic part whose item changes are animated (see the module docs).
pub struct Switch<E> {
    kind: Kind,
    curve: Curve,
    e: E,
}

/// The leaving item fades out while the entering one fades in.
pub fn crossfade<E>(curve: Curve, e: E) -> Switch<E> {
    Switch { kind: Kind::Crossfade, curve, e }
}
/// The leaving item moves out to one side and the entering one in from the other, both fading. Reviving the
/// leaving item (its key again) reverses the motion from where it is.
pub fn slide<E>(curve: Curve, e: E) -> Switch<E> {
    Switch { kind: Kind::Slide, curve, e }
}
/// Parts with the same `tag` in the leaving and the entering item move and scale from one's box to the other's
/// while cross-fading; parts without a partner fade.
pub fn morph<E>(curve: Curve, e: E) -> Switch<E> {
    Switch { kind: Kind::Morph, curve, e }
}

/// What a switch hands the dynamic part directly inside it (taken by that part's `mount_dyn` / `update_dyn`).
#[derive(Clone, Copy)]
pub(crate) struct SwitchCtx {
    pub kind: Kind,
    pub curve: Curve,
}

impl<V, E: Element<V>> Element<V> for Switch<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        m.rt.switching = Some(SwitchCtx { kind: self.kind, curve: self.curve });
        self.e.mount(m);
        m.rt.switching = None;
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        m.rt.switching = Some(SwitchCtx { kind: self.kind, curve: self.curve });
        self.e.update(m);
        m.rt.switching = None;
    }
    // no `seal`: a switch is never stepped over (its part may hold leaving records to remove)
}

/// A part's pairing name across the items of a `morph`.
pub struct Tagged<E> {
    tag: u32,
    e: E,
}
pub trait Tag: Sized {
    fn tag(self, t: u32) -> Tagged<Self> {
        Tagged { tag: t, e: self }
    }
}
impl<E> Tag for E {}
impl<V, E: Element<V>> Element<V> for Tagged<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        if m.rt.switches.tagging == 0 {
            return self.e.mount(m);
        }
        // inside an item of a morph: the part is mounted in a space of its own (its transition's affine)
        let s = m.rt.alloc_space_in(m.space);
        let o = m.rt.space(m.space);
        m.rt.set_space(s, o);
        let (old, r0, n0) = (m.space, m.recs.len(), m.scope.nodes.len());
        m.space = s;
        self.e.mount(m);
        m.space = old;
        let first = m.recs.get(r0).map_or(0, |r| r.handle);
        // its box: the first node it made that layout places
        let node = m.scope.nodes[n0..].iter().copied().find(|&n| m.rt.nodes[n as usize].lnode != NONE).unwrap_or(NONE);
        let first = if first != 0 { m.rt.bough.handle_gen(first) } else { NO_HANDLE };
        m.rt.switches.tagbuf.push(TagMark { tag: self.tag, space: s, first, node });
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        self.e.update(m)
    }
    fn dyn_key(&self) -> u64 {
        self.e.dyn_key()
    }
}

// =============================================================================== the runtime's half

const NO_HANDLE: Handle = Handle { idx: 0, generation: 0 };

#[derive(Clone, Copy)]
struct TagMark {
    tag: u32,
    space: u16,
    /// The part's first record (its subtree is the part's run).
    first: Handle,
    /// The node whose laid-out box is the part's box.
    node: u32,
}

/// An item's switch effects: its own space (`slide`), its tagged parts (`morph`).
struct ItemFx {
    /// 0: none.
    space: u16,
    /// Its first record and its record count (the run its space's affine covers).
    first: Handle,
    nrec: u32,
    tags: Vec<TagMark>,
}

impl Default for ItemFx {
    fn default() -> Self {
        ItemFx { space: 0, first: NO_HANDLE, nrec: 0, tags: Vec::new() }
    }
}

/// One end of a transition: alpha (nodes) or a space's affine (scale about a pivot, then translation).
#[derive(Clone, Copy, PartialEq, Debug)]
struct Fx {
    a: f32,
    sx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
}
const ID: Fx = Fx { a: 1.0, sx: 1.0, sy: 1.0, tx: 0.0, ty: 0.0 };
const OUT: Fx = Fx { a: 0.0, ..ID };

impl Fx {
    fn lerp(a: Fx, b: Fx, t: f32) -> Fx {
        if t >= 1.0 {
            return b;
        }
        let l = |x: f32, y: f32| x + (y - x) * t;
        Fx { a: l(a.a, b.a), sx: l(a.sx, b.sx), sy: l(a.sy, b.sy), tx: l(a.tx, b.tx), ty: l(a.ty, b.ty) }
    }
}

/// What a transition drives.
#[derive(Clone, Copy, PartialEq)]
enum Target {
    /// A node's paint: a text's colour or a box's opacity, from its own (`base`) value.
    Text { node: u32, base: u32 },
    Quad { node: u32, base: f32 },
    /// A space's affine: an item's (`slide`) or a tagged part's (`morph`), about `pivot` (space-local px).
    Item { fx: u32 },
    Tag { fx: u32, i: u32 },
}

impl Target {
    fn same(&self, o: &Target) -> bool {
        match (self, o) {
            (Target::Text { node: a, .. }, Target::Text { node: b, .. }) | (Target::Quad { node: a, .. }, Target::Quad { node: b, .. }) => a == b,
            (a, b) => a == b,
        }
    }
}

/// An end that needs layout: the affine that puts `own`'s laid-out box onto `other`'s.
#[derive(Clone, Copy)]
enum End {
    At(Fx),
    Onto { own: u32, other: u32 },
}

#[derive(Clone, Copy)]
struct Track {
    target: Target,
    from: End,
    to: End,
    pivot: [f32; 2],
    /// The value it shows (applied last); `fresh` until it was applied once.
    shown: Fx,
    fresh: bool,
}

/// A leaving item: its records stay in the slot until the transition ends.
struct Leaving {
    item: DynItem,
    /// Its detached layout children.
    kids: Vec<u32>,
    /// Its records' interaction flags before it left (restored if it comes back).
    flags: Vec<(Handle, u8)>,
}

/// A slot's segments in record order: the current item (`Own`, possibly empty: a ghost) and leaving ones.
enum Seg {
    Own,
    Gone(Leaving),
}

/// The transition of one item slot of a dynamic part.
struct Fade {
    d: u32,
    k: u32,
    segs: Vec<Seg>,
    tracks: Vec<Track>,
    t0: f64,
    curve: Curve,
    done: bool,
}

/// Every slot with a transition, the effects of switch items, and the mount-time state of tags.
#[derive(Default)]
pub struct Switches {
    fades: Vec<Fade>,
    fx: Vec<ItemFx>,
    fx_free: Vec<u32>,
    /// Mounting an item of a morph: tagged parts take spaces of their own.
    pub(crate) tagging: u32,
    tagbuf: Vec<TagMark>,
    /// Walks of switch parts (descriptions that reached one) and presenter steps, for tests and benches.
    pub walks: u64,
    pub steps: u64,
}

impl Switches {
    #[inline(always)]
    pub(crate) fn any(&self) -> bool {
        !self.fades.is_empty()
    }
    /// Is a transition playing (the presenter must keep stepping)?
    pub fn running(&self) -> bool {
        self.fades.iter().any(|f| !f.done)
    }
    /// Transitions that ended and wait for a description to remove their leaving records.
    pub fn ended(&self) -> usize {
        self.fades.iter().filter(|f| f.done).count()
    }
    /// Leaving items still holding records.
    pub fn leaving(&self) -> usize {
        self.fades.iter().map(|f| f.segs.iter().filter(|s| matches!(s, Seg::Gone(_))).count()).sum()
    }
    /// The affine a tagged part of the current items shows now, as (sx, sy, tx, ty) (tests).
    pub fn tag_shown(&self, tag: u32) -> Option<[f32; 4]> {
        for f in &self.fades {
            for t in &f.tracks {
                if let Target::Tag { fx, i } = t.target
                    && self.fx[fx as usize].tags[i as usize].tag == tag
                    && matches!(t.to, End::At(ID))
                {
                    return Some([t.shown.sx, t.shown.sy, t.shown.tx, t.shown.ty]);
                }
            }
        }
        None
    }
    /// The x offsets the items of the playing slides show now: (entering, leaving...) (tests).
    pub fn slide_shown(&self) -> Vec<f32> {
        self.fades.iter().flat_map(|f| f.tracks.iter().filter(|t| matches!(t.target, Target::Item { .. })).map(|t| t.shown.tx)).collect()
    }
}

pub(crate) fn has_part(rt: &Rt, d: u32) -> bool {
    rt.switches.fades.iter().any(|f| f.d == d)
}

fn take_fade(rt: &mut Rt, d: u32, k: u32) -> Option<Fade> {
    let i = rt.switches.fades.iter().position(|f| f.d == d && f.k == k)?;
    Some(rt.switches.fades.swap_remove(i))
}

fn alloc_fx(rt: &mut Rt) -> u32 {
    match rt.switches.fx_free.pop() {
        Some(i) => i,
        None => {
            rt.switches.fx.push(ItemFx::default());
            (rt.switches.fx.len() - 1) as u32
        }
    }
}

/// An item's effects go with it: its spaces (their affines cleared first: a space is reused).
pub(crate) fn free_fx(rt: &mut Rt, fx: u32) {
    let f = std::mem::take(&mut rt.switches.fx[fx as usize]);
    let mut spaces: Vec<u16> = f.tags.iter().map(|t| t.space).collect();
    if f.space != 0 {
        spaces.push(f.space);
    }
    for s in spaces {
        rt.set_space_xform(s, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0], [0.0, 0.0]);
        rt.free_space(s);
    }
    rt.switches.fx_free.push(fx);
}

/// The item's records were rewritten: its count (its space's run).
pub(crate) fn resized(rt: &mut Rt, fx: u32, nrec: u32) {
    rt.switches.fx[fx as usize].nrec = nrec;
}

/// Mount an item of a switch part: in a space of its own (`slide`), its tagged parts in theirs (`morph`).
pub(crate) fn mount_item_fx<V, E: Element<V>>(m: &mut Mounter<'_, V>, sw: SwitchCtx, e: E, mut into: Option<&mut Vec<RecordInit>>) -> DynItem {
    let fx = alloc_fx(m.rt);
    let old = m.space;
    let mut space = 0;
    if sw.kind == Kind::Slide {
        space = m.rt.alloc_space_in(m.space);
        let o = m.rt.space(m.space);
        m.rt.set_space(space, o);
        m.space = space;
    }
    let t0 = m.rt.switches.tagbuf.len();
    if sw.kind == Kind::Morph {
        m.rt.switches.tagging += 1;
    }
    let r0 = into.as_ref().map_or(m.recs.len(), |b| b.len());
    let mut it = mount_item(m, e, into.as_deref_mut());
    let first = match into.as_ref() {
        Some(b) => b.get(r0).map_or(0, |r| r.handle),
        None => m.recs.get(r0).map_or(0, |r| r.handle),
    };
    m.space = old;
    if sw.kind == Kind::Morph {
        m.rt.switches.tagging -= 1;
    }
    let tags = m.rt.switches.tagbuf.split_off(t0);
    let first = if first != 0 { m.rt.bough.handle_gen(first) } else { NO_HANDLE };
    m.rt.switches.fx[fx as usize] = ItemFx { space, first, nrec: it.nrec, tags };
    it.fx = fx;
    it
}

// ------------------------------------------------------------------------------- tracks

/// The painted nodes of a scope and of the items of its dynamic parts.
fn nodes_of(rt: &Rt, scope: &Scope, out: &mut Vec<Target>) {
    for &n in &scope.nodes {
        let nd = &rt.nodes[n as usize];
        if !nd.live {
            continue;
        }
        if nd.text != NONE {
            out.push(Target::Text { node: n, base: rt.texts[nd.text as usize].color });
        } else if nd.run != NONE && (nd.kind & 0x7f == NK_QUAD || nd.kind & 0x7f == NK_BORDER) {
            out.push(Target::Quad { node: n, base: nd.alpha });
        }
    }
    for &d in &scope.dyns {
        for it in &rt.dyns.parts[d as usize] {
            nodes_of(rt, &it.scope, out);
        }
    }
}

/// A track from what `t` shows now (or `start`) to `to`; a retargeted one keeps its base (the node shows a
/// transitioned value now, not its own) and its pivot.
fn track(old: &[Track], t: Target, start: End, to: End) -> Track {
    match old.iter().find(|x| x.target.same(&t)) {
        Some(x) => Track { target: x.target, from: End::At(x.shown), to, pivot: x.pivot, shown: x.shown, fresh: x.fresh },
        // what it shows now is its own value (the identity), whatever it starts from
        None => Track { target: t, from: start, to, pivot: [0.0; 2], shown: ID, fresh: true },
    }
}

/// The box layout gave `node` last (space-local px), if it was laid out.
fn box_of(rt: &Rt, node: u32) -> Option<[f32; 4]> {
    if node == NONE {
        return None;
    }
    let l = rt.nodes[node as usize].lnode;
    let li = rt.lay.info.get(l as usize)?;
    (l != NONE && li.written).then_some([li.ax, li.ay, li.aw, li.ah])
}

/// Resolve the ends that need layout; false while they cannot be (the entering item is not laid out yet).
fn resolve(rt: &Rt, t: &mut Track) -> bool {
    for end in [&mut t.from, &mut t.to] {
        if let End::Onto { own, other } = *end {
            let (Some(a), Some(b)) = (box_of(rt, own), box_of(rt, other)) else { return false };
            let c = [a[0] + 0.5 * a[2], a[1] + 0.5 * a[3]];
            t.pivot = c;
            let s = |x: f32, y: f32| if x > 0.0 && y > 0.0 { y / x } else { 1.0 };
            *end = End::At(Fx { a: 1.0, sx: s(a[2], b[2]), sy: s(a[3], b[3]), tx: b[0] + 0.5 * b[2] - c[0], ty: b[1] + 0.5 * b[3] - c[1] });
        }
    }
    true
}

fn at(e: End) -> Fx {
    match e {
        End::At(f) => f,
        End::Onto { .. } => ID,
    }
}

/// Apply a track's value (a resolved track).
fn apply(rt: &mut Rt, t: &mut Track, v: Fx) {
    let last = t.shown;
    t.shown = v;
    let scaled = |base: u32, a: f32| (base & 0x00ff_ffff) | ((((base >> 24) & 0xff) as f32 * a).round().clamp(0.0, 255.0) as u32) << 24;
    match &mut t.target {
        Target::Text { node, base } => {
            // a description or a wire wrote the colour since the last value: that is its own colour now
            let ti = rt.nodes[*node as usize].text;
            if ti != NONE && rt.texts[ti as usize].color != scaled(*base, last.a) {
                *base = rt.texts[ti as usize].color;
            }
            ApplyTextColor::apply(rt, *node, &Color(scaled(*base, v.a)));
        }
        Target::Quad { node, base } => {
            let cur = rt.nodes[*node as usize].alpha;
            if cur != *base * last.a {
                *base = cur;
            }
            ApplyOpacity::apply(rt, *node, &(*base * v.a));
        }
        &mut Target::Item { fx } => {
            let f = &rt.switches.fx[fx as usize];
            let (s, first, n) = (f.space, f.first, f.nrec as usize);
            xform(rt, s, first, Some(n), t.pivot, v);
        }
        &mut Target::Tag { fx, i } => {
            let m = rt.switches.fx[fx as usize].tags[i as usize];
            xform(rt, m.space, m.first, None, t.pivot, v);
        }
    }
}

/// A space's affine, both halves: the kernels' (every instance and glyph run of the space) and the records'
/// (their covers: damage, hit testing). `n`: the run's length, or the subtree of `first`.
fn xform(rt: &mut Rt, s: u16, first: Handle, n: Option<usize>, pivot: [f32; 2], v: Fx) {
    if s == 0 {
        return;
    }
    let m = [v.sx, 0.0, 0.0, v.sy];
    // a pure translation moves by whole px (sharp glyphs; the kernels' and the covers' arithmetic agree), as a
    // transformed box's does (ltemplate)
    let v = if m == [1.0, 0.0, 0.0, 1.0] { Fx { tx: v.tx.round(), ty: v.ty.round(), ..v } } else { v };
    rt.set_space_xform(s, m, pivot, [v.tx, v.ty]);
    let Some(pos) = rt.bough.pos_of(first) else { return };
    let n = n.unwrap_or_else(|| rt.bough.subtree_end(pos) - pos);
    let o = rt.space(s);
    rt.bough.set_run_xform(pos, n, m, [o[0] + pivot[0], o[1] + pivot[1]], [v.tx, v.ty]);
}

fn progress(f: &Fade, now: f64) -> (f32, bool) {
    let p = if f.curve.dur <= 0.0 { 1.0 } else { (((now - f.t0) / f.curve.dur as f64) as f32).clamp(0.0, 1.0) };
    (if p >= 1.0 { 1.0 } else { f.curve.ease.at(p) }, p >= 1.0)
}

fn step(rt: &mut Rt, f: &mut Fade, now: f64) {
    let (e, end) = progress(f, now);
    for k in 0..f.tracks.len() {
        let mut t = f.tracks[k];
        if resolve(rt, &mut t) {
            let v = Fx::lerp(at(t.from), at(t.to), e);
            let space = matches!(t.target, Target::Item { .. } | Target::Tag { .. });
            if v != t.shown || space || t.fresh {
                apply(rt, &mut t, v);
                t.fresh = false;
            }
        }
        f.tracks[k] = t;
    }
    if end {
        f.done = true;
    }
}

/// Item `k` of part `d` was described again while its transition plays: what the description wrote is the
/// items' own value now, and the transition's value goes back on top.
pub(crate) fn reapply(rt: &mut Rt, d: u32, k: u32) {
    let Some(i) = rt.switches.fades.iter().position(|f| f.d == d && f.k == k) else { return };
    let mut tracks = std::mem::take(&mut rt.switches.fades[i].tracks);
    for t in tracks.iter_mut() {
        if resolve(rt, t) {
            let v = t.shown;
            apply(rt, t, v);
        }
    }
    rt.switches.fades[i].tracks = tracks;
}

impl Rt {
    /// The presenter's clock is at `transitions.now`: every playing transition takes its value there. Returns
    /// (any played, any ended): an ended one waits for one description to remove its leaving records.
    pub(crate) fn step_switches(&mut self) -> (bool, bool) {
        if !self.switches.running() {
            return (false, false);
        }
        let now = self.transitions.now;
        let mut fades = std::mem::take(&mut self.switches.fades);
        let mut ended = false;
        for f in fades.iter_mut().filter(|f| !f.done) {
            step(self, f, now);
            ended |= f.done;
        }
        self.switches.steps += 1;
        fades.append(&mut self.switches.fades);
        self.switches.fades = fades;
        (true, ended)
    }
}

// ------------------------------------------------------------------------------- the walk's half

/// The records of the segments before segment `i`.
fn before(segs: &[Seg], own: &DynItem, i: usize) -> usize {
    segs[..i]
        .iter()
        .map(|s| match s {
            Seg::Own => own.nrec as usize,
            Seg::Gone(l) => l.item.nrec as usize,
        })
        .sum()
}

fn own_index(segs: &[Seg]) -> usize {
    segs.iter().position(|s| matches!(s, Seg::Own)).expect("a slot has its own item")
}

/// The item's lead and trail from its slot's segments.
fn recount(segs: &[Seg], it: &mut DynItem) {
    let oi = own_index(segs);
    it.lead = before(segs, it, oi) as u32;
    let total = before(segs, it, segs.len());
    it.trail = (total - it.lead as usize - it.nrec as usize) as u32;
}

/// Make the records `[pos, pos+n)` pointer-transparent, keeping their flags.
fn no_hit(rt: &mut Rt, pos: usize, n: usize, flags: &mut Vec<(Handle, u8)>) {
    for p in pos..pos + n {
        if let Some(h) = rt.bough.handle_at(p) {
            let k = rt.bough.kind_at(p) & (KF_INTERACT | KF_FOCUS | KF_NOHIT);
            flags.push((h, k));
            rt.bough.set_kind_flags(h, KF_NOHIT, KF_INTERACT | KF_FOCUS);
        }
    }
}

fn hit_again(rt: &mut Rt, flags: &[(Handle, u8)]) {
    for &(h, k) in flags {
        rt.bough.set_kind_flags(h, k, KF_NOHIT & !k);
    }
}

/// The item in the slot (its records at `at`) starts leaving: its kids detached, its records kept.
fn leave<V>(m: &mut Mounter<'_, V>, cur: DynItem, at: usize, lid: u32) -> Leaving {
    let kids: Vec<u32> = if lid != NONE && cur.nkid > 0 { m.rt.lay.eng.kids(lid)[m.kid..m.kid + cur.nkid as usize].to_vec() } else { Vec::new() };
    if !kids.is_empty() {
        m.rt.lay.eng.splice_kids(&m.rt.bough, lid, m.kid, kids.len(), &[]);
        m.rt.queue_container(lid);
    }
    let mut flags = Vec::new();
    no_hit(m.rt, at, cur.nrec as usize, &mut flags);
    Leaving { item: DynItem { lead: 0, trail: 0, ..cur }, kids, flags }
}

/// A leaving item's records are gone (removed by the caller): what it still owns goes.
fn release_leaving(rt: &mut Rt, l: Leaving) {
    for k in l.kids {
        rt.remove_layout_subtree(k);
    }
    rt.release_item(l.item);
}

/// The tracks of a slot whose current item is `own` (entering from transparent and, sliding, from the side,
/// unless it shows a value) and whose other segments leave; `old` are the tracks it had (retargeted from what
/// they show).
fn tracks_for(rt: &Rt, kind: Kind, segs: &[Seg], own: &DynItem, old: &[Track], width: f32, entering: bool) -> Vec<Track> {
    let mut out = Vec::new();
    let mut ts = Vec::new();
    nodes_of(rt, &own.scope, &mut ts);
    for t in ts.drain(..) {
        out.push(track(old, t, End::At(if entering { OUT } else { ID }), End::At(ID)));
    }
    // the side the entering item comes from (a revived one: where it is now); the leaving ones exit the other way
    let own_fx = (own.fx != NONE).then_some(own.fx);
    let mut from_x = width;
    if kind == Kind::Slide && let Some(fx) = own_fx {
        let tr = track(old, Target::Item { fx }, End::At(Fx { tx: width, ..ID }), End::At(ID));
        from_x = at(tr.from).tx;
        out.push(tr);
    }
    let exit_x = if from_x >= 0.0 { -width } else { width };
    let last_gone = segs.iter().rposition(|s| matches!(s, Seg::Gone(_)));
    for (i, s) in segs.iter().enumerate() {
        let Seg::Gone(l) = s else { continue };
        nodes_of(rt, &l.item.scope, &mut ts);
        for t in ts.drain(..) {
            out.push(track(old, t, End::At(ID), End::At(OUT)));
        }
        if l.item.fx == NONE {
            continue;
        }
        if kind == Kind::Slide {
            out.push(track(old, Target::Item { fx: l.item.fx }, End::At(ID), End::At(Fx { tx: exit_x, ..ID })));
        }
        if kind == Kind::Morph && Some(i) == last_gone && let Some(own_fx) = own_fx {
            // pairs by tag between the item that left last and the entering one
            let (a, b) = (&rt.switches.fx[own_fx as usize], &rt.switches.fx[l.item.fx as usize]);
            for (ia, ta) in a.tags.iter().enumerate() {
                if let Some((ib, tb)) = b.tags.iter().enumerate().find(|x| x.1.tag == ta.tag) {
                    out.push(track(old, Target::Tag { fx: own_fx, i: ia as u32 }, End::Onto { own: ta.node, other: tb.node }, End::At(ID)));
                    out.push(track(old, Target::Tag { fx: l.item.fx, i: ib as u32 }, End::At(ID), End::Onto { own: tb.node, other: ta.node }));
                }
            }
        }
    }
    out
}

fn width_of(rt: &Rt, lid: u32) -> f32 {
    let w = if lid != NONE { rt.lay.info.get(lid as usize).map_or(0.0, |l| l.aw) } else { 0.0 };
    if w > 0.0 { w * SLIDE } else { 120.0 * rt.scale }
}

fn start(rt: &mut Rt, mut f: Fade) {
    let now = rt.transitions.now;
    f.t0 = now;
    f.done = false;
    // the first value now: no frame may show the swap before the transition's start
    step(rt, &mut f, now);
    f.done = false;
    rt.switches.fades.push(f);
}

/// Item `k` of part `d` changed key under a switch (the walk is at its slot): it leaves, and the item with the new
/// key comes back from the leaving ones or is mounted after them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn replace<V, E: Element<V>>(m: &mut Mounter<'_, V>, sw: SwitchCtx, d: u32, k: u32, item: &mut DynItem, seals: &mut Seals, e: E, key: u64, lid: u32) {
    let slot = m.pos;
    let mut f = take_fade(m.rt, d, k).unwrap_or(Fade { d, k, segs: vec![Seg::Own], tracks: Vec::new(), t0: 0.0, curve: sw.curve, done: false });
    f.curve = sw.curve;
    let cur = std::mem::replace(item, DynItem::empty(key));
    seals.note_dead(cur.seal.1);
    let oi = own_index(&f.segs);
    let at = slot + before(&f.segs, &cur, oi);
    if cur.nrec > 0 || cur.nkid > 0 {
        let l = leave(m, cur, at, lid);
        f.segs[oi] = Seg::Gone(l);
    } else {
        f.segs.remove(oi);
        m.rt.release_item(cur);
    }
    let width = width_of(m.rt, lid);
    let back = f.segs.iter().position(|s| matches!(s, Seg::Gone(l) if l.item.key == key));
    let entering = match back {
        Some(i) => {
            // it comes back: current again, updated by this description where its records are
            let Seg::Gone(l) = std::mem::replace(&mut f.segs[i], Seg::Own) else { unreachable!() };
            hit_again(m.rt, &l.flags);
            if lid != NONE && !l.kids.is_empty() {
                m.rt.lay.eng.splice_kids(&m.rt.bough, lid, m.kid, 0, &l.kids);
                m.rt.queue_container(lid);
            }
            let mut it = l.item;
            it.seal = (0, 0);
            m.pos = slot + before(&f.segs, &it, i);
            let (p0, k0) = (m.pos, m.kid);
            let recs: *mut Vec<RecordInit> = m.recs;
            {
                // SAFETY: reborrow of the mounter's buffer for the sub-walk only.
                let mut sub = m.sub(unsafe { &mut *recs }, &mut it.scope, &mut it.tape as *mut Vec<u32>);
                e.update(&mut sub);
                let (p1, k1) = (sub.pos, sub.kid);
                drop(sub);
                m.pos = p1;
                m.kid = k1;
            }
            it.nrec = (m.pos - p0) as u32;
            it.nkid = (m.kid - k0) as u32;
            if it.fx != NONE {
                resized(m.rt, it.fx, it.nrec);
            }
            *item = it;
            false
        }
        None => {
            let mut buf: Vec<RecordInit> = Vec::new();
            let mut it = mount_item_fx(m, sw, e, Some(&mut buf));
            it.key = key;
            let end = slot + before(&f.segs, &it, f.segs.len());
            m.rt.bough.insert_run(end, &buf);
            if lid != NONE && it.nkid > 0 {
                m.rt.lay.eng.place_tail_kids(lid, m.kid, it.nkid as usize);
                m.rt.lay.eng.invalidate(&m.rt.bough, lid);
                m.rt.queue_container(lid);
            }
            m.kid += it.nkid as usize;
            f.segs.push(Seg::Own);
            *item = it;
            true
        }
    };
    item.key = key;
    recount(&f.segs, item);
    m.pos = slot + item.slot();
    f.tracks = tracks_for(m.rt, sw.kind, &f.segs, item, &f.tracks, width, entering);
    start(m.rt, f);
}

/// Item `k` appeared under a switch (a fresh item, inserted): it comes in with the transition.
pub(crate) fn enter(rt: &mut Rt, sw: SwitchCtx, d: u32, k: u32, it: &DynItem, lid: u32) {
    let width = width_of(rt, lid);
    let segs = vec![Seg::Own];
    let tracks = tracks_for(rt, sw.kind, &segs, it, &[], width, true);
    start(rt, Fade { d, k, segs, tracks, t0: 0.0, curve: sw.curve, done: false });
}

/// Item `k` is no longer described under a switch: it becomes a ghost (an empty item) holding its records while
/// they leave. The walk moves past its slot.
pub(crate) fn vanish<V>(m: &mut Mounter<'_, V>, sw: SwitchCtx, d: u32, k: u32, item: &mut DynItem, seals: &mut Seals, lid: u32) {
    let slot = m.pos;
    if item.key == GHOST {
        m.pos = slot + item.slot();
        return;
    }
    let mut f = take_fade(m.rt, d, k).unwrap_or(Fade { d, k, segs: vec![Seg::Own], tracks: Vec::new(), t0: 0.0, curve: sw.curve, done: false });
    f.curve = sw.curve;
    let cur = std::mem::replace(item, DynItem::empty(GHOST));
    seals.note_dead(cur.seal.1);
    let oi = own_index(&f.segs);
    let at = slot + before(&f.segs, &cur, oi);
    let l = leave(m, cur, at, lid);
    f.segs[oi] = Seg::Gone(l);
    f.segs.push(Seg::Own);
    recount(&f.segs, item);
    m.pos = slot + item.slot();
    let width = width_of(m.rt, lid);
    f.tracks = tracks_for(m.rt, sw.kind, &f.segs, item, &f.tracks, width, false);
    start(m.rt, f);
}

/// The walk is at item `k`'s slot: if its transition ended, its leaving records go now.
pub(crate) fn expire<V>(m: &mut Mounter<'_, V>, d: u32, k: u32, item: &mut DynItem, _lid: u32) {
    let Some(i) = m.rt.switches.fades.iter().position(|f| f.d == d && f.k == k && f.done) else { return };
    let f = m.rt.switches.fades.swap_remove(i);
    let slot = m.pos;
    // back to front: the offsets of the segments before stay valid
    let mut end = slot + before(&f.segs, item, f.segs.len());
    for s in f.segs.into_iter().rev() {
        match s {
            Seg::Own => end -= item.nrec as usize,
            Seg::Gone(l) => {
                let n = l.item.nrec as usize;
                end -= n;
                m.rt.bough.remove_run(end, n);
                release_leaving(m.rt, l);
            }
        }
    }
    item.lead = 0;
    item.trail = 0;
}

/// Item `k`'s slot went (its records removed by the caller): its leaving items' resources go.
pub(crate) fn drop_item(rt: &mut Rt, d: u32, k: u32) {
    if let Some(f) = take_fade(rt, d, k) {
        drop_fade(rt, f);
    }
}

/// Part `d` went: every slot's transition with it.
pub(crate) fn drop_part(rt: &mut Rt, d: u32) {
    while let Some(i) = rt.switches.fades.iter().position(|f| f.d == d) {
        let f = rt.switches.fades.swap_remove(i);
        drop_fade(rt, f);
    }
}

fn drop_fade(rt: &mut Rt, f: Fade) {
    for s in f.segs {
        if let Seg::Gone(l) = s {
            release_leaving(rt, l);
        }
    }
}

/// Part `d`'s scope is cut: its leaving items' scopes (their wires sit on live signals) are taken out to be cut.
pub(crate) fn leaving_scopes(rt: &mut Rt, d: u32) -> Vec<Scope> {
    let mut out = Vec::new();
    for f in rt.switches.fades.iter_mut().filter(|f| f.d == d) {
        for s in f.segs.iter_mut() {
            if let Seg::Gone(l) = s {
                // the scope keeps its nodes, runs and parts (released with the item); wires and the rest are cut
                out.push(std::mem::take(&mut l.item.scope));
            }
        }
    }
    out
}

/// Put cut scopes back (in the order `leaving_scopes` took them).
pub(crate) fn restore_scopes(rt: &mut Rt, d: u32, mut scopes: Vec<Scope>) {
    scopes.reverse();
    for f in rt.switches.fades.iter_mut().filter(|f| f.d == d) {
        for s in f.segs.iter_mut() {
            if let Seg::Gone(l) = s
                && let Some(sc) = scopes.pop()
            {
                l.item.scope = sc;
            }
        }
    }
}

/// Item `j` of part `d` was removed from the part's list: the slots after it moved down by one.
pub(crate) fn shift_items(rt: &mut Rt, d: u32, j: u32) {
    for f in rt.switches.fades.iter_mut().filter(|f| f.d == d && f.k > j) {
        f.k -= 1;
    }
}
