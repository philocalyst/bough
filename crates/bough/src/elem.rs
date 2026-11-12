//! Typed builders, operands (`IntoProp`, `TextSrc`) and their lowering into the runtime tables.
//!
//! `Element<V>` is implemented by every builder; `mount` lowers it: allocates records, instances, text
//! nodes, and installs wires for every operand that is a wired signal (`sig!`, see `signal.rs`). A plain
//! `&Signal<T>` borrow is a constant of this description (no wire).
use crate::color::Color;
use crate::rt::*;
use crate::signal::{Num, Signal, WMap, Wired};
use crate::text::{TShape, TextNode};
use bough_core::{Handle, NRect, RecordInit};
use bough_metal::fused::pay_irun;
use bough_metal::instance::Instance;
use std::marker::PhantomData;
use std::ops::Range;

// =============================================================================== mounter

pub struct Mounter<'a, V> {
    pub(crate) rt: &'a mut Rt,
    pub(crate) owner: *const V,
    pub(crate) recs: &'a mut Vec<RecordInit>,
    pub(crate) scope: &'a mut Scope,
    pub(crate) depth: u16,
    /// Origin in points, local to the current space.
    pub(crate) ox: f32,
    pub(crate) oy: f32,
    pub(crate) space: u16,
    /// bough-core clip id the records mounted now get.
    pub(crate) clip: u16,
    /// The `NumFmt` of a numeric text operand being lowered (handed to its text node).
    pub(crate) pending_num: Option<bough_text::numeric::NumFmt>,
    /// Owner class of `owner` (0: the app's root; list rows and entities have their own, `Rt::classes`).
    pub(crate) cls: u32,
    /// Re-description (`describe.rs`; none with `Config::mount_once`): the owner's tape (node ids, dynamic parts)
    /// recorded by `mount` and walked by `update` (`ti`); null for a fixed owner.
    pub(crate) tape: *mut Vec<u32>,
    pub(crate) ti: usize,
    /// `update`: the Bough position of the next record, and the layout child index within `rt.lay.cur`.
    pub(crate) pos: usize,
    pub(crate) kid: usize,
    /// An operand asked to keep the element's node addressable (`select`: an outside table names it).
    pub(crate) keep: bool,
    /// The path of `owner` from the root (`OwnerRef`): what a route lowered here is dispatched along.
    pub(crate) own: OwnerRef,
}

impl<V> Mounter<'_, V> {
    pub fn scale(&self) -> f32 {
        self.rt.scale
    }
    /// Is this mount recording a tape (a re-describing owner)?
    #[inline(always)]
    pub(crate) fn taping(&self) -> bool {
        !self.tape.is_null()
    }
    #[inline(always)]
    pub(crate) fn tape_push(&mut self, x: u32) {
        if !self.tape.is_null() {
            // SAFETY: `tape` points at the tape the caller of the mount owns for the mount's duration.
            unsafe { (*self.tape).push(x) };
        }
    }
    /// Length of the tape being recorded.
    #[inline(always)]
    pub(crate) fn ti_len(&self) -> usize {
        // SAFETY: as `tape_push`.
        unsafe { (&*self.tape).len() }
    }
    #[inline(always)]
    pub(crate) fn tape_set(&mut self, i: usize, x: u32) {
        // SAFETY: as `tape_push`.
        unsafe { (&mut *self.tape)[i] = x };
    }
    /// The next tape entry (`update`).
    #[inline(always)]
    pub(crate) fn tape_next(&mut self) -> u32 {
        // SAFETY: as `tape_push`; `update` runs only with a tape.
        let x = unsafe { (&*self.tape)[self.ti] };
        self.ti += 1;
        x
    }
    /// A mounter for a sub-part (a dynamic item) of this one: same position state, its own scope, records
    /// buffer and tape.
    pub(crate) fn sub<'b>(&'b mut self, recs: &'b mut Vec<RecordInit>, scope: &'b mut Scope, tape: *mut Vec<u32>) -> Mounter<'b, V> {
        Mounter { rt: &mut *self.rt, owner: self.owner, recs, scope, depth: self.depth, ox: self.ox, oy: self.oy, space: self.space, clip: self.clip, pending_num: None, cls: self.cls, tape, ti: 0, pos: self.pos, kid: self.kid, keep: false, own: self.own }
    }
    pub fn mount(&mut self, e: impl Element<V>) {
        e.mount(self)
    }
    /// Install a wire for `w` (an `offset_of!`-proven field of this mounter's owner type).
    pub(crate) fn wire<T>(&mut self, w: &Wired<'_, T, V>, patch: PatchFn, node: u32, recipe: [u8; 16]) {
        // The runtime inference the proof replaced survives as a debug check: the operand is the owner's
        // own field (not a field of another value of the same type).
        debug_assert_eq!(w.sig as *const Signal<T> as usize, self.owner as usize + w.off as usize, "sig! operand is not a field of the mounted owner");
        // SAFETY: `w.off` is `offset_of!`-proven for owner type `V` (`Wired<_, V>` comes only from `sig!`),
        // and every caller passes a patch monomorphised for the signal's `T` with the recipe it reads.
        let i = unsafe { self.rt.add_wire(w.sig.head(), w.off, self.cls, patch, node, recipe) };
        self.scope.wires.push(i);
    }
    /// Install a wire on any headed cell of this owner (lists, readers): `head` at `owner + off`.
    ///
    /// # Safety
    /// `off` is the `offset_of!`-proven offset of the cell whose head is `head` in this mounter's owner type,
    /// and `patch` reads exactly that cell type.
    pub(crate) unsafe fn wire_cell(&mut self, head: &std::cell::Cell<u32>, off: u32, patch: PatchFn, node: u32, recipe: [u8; 16]) -> u32 {
        debug_assert_eq!(head as *const _ as usize, self.owner as usize + off as usize, "operand is not a field of the mounted owner");
        // SAFETY: this fn's contract: `off` is the proven offset of the cell whose head is `head` in this owner type, and
        // `patch` reads that cell type (`add_wire` asks for the same).
        let i = unsafe { self.rt.add_wire(head, off, self.cls, patch, node, recipe) };
        self.scope.wires.push(i);
        i
    }
}

pub trait Element<V>: Sized {
    fn mount(self, m: &mut Mounter<'_, V>);
    /// Re-description: walk the owner's retained records in lockstep with this (new) description,
    /// patching what differs (`describe.rs`). Wired operands keep their wires.
    fn update(self, m: &mut Mounter<'_, V>);
    /// Identity of this element inside a dynamic part (0: positional). An item whose key changed between
    /// two descriptions is replaced (unmounted and mounted), not updated (`describe::update_dyn`).
    fn dyn_key(&self) -> u64 {
        0
    }
    /// Write this description's operands to `s` (`seal.rs`), or refuse (the default): an element whose
    /// retained state its description does not determine must not be stepped over.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.refuse()
    }
}

impl<V> Element<V> for () {
    fn mount(self, _: &mut Mounter<'_, V>) {}
    fn update(self, _: &mut Mounter<'_, V>) {}
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, _: &mut S) {}
}

macro_rules! tuple_elements {
    ($($t:ident),+) => {
        impl<V, $($t: Element<V>),+> Element<V> for ($($t,)+) {
            #[allow(non_snake_case)]
            fn mount(self, m: &mut Mounter<'_, V>) {
                let ($($t,)+) = self;
                $($t.mount(m);)+
            }
            #[allow(non_snake_case)]
            fn update(self, m: &mut Mounter<'_, V>) {
                let ($($t,)+) = self;
                $($t.update(m);)+
            }
            #[allow(non_snake_case)]
            #[inline(always)]
            fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
                let ($($t,)+) = self;
                $($t.seal(s);)+
            }
        }
    };
}
tuple_elements!(A);
tuple_elements!(A, B);
tuple_elements!(A, B, C);
tuple_elements!(A, B, C, D);
tuple_elements!(A, B, C, D, E);
tuple_elements!(A, B, C, D, E, F);
tuple_elements!(A, B, C, D, E, F, G);
tuple_elements!(A, B, C, D, E, F, G, H);

impl<V, E: Element<V>> Element<V> for Vec<E> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_dyn(m, self.into_iter());
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_dyn(m, self.into_iter());
    }
}

/// Children from an iterator (`each((0..30).map(|i| card(i)))`).
pub struct Each<I>(pub I);
pub fn each<I: Iterator>(i: I) -> Each<I> {
    Each(i)
}
impl<V, I: Iterator> Element<V> for Each<I>
where
    I::Item: Element<V>,
{
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_dyn(m, self.0);
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_dyn(m, self.0);
    }
}

// =============================================================================== operands

/// How a property's new value is applied to its node (monomorphic, one per property).
pub trait Apply {
    type P: 'static;
    /// The property is a plain colour-word store into the node's instance: a binding may be a direct wire
    /// (`direct.rs`), applied by the signal's write.
    const DIRECT: bool = false;
    fn apply(rt: &mut Rt, node: u32, v: &Self::P);
}

/// An operand of property type `P` for owner `V`: a constant, or a wired signal (`sig!`) / projection.
pub trait IntoProp<P: 'static, V>: Sized {
    /// The initial value; installs a wire when the operand is dynamic.
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P;
    /// The current value, at a re-description (a wired operand keeps its wire; applying its current value
    /// is a no-op through the property's equality cut-off).
    fn value(self) -> P;
    /// The operand's part of its element's seal (`seal.rs`); refuses by default.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.refuse()
    }
}

impl<V> IntoProp<f32, V> for f32 {
    #[inline]
    fn lower<K: Apply<P = f32>>(self, _: &mut Mounter<'_, V>, _: u32) -> f32 {
        self
    }
    #[inline]
    fn value(self) -> f32 {
        self
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.f32(*self)
    }
}
impl<V> IntoProp<(f32, f32), V> for (f32, f32) {
    #[inline]
    fn lower<K: Apply<P = (f32, f32)>>(self, _: &mut Mounter<'_, V>, _: u32) -> (f32, f32) {
        self
    }
    #[inline]
    fn value(self) -> (f32, f32) {
        self
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.f32(self.0);
        s.f32(self.1);
    }
}
impl<V> IntoProp<Color, V> for Color {
    #[inline]
    fn lower<K: Apply<P = Color>>(self, _: &mut Mounter<'_, V>, _: u32) -> Color {
        self
    }
    #[inline]
    fn value(self) -> Color {
        self
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.paint(self.0)
    }
}

/// A plain borrow is a constant of this description (re-read only when the owner re-describes).
impl<T: Clone + 'static, V> IntoProp<T, V> for &Signal<T> {
    fn lower<K: Apply<P = T>>(self, _: &mut Mounter<'_, V>, _: u32) -> T {
        (**self).clone()
    }
    fn value(self) -> T {
        (**self).clone()
    }
}

/// # Safety
/// The wire contract (`PatchFn`): `sig` is a `Signal<T>`.
unsafe fn patch_sig<K: Apply<P = T>, T: 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { (*(sig as *const Signal<T>)).peek() };
    K::apply(rt, w.node, s);
}

/// # Safety
/// As `patch_sig`; the recipe holds an `F` written by `lower`.
unsafe fn patch_map<K: Apply, T: 'static, F: Fn(&T) -> K::P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract; F is Copy and was written unaligned into the 16-byte recipe.
    let s = unsafe { (*(sig as *const Signal<T>)).peek() };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let v = f(s);
    K::apply(rt, w.node, &v);
}

fn recipe_of<F: Copy>(f: F) -> [u8; 16] {
    const { assert!(size_of::<F>() <= 16, "map closure: at most 16 bytes of captures") };
    let mut r = [0u8; 16];
    // SAFETY: size_of::<F>() <= 16 (checked at compile time); unaligned write into a byte array.
    unsafe { std::ptr::write_unaligned(r.as_mut_ptr() as *mut F, f) };
    r
}


/// A wired operand's part of a seal (`seal.rs`): the wire's identity, i.e. the offset of the signal it reads in
/// the owner and the bytes its patch function captured. The signal's *value* is not part of a description:
/// it reaches the record through the wire. Equal identity means the installed wire is the one this
/// description would install, so the element may be stepped over.
#[inline(always)]
fn seal_wire<S: crate::seal::Sealer>(s: &mut S, tag: u32, off: u32, recipe: [u8; 16]) {
    s.word(tag);
    s.word(off);
    for w in recipe.chunks_exact(4) {
        s.word(u32::from_ne_bytes([w[0], w[1], w[2], w[3]]));
    }
}

impl<'a, T: Clone + 'static, V> IntoProp<T, V> for Wired<'a, T, V> {
    fn lower<K: Apply<P = T>>(self, m: &mut Mounter<'_, V>, node: u32) -> T {
        m.wire(&self, patch_sig::<K, T>, node, [0; 16]);
        if K::DIRECT {
            // a plain colour store: the write itself may apply it (`direct.rs`)
            let w = *m.scope.wires.last().expect("the wire just installed");
            let rt = &mut *m.rt;
            rt.input.direct.bind(&rt.wires, w, self.sig.head(), node);
        }
        self.get().clone()
    }
    fn value(self) -> T {
        self.get().clone()
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_wire(s, 0x5749_5245, self.off, [0; 16])
    }
}

impl<'a, T: 'static, V, F, P: 'static> IntoProp<P, V> for WMap<'a, T, V, F>
where
    F: Fn(&T) -> P + Copy + 'static,
{
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        let init = (self.f)(self.w.get());
        m.wire(&self.w, patch_map::<K, T, F>, node, recipe_of(self.f));
        init
    }
    fn value(self) -> P {
        (self.f)(self.w.get())
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_wire(s, 0x574d_4150, self.w.off, recipe_of(self.f))
    }
}

// =============================================================================== properties

pub struct ApplyBg;
/// `ApplyBg` where nothing else writes the colour word (no opacity or paint operand on the element): its
/// bindings may be direct wires (`direct.rs`).
pub struct ApplyBgDirect;
impl Apply for ApplyBgDirect {
    type P = Color;
    const DIRECT: bool = true;
    #[inline]
    fn apply(rt: &mut Rt, node: u32, v: &Color) {
        ApplyBg::apply(rt, node, v)
    }
}
pub struct ApplyW;
pub struct ApplySpan;

impl Apply for ApplyBg {
    type P = Color;
    #[inline]
    fn apply(rt: &mut Rt, node: u32, v: &Color) {
        // The instance is the state: the element's eight hot bytes and its instance (at its own id) are
        // loaded side by side, the new colour is compared with the one the GPU reads, and stored there.
        let hot = rt.hot[node as usize];
        if hot.flags & HF_MOD != 0 {
            return apply_bg_mod(rt, node, v.0);
        }
        let p = &mut rt.inst[(rt.ibase + node) as usize];
        let field = if hot.kind == NK_BORDER { &mut p.color2 } else { &mut p.color };
        let old = *field;
        if old == v.0 {
            return;
        }
        *field = v.0;
        // the colour word is the record's whole paint state: logged as is (content addressing)
        rt.paint_write(rt.handle(node), old as u64, v.0 as u64);
    }
}

/// `ApplyBg` for a quad whose drawn colour is not its described one (`HF_MOD`: under an opacity operand, or
/// under variants): the described colour is the cold record's; the instance takes it through the opacity, or
/// under variants (`NK_HOVERED`, `flags.rs`) it is the table's base and the instance shows what the flags
/// select.
#[cold]
fn apply_bg_mod(rt: &mut Rt, node: u32, c: u32) {
    let hot = rt.hot[node as usize];
    let n = rt.cold_mut(node);
    if n.color == c {
        return;
    }
    let (old, a) = (std::mem::replace(&mut n.color, c), n.alpha);
    if hot.kind & NK_HOVERED != 0 {
        // under variants (`flags.rs`): the colour is the base; the table shows what the flags select
        let act = rt.input.act_of_handle(rt.handle(node));
        return rt.flags_rebase(act, Some(c), None);
    }
    let p = &mut rt.inst[(rt.ibase + node) as usize];
    let shown = crate::boxops::mul_alpha(c, a);
    if hot.kind & !NK_HOVERED == NK_BORDER {
        p.color2 = shown;
    } else {
        p.color = shown;
    }
    // the colour word with the opacity operand's alpha (`boxops::paint_state`) is the paint state
    rt.paint_write(rt.handle(node), crate::boxops::paint_state(old, a), crate::boxops::paint_state(c, a));
}

/// The laid-out (untranslated) box of quad `node`: its instance's, or under a paint offset the cold
/// record's position.
#[inline]
pub(crate) fn quad_box(rt: &Rt, node: u32) -> (f32, f32, f32, f32) {
    let p = &rt.inst[(rt.ibase + node) as usize];
    if rt.hot[node as usize].flags & HF_MOD != 0 {
        let c = rt.cold_of(node);
        (c.x, c.y, p.size[0], p.size[1])
    } else {
        (p.pos[0], p.pos[1], p.size[0], p.size[1])
    }
}

impl Apply for ApplyW {
    type P = f32;
    #[inline]
    fn apply(rt: &mut Rt, node: u32, v: &f32) {
        let (x, y, _, h) = quad_box(rt, node);
        rt.quad_geom(node, x, y, *v * rt.scale, h);
    }
}

impl Apply for ApplySpan {
    /// (y, height) in points relative to the element's origin.
    type P = (f32, f32);
    #[inline]
    fn apply(rt: &mut Rt, node: u32, v: &(f32, f32)) {
        let (x, _, w, _) = quad_box(rt, node);
        let (y, h) = (rt.aux[node as usize] + v.0 * rt.scale, v.1 * rt.scale);
        rt.quad_geom(node, x, y, w, h);
    }
}

// =============================================================================== quad

pub struct Quad<Bg = Color, Wd = f32, Sp = (f32, f32), Ch = ()> {
    x: f32,
    wd: Wd,
    sp: Sp,
    bg: Bg,
    radius: f32,
    border: Option<(f32, Color)>,
    ch: Ch,
}

/// An absolutely positioned rectangle (points, relative to the parent's content origin).
pub fn quad(x: f32, y: f32, w: f32, h: f32) -> Quad {
    Quad { x, wd: w, sp: (y, h), bg: Color(0), radius: 0.0, border: None, ch: () }
}

impl<Bg, Wd, Sp, Ch> Quad<Bg, Wd, Sp, Ch> {
    pub fn bg<B2>(self, bg: B2) -> Quad<B2, Wd, Sp, Ch> {
        Quad { x: self.x, wd: self.wd, sp: self.sp, bg, radius: self.radius, border: self.border, ch: self.ch }
    }
    /// Width operand (points).
    pub fn width<W2>(self, wd: W2) -> Quad<Bg, W2, Sp, Ch> {
        Quad { x: self.x, wd, sp: self.sp, bg: self.bg, radius: self.radius, border: self.border, ch: self.ch }
    }
    /// (y, height) operand (points, relative to the parent's content origin).
    pub fn span<S2>(self, sp: S2) -> Quad<Bg, Wd, S2, Ch> {
        Quad { x: self.x, wd: self.wd, sp, bg: self.bg, radius: self.radius, border: self.border, ch: self.ch }
    }
    pub fn rounded(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
    /// Border of `w` points inside the box; children are positioned inside it.
    pub fn border(mut self, w: f32, c: Color) -> Self {
        self.border = Some((w, c));
        self
    }
    pub fn children<C2>(self, ch: C2) -> Quad<Bg, Wd, Sp, C2> {
        Quad { x: self.x, wd: self.wd, sp: self.sp, bg: self.bg, radius: self.radius, border: self.border, ch }
    }
}

/// The 48-byte instance of a quad (kind from the radius/border).
pub(crate) fn quad_instance(bg: Color, radius_px: f32, border: Option<(f32, Color)>, scale: f32, space: u16) -> (Instance, u8, f32) {
    let kind = match (border.is_some(), radius_px > 0.0) {
        (true, _) => KIND_BORDER,
        (false, true) => KIND_RRECT,
        _ => KIND_SOLID,
    };
    let margin = if kind == KIND_SOLID { 0.0 } else { 1.0 };
    let mut ins = Instance { color: bg.0, radius: radius_px, space, kind, ..Default::default() };
    if let Some((bw, bc)) = border {
        ins.color = bc.0;
        ins.color2 = bg.0;
        ins.bw = snap(bw * scale);
    }
    (ins, kind, margin)
}

impl<V: 'static, Bg, Wd, Sp, Ch> Element<V> for Quad<Bg, Wd, Sp, Ch>
where
    Bg: IntoProp<Color, V>,
    Wd: IntoProp<f32, V>,
    Sp: IntoProp<(f32, f32), V>,
    Ch: Element<V>,
{
    fn mount(self, m: &mut Mounter<'_, V>) {
        let node = m.rt.alloc_node();
        let wires_before = m.scope.wires.len();
        let bg = self.bg.lower::<ApplyBgDirect>(m, node);
        let w = self.wd.lower::<ApplyW>(m, node);
        let (y, h) = self.sp.lower::<ApplySpan>(m, node);
        let s = m.rt.scale;
        // on the geometry grid from the start (`snap`): every later edge comparison is exact
        let (x_px, y0_px) = (snap((m.ox + self.x) * s), m.oy * s);
        let (y_px, h_px, w_px) = (snap(y0_px + y * s), snap(h * s), snap(w * s));
        let (mut ins, _kind, margin) = quad_instance(bg, snap(self.radius * s), self.border, s, m.space);
        ins.pos = [x_px, y_px];
        ins.size = [w_px, h_px];
        let ink = m.rt.ink_rect(m.space, x_px, y_px, w_px, h_px, margin);
        // a re-describing owner keeps every quad addressable (any constant may change)
        let dynamic = m.scope.wires.len() > wires_before || m.taping() || std::mem::take(&mut m.keep);
        m.tape_push(node);
        let mut handle_idx = 0;
        let inst = if dynamic {
            // an element: its instance is at its own id, its state is the instance and eight hot bytes
            handle_idx = node;
            m.rt.reflect.elem(node, std::any::type_name::<Self>());
            m.rt.hot[node as usize] = Hot { kind: if self.border.is_some() { NK_BORDER } else { NK_QUAD }, flags: 0, clip: m.clip, link: 0 };
            m.rt.aux[node as usize] = y0_px;
            m.rt.inst_of(node)
        } else {
            // static: no element state, an instance block the scope releases
            m.rt.drop_node(node);
            let inst = m.rt.alloc_inst();
            m.scope.insts.push(inst);
            inst
        };
        *m.rt.inst_mut(inst) = ins;
        m.recs.push(RecordInit { depth: m.depth, ink: ink.to_nrect(), kind: REC_QUAD, payload: inst, handle: handle_idx, clip: m.clip });
        // children: inside the border
        let bw = self.border.map_or(0.0, |b| b.0);
        let (ox, oy) = (m.ox, m.oy);
        m.ox += self.x + bw;
        m.oy += y + bw;
        m.depth += 1;
        self.ch.mount(m);
        m.depth -= 1;
        m.ox = ox;
        m.oy = oy;
    }

    fn update(self, m: &mut Mounter<'_, V>) {
        let node = m.tape_next();
        let bg = self.bg.value();
        let w = self.wd.value();
        let (y, h) = self.sp.value();
        let s = m.rt.scale;
        let (x_px, y0_px) = ((m.ox + self.x) * s, m.oy * s);
        let (y_px, h_px, w_px) = (y0_px + y * s, h * s, w * s);
        let (mut ins, _kind, margin) = quad_instance(bg, self.radius * s, self.border, s, m.space);
        ins.pos = [x_px, y_px];
        ins.size = [w_px, h_px];
        let nk = if self.border.is_some() { NK_BORDER } else { NK_QUAD };
        let inst = m.rt.inst_of(node);
        if m.rt.hot[node as usize].clip != m.clip {
            m.rt.bough.set_clip_run(m.pos, 1, m.clip);
            m.rt.hot[node as usize].clip = m.clip;
        }
        let old = *m.rt.inst_mut(inst);
        let geo = old.pos != ins.pos || old.size != ins.size || old.radius != ins.radius || old.kind != ins.kind || old.bw != ins.bw || old.space != ins.space;
        // the border colour is geometry-class damage (one record, one paint state: its fill, see `paint_write`);
        // under variants the description's border is the table's base, not what a variant shows
        let border_c = self.border.is_some() && m.rt.flags_base_border(node).unwrap_or(old.color) != ins.color;
        let _ = margin;
        m.rt.aux[node as usize] = y0_px;
        if geo || border_c || m.rt.hot[node as usize].kind & !NK_HOVERED != nk {
            // a re-described quad is rewritten whole (a paint modifier's cold record follows it)
            *m.rt.inst_mut(inst) = ins;
            let h = &mut m.rt.hot[node as usize];
            h.kind = nk | (h.kind & NK_HOVERED);
            if h.flags & HF_MOD != 0 {
                let c = m.rt.cold_mut(node);
                (c.x, c.y, c.color) = (x_px, y_px, bg.0);
            }
            m.rt.quad_ink(node);
            m.rt.flags_reshow_node(node);
        } else {
            ApplyBg::apply(m.rt, node, &bg);
        }
        m.pos += 1;
        let bw = self.border.map_or(0.0, |b| b.0);
        let (ox, oy) = (m.ox, m.oy);
        m.ox += self.x + bw;
        m.oy += y + bw;
        m.depth += 1;
        self.ch.update(m);
        m.depth -= 1;
        m.ox = ox;
        m.oy = oy;
    }

    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x10 | (self.border.is_some() as u32) << 8);
        s.f32(self.x);
        self.wd.seal(s);
        self.sp.seal(s);
        self.bg.seal(s);
        s.f32(self.radius);
        if let Some((w, c)) = self.border {
            s.f32(w);
            s.word(c.0);
        }
        self.ch.seal(s);
    }
}

// =============================================================================== text

/// A text content operand for owner `V`: a constant (`&'static str`, `String`, a plain `&Signal<String>`
/// borrow), a wired `Signal<String>`, a projection to `String`, or a numeric operand (`.num(f, fmt)`: the
/// value is rendered into the label without a `String`).
pub trait TextSrc<V>: Sized {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String;
    /// The current text, at a re-description.
    fn value(self) -> String;
    /// `value` lent to `f` without building a `String` where the source already is one.
    #[inline]
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(&self.value())
    }
    /// The text's part of its element's seal (`seal.rs`); refuses by default.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.refuse()
    }
}

/// Any borrowed string: the text is copied at mount, so the borrow need not outlive the description.
impl<V> TextSrc<V> for &str {
    fn lower(self, _: &mut Mounter<'_, V>, _: u32) -> String {
        self.to_owned()
    }
    fn value(self) -> String {
        self.to_owned()
    }
    #[inline]
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(self)
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.bytes(self.as_bytes())
    }
}
impl<V> TextSrc<V> for String {
    fn lower(self, _: &mut Mounter<'_, V>, _: u32) -> String {
        self
    }
    fn value(self) -> String {
        self
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.bytes(self.as_bytes())
    }
}
/// An integer as its decimal text, formatted on the stack: `text(row.id)` allocates nothing at a
/// re-description (a `format!("{id}")` costs an allocation and the formatting machinery per row per turn).
macro_rules! int_text {
    ($($t:ty),*) => {$(
        impl<V> TextSrc<V> for $t {
            fn lower(self, _: &mut Mounter<'_, V>, _: u32) -> String {
                decimal(self as i128, &mut [0u8; 40]).to_owned()
            }
            fn value(self) -> String {
                decimal(self as i128, &mut [0u8; 40]).to_owned()
            }
            #[inline]
            fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
                let mut buf = [0u8; 40];
                f(decimal(self as i128, &mut buf))
            }
            #[inline]
            fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
                // (IDEAS-OPUS.md, finding A1d) the text is a function of the integer, so the integer is the
                // operand: two words, no decimal formatting to compare.
                s.int(*self as i128 as u64);
            }
        }
    )*};
}
int_text!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// `v` in decimal, written at the end of `buf`.
#[inline]
fn decimal(v: i128, buf: &mut [u8; 40]) -> &str {
    let (neg, mut u) = (v < 0, v.unsigned_abs() as u64);
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    // SAFETY: only ASCII digits and '-' were written to `buf[i..]`.
    unsafe { std::str::from_utf8_unchecked(&buf[i..]) }
}

impl<V> TextSrc<V> for &Signal<String> {
    fn lower(self, _: &mut Mounter<'_, V>, _: u32) -> String {
        (**self).clone()
    }
    fn value(self) -> String {
        (**self).clone()
    }
    #[inline]
    fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        f(self)
    }
}

/// The text node of `node` was given a new value.
#[inline]
pub(crate) fn set_node_text(rt: &mut Rt, node: u32, s: &str) {
    let n = rt.hot[node as usize];
    if n.kind == NK_LTEXT {
        rt.lay_text_changed(node, s);
    } else {
        rt.text_set(n.link, s);
    }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<String>`.
unsafe fn patch_text(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { (*(sig as *const Signal<String>)).peek() };
    set_node_text(rt, w.node, s);
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; the recipe holds an `F`.
unsafe fn patch_text_map<T: 'static, F: Fn(&T) -> String + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { (*(sig as *const Signal<T>)).peek() };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let v = f(s);
    set_node_text(rt, w.node, &v);
}

/// Render `v` with `fmt` (16-byte vector, no `String`); `format!` only past 16 bytes or for non-finite values.
fn render_num(fmt: &bough_text::numeric::NumFmt, v: f32, buf: &mut [u8; 16], slow: &mut String) -> usize {
    *buf = [0; 16];
    let r = if fmt.decimals.is_some() { fmt.render_f32(v, buf) } else { fmt.render_u32(v as u32, buf) };
    match r {
        Some(n) => n,
        None => {
            slow.clear();
            use std::fmt::Write;
            let (p, s) = fmt.affixes();
            match fmt.decimals {
                Some(d) => write!(slow, "{p}{v:.*}{s}", d as usize).unwrap(),
                None => write!(slow, "{p}{}{s}", v as u32).unwrap(),
            }
            usize::MAX
        }
    }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; the recipe holds an `F`; the node's text has a `NumFmt`.
unsafe fn patch_text_num<T: 'static, F: Fn(&T) -> f32 + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { (*(sig as *const Signal<T>)).peek() };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let v = f(s);
    let ti = rt.text_of(w.node);
    let fmt = rt.texts[ti as usize].num.expect("numeric text node");
    let mut buf = [0u8; 16];
    let mut slow = String::new();
    let n = render_num(&fmt, v, &mut buf, &mut slow);
    if n == usize::MAX {
        set_node_text(rt, w.node, &slow);
    } else {
        set_node_text(rt, w.node, bough_text::numeric::as_str(&buf, n));
    }
}

impl<'a, V> TextSrc<V> for Wired<'a, String, V> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        m.wire(&self, patch_text, node, [0; 16]);
        self.get().clone()
    }
    fn value(self) -> String {
        self.get().clone()
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_wire(s, 0x5754_5854, self.off, [0; 16])
    }
}
impl<'a, T: 'static, V, F: Fn(&T) -> String + Copy + 'static> TextSrc<V> for WMap<'a, T, V, F> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        let init = (self.f)(self.w.get());
        m.wire(&self.w, patch_text_map::<T, F>, node, recipe_of(self.f));
        init
    }
    fn value(self) -> String {
        (self.f)(self.w.get())
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_wire(s, 0x5754_4d50, self.w.off, recipe_of(self.f))
    }
}
impl<'a, T: 'static, V, F: Fn(&T) -> f32 + Copy + 'static> TextSrc<V> for Num<'a, T, V, F> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        let v = (self.f)(self.w.get());
        m.wire(&self.w, patch_text_num::<T, F>, node, recipe_of(self.f));
        m.pending_num = Some(self.fmt);
        let mut buf = [0u8; 16];
        let mut slow = String::new();
        let n = render_num(&self.fmt, v, &mut buf, &mut slow);
        if n == usize::MAX { slow } else { bough_text::numeric::as_str(&buf, n).to_owned() }
    }
    fn value(self) -> String {
        let v = (self.f)(self.w.get());
        let mut buf = [0u8; 16];
        let mut slow = String::new();
        let n = render_num(&self.fmt, v, &mut buf, &mut slow);
        if n == usize::MAX { slow } else { bough_text::numeric::as_str(&buf, n).to_owned() }
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        seal_wire(s, 0x574e_554d, self.w.off, recipe_of(self.f));
        s.bytes(&self.fmt.identity());
    }
}

pub struct Text<S, G = ()> {
    s: S,
    x: f32,
    y: f32,
    size: f32,
    lh: f32,
    color: Color,
    g: G,
}

/// Text from a constant, a wired `sig!` operand, a projection or a numeric operand. Single line, or explicit
/// multi-line (`\n`); positioned at (x, y) with `size`/`line_height` in points.
pub fn text<S>(s: S) -> Text<S> {
    Text { s, x: 0.0, y: 0.0, size: 12.0, lh: 16.0, color: Color::rgba(255, 255, 255, 255), g: () }
}

impl<S, G> Text<S, G> {
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.x = x;
        self.y = y;
        self
    }
    pub fn size(mut self, pt: f32) -> Self {
        self.size = pt;
        self
    }
    pub fn line_height(mut self, pt: f32) -> Self {
        self.lh = pt;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
    /// Per-glyph alpha from a wired signal: one wire writes the colour word of every glyph instance of the
    /// text (`f(char index, char count, &value) -> alpha`; spaces own no instance). No reshaping, no geometry
    /// change: it is the `bars` mechanism for colour. For constant ASCII text.
    pub fn glyph_alpha<'a, T, V>(self, sig: Wired<'a, T, V>, f: fn(usize, usize, &T) -> u8) -> Text<S, GlyphAlpha<'a, T, V>> {
        Text { s: self.s, x: self.x, y: self.y, size: self.size, lh: self.lh, color: self.color, g: GlyphAlpha { sig, f } }
    }
}

/// A run-level operand of a text node (see `Text::glyph_alpha`); `()` is none.
pub trait GlyphOp<V>: Sized {
    /// Does it need one instance per glyph?
    const PER_GLYPH: bool;
    fn install(self, m: &mut Mounter<'_, V>, node: u32);
    /// The text was rebuilt by a re-description: write the per-glyph state again.
    fn refresh(self, m: &mut Mounter<'_, V>, node: u32);
    /// Its part of the text's seal (`seal.rs`): the wire's field and function.
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S);
}
impl<V> GlyphOp<V> for () {
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, _: &mut S) {}
    const PER_GLYPH: bool = false;
    #[inline]
    fn install(self, _: &mut Mounter<'_, V>, _: u32) {}
    #[inline]
    fn refresh(self, _: &mut Mounter<'_, V>, _: u32) {}
}
pub struct GlyphAlpha<'a, T, V> {
    sig: Wired<'a, T, V>,
    f: fn(usize, usize, &T) -> u8,
}

/// Write the glyph alphas; returns a hash of the new alpha vector (the record's paint state).
fn set_glyph_alpha<T>(rt: &mut Rt, node: u32, f: fn(usize, usize, &T) -> u8, v: &T) -> u64 {
    let ti = rt.text_of(node);
    let t = &rt.texts[ti as usize];
    let crate::text::TShape::Glyphs { run } = t.shape else { unreachable!("per-glyph text is glyph instances") };
    let (first, rgb) = (rt.runs[run as usize].first, t.color & 0x00ff_ffff);
    let count = rt.texts[ti as usize].text.len();
    let mut j = 0u32;
    let mut hsh = 0xcbf2_9ce4_8422_2325u64; // FNV-1a over the alphas
    for i in 0..count {
        if rt.texts[ti as usize].text.as_bytes()[i] == b' ' {
            continue;
        }
        let a = f(i, count, v) as u32;
        rt.inst[(first + j) as usize].color = rgb | a << 24;
        hsh = (hsh ^ a as u64).wrapping_mul(0x0000_0100_0000_01b3);
        j += 1;
    }
    hsh
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; the recipe holds the `fn`.
unsafe fn patch_galpha<T: 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { (*(sig as *const Signal<T>)).peek() };
    let f: fn(usize, usize, &T) -> u8 = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const _) };
    let new = set_glyph_alpha(rt, w.node, f, s);
    let t = &mut rt.texts[rt.hot[w.node as usize].link as usize];
    let (h, old) = (t.handle, std::mem::replace(&mut t.paint_key, new));
    if old != new {
        rt.paint_write(h, old, new);
    }
}

impl<'a, V: 'static, T: 'static> GlyphOp<V> for GlyphAlpha<'a, T, V> {
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x0220);
        s.word(self.sig.off);
        crate::seal::closure(s, &self.f);
    }
    const PER_GLYPH: bool = true;
    fn install(self, m: &mut Mounter<'_, V>, node: u32) {
        let hsh = set_glyph_alpha(m.rt, node, self.f, self.sig.get());
        let ti = m.rt.text_of(node);
        m.rt.texts[ti as usize].paint_key = hsh;
        m.wire(&self.sig, patch_galpha::<T>, node, recipe_of(self.f));
    }
    fn refresh(self, m: &mut Mounter<'_, V>, node: u32) {
        let hsh = set_glyph_alpha(m.rt, node, self.f, self.sig.get());
        let ti = m.rt.text_of(node);
        m.rt.texts[ti as usize].paint_key = hsh;
    }
}

impl<V: 'static, S: TextSrc<V>, G: GlyphOp<V>> Element<V> for Text<S, G> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let node = m.rt.alloc_node();
        let wires_before = m.scope.wires.len();
        m.pending_num = None;
        let init = self.s.lower(m, node);
        let num = m.pending_num.take();
        let s = m.rt.scale;
        let size_px = self.size * s;
        let lh_px = self.lh * s;
        let strike = m.rt.text.strike(size_px);
        let base = ((m.oy + self.y) * s + m.rt.text.baseline(size_px, lh_px)).round();
        let dynamic = G::PER_GLYPH || m.scope.wires.len() > wires_before || m.taping();
        m.tape_push(node);
        // a static text keeps no node: its record carries no handle and its text state is the scope's
        let hd = if dynamic {
            m.rt.handle(node)
        } else {
            m.rt.drop_node(node);
            Handle { idx: 0, generation: 0 }
        };
        if dynamic {
            m.rt.reflect.elem(hd.idx, std::any::type_name::<Self>());
        }
        let ti = m.rt.alloc_text(TextNode {
            text: init,
            strike: strike as u32,
            size_px,
            line_h: lh_px,
            color: self.color.0,
            ox: (m.ox + self.x) * s,
            oy: base,
            space: m.space,
            handle: hd,
            ink: bough_core::nrect::WRect::EMPTY,
            shape: TShape::None,
            per_glyph: G::PER_GLYPH,
            num,
            base_off: 0.0,
            paint_key: 0,
        });
        let payload = m.rt.text_build(ti);
        m.scope.texts.push(ti);
        if dynamic {
            // a text element's state is its text state (colour, size, shape); its hot entry links to it
            m.rt.hot[node as usize] = Hot { kind: if G::PER_GLYPH { NK_GALPHA } else { NK_TEXT }, flags: 0, clip: m.clip, link: ti };
            self.g.install(m, node);
        }
        let ink = m.rt.texts[ti as usize].ink;
        m.recs.push(RecordInit { depth: m.depth, ink: ink.to_nrect(), kind: REC_TEXT, payload, handle: hd.idx, clip: m.clip });
    }

    fn update(self, m: &mut Mounter<'_, V>) {
        let node = m.tape_next();
        let text = self.s.value();
        let s = m.rt.scale;
        let size_px = self.size * s;
        let lh_px = self.lh * s;
        let (ti, hd) = (m.rt.text_of(node), m.rt.handle(node));
        if m.rt.hot[node as usize].clip != m.clip {
            m.rt.bough.set_clip_run(m.pos, 1, m.clip);
            m.rt.hot[node as usize].clip = m.clip;
        }
        let base = ((m.oy + self.y) * s + m.rt.text.baseline(size_px, lh_px)).round();
        let ox = (m.ox + self.x) * s;
        let same_place = {
            let t = &m.rt.texts[ti as usize];
            t.size_px == size_px && t.line_h == lh_px && t.color == self.color.0 && t.ox == ox && t.oy == base && t.space == m.space
        };
        if same_place {
            m.rt.text_set(ti, &text);
        } else {
            // a moved / resized / recoloured text: rebuilt (old and new ink damaged)
            let old_ink = m.rt.cur_ink(hd);
            let old = std::mem::take(&mut m.rt.texts[ti as usize].shape);
            crate::text::release(m.rt, TextNode { shape: old, ..Default::default() });
            let strike = m.rt.text.strike(size_px);
            {
                let t = &mut m.rt.texts[ti as usize];
                t.text = text;
                t.strike = strike as u32;
                t.size_px = size_px;
                t.line_h = lh_px;
                t.color = self.color.0;
                t.ox = ox;
                t.oy = base;
                t.space = m.space;
            }
            let payload = m.rt.text_build(ti);
            self.g.refresh(m, node);
            m.rt.bough.set_payload_handle(hd, payload);
            let ink = m.rt.texts[ti as usize].ink;
            m.rt.bough.damage_rect(old_ink);
            m.rt.bough.set_ink_handle_wide(hd, ink);
        }
        m.pos += 1;
    }

    #[inline(always)]
    fn seal<Z: crate::seal::Sealer>(&self, s: &mut Z) {
        s.word(0x11);
        s.f32(self.x);
        s.f32(self.y);
        s.f32(self.size);
        s.f32(self.lh);
        s.word(self.color.0);
        self.g.seal(s);
        self.s.seal(s);
    }
}

// =============================================================================== grouping

/// Children offset by (x, y) points (no record, no depth change).
pub struct At<Ch>(pub f32, pub f32, pub Ch);
pub fn at<Ch>(x: f32, y: f32, ch: Ch) -> At<Ch> {
    At(x, y, ch)
}
impl<V, Ch: Element<V>> Element<V> for At<Ch> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let (ox, oy) = (m.ox, m.oy);
        m.ox += self.0;
        m.oy += self.1;
        self.2.mount(m);
        m.ox = ox;
        m.oy = oy;
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        let (ox, oy) = (m.ox, m.oy);
        m.ox += self.0;
        m.oy += self.1;
        self.2.update(m);
        m.ox = ox;
        m.oy = oy;
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x12);
        s.f32(self.0);
        s.f32(self.1);
        self.2.seal(s);
    }
}

/// Children clipped to a rect (points, relative to the current origin): a bough-core clip id on their
/// records (visible ink = ink ∩ clip for damage and cull; the GPU clamps every quad and glyph to it).
pub struct Clip<Ch>(pub [f32; 4], pub Ch);
pub fn clip<Ch>(x: f32, y: f32, w: f32, h: f32, ch: Ch) -> Clip<Ch> {
    Clip([x, y, w, h], ch)
}
impl<Ch> Clip<Ch> {
    fn enter<V>(&self, m: &mut Mounter<'_, V>) -> u16 {
        let s = m.rt.scale;
        let o = m.rt.space(m.space);
        let [x, y, w, h] = self.0;
        let r = [o[0] + (m.ox + x) * s, o[1] + (m.oy + y) * s, o[0] + (m.ox + x + w) * s, o[1] + (m.oy + y + h) * s];
        let prev = m.clip;
        let r = if prev != 0 {
            let p = m.rt.bough.clip_rect(prev).to_xyxy();
            [r[0].max(p[0] as f32), r[1].max(p[1] as f32), r[2].min(p[2] as f32), r[3].min(p[3] as f32)]
        } else {
            r
        };
        m.clip = m.rt.clip_rect(r);
        prev
    }
}
impl<V, Ch: Element<V>> Element<V> for Clip<Ch> {
    fn update(self, m: &mut Mounter<'_, V>) {
        let prev = self.enter(m);
        self.1.update(m);
        m.clip = prev;
    }
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x13);
        for v in self.0 {
            s.f32(v);
        }
        self.1.seal(s);
    }
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let o = m.rt.space(m.space);
        let [x, y, w, h] = self.0;
        let r = [o[0] + (m.ox + x) * s, o[1] + (m.oy + y) * s, o[0] + (m.ox + x + w) * s, o[1] + (m.oy + y + h) * s];
        let prev = m.clip;
        // nested clips compose (intersection, in `scoped_clip`); inside a scroll view's content the clip moves
        // with the content and is re-cut against its parent when either moves (template.rs)
        m.clip = m.rt.scoped_clip(m.scope, r, prev);
        self.1.mount(m);
        m.clip = prev;
    }
}

// =============================================================================== virtual list

/// A keyed virtual-list region: rows `visible(scroll)` are mounted (each an owner of its own wires), rows
/// that leave the range are unmounted, and rows are positioned through per-row spaces — a scroll writes the
/// space table and moves bough-core's page offsets, it never rewrites rows.
pub struct VirtualList<'a, V> {
    scroll: Wired<'a, f32, V>,
    row_h: f32,
    vp: [f32; 4],
    visible: fn(f32) -> Range<usize>,
    row: fn(&V, usize, &mut Mounter<'_, V>),
    _v: PhantomData<fn(&V)>,
}

/// `vp` = (x, y, w, h) of the viewport in points (the rows' clip); rows are `row_h` points tall.
pub fn virtual_list<'a, V>(
    scroll: Wired<'a, f32, V>,
    row_h: f32,
    vp: [f32; 4],
    visible: fn(f32) -> Range<usize>,
    row: fn(&V, usize, &mut Mounter<'_, V>),
) -> VirtualList<'a, V> {
    VirtualList { scroll, row_h, vp, visible, row, _v: PhantomData }
}

/// # Safety
/// `owner` points at the live owner `V`; `reg`'s row function is a `fn(&V, usize, &mut Mounter<V>)`.
pub(crate) unsafe fn mount_row<V: 'static>(rt: &mut Rt, owner: *const u8, reg: usize, row: usize, recs: &mut Vec<RecordInit>) -> RowInfo {
    let space = rt.alloc_space();
    let origin = rt.region_row_space_origin(reg, row);
    rt.set_space(space, origin);
    let (clip, depth, f, cls, own) = {
        let r = &rt.regions[reg];
        (r.clip, r.depth, r.row_fn, r.cls, r.own)
    };
    // SAFETY: the region was created by `VirtualList::<V>::mount` from a `fn(&V, usize, &mut Mounter<V>)`.
    let f: fn(&V, usize, &mut Mounter<'_, V>) = unsafe { std::mem::transmute::<usize, fn(&V, usize, &mut Mounter<'_, V>)>(f) };
    let mut scope = Scope::default();
    let before = recs.len();
    {
        let mut m = Mounter { rt, owner: owner as *const V, recs, scope: &mut scope, depth, ox: 0.0, oy: 0.0, space, clip, pending_num: None, cls, tape: std::ptr::null_mut(), ti: 0, pos: 0, kid: 0, keep: false, own };
        // SAFETY: see the function contract.
        f(unsafe { &*(owner as *const V) }, row, &mut m);
    }
    RowInfo { nrec: (recs.len() - before) as u32, space, scope }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<f32>`; `owner` is the region's owner.
pub(crate) unsafe fn patch_scroll(rt: &mut Rt, w: &Wire, sig: *const u8, owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { (*(sig as *const Signal<f32>)).peek() };
    unsafe { rt.region_scroll(w.node as usize, owner, *s) };
}

impl<'a, V: 'static> Element<V> for VirtualList<'a, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let o = m.rt.space(m.space);
        let [x, y, w, h] = self.vp;
        let (vx, vy) = (o[0] + (m.ox + x) * s, o[1] + (m.oy + y) * s);
        let clip = m.rt.clip_rect([vx, vy, vx + w * s, vy + h * s]);
        let vp = m.rt.bough.clip_rect(clip);
        let reg = m.rt.regions.len();
        m.rt.regions.push(VRegion {
            row_h: self.row_h * s,
            vp_x: vx,
            vp_y: vy,
            clip,
            vp,
            parent_clip: m.clip,
            depth: m.depth,
            base_pos: m.recs.len(),
            range: 0..0,
            rows: Default::default(),
            scroll_px: 0.0,
            visible: self.visible,
            count: 0,
            port_h: 0.0,
            scale_pt: s,
            mount_row: mount_row::<V>,
            row_fn: self.row as usize,
            cls: m.cls,
            own: m.own,
        });
        m.wire(&self.scroll, patch_scroll, reg as u32, [0; 16]);
        m.tape_push(reg as u32);
        // SAFETY: the owner pointer is the mounted owner (`App::new` / a row mount).
        unsafe { m.rt.region_initial(reg, m.owner as *const u8, *self.scroll.get(), m.recs) };
    }
    /// The rows are the region's (scroll-driven); a re-description only re-anchors the run.
    fn update(self, m: &mut Mounter<'_, V>) {
        let reg = m.tape_next() as usize;
        m.rt.regions[reg].base_pos = m.pos;
        m.pos += m.rt.regions[reg].rows.iter().map(|r| r.nrec as usize).sum::<usize>();
    }
}

// =============================================================================== bars (series run)

/// A sliding-window bar series: one record owning a run of `count` solid quads (the GPU expands the run).
/// Bar `i` has height `max(max_h * sample(key + i), min_h)` points growing up from `base`. When `key`
/// advances by one the window shifts and only the new rightmost sample is computed (the memoised samples
/// are the value that was there all along); any other jump recomputes every sample. One wire, one ink write
/// (old and new union) per update however many bars there are.
pub struct Bars<'a, F, V> {
    key: Wired<'a, u64, V>,
    sample: F,
    x: f32,
    step: f32,
    width: f32,
    count: usize,
    base: f32,
    max_h: f32,
    min_h: f32,
    color: Color,
}

#[allow(clippy::too_many_arguments)]
pub fn bars<'a, F: Fn(u64) -> f32 + Copy + 'static, V>(
    x: f32,
    step: f32,
    width: f32,
    count: usize,
    base: f32,
    max_h: f32,
    min_h: f32,
    key: Wired<'a, u64, V>,
    sample: F,
) -> Bars<'a, F, V> {
    Bars { key, sample, x, step, width, count, base, max_h, min_h, color: Color::rgba(255, 255, 255, 255) }
}

impl<F, V> Bars<'_, F, V> {
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
}

/// Recompute the node's samples for `key` (shifting when the key advanced by one) and write the bar
/// instances; returns the union ink. With `damage`, each bar whose top moved marks only the strip between
/// its old and new top (`Bough::damage_edges`, a solid's band): bars are bottom-anchored on the geometry
/// grid, so the bottom edge `y + h` is the same exact value every tick and never damages.
fn write_bars<F: Fn(u64) -> f32>(rt: &mut Rt, node: u32, key: u64, f: F, damage: bool) -> bough_core::nrect::WRect {
    let (clip, link, hd) = (rt.hot[node as usize].clip, rt.hot[node as usize].link, rt.handle(node));
    let Rt { cold, spans, .. } = rt;
    let n = &mut cold[link as usize];
    let count = n.series_n as usize;
    let ser = spans.floats_mut(n.series, count);
    // The samples that leave or are overwritten: with the new series they bound every old and new bar top
    // (a shift by one keeps all old samples but the first). The value was there all along: the strip union
    // comes from the series, not from reading the instances back.
    let (mut smin, mut smax) = (f32::INFINITY, f32::NEG_INFINITY);
    if n.series_ok && key == n.series_key.wrapping_add(1) {
        (smin, smax) = (ser[0], ser[0]);
        ser.copy_within(1.., 0);
        ser[count - 1] = f(key + count as u64 - 1);
    } else if !n.series_ok || key != n.series_key {
        for (i, s) in ser.iter_mut().enumerate() {
            smin = if *s < smin { *s } else { smin };
            smax = if *s > smax { *s } else { smax };
            *s = f(key + i as u64);
        }
    } else {
        return rt.bough.pos_of(hd).map_or(bough_core::nrect::WRect::EMPTY, |p| rt.bough.ink_wide_at(p));
    }
    n.series_ok = true;
    n.series_key = key;
    // geometry: n.x = x of bar 0 (px), n.w = bar width px, n.h = step px, n.y = base pt (space-local),
    // n.margin = max_h pt, n.line_h = min_h pt, n.size_px = scale
    let (x0, wpx, step, base_pt, max_h, min_h, scale, y0) = (n.x, n.w, n.h, n.y, n.margin, n.line_h, n.size_px, n.y0);
    let first = n.inst as usize;
    let space = n.space;
    let (bottom, wb) = (snap(y0 + base_pt * scale), snap(wpx));
    let [sx, sy] = rt.space(space);
    let band = edge_band(KIND_SOLID, 0.0, 0.0) as f32;
    let (inst, series) = (&mut rt.inst, rt.spans.floats(rt.cold[link as usize].series, count));
    // Bars are bottom-anchored on the geometry grid: only their top edges move (x, width and bottom are
    // fixed, written at mount; `bottom - y` is exact on the grid). The edge-band rule (`Bough::damage_edges`)
    // gives each moved bar the strip between its old and new top plus a solid's band; their union is
    // covered by ONE rect: the sparkline's columns between the highest and the lowest top any old or new
    // sample gives (2 selects a bar on the samples, no instance read back). The bars' bodies below that,
    // which old ∪ new repaints every tick, are never damaged.
    let y_of = |s: f32| snap(y0 + (base_pt - (max_h * s).max(min_h)) * scale);
    let mut top = f32::MAX;
    for i in 0..count {
        let s = series[i];
        smin = if s < smin { s } else { smin };
        smax = if s > smax { s } else { smax };
        let y = y_of(s);
        let p = &mut inst[first + i];
        if !damage {
            p.pos[0] = snap(x0 + i as f32 * step);
            p.size[0] = wb;
        }
        p.pos[1] = y;
        p.size[1] = bottom - y;
        top = if y < top { y } else { top };
    }
    if damage {
        // y falls as the sample rises: the highest top comes from the largest sample
        let (lo, hi) = (y_of(smax), y_of(smin));
        let xa = snap(x0) + sx;
        let r = NRect::new(xa.floor() as i32, (lo + sy - band).floor() as i32, (xa + snap((count as f32 - 1.0) * step) + wb).ceil() as i32, (hi + sy + band).ceil() as i32);
        rt.bough.damage_clipped(r, clip);
    }
    let xa = snap(x0);
    rt.ink_rect(space, xa, top, snap(x0 + (count as f32 - 1.0) * step) + wb - xa, bottom - top, 0.0)
}

/// # Safety
/// Wire contract: `sig` is a `Signal<u64>`; the recipe holds an `F`.
unsafe fn patch_bars<F: Fn(u64) -> f32 + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let key = *unsafe { (*(sig as *const Signal<u64>)).peek() };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    // each bar damages the strip its top moved through (inside write_bars); the record's ink is the union
    let ink = write_bars(rt, w.node, key, f, true);
    let h = rt.handle(w.node);
    rt.bough.set_ink_handle_nodamage_wide(h, ink);
}

impl<'a, V: 'static, F: Fn(u64) -> f32 + Copy + 'static> Element<V> for Bars<'a, F, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let node = m.rt.alloc_node();
        let run = m.rt.alloc_run(self.count);
        m.rt.runs[run as usize].len = self.count as u32;
        let first = m.rt.runs[run as usize].first;
        for i in 0..self.count {
            *m.rt.inst_mut(first + i as u32) = Instance { color: self.color.0, space: m.space, kind: KIND_SOLID, ..Default::default() };
        }
        m.rt.hot[node as usize] = Hot { kind: NK_BARS, flags: 0, clip: m.clip, link: 0 };
        {
            let n = m.rt.alloc_cold(node);
            (n.run, n.inst) = (run, first);
            n.space = m.space;
            n.x = (m.ox + self.x) * s;
            n.w = self.width * s;
            n.h = self.step * s;
            n.y = self.base;
            n.y0 = m.oy * s;
            n.margin = self.max_h;
            n.line_h = self.min_h;
            n.size_px = s;
            n.series_ok = false;
        }
        let series = m.rt.spans.alloc(crate::store::class_of(self.count));
        m.rt.spans.floats_mut(series, self.count).fill(0.0);
        let n = m.rt.cold_mut(node);
        (n.series, n.series_n) = (series, self.count as u32);
        m.scope.series.push((series, self.count as u32));
        let ink = write_bars(m.rt, node, *self.key.get(), self.sample, false);
        m.wire(&self.key, patch_bars::<F>, node, recipe_of(self.sample));
        let hd = m.rt.handle(node);
        m.scope.runs.push(run);
        m.recs.push(RecordInit { depth: m.depth, ink: ink.to_nrect(), kind: REC_QUAD, payload: pay_irun(first, self.count as u32), handle: hd.idx, clip: m.clip });
    }
    /// The series is wire-driven; its geometry constants are taken as fixed for the owner's life.
    fn update(self, m: &mut Mounter<'_, V>) {
        m.pos += 1;
    }
    /// Its re-description does nothing but step over its record: nothing to compare.
    #[inline(always)]
    fn seal<S: crate::seal::Sealer>(&self, s: &mut S) {
        s.word(0x14);
    }
}
