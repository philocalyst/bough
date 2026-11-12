//! Questions: a cell kept equal to an async function of other cells (`ask(src, f).into(dst)`), and the binding
//! of `bough-async`'s engine to cells, wires and turns. The rules are in `docs/lanes/async/DESIGN.md`; the
//! programs that asked for them are in `docs/lanes/async/programs/`.
//!
//! * A question is an element of `view` that mounts nothing. Its sources are wired in the question group of
//!   their heads (`G_QUESTION`): when one is written, the engine compares the input and asks, nets, or does
//!   nothing, before any other wire of that head runs, so a hold it decides is seen by all of them.
//! * Holds are a bit on the cell's head (`HELD`); a source of a holding question has `KEEP`, so its first write
//!   of a turn keeps a pre-image (`signal::keep`), which a description reads while the cell is held.
//! * The pump (`Rt::pump_async`) lands what the engine says is ready, one unit at a time, settling after each:
//!   answers outside publications, publications (two phases, cascades), actions.
use crate::elem::{Element, IntoProp, Mounter, TextSrc};
use crate::rt::*;
use crate::signal::{keep, Signal, WMap, Wired, HELD, KEEP, LOADING};
use bough_async::{Asking, Engine, Flight, QId, QOpts, Step};
use std::async_iter::AsyncIterator;
use std::cell::{Cell, RefCell};
use bough_async::addr::AddrMap as HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

// =============================================================================== the runtime's half

/// The async state of a runtime: the engine, and what the façade needs to apply its decisions.
pub struct Asks {
    pub engine: Engine,
    /// Per question id: its cells (sources with whether they are kept, and its subject).
    q_cells: Vec<(Vec<usize>, bool, usize)>,
    /// Source and subject cells -> (owner, class, questions using it).
    cells: HashMap<usize, (usize, u32, u32)>,
    /// Per action wire node: the action and its engine id.
    pub(crate) acts: Vec<crate::action::ActSlot>,
    /// The clock: `Some(t)` a fake one (tests advance it), `None` real time since `start`.
    pub fake: Option<Duration>,
    start: Instant,
    /// Do not ask anything (a reference app built from a published state, for oracles).
    pub inert: bool,
    /// This runtime's identity in the thread's pre-image table.
    pub(crate) id: u64,
    /// A pre-image may have been kept this turn (set by settle from the write log).
    pub(crate) kept_turn: bool,
    /// Scratch for the engine's decisions (kept across turns: applying them allocates nothing).
    scratch: [Vec<usize>; 3],
    /// `catch` boundaries (`catch.rs`), the ones open while mounting, and per question the boundary it raises to.
    pub(crate) catches: Vec<crate::catch::Catch>,
    pub(crate) catching: Vec<u32>,
    pub(crate) q_catch: Vec<u32>,
    /// Actions' truths while their overlays are swapped in for a description (one slot per action).
    pub(crate) overlaid: Vec<Option<Box<dyn std::any::Any>>>,
}

fn next_id() -> u64 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Default for Asks {
    fn default() -> Self {
        Asks { engine: Engine::new(), q_cells: Vec::new(), cells: HashMap::default(), acts: Vec::new(), fake: None, start: Instant::now(), inert: false, id: next_id(), kept_turn: false, scratch: Default::default(), catches: Vec::new(), catching: Vec::new(), q_catch: Vec::new(), overlaid: Vec::new() }
    }
}

impl Asks {
    pub fn now(&self) -> Duration {
        self.fake.unwrap_or_else(|| self.start.elapsed())
    }
}

thread_local! {
    /// Owners other than the root that a landing wrote into this step (settled after it).
    static TOUCHED: RefCell<Vec<(usize, u32)>> = const { RefCell::new(Vec::new()) };
}

/// A landing wrote into owner `owner` of class `cls` (settled after the step if it is not the root).
pub(crate) fn touch_owner(owner: usize, cls: u32) {
    if cls != 0 {
        TOUCHED.with(|t| {
            let mut t = t.borrow_mut();
            if !t.contains(&(owner, cls)) {
                t.push((owner, cls));
            }
        });
    }
}

#[inline(always)]
fn head_at(c: usize) -> &'static Cell<u32> {
    // SAFETY: every address the engine hands back is a registered cell's head, live while a question of its
    // owner is mounted (`revoke_ask` releases them before the scope goes).
    unsafe { &*(c as *const Cell<u32>) }
}

impl Rt {
    /// Apply the engine's decisions: holds, releases, loading flags. Each touched head is queued (quietly: no
    /// pre-image), so its status wires run, and on a release every wire.
    pub(crate) fn apply_async(&mut self) {
        let e = &mut self.asks.engine;
        if e.hold_out.is_empty() && e.release_out.is_empty() && e.status_out.is_empty() {
            return;
        }
        let [mut holds, mut releases, mut status] = std::mem::take(&mut self.asks.scratch);
        let e = &mut self.asks.engine;
        std::mem::swap(&mut holds, &mut e.hold_out);
        std::mem::swap(&mut releases, &mut e.release_out);
        std::mem::swap(&mut status, &mut e.status_out);
        for &c in &holds {
            let h = head_at(c);
            h.set(h.get() | HELD);
            keep::held(c);
            crate::signal::queue_quiet(h);
            self.touch(c);
        }
        for &c in &releases {
            let h = head_at(c);
            // while still held: a release during a description swaps intent back into the cell first
            keep::release(c);
            h.set(h.get() & !HELD);
            crate::signal::queue_quiet(h);
            self.touch(c);
        }
        for &c in &status {
            let on = self.asks.engine.loading(c);
            let h = head_at(c);
            let v = h.get();
            let nv = if on { v | LOADING } else { v & !LOADING };
            if nv != v {
                h.set(nv);
                crate::signal::queue_quiet(h);
                self.touch(c);
            }
        }
        holds.clear();
        releases.clear();
        status.clear();
        self.asks.scratch = [holds, releases, status];
    }

    fn touch(&mut self, c: usize) {
        if let Some(&(o, cls, _)) = self.asks.cells.get(&c) {
            if cls != 0 {
                TOUCHED.with(|t| {
                    let mut t = t.borrow_mut();
                    if !t.contains(&(o, cls)) {
                        t.push((o, cls));
                    }
                });
            }
        }
    }

    /// Settle the root and every other owner touched since the last call.
    ///
    /// # Safety
    /// `root` is the live root owner (`settle`'s contract).
    pub(crate) unsafe fn settle_async(&mut self, root: *const u8) {
        unsafe { self.settle(root) };
        loop {
            let t = TOUCHED.with(|t| std::mem::take(&mut *t.borrow_mut()));
            if t.is_empty() {
                break;
            }
            for (o, cls) in t {
                // SAFETY: a touched owner is the live owner of a mounted question or action (its class's type).
                unsafe { self.settle_cls(o as *const u8, cls) };
            }
            unsafe { self.settle(root) };
        }
    }

    /// Land what is ready, settling after each unit (see `bough_async::Step`). Returns whether anything landed.
    ///
    /// # Safety
    /// As `settle_async`.
    pub(crate) unsafe fn pump_async(&mut self, root: *const u8) -> bool {
        let mut any = false;
        for _ in 0..1024 {
            match self.asks.engine.step() {
                Step::Idle => return any,
                Step::Landed => {
                    any = true;
                    self.take_raised();
                    self.apply_async();
                    unsafe { self.settle_async(root) };
                }
                Step::Committing(p) => {
                    any = true;
                    self.take_raised();
                    self.apply_async();
                    unsafe { self.settle_async(root) };
                    while self.asks.engine.end_commit(p) {
                        self.take_raised();
                        self.apply_async();
                        unsafe { self.settle_async(root) };
                    }
                    self.apply_async();
                    unsafe { self.settle_async(root) };
                }
            }
        }
        panic!("async pump did not converge (a landing that always re-asks?)");
    }

    /// The start of a turn: advance the clock, poll what rang, land what is ready. Costs a load and a compare
    /// when nothing is in the air.
    ///
    /// # Safety
    /// As `settle_async`.
    pub(crate) unsafe fn admit_async(&mut self, root: *const u8) -> bool {
        // wake-driven: nothing rang (no waker, no clock), nothing to admit, whatever is pending
        if !self.async_on || !self.asks.engine.take_rung() {
            return false;
        }
        let now = self.asks.now();
        if self.asks.engine.poll(now) {
            self.apply_async();
            return unsafe { self.pump_async(root) };
        }
        self.apply_async();
        false
    }

    /// After a handler: the refreshes it asked for, then (after its settle) the turn's publication is complete in
    /// membership and what is ready lands.
    pub(crate) fn take_refreshes(&mut self, r: Vec<usize>) {
        for c in r {
            self.asks.engine.refresh(c);
        }
        self.apply_async();
    }

    /// The question group of the turn is closed and what is ready lands.
    ///
    /// # Safety
    /// As `settle_async`.
    pub(crate) unsafe fn finish_async(&mut self, root: *const u8) -> bool {
        if !self.async_on {
            return false;
        }
        self.asks.engine.close_group();
        if !self.asks.engine.has_work() {
            return false;
        }
        unsafe { self.pump_async(root) }
    }

    /// A question's scope was released: the engine drops its flight and releases the cells no live question
    /// reads any more (now, while their memory is the owner's: a row's slot is reused only after this turn).
    pub(crate) fn revoke_ask(&mut self, q: u32) {
        self.asks.engine.revoke(QId(q));
        self.apply_async();
        let (srcs, kept, about) = std::mem::take(&mut self.asks.q_cells[q as usize]);
        for c in srcs {
            if kept && keep::unregister(c) {
                let h = head_at(c);
                h.set(h.get() & !KEEP);
            }
            self.unregister_cell(c);
        }
        if about != 0 {
            self.unregister_cell(about);
        }
    }

    fn register_cell(&mut self, c: usize, owner: usize, cls: u32) {
        self.asks.cells.entry(c).or_insert((owner, cls, 0)).2 += 1;
    }
    fn unregister_cell(&mut self, c: usize) {
        if let Some(e) = self.asks.cells.get_mut(&c) {
            e.2 -= 1;
            if e.2 == 0 {
                self.asks.cells.remove(&c);
            }
        }
    }
}

// =============================================================================== sources

/// A source cell of a question: its head, its offset in the owner, and how to keep its value.
pub struct SrcCell {
    pub(crate) head: usize,
    pub(crate) off: u32,
    pub(crate) keep: keep::Fns,
}

/// What a question asks from: `sig!(x)`, `sig!(x).map(f)`, a tuple of those, `(a, b).map(f)`, or `()`.
pub trait Source<V> {
    /// The question's input: compared to decide whether to ask again.
    type In: Clone + PartialEq + 'static;
    fn cells(&self, out: &mut Vec<SrcCell>);
    /// The input from the owner's cells (kept by the question for its life).
    fn reader(self) -> Box<dyn Fn(*const u8) -> Self::In>;
    /// Its part of the question's seal: the offsets of its cells.
    fn seal_cells<SS: crate::seal::Sealer>(&self, s: &mut SS);
}

impl<V> Source<V> for () {
    type In = ();
    fn cells(&self, _: &mut Vec<SrcCell>) {}
    fn seal_cells<SS: crate::seal::Sealer>(&self, _: &mut SS) {}
    fn reader(self) -> Box<dyn Fn(*const u8)> {
        Box::new(|_| ())
    }
}

impl<'a, T: Clone + PartialEq + 'static, V> Source<V> for Wired<'a, T, V> {
    type In = T;
    fn seal_cells<SS: crate::seal::Sealer>(&self, s: &mut SS) {
        s.word(self.off);
    }
    fn cells(&self, out: &mut Vec<SrcCell>) {
        out.push(SrcCell { head: self.sig as *const Signal<T> as usize, off: self.off, keep: keep::fns::<T>() });
    }
    fn reader(self) -> Box<dyn Fn(*const u8) -> T> {
        let off = self.off as usize;
        // SAFETY: `off` is the `offset_of!`-proven `Signal<T>` field of the owner the reader is called with.
        Box::new(move |o| unsafe { (**(o.add(off) as *const Signal<T>)).clone() })
    }
}

impl<'a, T: Clone + 'static, V, S: Clone + PartialEq + 'static, F: Fn(&T) -> S + Copy + 'static> Source<V> for WMap<'a, T, V, F> {
    type In = S;
    fn seal_cells<SS: crate::seal::Sealer>(&self, s: &mut SS) {
        s.word(self.w.off);
    }
    fn cells(&self, out: &mut Vec<SrcCell>) {
        out.push(SrcCell { head: self.w.sig as *const Signal<T> as usize, off: self.w.off, keep: keep::fns::<T>() });
    }
    fn reader(self) -> Box<dyn Fn(*const u8) -> S> {
        let off = self.w.off as usize;
        let f = self.f;
        // SAFETY: as for `Wired`.
        Box::new(move |o| f(unsafe { &**(o.add(off) as *const Signal<T>) }))
    }
}

impl<V, A: Source<V>, B: Source<V>> Source<V> for (A, B) {
    type In = (A::In, B::In);
    fn seal_cells<SS: crate::seal::Sealer>(&self, s: &mut SS) {
        self.0.seal_cells(s);
        self.1.seal_cells(s);
    }
    fn cells(&self, out: &mut Vec<SrcCell>) {
        self.0.cells(out);
        self.1.cells(out);
    }
    fn reader(self) -> Box<dyn Fn(*const u8) -> Self::In> {
        let (a, b) = (self.0.reader(), self.1.reader());
        Box::new(move |o| (a(o), b(o)))
    }
}

impl<V, A: Source<V>, B: Source<V>, C: Source<V>> Source<V> for (A, B, C) {
    type In = (A::In, B::In, C::In);
    fn seal_cells<SS: crate::seal::Sealer>(&self, s: &mut SS) {
        self.0.seal_cells(s);
        self.1.seal_cells(s);
        self.2.seal_cells(s);
    }
    fn cells(&self, out: &mut Vec<SrcCell>) {
        self.0.cells(out);
        self.1.cells(out);
        self.2.cells(out);
    }
    fn reader(self) -> Box<dyn Fn(*const u8) -> Self::In> {
        let (a, b, c) = (self.0.reader(), self.1.reader(), self.2.reader());
        Box::new(move |o| (a(o), b(o), c(o)))
    }
}

/// `(sig!(a), sig!(b)).map(|(a, b)| ..)`: a question input computed from two cells by reference.
pub trait SourceMap<V>: Sized {
    type Refs<'r>;
    fn map<S, F>(self, f: F) -> Mapped<Self, F>
    where
        F: for<'r> Fn(Self::Refs<'r>) -> S + 'static,
    {
        Mapped { s: self, f }
    }
}

pub struct Mapped<S, F> {
    s: S,
    f: F,
}

impl<'a, 'b, TA: Clone + 'static, TB: Clone + 'static, V> SourceMap<V> for (Wired<'a, TA, V>, Wired<'b, TB, V>) {
    type Refs<'r> = (&'r TA, &'r TB);
}

impl<'a, 'b, TA: Clone + 'static, TB: Clone + 'static, V, S: Clone + PartialEq + 'static, F> Source<V> for Mapped<(Wired<'a, TA, V>, Wired<'b, TB, V>), F>
where
    F: for<'r> Fn((&'r TA, &'r TB)) -> S + 'static,
{
    type In = S;
    fn seal_cells<SS: crate::seal::Sealer>(&self, s: &mut SS) {
        s.word(self.s.0.off);
        s.word(self.s.1.off);
    }
    fn cells(&self, out: &mut Vec<SrcCell>) {
        out.push(SrcCell { head: self.s.0.sig as *const Signal<TA> as usize, off: self.s.0.off, keep: keep::fns::<TA>() });
        out.push(SrcCell { head: self.s.1.sig as *const Signal<TB> as usize, off: self.s.1.off, keep: keep::fns::<TB>() });
    }
    fn reader(self) -> Box<dyn Fn(*const u8) -> S> {
        let (oa, ob) = (self.s.0.off as usize, self.s.1.off as usize);
        let f = self.f;
        // SAFETY: as for `Wired`.
        Box::new(move |o| unsafe { f((&**(o.add(oa) as *const Signal<TA>), &**(o.add(ob) as *const Signal<TB>))) })
    }
}

// =============================================================================== what a question runs

/// A question's work for one input: one answer (`ask`) or many (`follow`). The flight is the author's future or
/// stream itself, held inline by the question (no allocation per ask).
pub trait Starts<In>: 'static {
    type Fl: Flight;
    fn start(&self, i: In) -> Self::Fl;
}

pub struct OnceFn<F>(F);
pub struct ManyFn<F>(F);
impl<F> OnceFn<F> {
    pub(crate) fn new(f: F) -> Self {
        OnceFn(f)
    }
}
impl<F> ManyFn<F> {
    pub(crate) fn new(f: F) -> Self {
        ManyFn(f)
    }
}

impl<In, F, Fut> Starts<In> for OnceFn<F>
where
    F: Fn(In) -> Fut + 'static,
    Fut: Future + 'static,
    Fut::Output: 'static,
{
    type Fl = bough_async::Once<Fut>;
    fn start(&self, i: In) -> Self::Fl {
        bough_async::Once((self.0)(i))
    }
}

impl<In, F, St> Starts<In> for ManyFn<F>
where
    F: Fn(In) -> St + 'static,
    St: AsyncIterator + 'static,
    St::Item: 'static,
{
    type Fl = bough_async::Many<St>;
    fn start(&self, i: In) -> Self::Fl {
        bough_async::Many((self.0)(i))
    }
}

// =============================================================================== landings

/// An answer `T` goes into a `Signal<T>` (replacing it) or a `Signal<Option<T>>` (as `Some`).
pub trait Accept<O> {
    fn accept(&mut self, o: O);
}
impl<T> Accept<T> for T {
    #[inline]
    fn accept(&mut self, o: T) {
        *self = o;
    }
}
impl<T> Accept<T> for Option<T> {
    #[inline]
    fn accept(&mut self, o: T) {
        *self = Some(o);
    }
}

/// Where a question's answers go.
pub trait Land<O>: 'static {
    /// # Safety
    /// `owner` is the live owner the question was mounted for.
    unsafe fn land(&self, owner: *mut u8, o: O, first: bool);
    /// Does it write only its subject cell?
    const INTO: bool;
    /// The boundary (`catch`) open where the question mounts (`u32::MAX`: none).
    fn bind(&mut self, _catch: u32) {}
}

/// `ok_into(dst)`: `Ok` answers replace `dst`; an `Err` goes to the nearest `catch` (`catch.rs`).
pub struct OkInto<D, E> {
    off: u32,
    catch: u32,
    _d: PhantomData<fn() -> (D, E)>,
}
impl<D: 'static, T, E: 'static> Land<Result<T, E>> for OkInto<D, E>
where
    D: Accept<T>,
{
    unsafe fn land(&self, owner: *mut u8, o: Result<T, E>, _: bool) {
        match o {
            Ok(v) => {
                // SAFETY: as `IntoCell`.
                let s = unsafe { &mut *(owner.add(self.off as usize) as *mut Signal<D>) };
                D::accept(&mut *s, v);
            }
            Err(e) => {
                debug_assert!(self.catch != u32::MAX, "an `ok_into` question raised an error with no `catch` around it");
                if self.catch != u32::MAX {
                    crate::catch::RAISED.with(|r| r.borrow_mut().push((self.catch, Box::new(e))));
                }
            }
        }
    }
    const INTO: bool = true;
    fn bind(&mut self, catch: u32) {
        self.catch = catch;
    }
}

pub struct IntoCell<D> {
    off: u32,
    _d: PhantomData<fn() -> D>,
}
impl<D: Accept<O> + 'static, O> Land<O> for IntoCell<D> {
    unsafe fn land(&self, owner: *mut u8, o: O, _: bool) {
        // SAFETY: `off` is the `offset_of!`-proven `Signal<D>` field of the owner (the function contract).
        let s = unsafe { &mut *(owner.add(self.off as usize) as *mut Signal<D>) };
        // a write like a handler's: its wires queue, a kept cell keeps its pre-image
        D::accept(&mut *s, o);
    }
    const INTO: bool = true;
}

pub struct CollectCell<C> {
    off: u32,
    _c: PhantomData<fn() -> C>,
}
impl<C: Default + Extend<O> + 'static, O> Land<O> for CollectCell<C> {
    unsafe fn land(&self, owner: *mut u8, o: O, first: bool) {
        // SAFETY: as `IntoCell`.
        let s = unsafe { &mut *(owner.add(self.off as usize) as *mut Signal<C>) };
        if first {
            **s = C::default();
        }
        s.extend(std::iter::once(o));
    }
    const INTO: bool = true;
}

pub struct ThenFn<V, O> {
    f: fn(&mut V, O),
}
impl<V: 'static, O: 'static> Land<O> for ThenFn<V, O> {
    unsafe fn land(&self, owner: *mut u8, o: O, _: bool) {
        // SAFETY: the owner is a `V` (the question was mounted by a `Mounter<V>`).
        (self.f)(unsafe { &mut *(owner as *mut V) }, o)
    }
    const INTO: bool = false;
}

/// A question as the engine sees it (typed; `bough_async::Asking` keeps its state around it).
struct QImpl<In, W, L> {
    read: Box<dyn Fn(*const u8) -> In>,
    w: W,
    land: L,
    owner: usize,
    cls: u32,
}

impl<In: Clone + PartialEq + 'static, W: Starts<In>, L: Land<<W::Fl as Flight>::Out>> bough_async::Ask for QImpl<In, W, L> {
    type In = In;
    type Fl = W::Fl;
    unsafe fn input(&self, owner: *mut u8) -> In {
        (self.read)(owner)
    }
    fn start(&self, i: In) -> W::Fl {
        self.w.start(i)
    }
    unsafe fn land(&self, owner: *mut u8, o: <W::Fl as Flight>::Out, first: bool) {
        touch_owner(self.owner, self.cls);
        // SAFETY: the engine passes the owner the question was mounted for.
        unsafe { self.land.land(owner, o, first) }
    }
}

// =============================================================================== the element

/// A question being built: `ask(src, f)` or `follow(src, f)`, then `.into(dst)`, `.collect_into(dst)` or
/// `.then(handler)`.
pub struct AskB<V, S, W> {
    src: S,
    w: W,
    opts: QOpts,
    _v: PhantomData<fn(&V)>,
}

/// `dst = f(src)`, asked again when `src` changes; `f` returns a future. See `DESIGN.md`.
pub fn ask<V, S: Source<V>, F, Fut>(src: S, f: F) -> AskB<V, S, OnceFn<F>>
where
    F: Fn(S::In) -> Fut + 'static,
    Fut: Future + 'static,
{
    AskB { src, w: OnceFn(f), opts: QOpts { hold: true, every: None, lands_into: true }, _v: PhantomData }
}

/// A question whose answer is a sequence (`f` returns an `AsyncIterator`): the first item publishes it, every
/// later item lands on its own.
pub fn follow<V, S: Source<V>, F, St>(src: S, f: F) -> AskB<V, S, ManyFn<F>>
where
    F: Fn(S::In) -> St + 'static,
    St: AsyncIterator + 'static,
{
    AskB { src, w: ManyFn(f), opts: QOpts { hold: true, every: None, lands_into: true }, _v: PhantomData }
}

impl<V, S, W> AskB<V, S, W> {
    /// Hold nothing while out: the destination lags its input (progress, prefetch, stats over a big list).
    pub fn deferred(mut self) -> Self {
        self.opts.hold = false;
        self
    }
    /// Ask again (quietly, holding nothing) every `d`.
    pub fn every(mut self, d: Duration) -> Self {
        self.opts.every = Some(d);
        self
    }
    /// Answers replace `dst` (a `Signal<T>`, or a `Signal<Option<T>>` taking `Some`).
    pub fn into<'a, D>(self, dst: Wired<'a, D, V>) -> Asked<V, S, W, IntoCell<D>> {
        Asked { src: self.src, w: self.w, opts: self.opts, about: dst.sig as *const Signal<D> as usize, land: IntoCell { off: dst.off, _d: PhantomData }, _v: PhantomData }
    }
    /// `Ok` answers replace `dst`; an `Err` goes to the nearest `catch` around the question (`catch.rs`).
    pub fn ok_into<'a, D, E>(self, dst: Wired<'a, D, V>) -> Asked<V, S, W, OkInto<D, E>> {
        Asked { src: self.src, w: self.w, opts: self.opts, about: dst.sig as *const Signal<D> as usize, land: OkInto { off: dst.off, catch: u32::MAX, _d: PhantomData }, _v: PhantomData }
    }
    /// The first answer for an input replaces `dst` with its default, and every answer extends it.
    pub fn collect_into<'a, C>(self, dst: Wired<'a, C, V>) -> Asked<V, S, W, CollectCell<C>> {
        Asked { src: self.src, w: self.w, opts: self.opts, about: dst.sig as *const Signal<C> as usize, land: CollectCell { off: dst.off, _c: PhantomData }, _v: PhantomData }
    }
    /// Answers are handed to `f` (a write like a handler's). Name the cell it fills with `.about(..)` for
    /// `refresh` and `pending` on that cell.
    pub fn then<O>(mut self, f: fn(&mut V, O)) -> Asked<V, S, W, ThenFn<V, O>> {
        self.opts.lands_into = false;
        Asked { src: self.src, w: self.w, opts: self.opts, about: 0, land: ThenFn { f }, _v: PhantomData }
    }
}

/// A complete question (an element of `view`).
pub struct Asked<V, S, W, L> {
    src: S,
    w: W,
    opts: QOpts,
    about: usize,
    land: L,
    _v: PhantomData<fn(&V)>,
}

impl<V, S, W, L> Asked<V, S, W, L> {
    /// The cell this question fills (for `refresh` and `pending` on it).
    pub fn about<'a, T>(mut self, c: Wired<'a, T, V>) -> Self {
        self.about = c.sig as *const Signal<T> as usize;
        self
    }
    pub fn deferred(mut self) -> Self {
        self.opts.hold = false;
        self
    }
    pub fn every(mut self, d: Duration) -> Self {
        self.opts.every = Some(d);
        self
    }
}

/// Mount a question of owner `owner` (class `cls`): register it with the engine, wire its source cells in the
/// question group of their heads, mark the sources of a holding question `KEEP`, ask it the first time. Shared by
/// the view element (`ask(..)`) and the field form (`Answer`).
pub(crate) fn mount_question(rt: &mut Rt, scope: &mut Scope, owner: *mut u8, cls: u32, cells: Vec<SrcCell>, q: Box<dyn bough_async::Question>, about: usize, opts: QOpts) {
    rt.async_on = true;
    let qid = rt.asks.engine.mount(q, owner, cells.iter().map(|c| c.head).collect(), about, opts);
    let qi = qid.0 as usize;
    if rt.asks.q_cells.len() <= qi {
        rt.asks.q_cells.resize_with(qi + 1, Default::default);
    }
    rt.asks.q_cells[qi] = (cells.iter().map(|c| c.head).collect(), opts.hold, about);
    // the innermost open `catch` boundary takes this question's errors (and retries it)
    if let Some(&b) = rt.asks.catching.last() {
        rt.asks.catches[b as usize].questions.push(qid.0);
        if rt.asks.q_catch.len() <= qi {
            rt.asks.q_catch.resize(qi + 1, u32::MAX);
        }
        rt.asks.q_catch[qi] = b;
    }
    for c in &cells {
        // SAFETY: `c.head` is the head of the `offset_of!`-proven cell at `c.off` of this owner type, and
        // `patch_question` reads nothing of it (the question reads its input through its reader).
        let head = unsafe { &*(c.head as *const Cell<u32>) };
        let w = unsafe { rt.add_wire_in(G_QUESTION, head, c.off, cls, patch_question, qid.0, [0; 16]) };
        scope.wires.push(w);
        rt.register_cell(c.head, owner as usize, cls);
        if opts.hold {
            keep::register(c.head, c.keep, rt.asks.id);
            head.set(head.get() | KEEP);
        }
    }
    if about != 0 {
        rt.register_cell(about, owner as usize, cls);
    }
    scope.asks.push(qid.0);
    if !rt.asks.inert {
        rt.asks.engine.first(qid);
        rt.apply_async();
    }
}

/// # Safety
/// The wire contract: node is a question id of this runtime.
unsafe fn patch_question(rt: &mut Rt, w: &Wire, _sig: *const u8, _owner: *const u8) {
    if rt.asks.inert {
        return;
    }
    rt.asks.engine.written(QId(w.node));
    rt.apply_async();
}

impl<V: 'static, S: Source<V>, W: Starts<S::In>, L: Land<<W::Fl as Flight>::Out>> Element<V> for Asked<V, S, W, L> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let mut cells = Vec::new();
        self.src.cells(&mut cells);
        let owner = m.owner as *mut u8;
        let cls = m.cls;
        let read = self.src.reader();
        let mut opts = self.opts;
        opts.lands_into = L::INTO && opts.lands_into;
        let mut land = self.land;
        land.bind(m.rt.asks.catching.last().copied().unwrap_or(u32::MAX));
        let q = QImpl { read, w: self.w, land, owner: owner as usize, cls };
        mount_question(m.rt, m.scope, owner, cls, cells, Asking::new(q), self.about, opts);
    }
    /// A mounted question is not re-described (its closures and cells are fixed at mount): a re-description
    /// steps over it, so its seal is its cells (the same question in the same place seals equal).
    fn update(self, _m: &mut Mounter<'_, V>) {}
    #[inline(always)]
    fn seal<SS: crate::seal::Sealer>(&self, s: &mut SS) {
        s.word(0x4153_4b21);
        self.src.seal_cells(s);
        s.word(self.about as u32);
    }
}

// =============================================================================== status operands

/// `pending(sig!(x))`: true while `x` is held (what shows it waits for an answer) or a question about `x` is in
/// the air. A status wire: it runs while the cell is held.
pub fn pending<'a, T, V>(w: Wired<'a, T, V>) -> Pending<'a, T, V> {
    Pending { w }
}

pub struct Pending<'a, T, V> {
    w: Wired<'a, T, V>,
}

impl<'a, T, V> Pending<'a, T, V> {
    pub fn map<P, F: Fn(bool) -> P + Copy + 'static>(self, f: F) -> PendMap<'a, T, V, F> {
        PendMap { w: self.w, f }
    }
}

pub struct PendMap<'a, T, V, F> {
    w: Wired<'a, T, V>,
    f: F,
}

fn recipe_of<F: Copy>(f: F) -> [u8; 16] {
    const { assert!(size_of::<F>() <= 16, "closure: at most 16 bytes of captures") };
    let mut r = [0u8; 16];
    // SAFETY: size checked at compile time; unaligned write into bytes.
    unsafe { std::ptr::write_unaligned(r.as_mut_ptr() as *mut F, f) };
    r
}

#[inline]
fn is_pending(head: &Cell<u32>) -> bool {
    head.get() & (HELD | LOADING) != 0
}

/// # Safety
/// The wire contract: `sig` is a headed cell; the recipe holds an `F`.
unsafe fn patch_pend<K: crate::elem::Apply, F: Fn(bool) -> K::P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let h = unsafe { &*(sig as *const Cell<u32>) };
    K::apply(rt, w.node, &f(is_pending(h)));
}

/// # Safety
/// As `patch_pend`.
unsafe fn patch_pend_text<F: Fn(bool) -> String + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let h = unsafe { &*(sig as *const Cell<u32>) };
    crate::elem::set_node_text(rt, w.node, &f(is_pending(h)));
}

impl<'a, T, V, P: 'static, F: Fn(bool) -> P + Copy + 'static> IntoProp<P, V> for PendMap<'a, T, V, F> {
    fn lower<K: crate::elem::Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        // SAFETY: `w` is an `offset_of!`-proven cell of the owner; the patch reads only its head.
        unsafe { m.wire_cell_in(G_STATUS, self.w.sig.head(), self.w.off, patch_pend::<K, F>, node, recipe_of(self.f)) };
        (self.f)(is_pending(self.w.sig.head()))
    }
    fn value(self) -> P {
        (self.f)(is_pending(self.w.sig.head()))
    }
}

impl<'a, T, V, F: Fn(bool) -> String + Copy + 'static> TextSrc<V> for PendMap<'a, T, V, F> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        // SAFETY: as above.
        unsafe { m.wire_cell_in(G_STATUS, self.w.sig.head(), self.w.off, patch_pend_text::<F>, node, recipe_of(self.f)) };
        (self.f)(is_pending(self.w.sig.head()))
    }
    fn value(self) -> String {
        (self.f)(is_pending(self.w.sig.head()))
    }
}
