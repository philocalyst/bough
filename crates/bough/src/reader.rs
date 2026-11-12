//! `Reader<T>`: an async question (fetch high, block low), and the staged publication that commits its answer.
//!
//! **Pending is a property of publication, not a mode of a reader.** A turn computes what it can; a value
//! that waits on something in flight is *staged*, and what is on screen keeps showing the last publication
//! until the staged one is complete.
//!
//! * A **producer** (`sig!(self.query).map_async(sig!(self.results), fetch)`, `try_map_async`, `compute`,
//!   `try_compute`, or the older `produce(src, dst, f)`) is a wire on its source signal. When the source's
//!   value changes (compared with the input frozen for the last request) it takes a new ticket, hands the
//!   work to the executor and joins the source's **cohort**: fetch high, at the write.
//! * **A cohort is one staged publication**: a held source, the producers asking on its behalf, and one
//!   staging slot per flight (`Producer::staged`). The cell keeps the model's intent (a handler reads its own
//!   write); the *presentation* keeps the last publication: the source's head carries `HELD`, so the scan
//!   that runs wires skips every wire of it that is not `latest`, a re-description leaves its wired operands
//!   alone, and a sink mounted during the hold starts from the cohort's retained value (`Wired::shown`).
//!   Block low: nothing but the sinks of the held cells waits; every other cell publishes as usual.
//! * **Completions are stamped.** A flight lands a `Completion` carrying its `RequestStamp` (producer slot,
//!   mount generation, ticket) in a `Send` sink; a `Waker` only schedules another poll. The next turn's
//!   admission accepts a completion only if its stamp is the producer's current one: a superseded request
//!   (`A -> B -> A`), a revoked producer or a reused slot never reaches presentation. Nothing compares values.
//! * **The cohort publishes atomically, or never.** When its last flight lands, every member's value moves
//!   in, their heads and the held source's are queued, and the settle that follows runs their wires: one
//!   turn, one damage epoch, one frame. Cost: the members, with no walk of anything else. A request that is
//!   superseded takes its staged answer with it.
//! * `is_pending(x)` and `latest(x)` are ordinary sources: wires on the reader's second head (`phead`,
//!   offset 4) or on the signal's own head, run on every transition.
//! * **Failures travel with the read** (`Failure`): a domain error (`try_map_async`), a worker fault (the
//!   job panicked or the executor dropped it), a revoked producer. A superseded request is none of these.
//!   A failed answer keeps the last value; `retry` asks again and a successful answer heals.
use crate::elem::{Apply, IntoProp, Mounter, TextSrc};
use crate::rt::*;
use crate::signal::WireHead;
use crate::signal::{Headed, Signal, Wired, HELD, QUEUED};
use std::any::Any;
use std::cell::{Cell, UnsafeCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::marker::PhantomData;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Wake, Waker};

const PENDING: u8 = 1;
const HAS: u8 = 2;
/// The last request failed: the reader keeps its last value and carries the failure until a request succeeds.
const FAULT: u8 = 4;
/// `retry` was asked and no producer has answered it yet.
const RETRY: u8 = 8;

// =============================================================================== failures

/// Why a reader has no fresh answer (design 05 §6). A superseded request is not a failure: it is discarded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailureKind {
    /// The operation's own error (`try_map_async`, `try_compute`, `Request::fallible`): expected, retryable.
    Domain,
    /// The job panicked, or the executor dropped it before it completed. Never forged into a domain error.
    Worker,
    /// The producer's scope was released while the reader lives: nobody asks this question any more.
    Revoked,
}

/// A reader's failure: its kind, a message, and (for a domain error) the error value itself.
pub struct Failure {
    kind: FailureKind,
    msg: String,
    payload: Option<Box<dyn Any + Send>>,
}

impl Failure {
    /// A domain error: the message is its `Display`, the value stays downcastable (`domain::<E>()`).
    pub fn domain<E: std::fmt::Display + Send + 'static>(e: E) -> Failure {
        Failure { kind: FailureKind::Domain, msg: e.to_string(), payload: Some(Box::new(e)) }
    }
    fn worker(msg: String) -> Failure {
        Failure { kind: FailureKind::Worker, msg, payload: None }
    }
    fn revoked() -> Failure {
        Failure { kind: FailureKind::Revoked, msg: "the producer of this reader was released".into(), payload: None }
    }
    pub fn kind(&self) -> FailureKind {
        self.kind
    }
    pub fn message(&self) -> &str {
        &self.msg
    }
    /// The domain error itself, if this is one of type `E`.
    pub fn domain_error<E: 'static>(&self) -> Option<&E> {
        self.payload.as_ref()?.downcast_ref::<E>()
    }
}

impl std::fmt::Debug for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}({})", self.kind, self.msg)
    }
}

type Outcome<T> = Result<T, Failure>;

fn panic_message(p: Box<dyn Any + Send>) -> String {
    match p.downcast::<String>() {
        Ok(s) => *s,
        Err(p) => match p.downcast::<&'static str>() {
            Ok(s) => (*s).to_string(),
            Err(_) => "the job panicked".to_string(),
        },
    }
}

// =============================================================================== the cell

#[repr(C)]
pub struct Reader<T> {
    head: crate::signal::WireHead,
    phead: Cell<u32>,
    state: Cell<u8>,
    value: UnsafeCell<Option<T>>,
    fault: UnsafeCell<Option<Failure>>,
}

impl<T> Default for Reader<T> {
    fn default() -> Self {
        Reader::new()
    }
}

impl<T> Reader<T> {
    pub const fn new() -> Reader<T> {
        Reader { head: WireHead::new(), phead: Cell::new(0), state: Cell::new(0), value: UnsafeCell::new(None), fault: UnsafeCell::new(None) }
    }
    /// The last committed (Ready) value; `None` before first readiness.
    pub fn latest(&self) -> Option<&T> {
        // SAFETY: `value` is written only by the runtime's commit, which holds the owner uniquely.
        unsafe { (*self.value.get()).as_ref() }
    }
    /// The last request's failure message (the reader keeps its last value), until a request succeeds.
    pub fn fault(&self) -> Option<&str> {
        self.failure().map(|f| f.message())
    }
    /// The last request's failure (kind, message, domain error), until a request succeeds.
    pub fn failure(&self) -> Option<&Failure> {
        // SAFETY: written only by the runtime's commit, which holds the owner uniquely.
        unsafe { (*self.fault.get()).as_ref() }
    }
    /// The last answer as an ordinary `Result`: `None` before the first one; `Err` while the last request
    /// failed (`latest()` still has the value shown).
    pub fn result(&self) -> Option<Result<&T, &Failure>> {
        match self.failure() {
            Some(f) => Some(Err(f)),
            None => self.latest().map(Ok),
        }
    }
    /// Does the next visible answer still wait on a request?
    pub fn is_pending(&self) -> bool {
        self.state.get() & PENDING != 0
    }
    /// The value's address (reflection; only while `has_value`).
    pub(crate) fn value_ptr(&self) -> *const T {
        // SAFETY: read-only peek at the committed value; reflection runs outside every turn.
        match unsafe { &*self.value.get() } {
            Some(v) => v as *const T,
            None => std::ptr::null(),
        }
    }
    pub fn has_value(&self) -> bool {
        self.state.get() & HAS != 0
    }
    /// Ask the same question again (after a failure, or to revalidate): the producer that feeds this reader
    /// sends a new request with its current input at this turn's settle. The value and the failure shown stay
    /// until the answer publishes; a successful answer heals the failure. Returns whether a mounted producer
    /// can hear it.
    pub fn retry(&mut self) -> bool {
        let h = self.phead.get();
        if h & HEAD_IDX == 0 {
            return false;
        }
        self.state.set(self.state.get() | RETRY);
        self.phead.set(h | QUEUED);
        true
    }
    /// A reader that is always ready: `fallback` stands in before first readiness and while the last request
    /// failed (design 05 §3.1). The one shape with a total `Deref`.
    pub fn or(self, fallback: T) -> Settled<T> {
        Settled { r: self, fallback }
    }
}

// SAFETY: `#[repr(C)]`, the head first.
unsafe impl<T> Headed for Reader<T> {
    type At<'a, V>
        = RWired<'a, T, V>
    where
        T: 'a;
    unsafe fn at<'a, V>(&'a self, off: u32) -> RWired<'a, T, V> {
        RWired { r: self, off, _v: PhantomData }
    }
}

pub struct RWired<'a, T, V> {
    pub(crate) r: &'a Reader<T>,
    pub(crate) off: u32,
    _v: PhantomData<fn(&V)>,
}
impl<T, V> Clone for RWired<'_, T, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, V> Copy for RWired<'_, T, V> {}

impl<'a, T, V> RWired<'a, T, V> {
    /// A property from the reader's value (`None` before first readiness); patched at each commit.
    pub fn map<P, F: Fn(Option<&T>) -> P + Copy + 'static>(self, f: F) -> RMap<'a, T, V, F> {
        RMap { w: self, f }
    }
    /// The pending flag as an operand (`.pending().map(|p| if p { 0.65 } else { 1.0 })`).
    pub fn pending(self) -> RPending<'a, T, V> {
        RPending { w: self }
    }
}

pub struct RMap<'a, T, V, F> {
    w: RWired<'a, T, V>,
    f: F,
}
pub struct RPending<'a, T, V> {
    w: RWired<'a, T, V>,
}
impl<'a, T, V> RPending<'a, T, V> {
    pub fn map<P, F: Fn(bool) -> P + Copy + 'static>(self, f: F) -> RPMap<'a, T, V, F> {
        RPMap { w: self.w, f }
    }
}
pub struct RPMap<'a, T, V, F> {
    w: RWired<'a, T, V>,
    f: F,
}

/// `latest(sig!(..))`: a signal operand that tracks current intent even while a cohort holds its source.
pub struct Latest<W>(pub W);
pub fn latest<W>(w: W) -> Latest<W> {
    Latest(w)
}

fn recipe<F: Copy>(f: F) -> [u8; 16] {
    const { assert!(size_of::<F>() <= 16, "closure: at most 16 bytes of captures") };
    let mut r = [0u8; 16];
    // SAFETY: size checked at compile time; unaligned write into bytes.
    unsafe { std::ptr::write_unaligned(r.as_mut_ptr() as *mut F, f) };
    r
}
/// # Safety
/// `r` holds an `F` written by `recipe`.
unsafe fn unrecipe<F: Copy>(r: &[u8; 16]) -> F {
    unsafe { std::ptr::read_unaligned(r.as_ptr() as *const F) }
}

unsafe fn patch_rmap<K: Apply, T: 'static, F: Fn(Option<&T>) -> K::P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: wire contract: `sig` is a `Reader<T>`; the recipe holds an `F`.
    let r = unsafe { &*(sig as *const Reader<T>) };
    let f: F = unsafe { unrecipe(&w.recipe) };
    K::apply(rt, w.node, &f(r.latest()));
}
unsafe fn patch_rpmap<K: Apply, T: 'static, F: Fn(bool) -> K::P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: wire contract: `sig` is the `phead` of a `Reader<T>` (offset 4); the recipe holds an `F`.
    let r = unsafe { &*(sig.sub(4) as *const Reader<T>) };
    let f: F = unsafe { unrecipe(&w.recipe) };
    K::apply(rt, w.node, &f(r.is_pending()));
}
unsafe fn patch_rmap_text<T: 'static, F: Fn(Option<&T>) -> String + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: as `patch_rmap`.
    let r = unsafe { &*(sig as *const Reader<T>) };
    let f: F = unsafe { unrecipe(&w.recipe) };
    crate::elem::set_node_text(rt, w.node, &f(r.latest()));
}
unsafe fn patch_rpmap_text<T: 'static, F: Fn(bool) -> String + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: as `patch_rpmap`.
    let r = unsafe { &*(sig.sub(4) as *const Reader<T>) };
    let f: F = unsafe { unrecipe(&w.recipe) };
    crate::elem::set_node_text(rt, w.node, &f(r.is_pending()));
}

impl<'a, T: 'static, V, P: 'static, F: Fn(Option<&T>) -> P + Copy + 'static> IntoProp<P, V> for RMap<'a, T, V, F> {
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        // SAFETY: `self.w` is an `offset_of!`-proven `Reader<T>`; the patch reads one.
        unsafe { m.wire_cell(&self.w.r.head, self.w.off, patch_rmap::<K, T, F>, node, recipe(self.f)) };
        (self.f)(self.w.r.latest())
    }
    fn value(self) -> P {
        (self.f)(self.w.r.latest())
    }
}
impl<'a, T: 'static, V, P: 'static, F: Fn(bool) -> P + Copy + 'static> IntoProp<P, V> for RPMap<'a, T, V, F> {
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        // SAFETY: `phead` sits at `off + 4` (`#[repr(C)]`); the patch reads the reader 4 bytes before it.
        unsafe { m.wire_cell(&self.w.r.phead, self.w.off + 4, patch_rpmap::<K, T, F>, node, recipe(self.f)) };
        (self.f)(self.w.r.is_pending())
    }
    fn value(self) -> P {
        (self.f)(self.w.r.is_pending())
    }
}
impl<'a, T: 'static, V, F: Fn(Option<&T>) -> String + Copy + 'static> TextSrc<V> for RMap<'a, T, V, F> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        // SAFETY: as the `IntoProp` impl.
        unsafe { m.wire_cell(&self.w.r.head, self.w.off, patch_rmap_text::<T, F>, node, recipe(self.f)) };
        (self.f)(self.w.r.latest())
    }
    fn value(self) -> String {
        (self.f)(self.w.r.latest())
    }
}
impl<'a, T: 'static, V, F: Fn(bool) -> String + Copy + 'static> TextSrc<V> for RPMap<'a, T, V, F> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        // SAFETY: as the `IntoProp` impl.
        unsafe { m.wire_cell(&self.w.r.phead, self.w.off + 4, patch_rpmap_text::<T, F>, node, recipe(self.f)) };
        (self.f)(self.w.r.is_pending())
    }
    fn value(self) -> String {
        (self.f)(self.w.r.is_pending())
    }
}

/// `latest(sig!(x).map(f))` as text: the wire runs while the source is held.
impl<'a, T: 'static, V, F: Fn(&T) -> String + Copy + 'static> TextSrc<V> for Latest<crate::signal::WMap<'a, T, V, F>> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        // current intent, not the cohort's retained value
        let s = (self.0.f)(self.0.w.get());
        m.wire(&self.0.w, crate::elem::patch_text_map::<T, F>, node, crate::elem::recipe_of(self.0.f));
        let w = *m.scope.wires.last().unwrap();
        m.rt.make_latest(w);
        s
    }
    fn value(self) -> String {
        (self.0.f)(self.0.w.get())
    }
}
impl<'a, T: 'static, V, P: 'static, F: Fn(&T) -> P + Copy + 'static> IntoProp<P, V> for Latest<crate::signal::WMap<'a, T, V, F>> {
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        let v = (self.0.f)(self.0.w.get());
        m.wire(&self.0.w, crate::elem::patch_map::<K, T, F>, node, crate::elem::recipe_of(self.0.f));
        let w = *m.scope.wires.last().unwrap();
        m.rt.make_latest(w);
        v
    }
    fn value(self) -> P {
        (self.0.f)(self.0.w.get())
    }
}
impl<'a, T: Clone + 'static, V> IntoProp<T, V> for Latest<Wired<'a, T, V>> {
    fn lower<K: Apply<P = T>>(self, m: &mut Mounter<'_, V>, node: u32) -> T {
        m.wire(&self.0, crate::elem::patch_sig::<K, T>, node, [0; 16]);
        let w = *m.scope.wires.last().unwrap();
        m.rt.make_latest(w);
        self.0.get().clone()
    }
    fn value(self) -> T {
        self.0.get().clone()
    }
}

// =============================================================================== is_pending

/// `is_pending(x)`: "does the next visible answer for this cell still wait on a request?" as an operand
/// (design 05 §5). `x` is a reader (`sig!(self.results)`) or the signal whose write began the cohort
/// (`sig!(self.query)`: true while a cohort holds it). Not a writable flag, not app-global.
pub fn is_pending<X: PendingSource>(x: X) -> X::Pending {
    x.pending_operand()
}
pub trait PendingSource {
    type Pending;
    fn pending_operand(self) -> Self::Pending;
}
impl<'a, T, V> PendingSource for RWired<'a, T, V> {
    type Pending = RPending<'a, T, V>;
    fn pending_operand(self) -> RPending<'a, T, V> {
        RPending { w: self }
    }
}
impl<'a, T, V> PendingSource for SWired<'a, T, V> {
    type Pending = RPending<'a, T, V>;
    fn pending_operand(self) -> RPending<'a, T, V> {
        RPending { w: RWired { r: &self.s.r, off: self.off, _v: PhantomData } }
    }
}
impl<'a, T, V> PendingSource for Wired<'a, T, V> {
    type Pending = SigPending<'a, T, V>;
    fn pending_operand(self) -> SigPending<'a, T, V> {
        SigPending { w: self }
    }
}
/// `is_pending(sig!(self.signal))`.
pub struct SigPending<'a, T, V> {
    w: Wired<'a, T, V>,
}
impl<'a, T, V> SigPending<'a, T, V> {
    pub fn map<P, F: Fn(bool) -> P + Copy + 'static>(self, f: F) -> SigPMap<'a, T, V, F> {
        SigPMap { w: self.w, f }
    }
}
pub struct SigPMap<'a, T, V, F> {
    w: Wired<'a, T, V>,
    f: F,
}
unsafe fn patch_sigp<K: Apply, F: Fn(bool) -> K::P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: wire contract: `sig` is a headed cell (its head first); the recipe holds an `F`.
    let held = unsafe { (*(sig as *const Cell<u32>)).get() } & HELD != 0;
    let f: F = unsafe { unrecipe(&w.recipe) };
    K::apply(rt, w.node, &f(held));
}
unsafe fn patch_sigp_text<F: Fn(bool) -> String + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: as `patch_sigp`.
    let held = unsafe { (*(sig as *const Cell<u32>)).get() } & HELD != 0;
    let f: F = unsafe { unrecipe(&w.recipe) };
    crate::elem::set_node_text(rt, w.node, &f(held));
}
impl<'a, T: 'static, V, P: 'static, F: Fn(bool) -> P + Copy + 'static> IntoProp<P, V> for SigPMap<'a, T, V, F> {
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        // a `latest` wire: it must run while the head is held (that is when it has something to say)
        m.wire(&self.w, patch_sigp::<K, F>, node, recipe(self.f));
        let w = *m.scope.wires.last().unwrap();
        m.rt.make_latest(w);
        (self.f)(self.w.is_held())
    }
    fn value(self) -> P {
        (self.f)(self.w.is_held())
    }
}
impl<'a, T: 'static, V, F: Fn(bool) -> String + Copy + 'static> TextSrc<V> for SigPMap<'a, T, V, F> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        m.wire(&self.w, patch_sigp_text::<F>, node, recipe(self.f));
        let w = *m.scope.wires.last().unwrap();
        m.rt.make_latest(w);
        (self.f)(self.w.is_held())
    }
    fn value(self) -> String {
        (self.f)(self.w.is_held())
    }
}

// =============================================================================== Settled

/// `Reader::new().or(fallback)`: a reader that is always ready (design 05 §3.1). It dereferences to the
/// last answer, or to `fallback` before first readiness and while the last request failed; opting in is
/// opting out of ever seeing the pending and failure verdicts (`reader()` still has them).
#[repr(C)]
pub struct Settled<T> {
    r: Reader<T>,
    fallback: T,
}
impl<T> Settled<T> {
    /// Is the fallback standing in (no answer yet, or the last request failed)?
    pub fn is_fallback(&self) -> bool {
        self.r.state.get() & (HAS | FAULT) != HAS
    }
    /// The reader underneath (pending, failure, `retry`).
    pub fn reader(&self) -> &Reader<T> {
        &self.r
    }
    pub fn reader_mut(&mut self) -> &mut Reader<T> {
        &mut self.r
    }
}
impl<T> Deref for Settled<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        match self.r.latest() {
            Some(v) if self.r.state.get() & FAULT == 0 => v,
            _ => &self.fallback,
        }
    }
}
// SAFETY: `#[repr(C)]`, the reader first, whose head is first.
unsafe impl<T> Headed for Settled<T> {
    type At<'a, V>
        = SWired<'a, T, V>
    where
        T: 'a;
    unsafe fn at<'a, V>(&'a self, off: u32) -> SWired<'a, T, V> {
        SWired { s: self, off, _v: PhantomData }
    }
}
/// `sig!(self.settled)`.
pub struct SWired<'a, T, V> {
    s: &'a Settled<T>,
    off: u32,
    _v: PhantomData<fn(&V)>,
}
impl<T, V> Clone for SWired<'_, T, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, V> Copy for SWired<'_, T, V> {}
impl<'a, T, V> SWired<'a, T, V> {
    /// A property from the settled value: plain `&T`, never an `Option`.
    pub fn map<P, F: Fn(&T) -> P + Copy + 'static>(self, f: F) -> SMap<'a, T, V, F> {
        SMap { w: self, f }
    }
}
pub struct SMap<'a, T, V, F> {
    w: SWired<'a, T, V>,
    f: F,
}
unsafe fn patch_smap<K: Apply, T: 'static, F: Fn(&T) -> K::P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: wire contract: `sig` is a `Settled<T>`; the recipe holds an `F`.
    let s = unsafe { &*(sig as *const Settled<T>) };
    let f: F = unsafe { unrecipe(&w.recipe) };
    K::apply(rt, w.node, &f(s));
}
unsafe fn patch_smap_text<T: 'static, F: Fn(&T) -> String + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: as `patch_smap`.
    let s = unsafe { &*(sig as *const Settled<T>) };
    let f: F = unsafe { unrecipe(&w.recipe) };
    crate::elem::set_node_text(rt, w.node, &f(s));
}
impl<'a, T: 'static, V, P: 'static, F: Fn(&T) -> P + Copy + 'static> IntoProp<P, V> for SMap<'a, T, V, F> {
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        // SAFETY: `self.w` is an `offset_of!`-proven `Settled<T>` (its reader's head first); the patch reads one.
        // The value changes only at a commit, which queues this head.
        unsafe { m.wire_cell(&self.w.s.r.head, self.w.off, patch_smap::<K, T, F>, node, recipe(self.f)) };
        (self.f)(self.w.s)
    }
    fn value(self) -> P {
        (self.f)(self.w.s)
    }
}
impl<'a, T: 'static, V, F: Fn(&T) -> String + Copy + 'static> TextSrc<V> for SMap<'a, T, V, F> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        // SAFETY: as the `IntoProp` impl.
        unsafe { m.wire_cell(&self.w.s.r.head, self.w.off, patch_smap_text::<T, F>, node, recipe(self.f)) };
        (self.f)(self.w.s)
    }
    fn value(self) -> String {
        (self.f)(self.w.s)
    }
}

/// Where a producer's answers land: `sig!(self.reader)` or `sig!(self.settled)`.
pub trait Dest<'a, T, V>: Copy {
    #[doc(hidden)]
    fn dest(self) -> RWired<'a, T, V>;
}
impl<'a, T, V> Dest<'a, T, V> for RWired<'a, T, V> {
    fn dest(self) -> RWired<'a, T, V> {
        self
    }
}
impl<'a, T, V> Dest<'a, T, V> for SWired<'a, T, V> {
    fn dest(self) -> RWired<'a, T, V> {
        RWired { r: &self.s.r, off: self.off, _v: PhantomData }
    }
}

// =============================================================================== executor

/// Work for the executor: a future, already wrapped so that all it does when it finishes is land a stamped
/// completion in the runtime's sink.
pub type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
/// CPU work for the pool, wrapped the same way.
pub type BoxJob = Box<dyn FnOnce() + Send + 'static>;

/// Where producers' work runs. Chosen once (`Config::executor`, or `Executor::install` for the process);
/// no handle is threaded through application code and no task value comes back (design 05 §1, 00 §6).
///
/// * `manual()`: the work waits until `App::run_jobs` / `Rt::run_job_at` polls it (deterministic: tests,
///   benches). The default of a bare `App`.
/// * `threads()`: bough's own minimal executor: each request on its own thread (a future is polled there,
///   parked between wakes).
/// * `custom(io)`: an adapter over a real runtime: `io` receives each boxed future
///   (`Executor::custom(move |f| { handle.spawn(f); })` for Tokio, `|f| smol::spawn(f).detach()` for smol);
///   `.cpu(pool)` sends `compute` jobs to a pool instead of bough's threads.
#[derive(Clone)]
pub struct Executor(ExecKind);

#[derive(Clone)]
enum ExecKind {
    Manual,
    Threads,
    Custom { io: Arc<dyn Fn(BoxFuture) + Send + Sync>, cpu: Option<Arc<dyn Fn(BoxJob) + Send + Sync>> },
}

static INSTALLED: OnceLock<Executor> = OnceLock::new();

impl Executor {
    pub fn manual() -> Executor {
        Executor(ExecKind::Manual)
    }
    pub fn threads() -> Executor {
        Executor(ExecKind::Threads)
    }
    pub fn custom(io: impl Fn(BoxFuture) + Send + Sync + 'static) -> Executor {
        Executor(ExecKind::Custom { io: Arc::new(io), cpu: None })
    }
    /// The pool `compute` / `try_compute` jobs go to (default: bough's threads).
    pub fn cpu(self, pool: impl Fn(BoxJob) + Send + Sync + 'static) -> Executor {
        match self.0 {
            ExecKind::Custom { io, .. } => Executor(ExecKind::Custom { io, cpu: Some(Arc::new(pool)) }),
            k => Executor(k),
        }
    }
    /// The executor of every `App` created after this call that does not name its own (`Config::executor`).
    /// Once per process: returns whether this call installed it.
    pub fn install(self) -> bool {
        INSTALLED.set(self).is_ok()
    }
    pub(crate) fn installed() -> Option<Executor> {
        INSTALLED.get().cloned()
    }
}

/// The identity of one request (design 05 §7): which producer slot asked, which mount of that slot, and
/// which of its requests. A producer freezes exactly one input per ticket, so the ticket is the version of
/// its frozen input: a completion is admitted only if all three still name the producer's current request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RequestStamp {
    pub producer: u32,
    pub generation: u32,
    pub ticket: u32,
}

/// What a finished flight puts in the sink: its stamp and its owned outcome. Inert and `Send`: enqueueing
/// one runs no application code and touches no cell.
struct Completion {
    stamp: RequestStamp,
    outcome: Box<dyn Any + Send>,
}

/// The runtime's ingress for completions: any thread pushes, the UI thread drains at the start of a turn.
#[derive(Default)]
struct Sink {
    done: Mutex<Vec<Completion>>,
    /// Something was pushed since the last drain (the turn's admission takes no lock otherwise).
    has: AtomicBool,
    /// The host's wake: ask the UI thread for a turn.
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
impl Sink {
    fn push(&self, c: Completion) {
        self.done.lock().unwrap().push(c);
        self.has.store(true, Ordering::Release);
        let w = self.wake.lock().unwrap().clone();
        if let Some(w) = w {
            w();
        }
    }
}

/// One request in the air, shared by the side that does the work and the side that owns it. The owner's
/// half is a `FlightHandle` in the producer: **dropping it is the cancellation**. The work not yet finished
/// (the user's future, a queued job, a stream's keep-alive) is dropped there and then, on the UI thread,
/// whatever the executor does with its own half afterwards; nothing more can land.
struct Shared {
    stamp: RequestStamp,
    sink: Arc<Sink>,
    /// False once the owner let go (superseded, revoked) : every later landing is refused at the source.
    open: AtomicBool,
    /// A one-shot flight that has not landed its answer yet (a lost one is a worker fault).
    owes: AtomicBool,
    work: Mutex<Option<Held>>,
    /// The typed outcome of a flight lost without an answer.
    lost: fn(Failure) -> Box<dyn Any + Send>,
}
enum Held {
    Future(Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>>),
    Job(Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>),
    /// A stream's keep-alive: whatever its `open` function returned, dropped when the stream closes.
    Keep(#[allow(dead_code)] Box<dyn Any + Send>),
}
impl Shared {
    fn land(&self, outcome: Box<dyn Any + Send>) -> bool {
        if !self.open.load(Ordering::Acquire) {
            return false;
        }
        self.owes.store(false, Ordering::Release);
        self.sink.push(Completion { stamp: self.stamp, outcome });
        true
    }
    fn fail(&self, msg: String) {
        self.land((self.lost)(Failure::worker(msg)));
    }
}
fn lost<T: Send + 'static>(f: Failure) -> Box<dyn Any + Send> {
    Box::new(Err::<T, Failure>(f))
}

/// The owner's half of a request. Drop = cancel.
struct FlightHandle(Arc<Shared>);
impl Drop for FlightHandle {
    fn drop(&mut self) {
        self.0.open.store(false, Ordering::Release);
        // the unfinished work goes now (a poll in progress on another thread finishes first: the lock)
        let work = self.0.work.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(work);
    }
}

/// The executor's half of an async request: polls the user's future and lands its outcome. A wake only
/// schedules another poll (the executor's business).
struct Flight(Arc<Shared>);
impl Future for Flight {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let sh = &self.0;
        let mut slot = sh.work.lock().unwrap_or_else(|e| e.into_inner());
        let Some(Held::Future(fut)) = slot.as_mut() else { return Poll::Ready(()) };
        let fut = fut.as_mut();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fut.poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(o)) => {
                *slot = None;
                drop(slot);
                sh.land(o);
                Poll::Ready(())
            }
            Err(p) => {
                *slot = None;
                drop(slot);
                sh.fail(panic_message(p));
                Poll::Ready(())
            }
        }
    }
}
impl Drop for Flight {
    fn drop(&mut self) {
        // dropped by the executor without an answer (it shut down, its thread died): a worker fault
        if self.0.owes.load(Ordering::Acquire) && self.0.open.load(Ordering::Acquire) {
            self.0.fail("the executor dropped the request before it completed".into());
        }
    }
}
/// The executor's half of a CPU request.
fn cpu_job(sh: Arc<Shared>) -> BoxJob {
    let fl = Flight(sh);
    Box::new(move || {
        let job = fl.0.work.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(Held::Job(job)) = job {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)) {
                Ok(o) => fl.0.land(o),
                Err(p) => {
                    fl.0.fail(panic_message(p));
                    true
                }
            };
        }
    })
}

/// The sending end of a stream of answers (`subscribe`): `Send`, clonable, usable from any thread. Each
/// value sent is one publication of the reader. Once the input that opened the stream is superseded or its
/// producer released, `send` answers `Closed` and nothing more is delivered.
pub struct Inlet<T> {
    sh: Arc<Shared>,
    _t: PhantomData<fn(T)>,
}
impl<T> Clone for Inlet<T> {
    fn clone(&self) -> Self {
        Inlet { sh: self.sh.clone(), _t: PhantomData }
    }
}
/// The stream this inlet fed is over (its input changed, or its owner is gone).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Closed;
impl<T: Send + 'static> Inlet<T> {
    pub fn send(&self, v: T) -> Result<(), Closed> {
        if self.sh.land(Box::new(Ok::<T, Failure>(v))) { Ok(()) } else { Err(Closed) }
    }
    /// Deliver a domain error instead of a value (the reader keeps its last value and carries the failure).
    pub fn fail<E: std::fmt::Display + Send + 'static>(&self, e: E) -> Result<(), Closed> {
        if self.sh.land(Box::new(Err::<T, Failure>(Failure::domain(e)))) { Ok(()) } else { Err(Closed) }
    }
    pub fn is_closed(&self) -> bool {
        !self.sh.open.load(Ordering::Acquire)
    }
}

/// What a producer makes of one frozen input: the work the request owns until it lands.
pub struct Work(WorkKind);
enum WorkKind {
    Io(Held),
    Cpu(Held),
    Stream(Held),
}

const KIND_IO: u8 = 0;
const KIND_CPU: u8 = 1;
const KIND_STREAM: u8 = 2;

struct Runnable(AtomicBool);
impl Wake for Runnable {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}
/// A task of the manual executor: polled only when new or woken since its last poll.
struct Task {
    fut: BoxFuture,
    runnable: Arc<Runnable>,
}

struct Unpark(std::thread::Thread);
impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
/// bough's minimal driver for one future on the calling thread.
fn block_on(mut fut: BoxFuture) {
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    while fut.as_mut().poll(&mut cx).is_pending() {
        std::thread::park();
    }
}

// =============================================================================== producers, cohorts

/// One mounted producer: a slot in the runtime's slab, named by completions through its `RequestStamp`.
pub(crate) struct Producer {
    /// Bumped when the slot is revoked: the stamp's mount generation.
    generation: u32,
    /// The current request's ticket (monotonic while the slot lives).
    ticket: u32,
    live: bool,
    /// The request of `ticket` has not landed.
    flying: bool,
    /// Cohort slot + 1 while this producer has a request in a staged publication.
    cohort: u32,
    /// Its index in that cohort's members.
    slot: u32,
    cls: u32,
    owner: usize,
    reader: usize,
    src_off: u32,
    dst_off: u32,
    /// The input frozen for the current request (the equality cut-off of the next write).
    last: Option<Box<dyn Any>>,
    /// The current request, owned: replacing or dropping it cancels the flight (`FlightHandle`).
    flight: Option<FlightHandle>,
    ask: [u8; 16],
    fire: FireFn,
    commit: unsafe fn(usize, Box<dyn Any + Send>),
    revoked: unsafe fn(usize),
    /// The sabotaged admission's evidence: every frozen input by ticket, and how to compare two.
    #[cfg(feature = "sabotage_alias")]
    history: Vec<Box<dyn Any>>,
    #[cfg(feature = "sabotage_alias")]
    same: fn(&dyn Any, &dyn Any) -> bool,
}

/// `(runtime, producer, source cell, owner, force)`.
type FireFn = unsafe fn(&mut Rt, u32, *const u8, *const u8, bool);

/// One staged publication: a held source and the producers asking on its behalf.
struct Cohort {
    live: bool,
    /// The held source's head.
    src: usize,
    owner: usize,
    cls: u32,
    /// The source's value in the last publication: what its sinks show, and what a sink mounted during the
    /// hold starts from.
    retained: Option<Box<dyn Any>>,
    members: Vec<u32>,
    /// The staged values, owned by the publication: member `i`'s accepted outcome, until all have one.
    staged: Vec<Option<Box<dyn Any + Send>>>,
    /// Members whose current request has not landed.
    waiting: u32,
}

pub struct Async {
    exec: Executor,
    manual: VecDeque<Task>,
    sink: Arc<Sink>,
    pub(crate) producers: Vec<Producer>,
    free: Vec<u32>,
    cohorts: Vec<Cohort>,
    cohort_free: Vec<u32>,
    /// Held source head -> its cohort.
    by_src: HashMap<usize, u32>,
    /// Cohorts whose every flight has landed, to publish.
    ready: Vec<u32>,
    /// Producers of non-root owners that queued a head of their owner outside its settle.
    resettle: Vec<u32>,
    /// Async work is waiting for the settle loop: a head queued behind the scan, a cohort to publish, an
    /// owner to settle. The one flag a turn without async work tests.
    pub(crate) rescan: bool,
    /// Readers committed since the last `changed` (for `Changed::reader`).
    pub committed: Vec<usize>,
    /// Boundaries (`boundary`): their branch clips.
    pub(crate) bounds: Vec<Bound>,
    pub stats: AsyncStats,
}
#[derive(Default, Clone, Copy, Debug)]
pub struct AsyncStats {
    /// Requests sent.
    pub requests: u64,
    /// Completions whose stamp was current.
    pub accepted: u64,
    /// Completions discarded: superseded, revoked, or a reused slot.
    pub stale: u64,
    /// Cohorts published.
    pub commits: u64,
    /// Cohorts that never published: every member revoked.
    pub dissolved: u64,
}

impl Default for Async {
    fn default() -> Self {
        Async {
            exec: Executor::installed().unwrap_or_else(Executor::manual),
            manual: VecDeque::new(),
            sink: Arc::default(),
            producers: Vec::new(),
            free: Vec::new(),
            cohorts: Vec::new(),
            cohort_free: Vec::new(),
            by_src: HashMap::new(),
            ready: Vec::new(),
            resettle: Vec::new(),
            rescan: false,
            committed: Vec::new(),
            bounds: Vec::new(),
            stats: AsyncStats::default(),
        }
    }
}

impl Async {
    /// Choose the executor (before the first request: `App::new` does it from `Config::executor`).
    pub fn set_executor(&mut self, e: Executor) {
        self.exec = e;
    }
    /// The host's wake: called (on any thread) whenever a completion lands, to ask the UI thread for a turn.
    pub fn set_waker(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        *self.sink.wake.lock().unwrap() = Some(Arc::new(wake));
    }
    /// Cohorts currently staged (held sources).
    pub fn cohorts_pending(&self) -> usize {
        self.by_src.len()
    }
    /// Producers currently mounted.
    pub fn producers_live(&self) -> usize {
        self.producers.len() - self.free.len()
    }
    /// The value a held source's sinks show (its value in the last publication).
    #[cold]
    pub(crate) fn retained(&self, head: usize) -> Option<&dyn Any> {
        let c = *self.by_src.get(&head)?;
        self.cohorts[c as usize].retained.as_deref()
    }
    fn spawn(&mut self, sh: Arc<Shared>, kind: u8) {
        match (&self.exec.0, kind) {
            (_, KIND_STREAM) => {}
            (ExecKind::Manual, k) => {
                let fut: BoxFuture = if k == KIND_IO {
                    Box::pin(Flight(sh))
                } else {
                    let j = cpu_job(sh);
                    Box::pin(async move { j() })
                };
                self.manual.push_back(Task { fut, runnable: Arc::new(Runnable(AtomicBool::new(true))) });
            }
            (ExecKind::Custom { io, .. }, KIND_IO) => io(Box::pin(Flight(sh))),
            (ExecKind::Custom { cpu: Some(pool), .. }, KIND_CPU) => pool(cpu_job(sh)),
            (_, KIND_IO) => {
                std::thread::spawn(move || block_on(Box::pin(Flight(sh))));
            }
            (_, _) => {
                std::thread::spawn(cpu_job(sh));
            }
        }
    }
    fn new_cohort(&mut self, c: Cohort) -> u32 {
        match self.cohort_free.pop() {
            Some(i) => {
                self.cohorts[i as usize] = c;
                i
            }
            None => {
                self.cohorts.push(c);
                (self.cohorts.len() - 1) as u32
            }
        }
    }
}

/// # Safety
/// `addr` is a live `Reader<T>` whose owner the runtime holds uniquely; `out` an `Outcome<T>`.
unsafe fn commit<T: 'static>(addr: usize, out: Box<dyn Any + Send>) {
    let r = unsafe { &*(addr as *const Reader<T>) };
    let out = out.downcast::<Outcome<T>>().expect("a completion of the reader's type");
    let failed = match *out {
        Ok(v) => {
            // SAFETY: the function contract.
            unsafe {
                *r.value.get() = Some(v);
                *r.fault.get() = None;
            }
            false
        }
        // a failure keeps the last value presented (keep-stale; `boundary` shows its `failed` branch)
        Err(e) => {
            // SAFETY: the function contract.
            unsafe { *r.fault.get() = Some(e) };
            true
        }
    };
    let has = r.state.get() & HAS != 0 || !failed;
    r.state.set(if has { HAS } else { 0 } | if failed { FAULT } else { 0 } | r.state.get() & RETRY);
    queue_both(r);
}
/// # Safety
/// `addr` is a live `Reader<T>` whose owner the runtime holds uniquely.
unsafe fn revoked<T: 'static>(addr: usize) {
    let r = unsafe { &*(addr as *const Reader<T>) };
    // SAFETY: the function contract.
    unsafe { *r.fault.get() = Some(Failure::revoked()) };
    r.state.set(r.state.get() & HAS | FAULT);
    queue_both(r);
}
fn queue_both<T>(r: &Reader<T>) {
    for h in [&r.head.0, &r.phead] {
        let x = h.get();
        if x & HEAD_IDX != 0 {
            h.set(x | QUEUED);
        }
    }
}

impl Rt {
    /// Mark wire `w` `latest`: it runs while its source is held (`run_head`).
    pub(crate) fn make_latest(&mut self, w: u32) {
        self.wires[w as usize].latest = true;
    }

    /// Admit completions (the start of a turn): a completion whose stamp is not its producer's current
    /// request is dropped; an accepted outcome is staged in its flight's slot; a cohort whose last flight
    /// landed publishes.
    pub(crate) fn admit(&mut self) {
        if self.asy.producers.is_empty() {
            return; // no producer was ever mounted: nothing can complete
        }
        self.asy.committed.clear();
        if !self.asy.sink.has.swap(false, Ordering::AcqRel) {
            return;
        }
        let done: Vec<Completion> = std::mem::take(&mut *self.asy.sink.done.lock().unwrap());
        for c in done {
            let s = c.stamp;
            let Some(p) = self.asy.producers.get_mut(s.producer as usize) else {
                self.asy.stats.stale += 1;
                continue;
            };
            let mine = p.live && p.generation == s.generation;
            let ok = mine && p.ticket == s.ticket;
            // the deliberately wrong admission (the alias test must catch it): any completion computed from
            // an input equal to the current one is taken, whichever request it answered
            #[cfg(feature = "sabotage_alias")]
            let ok = ok || mine && matches!((p.history.get(s.ticket as usize), p.last.as_ref()), (Some(a), Some(b)) if (p.same)(&**a, &**b));
            if !ok {
                self.asy.stats.stale += 1;
                continue;
            }
            self.asy.stats.accepted += 1;
            if p.cohort == 0 {
                // a further value of a stream whose publication is not staged: a publication of its own
                let (reader, owner, cls, commit) = (p.reader, p.owner, p.cls, p.commit);
                // SAFETY: a live producer's reader is a field of its live owner, of the type `commit` was
                // monomorphised for, and the outcome is one its own request landed.
                unsafe { commit(reader, c.outcome) };
                self.asy.committed.push(reader);
                self.asy.stats.commits += 1;
                if cls != 0 {
                    // SAFETY: as above: the owner is live, of class `cls`.
                    unsafe { self.settle_cls(owner as *const u8, cls) };
                }
                continue;
            }
            let ci = p.cohort - 1;
            let co = &mut self.asy.cohorts[ci as usize];
            // latest wins inside one staged publication
            co.staged[p.slot as usize] = Some(c.outcome);
            if p.flying {
                p.flying = false;
                co.waiting -= 1;
                if co.waiting == 0 {
                    self.asy.ready.push(ci);
                }
            }
        }
        self.publish_ready();
    }

    /// Publish every cohort whose flights have all landed: each member's staged outcome moves into its
    /// reader, the readers' heads and the held source's are queued. O(members).
    fn publish_ready(&mut self) {
        while let Some(ci) = self.asy.ready.pop() {
            let c = &mut self.asy.cohorts[ci as usize];
            // a slot dissolved or re-asked since it was listed is not ready
            if !c.live || c.waiting != 0 || c.members.is_empty() {
                continue;
            }
            c.live = false;
            c.retained = None;
            let (src, owner, cls) = (c.src, c.owner, c.cls);
            let members = std::mem::take(&mut c.members);
            let mut staged = std::mem::take(&mut c.staged);
            self.asy.by_src.remove(&src);
            for (&pi, out) in members.iter().zip(staged.iter_mut()) {
                let p = &mut self.asy.producers[pi as usize];
                p.cohort = 0;
                let out = out.take().expect("a landed member's outcome");
                // SAFETY: a member is a live producer: its reader is a field of its live owner, of the type its
                // commit was monomorphised for, and the outcome is the one its own flight landed.
                unsafe { (p.commit)(p.reader, out) };
                self.asy.committed.push(p.reader);
            }
            // SAFETY: the cohort's source is a headed cell of the same live owner as its members.
            let head = unsafe { &*(src as *const Cell<u32>) };
            let x = head.get() & !HELD;
            head.set(if x & HEAD_IDX != 0 { x | QUEUED } else { x });
            self.asy.stats.commits += 1;
            // hand the member list's allocation back to the slot
            let c = &mut self.asy.cohorts[ci as usize];
            c.members = members;
            c.members.clear();
            c.staged = staged;
            c.staged.clear();
            self.asy.cohort_free.push(ci);
            if cls != 0 {
                // SAFETY: as above: the owner is live, of class `cls`.
                unsafe { self.settle_cls(owner as *const u8, cls) };
            } else {
                // the root: the turn's settle runs the queued heads (again, if it is already past them)
                self.asy.rescan = true;
            }
        }
    }

    /// The async work a settle scan left behind (`Async::rescan`): publish cohorts made ready during the
    /// scan and settle the non-root owners whose heads a producer queued.
    ///
    /// # Safety
    /// As `settle`.
    #[cold]
    pub(crate) unsafe fn settle_async(&mut self) {
        self.asy.rescan = false;
        self.publish_ready();
        while let Some(pi) = self.asy.resettle.pop() {
            let p = &self.asy.producers[pi as usize];
            if p.live && p.cls != 0 {
                let (o, cls) = (p.owner, p.cls);
                // SAFETY: a live producer's owner is live, of the producer's class.
                unsafe { self.settle_cls(o as *const u8, cls) };
            }
        }
    }

    /// A producer's scope was released (L5): it asks nothing more, its flight in the air is cancelled (its
    /// completion, if it still lands, names a dead generation), and its reader leaves its cohort.
    pub(crate) fn revoke_producer(&mut self, pi: u32) {
        let p = &mut self.asy.producers[pi as usize];
        if !p.live {
            return;
        }
        p.live = false;
        p.generation = p.generation.wrapping_add(1);
        // the flight in the air is dropped here: ownership is the cancellation
        p.flight = None;
        p.ticket = 0;
        p.last = None;
        let (ci, slot, flying, root, reader, revoked) = (p.cohort, p.slot, p.flying, p.cls == 0, p.reader, p.revoked);
        p.cohort = 0;
        p.flying = false;
        self.asy.free.push(pi);
        if ci != 0 {
            let ci = ci - 1;
            let c = &mut self.asy.cohorts[ci as usize];
            c.members.swap_remove(slot as usize);
            c.staged.swap_remove(slot as usize);
            if let Some(&moved) = c.members.get(slot as usize) {
                self.asy.producers[moved as usize].slot = slot;
            }
            if flying {
                c.waiting -= 1;
            }
            if c.members.is_empty() {
                // nobody waits for this publication any more: it never happens
                c.live = false;
                c.retained = None;
                c.staged.clear();
                let src = c.src;
                self.asy.by_src.remove(&src);
                self.asy.cohort_free.push(ci);
                self.asy.stats.dissolved += 1;
                if root {
                    // SAFETY: the root owner outlives every scope: its source cell is live. Its sinks catch up
                    // with the model at the next settle.
                    let head = unsafe { &*(src as *const Cell<u32>) };
                    let x = head.get() & !HELD;
                    head.set(if x & HEAD_IDX != 0 { x | QUEUED } else { x });
                    self.asy.rescan = true;
                }
            } else if c.waiting == 0 {
                self.asy.ready.push(ci);
                self.asy.rescan = true;
            }
        }
        if root && reader != 0 {
            // SAFETY: the root owner outlives every scope, so the reader (its field) is live; `revoked` was
            // monomorphised for its type. A row's or entity's reader may already be gone with its owner, and
            // is not touched.
            unsafe { revoked(reader) };
            self.asy.rescan = true;
        }
    }

    /// Which branch boundary `i` shows (0 loading, 1 failed, 2 child).
    pub fn boundary_shown(&self, i: usize) -> u8 {
        self.asy.bounds[i].shown
    }

    /// Poll up to `n` runnable tasks of the manual executor, front first (a task is runnable when new or
    /// woken since its last poll). Returns how many were polled.
    pub fn run_jobs(&mut self, n: usize) -> usize {
        let (mut k, mut i) = (0, 0);
        while k < n && i < self.asy.manual.len() {
            if !self.asy.manual[i].runnable.0.load(Ordering::Acquire) {
                i += 1;
                continue;
            }
            if !self.poll_task(i) {
                i += 1;
            }
            k += 1;
        }
        k
    }
    /// Poll the queued task at index `i` (out-of-order completion, tests). Returns whether there was one.
    pub fn run_job_at(&mut self, i: usize) -> bool {
        if i >= self.asy.manual.len() {
            return false;
        }
        self.poll_task(i);
        true
    }
    /// Poll task `i` once; a finished task leaves the queue (returns true).
    fn poll_task(&mut self, i: usize) -> bool {
        let t = &mut self.asy.manual[i];
        t.runnable.0.store(false, Ordering::Release);
        let waker = Waker::from(t.runnable.clone());
        let mut cx = Context::from_waker(&waker);
        if t.fut.as_mut().poll(&mut cx).is_ready() {
            drop(self.asy.manual.remove(i));
            true
        } else {
            false
        }
    }
    /// Tasks the manual executor holds (finished ones have left).
    pub fn jobs_queued(&self) -> usize {
        self.asy.manual.len()
    }
    /// Drop every task the manual executor holds without completing it (an executor shutting down).
    pub fn drop_jobs(&mut self) {
        self.asy.manual.clear();
    }
}

/// How a producer turns one frozen input into work (`map_async`, `compute`, `subscribe`, ... each have one).
pub trait Ask<S, T>: Copy + 'static {
    #[doc(hidden)]
    fn work(self, input: &S, inlet: Inlet<T>) -> Work;
}

/// # Safety
/// `src` is the live `Signal<S>` of the live owner `owner` that producer `pi` was mounted for; the producer's
/// `dst_off` is an `offset_of!`-proven `Reader<T>` of that owner; its `ask` bytes hold an `A`.
unsafe fn fire<S: PartialEq + Clone + 'static, T: Send + 'static, A: Ask<S, T>>(rt: &mut Rt, pi: u32, src: *const u8, owner: *const u8, force: bool) {
    // SAFETY: the function contract.
    let s = unsafe { &*(src as *const Signal<S>) };
    let asy = &mut rt.asy;
    let p = &mut asy.producers[pi as usize];
    if !p.live {
        return;
    }
    let same = p.last.as_ref().and_then(|b| b.downcast_ref::<S>()) == Some(&**s);
    if same && !force {
        return;
    }
    // a new request: a new ticket; the previous one (in the air or landed and staged) is superseded
    let prev = p.last.replace(Box::new((**s).clone()));
    p.ticket = p.ticket.wrapping_add(1);
    // the superseded request is dropped: its future, its queued job or its stream's keep-alive goes now
    p.flight = None;
    p.owner = owner as usize;
    p.reader = owner as usize + p.dst_off as usize;
    #[cfg(feature = "sabotage_alias")]
    {
        p.history.resize_with(p.ticket as usize, || Box::new(()));
        p.history.push(Box::new((**s).clone()));
    }
    let (reader, cls, stamp) = (p.reader, p.cls, RequestStamp { producer: pi, generation: p.generation, ticket: p.ticket });
    let ask: A = unsafe { unrecipe(&p.ask) };
    // the source's cohort: this publication, staged until its last flight lands
    let head = src as usize;
    let ci = match asy.by_src.get(&head) {
        Some(&ci) => ci,
        None => {
            // what the source's sinks show now: its value at the last request, which the last publication
            // published (a first request: its current value)
            let retained: Box<dyn Any> = match prev {
                Some(prev) if !same => prev,
                _ => Box::new((**s).clone()),
            };
            let ci = asy.new_cohort(Cohort { live: true, src: head, owner: owner as usize, cls, retained: Some(retained), members: Vec::new(), staged: Vec::new(), waiting: 0 });
            asy.by_src.insert(head, ci);
            s.head().set(s.head().get() | HELD);
            ci
        }
    };
    let c = &mut asy.cohorts[ci as usize];
    let p = &mut asy.producers[pi as usize];
    if p.cohort == 0 {
        p.cohort = ci + 1;
        p.slot = c.members.len() as u32;
        c.members.push(pi);
        c.staged.push(None);
        p.flying = true;
        c.waiting += 1;
    } else {
        // superseded inside the same staged publication: its staged answer, if it had landed, goes with it
        c.staged[p.slot as usize] = None;
        if !p.flying {
            p.flying = true;
            c.waiting += 1;
        }
    }
    // SAFETY: the function contract.
    let r = unsafe { &*(reader as *const Reader<T>) };
    let st = r.state.get() & !RETRY;
    r.state.set(st | PENDING);
    if st & PENDING == 0 {
        let x = r.phead.get();
        if x & HEAD_IDX != 0 {
            r.phead.set(x | QUEUED);
            asy.rescan = true;
            if cls != 0 {
                asy.resettle.push(pi);
            }
        }
    }
    asy.stats.requests += 1;
    let sh = Arc::new(Shared { stamp, sink: asy.sink.clone(), open: AtomicBool::new(true), owes: AtomicBool::new(false), work: Mutex::new(None), lost: lost::<T> });
    // the user's function runs here, on the UI thread, with a borrow of the input; only what it returns
    // crosses to a worker
    let (held, kind) = match ask.work(s, Inlet { sh: sh.clone(), _t: PhantomData }).0 {
        WorkKind::Io(h) => (h, KIND_IO),
        WorkKind::Cpu(h) => (h, KIND_CPU),
        WorkKind::Stream(h) => (h, KIND_STREAM),
    };
    sh.owes.store(kind != KIND_STREAM, Ordering::Release);
    *sh.work.lock().unwrap() = Some(held);
    asy.producers[pi as usize].flight = Some(FlightHandle(sh.clone()));
    asy.spawn(sh, kind);
}

unsafe fn patch_produce<S: PartialEq + Clone + 'static, T: Send + 'static, A: Ask<S, T>>(rt: &mut Rt, w: &Wire, sig: *const u8, owner: *const u8) {
    // SAFETY: the wire contract is `fire`'s: the wire sits on the producer's source.
    unsafe { fire::<S, T, A>(rt, w.node, sig, owner, false) };
}
/// The retry wire, on the reader's `phead`: `Reader::retry` queued it.
unsafe fn patch_retry<T: 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, owner: *const u8) {
    // SAFETY: wire contract: `sig` is the `phead` of a `Reader<T>` (offset 4).
    let r = unsafe { &*(sig.sub(4) as *const Reader<T>) };
    if r.state.get() & RETRY == 0 {
        return;
    }
    let p = &rt.asy.producers[w.node as usize];
    if !p.live {
        return;
    }
    let (f, off) = (p.fire, p.src_off);
    // SAFETY: the producer's source is the `offset_of!`-proven cell at `src_off` of this same owner.
    unsafe { f(rt, w.node, owner.add(off as usize), owner, true) };
}

/// The work a `produce` producer hands the executor for one input (built by its `fn(&S) -> Request<T>`).
pub struct Request<T>(Box<dyn FnOnce() -> Result<T, String> + Send>);
impl<T: 'static> Request<T> {
    pub fn new(f: impl FnOnce() -> T + Send + 'static) -> Request<T> {
        Request(Box::new(move || Ok(f())))
    }
    /// A request that may fail: its error becomes the reader's `fault` (the last value stays).
    pub fn fallible(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Request<T> {
        Request(Box::new(f))
    }
}

/// `produce`: a synchronous job built on the UI thread from a borrow of the input.
pub struct AskRequest<S, T>(fn(&S) -> Request<T>);
impl<S, T> Clone for AskRequest<S, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S, T> Copy for AskRequest<S, T> {}
fn boxed<T: Send + 'static>(o: Outcome<T>) -> Box<dyn Any + Send> {
    Box::new(o)
}
impl<S: 'static, T: Send + 'static> Ask<S, T> for AskRequest<S, T> {
    fn work(self, input: &S, _: Inlet<T>) -> Work {
        let job = (self.0)(input).0;
        Work(WorkKind::Cpu(Held::Job(Box::new(move || boxed(job().map_err(Failure::domain))))))
    }
}
/// `map_async`: a future that cannot fail.
#[derive(Clone, Copy)]
pub struct AskAsync<F>(F);
impl<S: Clone + 'static, T: Send + 'static, Fut: Future<Output = T> + Send + 'static, F: Fn(S) -> Fut + Copy + 'static> Ask<S, T> for AskAsync<F> {
    fn work(self, input: &S, _: Inlet<T>) -> Work {
        let fut = (self.0)(input.clone());
        Work(WorkKind::Io(Held::Future(Box::pin(async move { boxed(Ok::<T, Failure>(fut.await)) }))))
    }
}
/// `try_map_async`: a future with a domain error.
#[derive(Clone, Copy)]
pub struct AskTryAsync<F>(F);
impl<S: Clone + 'static, T: Send + 'static, E: std::fmt::Display + Send + 'static, Fut: Future<Output = Result<T, E>> + Send + 'static, F: Fn(S) -> Fut + Copy + 'static> Ask<S, T> for AskTryAsync<F> {
    fn work(self, input: &S, _: Inlet<T>) -> Work {
        let fut = (self.0)(input.clone());
        Work(WorkKind::Io(Held::Future(Box::pin(async move { boxed(fut.await.map_err(Failure::domain)) }))))
    }
}
/// `compute`: CPU work on the pool.
#[derive(Clone, Copy)]
pub struct AskCompute<F>(F);
impl<S: Clone + Send + 'static, T: Send + 'static, F: Fn(S) -> T + Copy + Send + 'static> Ask<S, T> for AskCompute<F> {
    fn work(self, input: &S, _: Inlet<T>) -> Work {
        let (f, input) = (self.0, input.clone());
        Work(WorkKind::Cpu(Held::Job(Box::new(move || boxed(Ok::<T, Failure>(f(input)))))))
    }
}
/// `try_compute`: CPU work with a domain error.
#[derive(Clone, Copy)]
pub struct AskTryCompute<F>(F);
impl<S: Clone + Send + 'static, T: Send + 'static, E: std::fmt::Display + Send + 'static, F: Fn(S) -> Result<T, E> + Copy + Send + 'static> Ask<S, T> for AskTryCompute<F> {
    fn work(self, input: &S, _: Inlet<T>) -> Work {
        let (f, input) = (self.0, input.clone());
        Work(WorkKind::Cpu(Held::Job(Box::new(move || boxed(f(input).map_err(Failure::domain))))))
    }
}
/// `subscribe`: a stream of answers.
#[derive(Clone, Copy)]
pub struct AskStream<F>(F);
impl<S: 'static, T: Send + 'static, G: Send + 'static, F: Fn(&S, Inlet<T>) -> G + Copy + 'static> Ask<S, T> for AskStream<F> {
    fn work(self, input: &S, inlet: Inlet<T>) -> Work {
        Work(WorkKind::Stream(Held::Keep(Box::new((self.0)(input, inlet)))))
    }
}

/// A producer: whenever `src`'s value changes, the question is asked again with the new input and its
/// answer lands in `dst` through the source's cohort (see the module docs). Mounts nothing; it belongs to
/// the scope that declared it and is revoked with it (L5).
pub struct Produce<'a, S, T, V, A> {
    src: Wired<'a, S, V>,
    dst: RWired<'a, T, V>,
    ask: A,
}
pub fn produce<'a, S, T, V>(src: Wired<'a, S, V>, dst: RWired<'a, T, V>, f: fn(&S) -> Request<T>) -> Produce<'a, S, T, V, AskRequest<S, T>> {
    Produce { src, dst, ask: AskRequest(f) }
}

impl<'a, S, V> Wired<'a, S, V> {
    /// An async question about this signal: whenever its value changes, `f(value)`'s future runs on the
    /// executor's I/O lane and its answer lands in `dst` (a `Reader` or `Settled` field). Latest input wins:
    /// a superseded or unmounted request cannot affect presentation.
    pub fn map_async<T, D, Fut, F>(self, dst: D, f: F) -> Produce<'a, S, T, V, AskAsync<F>>
    where
        D: Dest<'a, T, V>,
        Fut: Future<Output = T> + Send + 'static,
        F: Fn(S) -> Fut + Copy + 'static,
    {
        Produce { src: self, dst: dst.dest(), ask: AskAsync(f) }
    }
    /// `map_async` for a fallible future: its `E` becomes the reader's domain `Failure` (the last value stays).
    pub fn try_map_async<T, E, D, Fut, F>(self, dst: D, f: F) -> Produce<'a, S, T, V, AskTryAsync<F>>
    where
        D: Dest<'a, T, V>,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        F: Fn(S) -> Fut + Copy + 'static,
    {
        Produce { src: self, dst: dst.dest(), ask: AskTryAsync(f) }
    }
    /// The CPU-pool twin of `map_async`: `f(value)` runs on the executor's pool.
    pub fn compute<T, D, F>(self, dst: D, f: F) -> Produce<'a, S, T, V, AskCompute<F>>
    where
        D: Dest<'a, T, V>,
        F: Fn(S) -> T + Copy + Send + 'static,
    {
        Produce { src: self, dst: dst.dest(), ask: AskCompute(f) }
    }
    /// Answers that keep arriving: whenever this signal's value changes, `open(value, inlet)` runs on the UI
    /// thread and hands the `Send` inlet to whatever feeds it (a socket, a timer, a file watcher, a decoder).
    /// The first value sent completes the staged publication like any answer; each later one is a publication
    /// of its own. What `open` returns is kept while the stream is current and dropped when it is superseded
    /// or its producer released: put the subscription's guard there.
    pub fn subscribe<T, D, G, F>(self, dst: D, open: F) -> Produce<'a, S, T, V, AskStream<F>>
    where
        D: Dest<'a, T, V>,
        G: Send + 'static,
        F: Fn(&S, Inlet<T>) -> G + Copy + 'static,
    {
        Produce { src: self, dst: dst.dest(), ask: AskStream(open) }
    }
    /// The CPU-pool twin of `try_map_async`.
    pub fn try_compute<T, E, D, F>(self, dst: D, f: F) -> Produce<'a, S, T, V, AskTryCompute<F>>
    where
        D: Dest<'a, T, V>,
        F: Fn(S) -> Result<T, E> + Copy + Send + 'static,
    {
        Produce { src: self, dst: dst.dest(), ask: AskTryCompute(f) }
    }
}

#[cfg(feature = "sabotage_alias")]
fn same_input<S: PartialEq + 'static>(a: &dyn Any, b: &dyn Any) -> bool {
    matches!((a.downcast_ref::<S>(), b.downcast_ref::<S>()), (Some(a), Some(b)) if a == b)
}

impl<'a, S: PartialEq + Clone + 'static, T: Send + 'static, V: 'static, A: Ask<S, T>> crate::elem::Element<V> for Produce<'a, S, T, V, A> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // a producer belongs to the scope that declared it (a row's dies with the row: `revoke_producer`)
        let asy = &mut m.rt.asy;
        let p = Producer {
            generation: 0,
            ticket: 0,
            live: true,
            flying: false,
            cohort: 0,
            slot: 0,
            cls: m.cls,
            owner: m.owner as usize,
            reader: 0,
            src_off: self.src.off,
            dst_off: self.dst.off,
            last: None,
            flight: None,
            ask: recipe(self.ask),
            fire: fire::<S, T, A>,
            commit: commit::<T>,
            revoked: revoked::<T>,
            #[cfg(feature = "sabotage_alias")]
            history: Vec::new(),
            #[cfg(feature = "sabotage_alias")]
            same: same_input::<S>,
        };
        let pi = match asy.free.pop() {
            Some(i) => {
                // the slot's generation outlives its tenants: a completion of the last one names an older one
                let generation = asy.producers[i as usize].generation;
                asy.producers[i as usize] = Producer { generation, ..p };
                i
            }
            None => {
                asy.producers.push(p);
                (asy.producers.len() - 1) as u32
            }
        };
        m.scope.prods.push(pi);
        // SAFETY: `src` is an `offset_of!`-proven `Signal<S>` of `V`; `patch_produce::<S, T, A>` reads one, and
        // its producer names an `offset_of!`-proven `Reader<T>` of `V` and holds an `A`.
        let w = unsafe { m.wire_cell(self.src.sig.head(), self.src.off, patch_produce::<S, T, A>, pi, [0; 16]) };
        // a producer runs first of its source's wires, held or not: the wires after it see the hold it began
        m.rt.make_latest(w);
        m.rt.wire_to_front(self.src.sig.head(), w);
        // SAFETY: `phead` sits at `dst.off + 4` of the proven reader; the patch reads the reader 4 bytes before.
        unsafe { m.wire_cell(&self.dst.r.phead, self.dst.off + 4, patch_retry::<T>, pi, [0; 16]) };
        // the first request: fetch high, at mount
        let owner = m.owner as *const u8;
        // SAFETY: as the wire itself.
        unsafe { fire::<S, T, A>(m.rt, pi, self.src.sig as *const Signal<S> as *const u8, owner, false) };
    }
    fn update(self, _m: &mut Mounter<'_, V>) {}
}

// =============================================================================== boundary

/// `boundary(sig!(self.r)).loading(L).failed(F).child(C)`: fetch high, block low. All three branches are
/// mounted once, each under a clip of its own; the reader's transitions (a wire on its pending/fault head)
/// show one branch and hide the others by rewriting two clip rects (damage: their xor). `loading` shows
/// until the first readiness, `failed` while the last request failed and no value exists (with a value, a
/// failure keeps the stale content: the reader's `fault()` is there to adorn it), `child` once a value
/// exists: a revalidation never shows `loading` again. In a layout container the branches stack (their
/// layout boxes all take space): put them in absolutely positioned boxes there.
pub struct Boundary<'a, T, V, L, F, C> {
    r: RWired<'a, T, V>,
    loading: L,
    failed: F,
    child: C,
}
pub fn boundary<'a, T, V>(r: RWired<'a, T, V>) -> Boundary<'a, T, V, (), (), ()> {
    Boundary { r, loading: (), failed: (), child: () }
}
impl<'a, T, V, L, F, C> Boundary<'a, T, V, L, F, C> {
    pub fn loading<L2>(self, l: L2) -> Boundary<'a, T, V, L2, F, C> {
        Boundary { r: self.r, loading: l, failed: self.failed, child: self.child }
    }
    pub fn failed<F2>(self, f: F2) -> Boundary<'a, T, V, L, F2, C> {
        Boundary { r: self.r, loading: self.loading, failed: f, child: self.child }
    }
    pub fn child<C2>(self, c: C2) -> Boundary<'a, T, V, L, F, C2> {
        Boundary { r: self.r, loading: self.loading, failed: self.failed, child: c }
    }
}

pub(crate) struct Bound {
    clips: [u16; 3],
    rect: bough_core::NRect,
    shown: u8,
}

fn branch_of<T>(r: &Reader<T>) -> u8 {
    if r.has_value() {
        2
    } else if r.fault().is_some() {
        1
    } else {
        0
    }
}

unsafe fn patch_boundary<T: 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: wire contract: `sig` is the `phead` of a `Reader<T>` (offset 4).
    let r = unsafe { &*(sig.sub(4) as *const Reader<T>) };
    let b = &mut rt.asy.bounds[w.node as usize];
    let now = branch_of(r);
    if now == b.shown {
        return;
    }
    let (old, rect, clips) = (b.shown, b.rect, b.clips);
    b.shown = now;
    rt.bough.set_clip_rect(clips[old as usize], bough_core::NRect::EMPTY);
    rt.bough.set_clip_rect(clips[now as usize], rect);
}

impl<'a, T: 'static, V: 'static, L: crate::elem::Element<V>, F: crate::elem::Element<V>, C: crate::elem::Element<V>> crate::elem::Element<V> for Boundary<'a, T, V, L, F, C> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        crate::describe::mount_opaque(m, |m| {
            let rect = if m.clip == 0 { m.rt.bough.window() } else { m.rt.bough.clip_rect(m.clip) };
            let shown = branch_of(self.r.r);
            let clips = [0u8, 1, 2].map(|i| m.rt.bough.new_clip(if i == shown { rect } else { bough_core::NRect::EMPTY }));
            let bi = m.rt.asy.bounds.len() as u32;
            m.rt.asy.bounds.push(Bound { clips, rect, shown });
            // SAFETY: `phead` sits at `off + 4`; the patch reads the reader 4 bytes before it.
            unsafe { m.wire_cell(&self.r.r.phead, self.r.off + 4, patch_boundary::<T>, bi, [0; 16]) };
            let prev = m.clip;
            m.clip = clips[0];
            self.loading.mount(m);
            m.clip = clips[1];
            self.failed.mount(m);
            m.clip = clips[2];
            self.child.mount(m);
            m.clip = prev;
        });
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}
