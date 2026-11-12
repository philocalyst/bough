//! `Reader<T>`: an async question (fetch high, block low), and the cohort that commits its answers.
//!
//! * A **producer** (`produce(sig!(self.query), sig!(self.results), f)`) is a wire on its source signal:
//!   when the source's value changes (equality against the last frozen input), it bumps the reader's ticket,
//!   hands `f(&input)`'s job to the executor and joins the turn's **cohort**.
//! * **Pending holds.** The reader keeps its last Ready value. The source that began the cohort is held:
//!   its head carries `HELD`, so the same scan that runs wires skips every non-`latest` wire of it (its
//!   sinks keep their pre-write operands); `latest(..)` operands opt out and track the input now.
//! * **Admission** happens at the start of the next turn: a completion whose ticket is not the reader's
//!   current one is discarded (superseded); an accepted one is staged.
//! * **The cohort commits in one epoch.** When the last member is staged, every member's value moves in,
//!   their heads (and the held sources') are queued and the settle that follows runs all their wires: one
//!   turn, one damage epoch, one frame. A consumer can never see half a cohort.
//! * `pending()` operands are wires on a second head (`phead`, offset 4) queued on every pending
//!   transition.
use crate::signal::WireHead;
use crate::elem::{Apply, IntoProp, Mounter, TextSrc};
use crate::rt::*;
use crate::signal::{Headed, Signal, Wired, HELD};
use std::any::Any;
use std::cell::{Cell, UnsafeCell};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

const PENDING: u8 = 1;
const HAS: u8 = 2;
/// The last request failed: the reader keeps its last value and carries the failure until a request succeeds.
const FAULT: u8 = 4;

#[repr(C)]
pub struct Reader<T> {
    head: crate::signal::WireHead,
    phead: Cell<u32>,
    state: Cell<u8>,
    ticket: Cell<u32>,
    value: UnsafeCell<Option<T>>,
    staged: UnsafeCell<Option<Result<T, String>>>,
    fault: UnsafeCell<Option<String>>,
}

impl<T> Default for Reader<T> {
    fn default() -> Self {
        Reader::new()
    }
}

impl<T> Reader<T> {
    pub const fn new() -> Reader<T> {
        Reader { head: WireHead::new(), phead: Cell::new(0), state: Cell::new(0), ticket: Cell::new(0), value: UnsafeCell::new(None), staged: UnsafeCell::new(None), fault: UnsafeCell::new(None) }
    }
    /// The last committed (Ready) value; `None` before first readiness.
    pub fn latest(&self) -> Option<&T> {
        // SAFETY: `value` is written only by the runtime's commit, which holds the owner uniquely.
        unsafe { (*self.value.get()).as_ref() }
    }
    /// Does the next visible answer still wait on a request?
    /// The last request's failure (the reader keeps its last value), until a request succeeds.
    pub fn fault(&self) -> Option<&str> {
        // SAFETY: written only by the runtime's commit, which holds the owner uniquely.
        unsafe { (*self.fault.get()).as_deref() }
    }
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
/// # Safety
/// `PatchFn`'s contract: `sig` is the `head` (offset 0) of a `Reader<T>`; the recipe holds an `F`.
unsafe fn patch_rmap_text<T: 'static, F: Fn(Option<&T>) -> String + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: the contract: `sig` is a `Reader<T>` (its head is the first field, `#[repr(C)]`); the recipe holds an `F`.
    let r = unsafe { &*(sig as *const Reader<T>) };
    let f: F = unsafe { unrecipe(&w.recipe) };
    crate::elem::set_node_text(rt, w.node, &f(r.latest()));
}
/// # Safety
/// `PatchFn`'s contract: `sig` is the `phead` of a `Reader<T>` (offset 4); the recipe holds an `F`.
unsafe fn patch_rpmap_text<T: 'static, F: Fn(bool) -> String + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: the contract: `phead` sits 4 bytes after the reader's start (`#[repr(C)]`); the recipe holds an `F`.
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
        // SAFETY: `self.w` is an `offset_of!`-proven `Reader<T>`; `patch_rmap_text` reads one.
        unsafe { m.wire_cell(&self.w.r.head, self.w.off, patch_rmap_text::<T, F>, node, recipe(self.f)) };
        (self.f)(self.w.r.latest())
    }
    fn value(self) -> String {
        (self.f)(self.w.r.latest())
    }
}
impl<'a, T: 'static, V, F: Fn(bool) -> String + Copy + 'static> TextSrc<V> for RPMap<'a, T, V, F> {
    fn lower(self, m: &mut Mounter<'_, V>, node: u32) -> String {
        // SAFETY: `phead` sits at `off + 4` (`#[repr(C)]`); `patch_rpmap_text` reads the reader 4 bytes before it.
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
        let i = m.rt.wires.len();
        let s = <crate::signal::WMap<'a, T, V, F> as TextSrc<V>>::lower(self.0, m, node);
        let w = *m.scope.wires.last().unwrap();
        debug_assert!(w as usize <= i);
        m.rt.make_latest(w);
        s
    }
    /// Intent, also inside a description (where a held cell reads its shown value).
    fn value(self) -> String {
        (self.0.f)(self.0.w.sig.latest())
    }
}
impl<'a, T: Clone + 'static, V> IntoProp<T, V> for Latest<Wired<'a, T, V>> {
    fn lower<K: Apply<P = T>>(self, m: &mut Mounter<'_, V>, node: u32) -> T {
        let v = self.0.lower::<K>(m, node);
        let w = *m.scope.wires.last().unwrap();
        m.rt.make_latest(w);
        v
    }
    /// Intent, also inside a description (where a held cell reads its shown value).
    fn value(self) -> T {
        self.0.sig.latest().clone()
    }
}

// =============================================================================== executor, cohort

pub type Job = Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>;
type Done = Arc<Mutex<Vec<(usize, u32, Box<dyn Any + Send>)>>>;

/// Where producers' jobs run: `Manual` keeps them until `App::run_jobs` (deterministic: tests, benches);
/// `Threads` runs each on its own thread, completions waiting for the next turn.
pub enum Exec {
    Manual(VecDeque<(usize, u32, Job)>),
    Threads,
}

struct Member {
    reader: usize,
    /// The reader's owner and its class (a row-owned reader settles with its row's class).
    owner: usize,
    cls: u32,
    staged: bool,
    stage: unsafe fn(usize, u32, Box<dyn Any + Send>) -> bool,
    commit: unsafe fn(usize),
}

#[derive(Default)]
struct Cohort {
    members: Vec<Member>,
    held: Vec<usize>,
}

pub struct Async {
    pub exec: Exec,
    done: Done,
    cohort: Option<Cohort>,
    pub(crate) producers: Vec<Producer>,
    /// A producer queued a head the current settle scan may have passed: scan again.
    pub(crate) rescan: bool,
    /// Readers committed in the last admission (for `changed`).
    pub committed: Vec<usize>,
    /// Non-root owners of committed readers, settled after the admission.
    pub(crate) settle_owners: Vec<(usize, u32)>,
    /// Boundaries (`boundary`): their branch clips.
    pub(crate) bounds: Vec<Bound>,
    pub stats: AsyncStats,
}
#[derive(Default, Clone, Copy, Debug)]
pub struct AsyncStats {
    pub requests: u64,
    pub accepted: u64,
    pub stale: u64,
    pub commits: u64,
}

impl Default for Async {
    fn default() -> Self {
        Async { exec: Exec::Manual(VecDeque::new()), done: Arc::default(), cohort: None, producers: Vec::new(), rescan: false, committed: Vec::new(), settle_owners: Vec::new(), bounds: Vec::new(), stats: AsyncStats::default() }
    }
}

pub(crate) struct Producer {
    dst_off: u32,
    f: usize,
    last: Option<Box<dyn Any>>,
    /// Revoked with its scope (`Rt::revoke_producer`): it requests nothing more.
    dead: bool,
    reader: usize,
}

/// # Safety
/// `addr` is a live `Reader<T>`.
unsafe fn stage<T: 'static>(addr: usize, ticket: u32, v: Box<dyn Any + Send>) -> bool {
    let r = unsafe { &*(addr as *const Reader<T>) };
    if r.ticket.get() != ticket {
        return false;
    }
    let v = v.downcast::<Result<T, String>>().expect("job result of the reader's type");
    // SAFETY: the runtime owns the owner uniquely while admitting.
    unsafe { *r.staged.get() = Some(*v) };
    true
}
/// # Safety
/// `addr` is a live `Reader<T>` with a staged value.
unsafe fn commit<T: 'static>(addr: usize) {
    let r = unsafe { &*(addr as *const Reader<T>) };
    let failed = unsafe {
        match (*r.staged.get()).take() {
            Some(Ok(v)) => {
                *r.value.get() = Some(v);
                *r.fault.get() = None;
                false
            }
            // a failure keeps the last value presented (keep-stale; `boundary` shows its `failed` branch)
            Some(Err(e)) => {
                *r.fault.get() = Some(e);
                true
            }
            None => false,
        }
    };
    let has = r.state.get() & HAS != 0 || !failed;
    r.state.set(if has { HAS } else { 0 } | if failed { FAULT } else { 0 });
    for h in [&r.head, &r.phead] {
        crate::signal::queue(h);
    }
}

impl Rt {
    /// Move wire `w` into the status group of its chain (`latest` operands) or, `question`, the question group
    /// (producers): it runs while its cell is held, before the wires a hold skips. Moving it (not only flagging it,
    /// as before round 7) is what keeps a status wire declared before its producer from missing the hold.
    pub(crate) fn make_latest(&mut self, w: u32) {
        self.regroup(w, G_STATUS);
    }
    pub(crate) fn make_question(&mut self, w: u32) {
        self.regroup(w, G_QUESTION);
    }
    pub(crate) fn regroup(&mut self, w: u32, group: u8) {
        let addr = self.wires[w as usize].head;
        if self.wires[w as usize].group == group || addr == 0 {
            self.wires[w as usize].group = group;
            return;
        }
        // SAFETY: a live wire's `head` is the address of the head it was linked into (still live: the wire is being
        // installed by a mount of its owner right now).
        let head = unsafe { &*(addr as *const Cell<u32>) };
        // unlink
        let mut prev = 0u32;
        let mut i = head.get() & HEAD_IDX;
        while i != 0 && i != w {
            prev = i;
            i = self.wires[i as usize].next;
        }
        if i != w {
            return;
        }
        let next = self.wires[w as usize].next;
        if prev == 0 {
            head.set(next | (head.get() & !HEAD_IDX));
        } else {
            self.wires[prev as usize].next = next;
        }
        // re-insert at the front of its new group (newest first within a group)
        self.wires[w as usize].group = group;
        let mut after = 0u32;
        let mut p = head.get() & HEAD_IDX;
        while p != 0 && self.wires[p as usize].group < group && self.wires[p as usize].head == addr {
            after = p;
            p = self.wires[p as usize].next;
        }
        self.wires[w as usize].next = p;
        if after == 0 {
            head.set(w | (head.get() & !HEAD_IDX));
        } else {
            self.wires[after as usize].next = w;
        }
    }

    /// Admit completions (the start of a turn): stale tickets are dropped, accepted values staged; a cohort
    /// whose every member is staged commits (values in, heads and held sources queued).
    pub(crate) fn admit(&mut self) {
        self.asy.committed.clear();
        if self.asy.producers.is_empty() {
            return; // no producer: nothing can complete (no lock taken)
        }
        let done: Vec<_> = std::mem::take(&mut *self.asy.done.lock().unwrap());
        if done.is_empty() {
            return;
        }
        let Some(c) = self.asy.cohort.as_mut() else {
            self.asy.stats.stale += done.len() as u64;
            return;
        };
        for (addr, ticket, v) in done {
            match c.members.iter_mut().find(|m| m.reader == addr) {
                // SAFETY: members are live readers of the mounted owner, staged with their own type.
                Some(m) if unsafe { (m.stage)(addr, ticket, v) } => {
                    m.staged = true;
                    self.asy.stats.accepted += 1;
                }
                _ => self.asy.stats.stale += 1,
            }
        }
        if c.members.iter().all(|m| m.staged) {
            let c = self.asy.cohort.take().unwrap();
            for m in &c.members {
                // SAFETY: as above.
                unsafe { (m.commit)(m.reader) };
                if m.cls != 0 {
                    self.asy.settle_owners.push((m.owner, m.cls));
                }
                self.asy.committed.push(m.reader);
            }
            for &h in &c.held {
                // SAFETY: a held head is a signal head of the live owner.
                let head = unsafe { &*(h as *const Cell<u32>) };
                crate::signal::keep::release(h);
                head.set(head.get() & !HELD);
                crate::signal::queue_quiet(head);
            }
            self.asy.stats.commits += 1;
            for (o, cls) in std::mem::take(&mut self.asy.settle_owners) {
                // SAFETY: a member's owner is live (a released one revoked its producer and left the cohort).
                unsafe { self.settle_cls(o as *const u8, cls) };
            }
        }
    }

    /// A producer's scope was released (L5): it requests nothing more and its reader leaves the cohort (a
    /// completion still in flight finds no member and is discarded).
    pub(crate) fn revoke_producer(&mut self, p: u32) {
        let pr = &mut self.asy.producers[p as usize];
        pr.dead = true;
        let r = pr.reader;
        if let Some(c) = self.asy.cohort.as_mut() {
            c.members.retain(|m| m.reader != r);
            if c.members.is_empty() {
                let c = self.asy.cohort.take().unwrap();
                for &h in &c.held {
                    // SAFETY: a held head is a signal head of a live owner (a dead row's heads were cleared).
                    let head = unsafe { &*(h as *const Cell<u32>) };
                    crate::signal::keep::release(h);
                    head.set(head.get() & !HELD);
                }
            }
        }
    }

    /// Which branch boundary `i` shows (0 loading, 1 failed, 2 child).
    pub fn boundary_shown(&self, i: usize) -> u8 {
        self.asy.bounds[i].shown
    }

    /// Run up to `n` queued jobs (manual executor).
    pub fn run_jobs(&mut self, n: usize) -> usize {
        let mut k = 0;
        while k < n {
            let Exec::Manual(q) = &mut self.asy.exec else { break };
            let Some((addr, ticket, job)) = q.pop_front() else { break };
            let v = job();
            self.asy.done.lock().unwrap().push((addr, ticket, v));
            k += 1;
        }
        k
    }
    /// Run the queued job at index `i` (out-of-order completion, tests).
    pub fn run_job_at(&mut self, i: usize) -> bool {
        let Exec::Manual(q) = &mut self.asy.exec else { return false };
        let Some((addr, ticket, job)) = q.remove(i) else { return false };
        let v = job();
        self.asy.done.lock().unwrap().push((addr, ticket, v));
        true
    }
    pub fn jobs_queued(&self) -> usize {
        match &self.asy.exec {
            Exec::Manual(q) => q.len(),
            Exec::Threads => 0,
        }
    }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<S>` of the live root owner `owner`; the producer's `dst_off` an
/// `offset_of!`-proven `Reader<T>` of it; its `f` a `fn(&S) -> Request<T>`.
unsafe fn patch_produce<S: PartialEq + Clone + 'static, T: Send + 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, owner: *const u8) {
    let src = unsafe { &*(sig as *const Signal<S>) };
    let p = &mut rt.asy.producers[w.node as usize];
    if p.dead {
        return;
    }
    if p.last.as_ref().and_then(|b| b.downcast_ref::<S>()) == Some(&**src) {
        return;
    }
    p.last = Some(Box::new((**src).clone()));
    // SAFETY: `p.f` was stored from a `fn(&S) -> Request<T>` of this same `S` and `T` when `Produce::mount` pushed
    // the producer (`f: self.f as usize`); a fn pointer round-trips through `usize` on every supported target.
    let f: fn(&S) -> Request<T> = unsafe { std::mem::transmute::<usize, fn(&S) -> Request<T>>(p.f) };
    let addr = owner as usize + p.dst_off as usize;
    p.reader = addr;
    // SAFETY: the contract.
    let r = unsafe { &*(addr as *const Reader<T>) };
    let ticket = r.ticket.get().wrapping_add(1);
    r.ticket.set(ticket);
    let job = f(src).0;
    rt.asy.stats.requests += 1;
    if r.state.get() & PENDING == 0 {
        r.state.set(r.state.get() | PENDING);
        if r.phead.get() != 0 {
            crate::signal::queue(&r.phead);
            rt.asy.rescan = true;
        }
    }
    let c = rt.asy.cohort.get_or_insert_with(Cohort::default);
    match c.members.iter_mut().find(|m| m.reader == addr) {
        Some(m) => m.staged = false,
        None => c.members.push(Member { reader: addr, owner: owner as usize, cls: w.cls, staged: false, stage: stage::<T>, commit: commit::<T> }),
    }
    let h = sig as usize;
    if !c.held.contains(&h) {
        c.held.push(h);
    }
    if src.head().get() & HELD == 0 {
        src.head().set(src.head().get() | HELD);
        crate::signal::keep::held(h);
    }
    let job: Job = Box::new(move || Box::new(job()) as Box<dyn Any + Send>);
    match &mut rt.asy.exec {
        Exec::Manual(q) => q.push_back((addr, ticket, job)),
        Exec::Threads => {
            let done = rt.asy.done.clone();
            std::thread::spawn(move || {
                let v = job();
                done.lock().unwrap().push((addr, ticket, v));
            });
        }
    }
}

/// The work a producer hands the executor for one input (built by the producer's `fn(&S) -> Request<T>`).
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

/// A producer: whenever `src`'s value changes, `f(&value)`'s request runs on the executor and its answer
/// lands in `dst` through the cohort (see the module docs). Mounts nothing.
pub struct Produce<'a, S, T, V> {
    src: Wired<'a, S, V>,
    dst: RWired<'a, T, V>,
    f: fn(&S) -> Request<T>,
}
pub fn produce<'a, S, T, V>(src: Wired<'a, S, V>, dst: RWired<'a, T, V>, f: fn(&S) -> Request<T>) -> Produce<'a, S, T, V> {
    Produce { src, dst, f }
}

impl<'a, S: PartialEq + Clone + 'static, T: Send + 'static, V: 'static> crate::elem::Element<V> for Produce<'a, S, T, V> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // a producer belongs to the scope that declared it (a row's dies with the row: `revoke_producer`)
        let pi = m.rt.asy.producers.len() as u32;
        m.rt.asy.producers.push(Producer { dst_off: self.dst.off, f: self.f as usize, last: None, dead: false, reader: 0 });
        m.scope.prods.push(pi);
        // SAFETY: `src` is an `offset_of!`-proven `Signal<S>` of `V`; `patch_produce::<S, T>` reads one, and its
        // producer names an `offset_of!`-proven `Reader<T>` of `V`.
        let w = unsafe { m.wire_cell(self.src.sig.head(), self.src.off, patch_produce::<S, T>, pi, [0; 16]) };
        m.rt.make_question(w);
        // the source keeps a pre-image of its first write of a turn: a description reads it while held (round 7)
        let h = self.src.sig.head();
        crate::signal::keep::register(h as *const _ as usize, crate::signal::keep::fns::<S>(), m.rt.asks.id);
        h.set(h.get() | crate::signal::KEEP);
        // the first request: fetch high, at mount
        let owner = m.owner as *const u8;
        let wire = m.rt.wires[w as usize];
        // SAFETY: as the wire itself.
        unsafe { patch_produce::<S, T>(m.rt, &wire, self.src.sig as *const Signal<S> as *const u8, owner) };
    }
    fn update(self, _m: &mut Mounter<'_, V>) {}
}

// =============================================================================== boundary

/// `boundary(sig!(self.r)).loading(L).failed(F).child(C)`: fetch high, block low. All three branches are
/// mounted once, each under a clip of its own; the reader's transitions (a wire on its pending/fault head)
/// show one branch and hide the others by rewriting two clip rects (damage: their xor). `loading` shows
/// until the first readiness, `failed` while the last request failed and no value exists (with a value, a
/// failure keeps the stale content: the reader's `fault()` is there to adorn it), `child` once a value
/// exists. In a layout container the branches stack (their layout boxes all take space): put them in
/// absolutely positioned boxes there.
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
