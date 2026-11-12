//! `Answer<T>`: a question as a field (`hits: Answer::to(field!(Search.query), search)`), the alternative to the
//! view element `ask(..).into(..)` (`docs/lanes/async/programs/p01b_search_field.rs`). The relation is declared
//! once, beside the state; the field reads like a `Signal` (`*self.hits`, `sig!(self.hits)`), knows whether it is
//! `pending`, and `refresh`es. It is found at mount from the owner type's compile-time layout, as actions are.
//!
//! `field!(Type.path)` names a cell of a type the way `sig!` names one of a value: the offset comes from
//! `offset_of!` and an accessor pins the field's type, so the runtime reads `owner + offset` only as that type.
use crate::ask::{Accept, SrcCell};
use crate::elem::Mounter;
use crate::signal::{keep, Headed, Signal, Wired};
use bough_async::{Ask, Asking, Flight, QOpts};
use std::any::TypeId;
use std::async_iter::AsyncIterator;
use std::future::Future;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::DynMetadata;
use std::time::Duration;

/// A struct field of `V` of cell type `C`, at an offset the compiler proved (`field!`).
pub struct Field<V, C> {
    off: u32,
    _p: PhantomData<fn(&V) -> &C>,
}
impl<V, C> Clone for Field<V, C> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V, C> Copy for Field<V, C> {}

impl<V, C> Field<V, C> {
    /// # Safety
    /// `off` is `offset_of!(V, path)` for the struct field path the accessor names (the macro computes both).
    #[doc(hidden)]
    pub unsafe fn new(off: usize, _acc: fn(&V) -> &C) -> Self {
        assert!(off + size_of::<C>() <= size_of::<V>() && off <= u32::MAX as usize);
        Field { off: off as u32, _p: PhantomData }
    }
    /// The cell's offset in `V`.
    pub fn offset(&self) -> u32 {
        self.off
    }
    /// An input computed from the cell (`field!(Reader.story).map(|s| s.as_ref().map(|s| s.author))`).
    pub fn map<S, F: Fn(&C::T) -> S>(self, f: F) -> FieldMap<V, C, F>
    where
        C: SourceCell,
    {
        FieldMap { f: self, m: f }
    }
}

/// `field!(Type.path)`: a cell of `Type` named by its struct field path (see the module docs).
#[macro_export]
macro_rules! field {
    ($t:ident . $($f:ident).+) => {{
        let p: fn(&$t) -> &_ = |o: &$t| &o.$($f).+;
        // SAFETY: the offset and the accessor name the same struct field path of `$t`.
        unsafe { $crate::answer::Field::<$t, _>::new(::core::mem::offset_of!($t, $($f).+), p) }
    }};
    ($t:ty => $($f:ident).+) => {{
        let p: fn(&$t) -> &_ = |o: &$t| &o.$($f).+;
        // SAFETY: as above.
        unsafe { $crate::answer::Field::<$t, _>::new(::core::mem::offset_of!($t, $($f).+), p) }
    }};
}

pub struct FieldMap<V, C, F> {
    f: Field<V, C>,
    m: F,
}

/// A cell a field can name as a question's source: its value type.
pub trait SourceCell: 'static {
    type T: Clone + 'static;
}
impl<T: Clone + 'static> SourceCell for Signal<T> {
    type T = T;
}
impl<T: Clone + 'static> SourceCell for Answer<T> {
    type T = T;
}

/// A source cell of a field question: its offset in `V` and how to keep its value (opaque).
#[doc(hidden)]
pub struct CellDesc(pub(crate) u32, pub(crate) keep::Fns);

/// What a field question asks from: a field, a mapped field, or a pair.
pub trait FieldSource<V>: 'static {
    type In: Clone + PartialEq + 'static;
    /// Offsets (in `V`) of its cells, with how to keep their values.
    #[doc(hidden)]
    fn cells(&self, out: &mut Vec<CellDesc>);
    fn reader(self) -> Box<dyn Fn(*const u8) -> Self::In>;
}

// a source cell's value: a `Signal<T>` is read as itself; an `Answer<T>` starts with its `Signal<T>` (`repr(C)`)
#[inline(always)]
unsafe fn value_at<'a, T>(o: *const u8, off: u32) -> &'a T {
    // SAFETY: the caller's contract: a `Signal<T>` (or an `Answer<T>`, whose first field is one) lives there.
    unsafe { &**(o.add(off as usize) as *const Signal<T>) }
}

impl<V: 'static, C: SourceCell> FieldSource<V> for Field<V, C>
where
    C::T: PartialEq,
{
    type In = C::T;
    fn cells(&self, out: &mut Vec<CellDesc>) {
        out.push(CellDesc(self.off, keep::fns::<C::T>()));
    }
    fn reader(self) -> Box<dyn Fn(*const u8) -> C::T> {
        let off = self.off;
        // SAFETY: `off` is a proven `C` field of `V`, and the reader is called with a `V`.
        Box::new(move |o| unsafe { value_at::<C::T>(o, off) }.clone())
    }
}

impl<V: 'static, C: SourceCell, S: Clone + PartialEq + 'static, F: Fn(&C::T) -> S + 'static> FieldSource<V> for FieldMap<V, C, F> {
    type In = S;
    fn cells(&self, out: &mut Vec<CellDesc>) {
        out.push(CellDesc(self.f.off, keep::fns::<C::T>()));
    }
    fn reader(self) -> Box<dyn Fn(*const u8) -> S> {
        let (off, m) = (self.f.off, self.m);
        // SAFETY: as for `Field`.
        Box::new(move |o| m(unsafe { value_at::<C::T>(o, off) }))
    }
}

impl<V: 'static, A: FieldSource<V>, B: FieldSource<V>> FieldSource<V> for (A, B) {
    type In = (A::In, B::In);
    fn cells(&self, out: &mut Vec<CellDesc>) {
        self.0.cells(out);
        self.1.cells(out);
    }
    fn reader(self) -> Box<dyn Fn(*const u8) -> Self::In> {
        let (a, b) = (self.0.reader(), self.1.reader());
        Box::new(move |o| (a(o), b(o)))
    }
}

// =============================================================================== the field

/// The answer to a question, as a field (see the module docs). Reads like a `Signal<T>`.
#[repr(C)]
pub struct Answer<T> {
    sig: Signal<T>,
    spec: Option<Box<dyn Spec>>,
}

impl<T> Deref for Answer<T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T {
        &self.sig
    }
}

impl<T: 'static> Answer<T> {
    /// The answer to `f(src)`: asked when `src` changes, latest wins, its sources held while it is out. Starts at
    /// `T::default()` (`None` for an `Option`: the loading state).
    pub fn to<V: 'static, S: FieldSource<V>, F, Fut>(src: S, f: F) -> Answer<T>
    where
        T: Default + Accept<Fut::Output>,
        F: Fn(S::In) -> Fut + 'static,
        Fut: Future + 'static,
        Fut::Output: 'static,
    {
        Answer { sig: Signal::new(T::default()), spec: Some(Box::new(SpecOf::<V, S, crate::ask::OnceFn<F>, T> { src: Some(src), w: Some(crate::ask::OnceFn::new(f)), opts: QOpts { hold: true, every: None, lands_into: true }, _p: PhantomData })) }
    }
    /// The answers of the stream `f(src)`: each replaces the value.
    pub fn following<V: 'static, S: FieldSource<V>, F, St>(src: S, f: F) -> Answer<T>
    where
        T: Default + Accept<St::Item>,
        F: Fn(S::In) -> St + 'static,
        St: AsyncIterator + 'static,
        St::Item: 'static,
    {
        Answer { sig: Signal::new(T::default()), spec: Some(Box::new(SpecOf::<V, S, crate::ask::ManyFn<F>, T> { src: Some(src), w: Some(crate::ask::ManyFn::new(f)), opts: QOpts { hold: true, every: None, lands_into: true }, _p: PhantomData })) }
    }
    /// Start from `v` instead of the default.
    pub fn starting(mut self, v: T) -> Self {
        self.sig = Signal::new(v);
        self
    }
    /// Hold nothing while out (the value lags its input).
    pub fn deferred(mut self) -> Self {
        if let Some(s) = self.spec.as_mut() {
            s.opts().hold = false;
        }
        self
    }
    /// Ask again (quietly) every `d`.
    pub fn every(mut self, d: Duration) -> Self {
        if let Some(s) = self.spec.as_mut() {
            s.opts().every = Some(d);
        }
        self
    }
    /// Is a question about it in the air, or is it held?
    pub fn pending(&self) -> bool {
        self.sig.pending()
    }
    /// Ask again with the same input (stale-while-revalidate, a retry).
    pub fn refresh(&mut self) {
        self.sig.refresh()
    }
}

// SAFETY: `#[repr(C)]`, its `Signal<T>` (head first) first: the runtime and `sig!` see a `Signal<T>` there.
unsafe impl<T> Headed for Answer<T> {
    type At<'a, V>
        = Wired<'a, T, V>
    where
        T: 'a;
    #[inline(always)]
    unsafe fn at<'a, V>(&'a self, off: u32) -> Wired<'a, T, V> {
        // SAFETY: the caller's contract; the signal is at the answer's own offset.
        unsafe { self.sig.at(off) }
    }
}

/// How an answer's question is built at mount (type-erased; `V` checked against the struct holding it).
trait Spec {
    fn owner_type(&self) -> TypeId;
    fn opts(&mut self) -> &mut QOpts;
    /// The question for the answer at `dst` (offset in the struct `parent`), the struct at `base` in the owner.
    fn build(&mut self, owner: *mut u8, cls: u32, base: u32, dst: u32) -> (Vec<SrcCell>, Box<dyn bough_async::Question>, QOpts);
}

struct SpecOf<V, S, W, T> {
    src: Option<S>,
    w: Option<W>,
    opts: QOpts,
    _p: PhantomData<fn(&V) -> T>,
}

impl<V: 'static, S: FieldSource<V>, W: crate::ask::Starts<S::In>, T: Accept<<W::Fl as Flight>::Out> + 'static> Spec for SpecOf<V, S, W, T> {
    fn owner_type(&self) -> TypeId {
        TypeId::of::<V>()
    }
    fn opts(&mut self) -> &mut QOpts {
        &mut self.opts
    }
    fn build(&mut self, owner: *mut u8, cls: u32, base: u32, dst: u32) -> (Vec<SrcCell>, Box<dyn bough_async::Question>, QOpts) {
        let src = self.src.take().expect("an answer mounts once");
        let mut offs = Vec::new();
        src.cells(&mut offs);
        // SAFETY: `base` is the offset of the struct holding the answer (a `V`) inside the owner; its fields' heads
        // are at `base + off`.
        let cells = offs.into_iter().map(|CellDesc(off, keep)| SrcCell { head: unsafe { owner.add((base + off) as usize) } as usize, off: base + off, keep }).collect();
        let read = src.reader();
        let parent = base as usize;
        let q = FieldQ { read: Box::new(move |o: *const u8| read(o.wrapping_add(parent))), w: self.w.take().unwrap(), dst, owner: owner as usize, cls, _t: PhantomData::<fn() -> T> };
        (cells, Asking::new(q), self.opts)
    }
}

struct FieldQ<In, W, T> {
    read: Box<dyn Fn(*const u8) -> In>,
    w: W,
    dst: u32,
    owner: usize,
    cls: u32,
    _t: PhantomData<fn() -> T>,
}

impl<In: Clone + PartialEq + 'static, W: crate::ask::Starts<In>, T: Accept<<W::Fl as Flight>::Out> + 'static> Ask for FieldQ<In, W, T> {
    type In = In;
    type Fl = W::Fl;
    unsafe fn input(&self, owner: *mut u8) -> In {
        (self.read)(owner)
    }
    fn start(&self, i: In) -> W::Fl {
        self.w.start(i)
    }
    unsafe fn land(&self, owner: *mut u8, o: <W::Fl as Flight>::Out, _first: bool) {
        crate::ask::touch_owner(self.owner, self.cls);
        // SAFETY: `dst` is the offset of this `Answer<T>` in the owner (found in its layout); its signal is first.
        let s = unsafe { &mut *(owner.add(self.dst as usize) as *mut Signal<T>) };
        T::accept(&mut *s, o);
    }
}

// =============================================================================== found at mount

/// What the runtime needs of any `Answer<T>`, reached through the vtable the owner's layout gives (implemented
/// for every `Answer<T>`, with no bounds: the compile-time lookup does not see bounded impls).
pub trait AnswerCell {
    fn take_spec(&mut self) -> Option<TypeId>;
    fn build(&mut self, owner: *mut u8, cls: u32, base: u32, dst: u32) -> Option<(Vec<SrcCell>, Box<dyn bough_async::Question>, QOpts)>;
}

impl<T> AnswerCell for Answer<T> {
    fn take_spec(&mut self) -> Option<TypeId> {
        self.spec.as_ref().map(|s| s.owner_type())
    }
    fn build(&mut self, owner: *mut u8, cls: u32, base: u32, dst: u32) -> Option<(Vec<SrcCell>, Box<dyn bough_async::Question>, QOpts)> {
        let mut s = self.spec.take()?;
        Some(s.build(owner, cls, base, dst))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AnsField {
    pub off: u32,
    pub parent_off: u32,
    pub parent: TypeId,
    pub vt: Option<DynMetadata<dyn AnswerCell>>,
}

pub(crate) const MAX_ANSWERS: usize = 32;

#[derive(Clone, Copy)]
pub(crate) struct AnsScan {
    pub n: usize,
    pub f: [AnsField; MAX_ANSWERS],
}

pub(crate) fn answers_of<T>() -> &'static AnsScan {
    const { &scan(std::mem::type_info::of::<T>()) }
}

#[rustc_comptime]
fn scan(t: TypeId) -> AnsScan {
    let blank = AnsField { off: 0, parent_off: 0, parent: t, vt: None };
    let mut s = AnsScan { n: 0, f: [blank; MAX_ANSWERS] };
    walk(&mut s, t, 0, 0);
    s
}

#[rustc_comptime]
fn walk(s: &mut AnsScan, t: TypeId, base: usize, depth: u8) {
    if depth > 6 || t.trait_info_of::<dyn crate::reflect::IsTuple>().is_some() || t.trait_info_of::<dyn crate::reflect::Opaque>().is_some() {
        return;
    }
    if !matches!(t.info().kind, std::mem::type_info::TypeKind::Struct) {
        return;
    }
    let n = t.fields(0);
    let mut k = 0;
    while k < n {
        let f = t.field(0, k);
        let ft = f.type_id();
        let off = base + f.offset();
        match ft.trait_info_of::<dyn AnswerCell>() {
            Some(v) => {
                if s.n < MAX_ANSWERS {
                    s.f[s.n] = AnsField { off: off as u32, parent_off: base as u32, parent: t, vt: Some(v.get_vtable()) };
                    s.n += 1;
                }
            }
            None => {
                let cell = matches!(ft.info().kind, std::mem::type_info::TypeKind::Struct) && ft.fields(0) >= 1 && ft.field(0, 0).type_id() == std::mem::type_info::of::<crate::signal::WireHead>();
                if !cell {
                    walk(s, ft, off, depth + 1);
                }
            }
        }
        k += 1;
    }
}

/// Mount every `Answer` field of the owner being mounted (`App::new`, a list row's mount).
pub(crate) fn mount_answers<V: 'static>(m: &mut Mounter<'_, V>) {
    let s = answers_of::<V>();
    for f in &s.f[..s.n] {
        let vt = f.vt.expect("an answer's vtable");
        let owner = m.owner as *mut u8;
        // SAFETY: `f.off` is a struct field offset of `V` from the compiler's layout, of a type implementing
        // `AnswerCell` with this vtable; the owner is live and does not move while mounted.
        let cell: *mut dyn AnswerCell = std::ptr::from_raw_parts_mut(unsafe { owner.add(f.off as usize) } as *mut (), vt);
        let Some(t) = (unsafe { (*cell).take_spec() }) else { continue };
        assert!(t == f.parent, "an Answer's `field!` sources name another type than the struct holding the answer");
        let Some((cells, q, opts)) = (unsafe { (*cell).build(owner, m.cls, f.parent_off, f.off) }) else { continue };
        let about = owner as usize + f.off as usize;
        crate::ask::mount_question(m.rt, m.scope, owner, m.cls, cells, q, about, opts);
    }
}
