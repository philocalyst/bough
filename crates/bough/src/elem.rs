//! Typed builders, operands (`IntoProp`) and their lowering into the runtime tables.
//!
//! `Element<V>` is implemented by every builder; `mount` lowers it: allocates records / instance
//! runs / nodes, and installs wires for every operand that is a signal or a `sig.map(f)` of a field
//! of the owner (the owner-relative-offset trick: `offset = addr(field) - addr(owner)`; a signal
//! outside the owner's storage is read once, as a constant of this description).
use crate::color::Color;
use crate::rt::*;
use crate::signal::{Map, Signal};
use bough_core::RecordInit;
use bough_text::atlas::Instance;
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
    pub(crate) clip: u16,
}

impl<V> Mounter<'_, V> {
    pub fn scale(&self) -> f32 {
        self.rt.scale
    }
    pub fn mount(&mut self, e: impl Element<V>) {
        e.mount(self)
    }
    /// Owner-relative offset of a signal, if it lives inside the owner.
    fn offset_of<T>(&self, s: &Signal<T>) -> Option<u32> {
        let a = s as *const Signal<T> as usize;
        let o = self.owner as usize;
        (a >= o && a + std::mem::size_of::<Signal<T>>() <= o + std::mem::size_of::<V>()).then(|| (a - o) as u32)
    }
    fn wire(&mut self, head: &std::cell::Cell<u32>, off: u32, patch: PatchFn, node: u32, recipe: [u8; 16]) {
        let w = self.rt.add_wire(head, off, patch, node, recipe);
        self.scope.wires.push(w);
    }
}

pub trait Element<V>: Sized {
    fn mount(self, m: &mut Mounter<'_, V>);
}

impl<V> Element<V> for () {
    fn mount(self, _: &mut Mounter<'_, V>) {}
}

macro_rules! tuple_elements {
    ($($t:ident),+) => {
        impl<V, $($t: Element<V>),+> Element<V> for ($($t,)+) {
            #[allow(non_snake_case)]
            fn mount(self, m: &mut Mounter<'_, V>) {
                let ($($t,)+) = self;
                $($t.mount(m);)+
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
        for e in self {
            e.mount(m);
        }
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
        for e in self.0 {
            e.mount(m);
        }
    }
}

// =============================================================================== operands

/// How a property's new value is applied to its node (monomorphic, one per property).
pub trait Apply {
    type P: 'static;
    /// # Safety
    /// `node` must be a live node of the kind this property belongs to.
    unsafe fn apply(rt: &mut Rt, node: u32, v: &Self::P);
}

pub trait IntoProp<P: 'static>: Sized {
    /// The initial value; installs a wire when the operand is dynamic.
    fn lower<K: Apply<P = P>, V: 'static>(self, m: &mut Mounter<'_, V>, node: u32) -> P;
}

macro_rules! const_prop {
    ($($t:ty),*) => {$(
        impl IntoProp<$t> for $t {
            #[inline]
            fn lower<K: Apply<P = $t>, V: 'static>(self, _: &mut Mounter<'_, V>, _: u32) -> $t { self }
        }
    )*};
}
const_prop!(f32, (f32, f32), Color, String);

impl IntoProp<String> for &'static str {
    fn lower<K: Apply<P = String>, V: 'static>(self, _: &mut Mounter<'_, V>, _: u32) -> String {
        self.to_owned()
    }
}

unsafe fn patch_sig<K: Apply, T: 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8)
where
    K: Apply<P = T>,
{
    let s = unsafe { &*(sig as *const Signal<T>) };
    unsafe { K::apply(rt, w.node, &**s) };
}

unsafe fn patch_map<K: Apply, T: 'static, F: Fn(&T) -> K::P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    let s = unsafe { &*(sig as *const Signal<T>) };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let v = f(&**s);
    unsafe { K::apply(rt, w.node, &v) };
}

impl<'a, T: Clone + 'static> IntoProp<T> for &'a Signal<T> {
    fn lower<K: Apply<P = T>, V: 'static>(self, m: &mut Mounter<'_, V>, node: u32) -> T {
        if let Some(off) = m.offset_of(self) {
            m.wire(self.head(), off, patch_sig::<K, T>, node, [0; 16]);
        }
        (**self).clone()
    }
}

impl<'a, T: 'static, F, P: 'static> IntoProp<P> for Map<'a, T, F>
where
    F: Fn(&T) -> P + Copy + 'static,
{
    fn lower<K: Apply<P = P>, V: 'static>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        const { assert!(std::mem::size_of::<F>() <= 16 && std::mem::align_of::<F>() <= 8, "map closure: at most 16 bytes of captures") };
        let init = (self.f)(&**self.sig);
        if let Some(off) = m.offset_of(self.sig) {
            let mut recipe = [0u8; 16];
            unsafe { std::ptr::write_unaligned(recipe.as_mut_ptr() as *mut F, self.f) };
            m.wire(self.sig.head(), off, patch_map::<K, T, F>, node, recipe);
        }
        init
    }
}

// =============================================================================== properties

pub struct ApplyBg;
pub struct ApplyW;
pub struct ApplySpan;
pub struct ApplyText;

impl Apply for ApplyBg {
    type P = Color;
    #[inline]
    unsafe fn apply(rt: &mut Rt, node: u32, v: &Color) {
        let n = &mut rt.nodes[node as usize];
        if n.color == v.0 {
            return;
        }
        n.color = v.0;
        let (h, inst, bordered) = (n.handle, rt.runs[n.run as usize].first, n.kind == 3);
        let p = rt.inst_ptr(inst);
        unsafe {
            if bordered {
                (*p).aux[0] = v.0;
            } else {
                (*p).color = v.0;
            }
        }
        rt.bough.touch_handle(h);
    }
}

impl Apply for ApplyW {
    type P = f32;
    #[inline]
    unsafe fn apply(rt: &mut Rt, node: u32, v: &f32) {
        let w = *v * rt.scale;
        let n = &mut rt.nodes[node as usize];
        if n.w == w {
            return;
        }
        n.w = w;
        let inst = rt.runs[n.run as usize].first;
        unsafe { (*rt.inst_ptr(inst)).wh[0] = w };
        rt.quad_ink(node);
    }
}

impl Apply for ApplySpan {
    /// (y, height) in points relative to the element's origin.
    type P = (f32, f32);
    #[inline]
    unsafe fn apply(rt: &mut Rt, node: u32, v: &(f32, f32)) {
        let n = &mut rt.nodes[node as usize];
        let (y, h) = (n.y0 + v.0 * rt.scale, v.1 * rt.scale);
        if n.y == y && n.h == h {
            return;
        }
        n.y = y;
        n.h = h;
        let inst = rt.runs[n.run as usize].first;
        unsafe {
            let p = rt.inst_ptr(inst);
            (*p).xy[1] = y;
            (*p).wh[1] = h;
        }
        rt.quad_ink(node);
    }
}

impl Apply for ApplyText {
    type P = String;
    #[inline]
    unsafe fn apply(rt: &mut Rt, node: u32, v: &String) {
        let n = &mut rt.nodes[node as usize];
        if n.text == *v {
            return;
        }
        n.text.clear();
        n.text.push_str(v);
        if !n.dirty {
            n.dirty = true;
            rt.text_dirty.push(node);
        }
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

impl<V: 'static, Bg, Wd, Sp, Ch> Element<V> for Quad<Bg, Wd, Sp, Ch>
where
    Bg: IntoProp<Color>,
    Wd: IntoProp<f32>,
    Sp: IntoProp<(f32, f32)>,
    Ch: Element<V>,
{
    fn mount(self, m: &mut Mounter<'_, V>) {
        let node = m.rt.alloc_node();
        let wires_before = m.scope.wires.len();
        let bg = self.bg.lower::<ApplyBg, V>(m, node);
        let w = self.wd.lower::<ApplyW, V>(m, node);
        let (y, h) = self.sp.lower::<ApplySpan, V>(m, node);
        let s = m.rt.scale;
        let (x_px, y0_px) = ((m.ox + self.x) * s, m.oy * s);
        let (y_px, h_px, w_px) = (y0_px + y * s, h * s, w * s);
        let kind = match (self.border.is_some(), self.radius > 0.0) {
            (true, _) => KIND_BORDER,
            (false, true) => KIND_RRECT,
            _ => KIND_SOLID,
        };
        let margin = if kind == KIND_SOLID { 0.0 } else { 1.0 };
        let run = m.rt.alloc_run(1);
        m.rt.runs[run as usize].len = 1;
        let inst = m.rt.runs[run as usize].first;
        let mut ins = Instance { xy: [x_px, y_px], wh: [w_px, h_px], uv: [0; 4], color: bg.0, kind, space: m.space, aux: [0, 0, 0, m.clip as u32] };
        if let Some((bw, bc)) = self.border {
            ins.color = bc.0;
            ins.aux[0] = bg.0;
            ins.aux[2] = (bw * s).to_bits();
        }
        ins.aux[1] = (self.radius * s).to_bits();
        unsafe { m.rt.inst_ptr(inst).write(ins) };
        let ink = m.rt.ink_rect(m.space, x_px, y_px, w_px, h_px, margin);
        let dynamic = m.scope.wires.len() > wires_before;
        let mut handle_idx = 0;
        if dynamic {
            let hd = m.rt.bough.new_handle();
            handle_idx = hd.idx;
            let n = &mut m.rt.nodes[node as usize];
            n.handle = hd;
            n.run = run;
            n.space = m.space;
            n.clip = m.clip;
            n.kind = if self.border.is_some() { 3 } else { 1 };
            n.x = x_px;
            n.y = y_px;
            n.y0 = y0_px;
            n.w = w_px;
            n.h = h_px;
            n.color = bg.0;
            n.margin = margin;
            m.scope.nodes.push(node);
        } else {
            m.rt.free_node(node);
        }
        m.scope.runs.push(run);
        m.recs.push(RecordInit { depth: m.depth, ink, kind: REC_QUAD, payload: run, handle: handle_idx });
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
}

// =============================================================================== text

pub struct Text<S> {
    s: S,
    x: f32,
    y: f32,
    size: f32,
    lh: f32,
    color: Color,
}

/// Text from a constant, `&Signal<String>`-like or `sig.map(f)` operand. Single line, or explicit
/// multi-line (`\n`); positioned at (x, y) with `size`/`line_height` in points.
pub fn text<S>(s: S) -> Text<S> {
    Text { s, x: 0.0, y: 0.0, size: 12.0, lh: 16.0, color: Color::rgba(255, 255, 255, 255) }
}

impl<S> Text<S> {
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
}

impl<V: 'static, S: IntoProp<String>> Element<V> for Text<S> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let node = m.rt.alloc_node();
        let wires_before = m.scope.wires.len();
        let init = self.s.lower::<ApplyText, V>(m, node);
        let s = m.rt.scale;
        let size_px = self.size * s;
        let lh_px = self.lh * s;
        let strike = m.rt.text.strike(size_px);
        let base = ((m.oy + self.y) * s + m.rt.text.baseline(size_px, lh_px)).round();
        let run = m.rt.alloc_run(init.len().max(4));
        {
            let n = &mut m.rt.nodes[node as usize];
            n.text = init;
            n.run = run;
            n.space = m.space;
            n.clip = m.clip;
            n.kind = 2;
            n.x = (m.ox + self.x) * s;
            n.y = base;
            n.color = self.color.0;
            n.strike = strike as u32;
            n.size_px = size_px;
            n.line_h = lh_px;
        }
        let ink = m.rt.shape_node(node);
        let dynamic = m.scope.wires.len() > wires_before;
        let mut handle_idx = 0;
        if dynamic {
            let hd = m.rt.bough.new_handle();
            handle_idx = hd.idx;
            m.rt.nodes[node as usize].handle = hd;
            m.scope.nodes.push(node);
        } else {
            m.rt.free_node(node);
        }
        m.scope.runs.push(run);
        m.recs.push(RecordInit { depth: m.depth, ink, kind: REC_TEXT, payload: run, handle: handle_idx });
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
}

/// Children clipped to a rect (points, relative to the current origin): one vertex-stage clamp.
pub struct Clip<Ch>(pub [f32; 4], pub Ch);
pub fn clip<Ch>(x: f32, y: f32, w: f32, h: f32, ch: Ch) -> Clip<Ch> {
    Clip([x, y, w, h], ch)
}
impl<V, Ch: Element<V>> Element<V> for Clip<Ch> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let o = m.rt.space(m.space);
        let [x, y, w, h] = self.0;
        let r = [o[0] + (m.ox + x) * s, o[1] + (m.oy + y) * s, o[0] + (m.ox + x + w) * s, o[1] + (m.oy + y + h) * s];
        let prev = m.clip;
        m.clip = m.rt.clip_rect(r);
        self.1.mount(m);
        m.clip = prev;
    }
}

// =============================================================================== virtual list

/// A keyed virtual-list region: rows `visible(scroll)` are mounted (each an owner of its own
/// wires), rows that leave the range are unmounted, and rows are positioned through per-row
/// spaces — a scroll writes the space table and translates the rows' ink, it never rewrites rows.
pub struct VirtualList<'a, V> {
    scroll: &'a Signal<f32>,
    row_h: f32,
    vp: [f32; 4],
    visible: fn(f32) -> Range<usize>,
    row: fn(&V, usize, &mut Mounter<'_, V>),
    _v: PhantomData<fn(&V)>,
}

/// `vp` = (x, y, w, h) of the viewport in points (the rows' clip); rows are `row_h` points tall.
pub fn virtual_list<'a, V>(
    scroll: &'a Signal<f32>,
    row_h: f32,
    vp: [f32; 4],
    visible: fn(f32) -> Range<usize>,
    row: fn(&V, usize, &mut Mounter<'_, V>),
) -> VirtualList<'a, V> {
    VirtualList { scroll, row_h, vp, visible, row, _v: PhantomData }
}

unsafe fn mount_row<V: 'static>(rt: &mut Rt, owner: *const u8, reg: usize, row: usize, recs: &mut Vec<RecordInit>) -> RowInfo {
    let space = rt.alloc_space();
    let origin = rt.region_row_space_origin(reg, row);
    rt.set_space(space, origin);
    let (clip, depth, f) = {
        let r = &rt.regions[reg];
        (r.clip, r.depth, r.row_fn)
    };
    let f: fn(&V, usize, &mut Mounter<'_, V>) = unsafe { std::mem::transmute::<usize, fn(&V, usize, &mut Mounter<'_, V>)>(f) };
    let mut scope = Scope::default();
    let before = recs.len();
    {
        let mut m = Mounter { rt, owner: owner as *const V, recs, scope: &mut scope, depth, ox: 0.0, oy: 0.0, space, clip };
        f(unsafe { &*(owner as *const V) }, row, &mut m);
    }
    RowInfo { nrec: (recs.len() - before) as u32, space, scope }
}

unsafe fn patch_scroll(rt: &mut Rt, w: &Wire, sig: *const u8, owner: *const u8) {
    let s = unsafe { &*(sig as *const Signal<f32>) };
    unsafe { rt.region_scroll(w.node as usize, owner, **s) };
}

impl<'a, V: 'static> Element<V> for VirtualList<'a, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let o = m.rt.space(m.space);
        let [x, y, w, h] = self.vp;
        let (vx, vy) = (o[0] + (m.ox + x) * s, o[1] + (m.oy + y) * s);
        let clip = m.rt.clip_rect([vx, vy, vx + w * s, vy + h * s]);
        let reg = m.rt.regions.len();
        m.rt.regions.push(Region {
            row_h: self.row_h * s,
            vp_x: vx,
            vp_y: vy,
            clip,
            depth: m.depth,
            base_pos: m.recs.len(),
            range: 0..0,
            rows: Default::default(),
            scroll_px: 0.0,
            visible: self.visible,
            mount_row: mount_row::<V>,
            row_fn: self.row as usize,
        });
        if let Some(off) = m.offset_of(self.scroll) {
            m.wire(self.scroll.head(), off, patch_scroll, reg as u32, [0; 16]);
        }
        unsafe { m.rt.region_initial(reg, m.owner as *const u8, **self.scroll, m.recs) };
    }
}

// =============================================================================== bars (series run)

/// A sliding-window bar series: one record owning a run of `count` solid quads. Bar `i` has height
/// `max(max_h * sample(key + i), min_h)` points growing up from `base`. When `key` advances by one
/// the window shifts and only the new rightmost sample is computed (the memoised samples are the
/// value that was there all along); any other jump recomputes every sample. One wire, one ink write
/// (old and new union) per update however many bars there are.
pub struct Bars<'a, F> {
    key: &'a Signal<u64>,
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

pub fn bars<'a, F: Fn(u64) -> f32 + Copy + 'static>(
    x: f32,
    step: f32,
    width: f32,
    count: usize,
    base: f32,
    max_h: f32,
    min_h: f32,
    key: &'a Signal<u64>,
    sample: F,
) -> Bars<'a, F> {
    Bars { key, sample, x, step, width, count, base, max_h, min_h, color: Color::rgba(255, 255, 255, 255) }
}

impl<F> Bars<'_, F> {
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
}

/// Recompute the node's samples for `key` (shifting when the key advanced by one) and write the
/// bar instances; returns the union ink.
fn write_bars<F: Fn(u64) -> f32>(rt: &mut Rt, node: u32, key: u64, f: F) -> bough_core::NRect {
    let n = &mut rt.nodes[node as usize];
    let count = n.series.len();
    if n.series_ok && key == n.series_key.wrapping_add(1) {
        n.series.copy_within(1.., 0);
        n.series[count - 1] = f(key + count as u64 - 1);
    } else if !n.series_ok || key != n.series_key {
        for (i, s) in n.series.iter_mut().enumerate() {
            *s = f(key + i as u64);
        }
    }
    n.series_ok = true;
    n.series_key = key;
    // geometry: n.x = x of bar 0 (px), n.w = bar width px, n.h = step px, n.radius = base px (space-local),
    // n.margin = max_h pt, n.line_h = min_h pt, n.size_px = scale
    let (x0, wpx, step, base_pt, max_h, min_h, scale, y0) = (n.x, n.w, n.h, n.y, n.margin, n.line_h, n.size_px, n.y0);
    let first = rt.runs[n.run as usize].first;
    let mut top = f32::MAX;
    for i in 0..count {
        let h = (max_h * rt.nodes[node as usize].series[i]).max(min_h);
        let y = y0 + (base_pt - h) * scale;
        let p = rt.inst_ptr(first + i as u32);
        unsafe {
            (*p).xy = [x0 + i as f32 * step, y];
            (*p).wh = [wpx, h * scale];
        }
        top = top.min(y);
    }
    let space = rt.nodes[node as usize].space;
    let bottom = y0 + base_pt * scale;
    rt.ink_rect(space, x0, top, (count as f32 - 1.0) * step + wpx, bottom - top, 0.0)
}

unsafe fn patch_bars<F: Fn(u64) -> f32 + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    let key = **unsafe { &*(sig as *const Signal<u64>) };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let ink = write_bars(rt, w.node, key, f);
    let h = rt.nodes[w.node as usize].handle;
    rt.bough.set_ink_handle(h, ink);
}

impl<'a, V: 'static, F: Fn(u64) -> f32 + Copy + 'static> Element<V> for Bars<'a, F> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        const { assert!(std::mem::size_of::<F>() <= 16 && std::mem::align_of::<F>() <= 8) };
        let s = m.rt.scale;
        let node = m.rt.alloc_node();
        let run = m.rt.alloc_run(self.count);
        m.rt.runs[run as usize].len = self.count as u32;
        let first = m.rt.runs[run as usize].first;
        for i in 0..self.count {
            let ins = Instance { xy: [0.0; 2], wh: [0.0; 2], uv: [0; 4], color: self.color.0, kind: KIND_SOLID, space: m.space, aux: [0, 0, 0, m.clip as u32] };
            unsafe { m.rt.inst_ptr(first + i as u32).write(ins) };
        }
        {
            let n = &mut m.rt.nodes[node as usize];
            n.run = run;
            n.space = m.space;
            n.clip = m.clip;
            n.kind = 4;
            n.x = (m.ox + self.x) * s;
            n.w = self.width * s;
            n.h = self.step * s;
            n.y = self.base;
            n.y0 = m.oy * s;
            n.margin = self.max_h;
            n.line_h = self.min_h;
            n.size_px = s;
            n.series.clear();
            n.series.resize(self.count, 0.0);
            n.series_ok = false;
        }
        let ink = write_bars(m.rt, node, **self.key, self.sample);
        let mut recipe = [0u8; 16];
        unsafe { std::ptr::write_unaligned(recipe.as_mut_ptr() as *mut F, self.sample) };
        let before = m.scope.wires.len();
        if let Some(off) = m.offset_of(self.key) {
            m.wire(self.key.head(), off, patch_bars::<F>, node, recipe);
        }
        let mut handle_idx = 0;
        if m.scope.wires.len() > before {
            let hd = m.rt.bough.new_handle();
            handle_idx = hd.idx;
            m.rt.nodes[node as usize].handle = hd;
            m.scope.nodes.push(node);
        } else {
            m.rt.free_node(node);
        }
        m.scope.runs.push(run);
        m.recs.push(RecordInit { depth: m.depth, ink, kind: REC_QUAD, payload: run, handle: handle_idx });
    }
}
