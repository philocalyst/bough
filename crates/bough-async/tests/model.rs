#![feature(async_iterator, gen_blocks)]
//! Model checker of the publication engine: the engine as bough uses it, bound to a tiny world of integer
//! cells instead of signals, driven by random and exhaustive schedules of writes, completions in any order,
//! refreshes, unmounts and remounts, with the screen checked after every turn.
//!
//! The world mirrors the façade's rules (`DESIGN.md` section 4): a write to a source cell of a holding
//! question keeps a pre-image; a held cell shows its kept value; a release drops it and queues the cell.
//!
//! The application: `a`, `b` written by "handlers"; questions `d1 = f1(a)` and `d2 = f2(a)` (one source, so
//! one publication), `d3 = f3(d1)` (a cascade), `d4 = f4(b)` deferred (holds nothing), `ds = stream(a)` (a
//! stream: two items per input), `d5 = f5(a, b)` (two sources), `d6` asked every 7 ms with no input (quiet
//! re-asks), and an action whose runs (inputs 0, 1, 2, ... in the order sent) land in a log. Answers encode
//! their input, so the oracle can check them.
//!
//! Oracle, after every turn ("frame"):
//! * F-1: a loud question's destination, if answered, is its function of the *shown* source (`d1 == f1(a)`
//!   on screen, `d3 == f3(d1)` on screen, `ds` an item of `stream(a)` on screen);
//! * nothing is held unless a question is in the air, and the façade's held set is the engine's;
//! * at most one flight per question (latest wins);
//! * drained (every completion delivered): nothing held, nothing kept, every destination equals its function
//!   of intent (the deferred one included).
//!
//! `BOUGH_MODEL_SEEDS` (default 300) random schedules of `BOUGH_MODEL_STEPS` (default 200) steps; the
//! exhaustive run (`exhaustive`) enumerates every schedule of `BOUGH_MODEL_DEPTH` (default 5) ops.
use bough_async::{Ask, Asking, Engine, Many, Once, Out, QId, QOpts, Step, Work};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

type Val = Option<i64>;

const A: usize = 1;
const B: usize = 2;
const D1: usize = 3;
const D2: usize = 4;
const D3: usize = 5;
const D4: usize = 6;
const DS: usize = 7;
const D5: usize = 8;
const D6: usize = 9;
const NCELL: usize = 10;

fn f1(x: Val) -> Val {
    x.map(|x| x * 10 + 1)
}
fn f2(x: Val) -> Val {
    x.map(|x| x * 10 + 2)
}
fn f3(x: Val) -> Val {
    x.map(|x| x * 10 + 3)
}
fn f4(x: Val) -> Val {
    x.map(|x| x * 10 + 4)
}
/// Stream item `k` (0, 1) for input x.
fn fs(x: Val, k: i64) -> Val {
    x.map(|x| x * 100 + 50 + k)
}
fn f5(a: Val, b: Val) -> Val {
    Some(a? * 1000 + b? * 100 + 5)
}

// ============================================================================================ gates

/// A pending completion the schedule resolves: (question, input, item index).
struct Gate {
    q: &'static str,
    input: Val,
    k: i64,
    answer: Option<Val>,
    closed: bool,
    waker: Option<Waker>,
    dropped: bool,
    done: bool,
}

thread_local! {
    static GATES: RefCell<Vec<Gate>> = const { RefCell::new(Vec::new()) };
}

fn open_gate(q: &'static str, input: Val, k: i64) -> usize {
    GATES.with(|g| {
        let mut g = g.borrow_mut();
        g.push(Gate { q, input, k, answer: None, closed: false, waker: None, dropped: false, done: false });
        g.len() - 1
    })
}

struct GateFut(usize);
impl Future for GateFut {
    type Output = Val;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Val> {
        GATES.with(|g| {
            let mut g = g.borrow_mut();
            let gt = &mut g[self.0];
            match gt.answer.take() {
                Some(v) => {
                    gt.done = true;
                    Poll::Ready(v)
                }
                None => {
                    gt.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
    }
}
impl Drop for GateFut {
    fn drop(&mut self) {
        GATES.with(|g| {
            if let Ok(mut g) = g.try_borrow_mut() {
                let gt = &mut g[self.0];
                if !gt.done {
                    gt.dropped = true;
                }
            }
        })
    }
}

/// Indices of gates the schedule can resolve now.
fn pending_gates() -> Vec<usize> {
    GATES.with(|g| g.borrow().iter().enumerate().filter(|(_, x)| !x.done && !x.dropped && x.answer.is_none() && !x.closed).map(|(i, _)| i).collect())
}

fn resolve(i: usize) {
    GATES.with(|g| {
        let mut g = g.borrow_mut();
        let gt = &mut g[i];
        let v = match gt.q {
            "f1" => f1(gt.input),
            "f2" => f2(gt.input),
            "f3" => f3(gt.input),
            "f4" => f4(gt.input),
            // f5's gate carries the pair encoded as a * 10 + b
            "f5" => gt.input.and_then(|p| f5(Some(p / 10), Some(p % 10))),
            "f6" => Some(600),
            "act" => gt.input,
            _ => fs(gt.input, gt.k),
        };
        gt.answer = Some(v);
        if let Some(w) = gt.waker.take() {
            w.wake();
        }
    })
}

// ============================================================================================ world

/// The model owner: cell values and the façade's hold state.
struct World {
    vals: [Val; NCELL],
    /// Cells with a KEEP bit (sources of holding questions).
    keep: HashSet<usize>,
    held: HashSet<usize>,
    kept: HashMap<usize, Val>,
    /// Kept values that are this turn's pre-images (not yet held).
    turn_pre: Vec<usize>,
    /// Cells written and not yet settled.
    queued: Vec<usize>,
    /// Questions wired on each cell (their source wires).
    wires: Vec<Vec<QId>>,
    /// A destination has been landed into since its question was (re)mounted.
    landed: [bool; NCELL],
    /// The action: inputs sent, in order, and answers landed, in order.
    sent: Vec<i64>,
    acted: Vec<i64>,
}

impl World {
    fn write(&mut self, c: usize, v: Val) {
        if (self.keep.contains(&c) || self.held.contains(&c)) && !self.kept.contains_key(&c) && !self.queued.contains(&c) {
            self.kept.insert(c, self.vals[c]);
            if !self.held.contains(&c) {
                self.turn_pre.push(c);
            }
        }
        self.vals[c] = v;
        if !self.queued.contains(&c) {
            self.queued.push(c);
        }
    }
    fn shown(&self, c: usize) -> Val {
        if self.held.contains(&c) {
            if let Some(v) = self.kept.get(&c) {
                return *v;
            }
        }
        self.vals[c]
    }
}

/// A model question's flight: a gate (one answer), an immediate answer, or a stream of gated items.
enum MFl {
    Gate(Once<GateFut>),
    Now(Option<Val>),
    Items(Many<std::pin::Pin<Box<dyn std::async_iter::AsyncIterator<Item = Val>>>>),
}
impl bough_async::Flight for MFl {
    type Out = Val;
    const STREAM: bool = false;
    fn poll_flight(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Val>> {
        // SAFETY: structural pinning; the enum is never moved out of its pin.
        match unsafe { self.get_unchecked_mut() } {
            MFl::Gate(g) => unsafe { Pin::new_unchecked(g) }.poll_flight(cx),
            MFl::Now(v) => Poll::Ready(Some(v.take().flatten())),
            MFl::Items(m) => unsafe { Pin::new_unchecked(m) }.poll_flight(cx),
        }
    }
}

/// A stream question's flight (its own type: `STREAM` is a constant of the flight type).
struct SFl(Many<std::pin::Pin<Box<dyn std::async_iter::AsyncIterator<Item = Val>>>>);
impl bough_async::Flight for SFl {
    type Out = Val;
    const STREAM: bool = true;
    fn poll_flight(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Val>> {
        // SAFETY: structural pinning.
        unsafe { self.map_unchecked_mut(|s| &mut s.0) }.poll_flight(cx)
    }
}

struct MQ {
    name: &'static str,
    src: usize,
    dst: usize,
}

impl Ask for MQ {
    type In = Val;
    type Fl = MFl;
    unsafe fn input(&self, owner: *mut u8) -> Val {
        let w = unsafe { &*(owner as *const World) };
        w.vals[self.src]
    }
    fn start(&self, x: Val) -> MFl {
        if x.is_none() {
            // ready on its first poll: no request
            return MFl::Now(Some(None));
        }
        MFl::Gate(Once(GateFut(open_gate(self.name, x, 0))))
    }
    unsafe fn land(&self, owner: *mut u8, out: Val, _first: bool) {
        let w = unsafe { &mut *(owner as *mut World) };
        w.landed[self.dst] = true;
        w.write(self.dst, out);
    }
}

/// `ds = stream(a)`: two gated items per input.
struct MS;
impl Ask for MS {
    type In = Val;
    type Fl = SFl;
    unsafe fn input(&self, owner: *mut u8) -> Val {
        let w = unsafe { &*(owner as *const World) };
        w.vals[A]
    }
    fn start(&self, x: Val) -> SFl {
        SFl(Many(Box::pin(async gen move {
            if x.is_none() {
                return;
            }
            for k in 0..2 {
                let g = open_gate("fs", x, k);
                yield GateFut(g).await;
            }
        })))
    }
    unsafe fn land(&self, owner: *mut u8, out: Val, _first: bool) {
        let w = unsafe { &mut *(owner as *mut World) };
        w.landed[DS] = true;
        w.write(DS, out);
    }
}

/// `d5 = f5(a, b)`: two sources.
struct MQ2;
impl Ask for MQ2 {
    type In = (Val, Val);
    type Fl = Once<GateFut>;
    unsafe fn input(&self, owner: *mut u8) -> (Val, Val) {
        let w = unsafe { &*(owner as *const World) };
        (w.vals[A], w.vals[B])
    }
    fn start(&self, (a, b): (Val, Val)) -> Once<GateFut> {
        Once(GateFut(open_gate("f5", a.zip(b).map(|(a, b)| a * 10 + b), 0)))
    }
    unsafe fn land(&self, owner: *mut u8, out: Val, _first: bool) {
        let w = unsafe { &mut *(owner as *mut World) };
        w.landed[D5] = true;
        w.write(D5, out);
    }
}

/// `d6`, asked every 7 ms with no input.
struct MQ6;
impl Ask for MQ6 {
    type In = ();
    type Fl = Once<GateFut>;
    unsafe fn input(&self, _owner: *mut u8) {}
    fn start(&self, _: ()) -> Once<GateFut> {
        Once(GateFut(open_gate("f6", Some(6), 0)))
    }
    unsafe fn land(&self, owner: *mut u8, out: Val, _first: bool) {
        let w = unsafe { &mut *(owner as *mut World) };
        w.write(D6, out);
    }
}

/// The action's landing: answers append to `acted`.
struct ALog(*mut World);
impl bough_async::Landing for ALog {
    unsafe fn land(&self, out: Out) {
        let w = unsafe { &mut *self.0 };
        w.acted.push(out.downcast::<Val>().unwrap().unwrap());
    }
    unsafe fn finished(&self) {}
}

struct Harness {
    e: Engine,
    w: Box<World>,
    qs: [Option<QId>; 5],
    q5: QId,
    act: bough_async::AId,
    now: Duration,
    log: Vec<String>,
}

const SPECS: [(&str, usize, usize, bool, bool); 5] = [("f1", A, D1, true, false), ("f2", A, D2, true, false), ("f3", D1, D3, true, false), ("f4", B, D4, false, false), ("fs", A, DS, true, true)];

impl Harness {
    fn new() -> Harness {
        GATES.with(|g| g.borrow_mut().clear());
        let mut w = Box::new(World { vals: [None; NCELL], keep: HashSet::new(), held: HashSet::new(), kept: HashMap::new(), turn_pre: Vec::new(), queued: Vec::new(), wires: vec![Vec::new(); NCELL], landed: [false; NCELL], sent: Vec::new(), acted: Vec::new() });
        w.vals[A] = Some(0);
        w.vals[B] = Some(0);
        let mut e = Engine::new();
        let owner = &mut *w as *mut World as *mut u8;
        let q5 = e.mount(Asking::new(MQ2), owner, vec![A, B], D5, QOpts { hold: true, every: None, lands_into: true });
        let act = e.add_action(Box::new(ALog(owner as *mut World)));
        let mut h = Harness { e, w, qs: [None; 5], q5, act, now: Duration::ZERO, log: Vec::new() };
        h.w.wires[A].push(q5);
        h.w.wires[B].push(q5);
        h.w.keep.insert(A);
        h.w.keep.insert(B);
        h.e.first(q5);
        let q6 = h.e.mount(Asking::new(MQ6), owner, vec![], D6, QOpts { hold: false, every: Some(Duration::from_millis(7)), lands_into: true });
        h.e.first(q6);
        h.drain_out();
        for i in 0..5 {
            h.mount(i);
        }
        h.settle_and_pump();
        h.end_turn();
        h
    }

    fn owner(&mut self) -> *mut u8 {
        &mut *self.w as *mut World as *mut u8
    }

    fn mount(&mut self, i: usize) {
        let (name, src, dst, hold, stream) = SPECS[i];
        let owner = self.owner();
        let q: Box<dyn bough_async::Question> = if stream { Asking::new(MS) } else { Asking::new(MQ { name, src, dst }) };
        let q = self.e.mount(q, owner, vec![src], dst, QOpts { hold, every: None, lands_into: true });
        self.qs[i] = Some(q);
        self.w.landed[dst] = false;
        self.w.wires[src].push(q);
        if hold {
            self.w.keep.insert(src);
        }
        self.e.first(q);
        self.drain_out();
    }

    fn unmount(&mut self, i: usize) {
        if let Some(q) = self.qs[i].take() {
            let (_, src, _, _, _) = SPECS[i];
            self.w.wires[src].retain(|&x| x != q);
            self.e.revoke(q);
            // KEEP stays while another holding question sources the cell
            let still = (0..5).any(|j| j != i && self.qs[j].is_some() && SPECS[j].1 == src && SPECS[j].3) || src == A || src == B;
            if !still {
                self.w.keep.remove(&src);
            }
            self.drain_out();
        }
    }

    fn drain_out(&mut self) {
        for c in std::mem::take(&mut self.e.hold_out) {
            self.w.held.insert(c);
            // this turn's pre-image (if the cell was written) is now the hold's kept value
            self.w.turn_pre.retain(|&x| x != c);
        }
        for c in std::mem::take(&mut self.e.release_out) {
            self.w.held.remove(&c);
            self.w.kept.remove(&c);
            // a release queues the cell: its wires (and questions) run
            if !self.w.queued.contains(&c) {
                self.w.queued.push(c);
            }
        }
        self.e.status_out.clear();
    }

    /// Run the questions wired on queued cells until nothing is queued.
    fn settle(&mut self) {
        for _ in 0..64 {
            let q = std::mem::take(&mut self.w.queued);
            if q.is_empty() {
                return;
            }
            for c in q {
                let wires = self.w.wires[c].clone();
                for qid in wires {
                    self.e.written(qid);
                    self.drain_out();
                }
            }
        }
        panic!("settle did not converge");
    }

    fn pump(&mut self) {
        for _ in 0..256 {
            match self.e.step() {
                Step::Idle => return,
                Step::Landed => {
                    self.drain_out();
                    self.settle();
                }
                Step::Committing(p) => {
                    self.drain_out();
                    self.settle();
                    while self.e.end_commit(p) {
                        self.drain_out();
                        self.settle();
                    }
                    self.drain_out();
                    self.settle();
                }
            }
        }
        panic!("pump did not converge");
    }

    fn settle_and_pump(&mut self) {
        self.settle();
        self.e.close_group();
        self.pump();
    }

    fn end_turn(&mut self) {
        for c in std::mem::take(&mut self.w.turn_pre) {
            if !self.w.held.contains(&c) {
                self.w.kept.remove(&c);
            }
        }
    }

    /// A handler turn.
    fn turn(&mut self, f: impl FnOnce(&mut World)) {
        self.admit();
        self.e.begin_turn();
        f(&mut self.w);
        self.settle_and_pump();
        self.end_turn();
    }

    /// Admission: poll what rang, land what is ready.
    fn admit(&mut self) {
        self.now += Duration::from_millis(1);
        self.e.poll(self.now);
        self.drain_out();
        self.pump();
    }

    fn idle_turn(&mut self) {
        self.admit();
        self.end_turn();
    }

    fn check(&self, ctx: &str) {
        let w = &*self.w;
        let fail = |m: String| panic!("{ctx}: {m}\nheld {:?} kept {:?} vals {:?}\n{}log:\n{}", w.held, w.kept, w.vals, self.e.dump(), self.log.join("\n"));
        let s = |c| w.shown(c);
        if self.qs[0].is_some() && s(D1).is_some() && s(D1) != f1(s(A)) {
            fail(format!("d1 {:?} beside a {:?}", s(D1), s(A)));
        }
        // a remounted question's destination shows the old answer until its first landing (DESIGN.md, open
        // question 3): checked from then on
        if self.qs[1].is_some() && w.landed[D2] && s(D2).is_some() && s(D2) != f2(s(A)) {
            fail(format!("d2 {:?} beside a {:?}", s(D2), s(A)));
        }
        if self.qs[2].is_some() && self.qs[0].is_some() && s(D3).is_some() && s(D3) != f3(s(D1)) {
            fail(format!("d3 {:?} beside d1 {:?}", s(D3), s(D1)));
        }
        if self.qs[4].is_some() && s(DS).is_some() && s(DS) != fs(s(A), 0) && s(DS) != fs(s(A), 1) {
            fail(format!("ds {:?} beside a {:?}", s(DS), s(A)));
        }
        if w.landed[D5] && s(D5).is_some() && s(D5) != f5(s(A), s(B)) {
            fail(format!("d5 {:?} beside a {:?} b {:?}", s(D5), s(A), s(B)));
        }
        if !w.sent.starts_with(&w.acted) {
            fail(format!("action answers {:?} out of the order sent {:?}", w.acted, w.sent));
        }
        if w.held.len() != self.e.held_cells() {
            fail(format!("façade holds {} cells, engine {}", w.held.len(), self.e.held_cells()));
        }
        if !w.held.is_empty() && self.e.flights() == 0 {
            fail("cells held with nothing in the air".into());
        }
        for c in w.kept.keys() {
            if !w.held.contains(c) {
                fail(format!("kept value of cell {c} outside a hold after the turn"));
            }
        }
        // latest wins: at most one live gate per question (a stream has one per item, one at a time)
        for q in ["f1", "f2", "f3", "f4", "fs", "f5", "f6"] {
            let n = GATES.with(|g| g.borrow().iter().filter(|x| x.q == q && !x.done && !x.dropped).count());
            if n > 1 {
                fail(format!("{n} live requests of {q}"));
            }
        }
    }

    fn drain(&mut self, ctx: &str) {
        for _ in 0..500 {
            let p = pending_gates();
            if p.is_empty() && !self.e.woken() {
                break;
            }
            if let Some(&i) = p.first() {
                resolve(i);
            }
            self.idle_turn();
            self.check(ctx);
        }
        assert!(pending_gates().is_empty(), "{ctx}: gates left after draining");
        let w = &*self.w;
        assert!(w.held.is_empty() && w.kept.is_empty(), "{ctx}: drained but held {:?} kept {:?}\n{}", w.held, w.kept, self.log.join("\n"));
        assert_eq!(self.e.publications(), 0, "{ctx}: drained but a publication is live\n{}", self.log.join("\n"));
        let v = |c: usize| w.vals[c];
        if self.qs[0].is_some() {
            assert_eq!(v(D1), f1(v(A)), "{ctx}: d1 after drain\n{}", self.log.join("\n"));
        }
        if self.qs[1].is_some() {
            assert_eq!(v(D2), f2(v(A)), "{ctx}: d2 after drain\n{}", self.log.join("\n"));
        }
        if self.qs[2].is_some() && self.qs[0].is_some() {
            assert_eq!(v(D3), f3(v(D1)), "{ctx}: d3 after drain\n{}", self.log.join("\n"));
        }
        if self.qs[3].is_some() {
            assert_eq!(v(D4), f4(v(B)), "{ctx}: d4 after drain\n{}", self.log.join("\n"));
        }
        if self.qs[4].is_some() {
            assert_eq!(v(DS), fs(v(A), 1), "{ctx}: ds after drain\n{}", self.log.join("\n"));
        }
        assert_eq!(v(D5), f5(v(A), v(B)), "{ctx}: d5 after drain\n{}", self.log.join("\n"));
        assert_eq!(w.acted, w.sent, "{ctx}: every action run landed, in order");
    }

    /// Apply op `op` (see `OPS`).
    fn apply(&mut self, op: u8, pick: usize) {
        match op {
            0..=2 => {
                self.log.push(format!("a := {op}"));
                self.turn(|w| w.write(A, Some(op as i64)));
            }
            3..=4 => {
                let v = (op - 3) as i64;
                self.log.push(format!("b := {v}"));
                self.turn(|w| w.write(B, Some(v)));
            }
            5 => {
                let p = pending_gates();
                if !p.is_empty() {
                    let i = p[pick % p.len()];
                    let d = GATES.with(|g| {
                        let g = g.borrow();
                        format!("resolve {}({:?})#{}", g[i].q, g[i].input, g[i].k)
                    });
                    self.log.push(d);
                    resolve(i);
                }
                self.idle_turn();
            }
            6 => {
                self.log.push("refresh d1".into());
                self.turn(|_| {});
                let n = self.e.refresh(D1);
                let _ = n;
                self.drain_out();
                self.settle_and_pump();
                self.end_turn();
            }
            7 => {
                // unmount or remount the second question of `a` (a pane closing and opening)
                if self.qs[1].is_some() {
                    self.log.push("unmount f2".into());
                    self.unmount(1);
                    // a revocation can make a publication ready: the turn pumps after it (as the façade does
                    // after re-description, where scopes are released)
                    self.settle_and_pump();
                } else {
                    self.log.push("mount f2".into());
                    self.mount(1);
                    self.settle_and_pump();
                }
                self.end_turn();
            }
            8 => {
                self.log.push("a := a + 1, b := 1 - b".into());
                self.turn(|w| {
                    let a = w.vals[A].unwrap();
                    w.write(A, Some((a + 1) % 3));
                    let b = w.vals[B].unwrap();
                    w.write(B, Some(1 - b));
                });
            }
            9 => {
                let k = self.w.sent.len() as i64;
                self.log.push(format!("run action {k}"));
                self.w.sent.push(k);
                let g = open_gate("act", Some(k), 0);
                self.e.run(self.act, Work::once(GateFut(g)));
                self.drain_out();
                self.settle_and_pump();
                self.end_turn();
            }
            10 => {
                self.log.push("advance 5 ms".into());
                self.now += Duration::from_millis(4);
                self.idle_turn();
            }
            _ => {
                self.log.push("idle".into());
                self.idle_turn();
            }
        }
    }
}

const OPS: u8 = 12;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn env(k: &str, d: u64) -> u64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

#[test]
fn random_schedules() {
    let seeds = env("BOUGH_MODEL_SEEDS", 300);
    let steps = env("BOUGH_MODEL_STEPS", 200);
    let first = env("BOUGH_MODEL_FIRST_SEED", 1);
    for seed in first..first + seeds {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut h = Harness::new();
        h.check(&format!("seed {seed} mount"));
        for step in 0..steps {
            // completions are as likely as writes
            let op = match r.next() % 16 {
                0..=4 => 5,
                k => (k % OPS as u64) as u8,
            };
            h.apply(op, r.next() as usize);
            h.check(&format!("seed {seed} step {step}"));
        }
        h.drain(&format!("seed {seed} drained"));
    }
}

#[test]
fn exhaustive() {
    let depth = env("BOUGH_MODEL_DEPTH", 5) as u32;
    // ops: a := 0..2, b := 0, resolve the first / last pending, refresh, unmount/remount, both
    let alphabet: [(u8, usize); 11] = [(0, 0), (1, 0), (2, 0), (3, 0), (5, 0), (5, usize::MAX), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0)];
    let n = alphabet.len() as u64;
    let total = n.pow(depth);
    let shard = env("BOUGH_MODEL_SHARD", 0);
    let shards = env("BOUGH_MODEL_SHARDS", 1);
    let mut runs = 0u64;
    for code in (0..total).filter(|c| c % shards == shard) {
        let mut h = Harness::new();
        let mut c = code;
        for step in 0..depth {
            let (op, pick) = alphabet[(c % n) as usize];
            c /= n;
            h.apply(op, pick);
            h.check(&format!("schedule {code} step {step}"));
        }
        h.drain(&format!("schedule {code} drained"));
        runs += 1;
    }
    eprintln!("exhaustive: {runs} schedules of depth {depth} (shard {shard}/{shards})");
}

/// Deterministic cases from DESIGN.md, named.
#[test]
fn cascade_holds_the_selection_until_the_author_lands() {
    let mut h = Harness::new();
    // first answers
    while let Some(&i) = pending_gates().first() {
        resolve(i);
        h.idle_turn();
    }
    h.check("loaded");
    assert_eq!((h.w.shown(D1), h.w.shown(D3)), (Some(1), Some(13)));
    h.turn(|w| w.write(A, Some(2)));
    h.check("asked 2");
    assert!(h.w.held.contains(&A));
    // answer f1(2), f2(2) and f5(2, 0): the cascade asks f3(21); `a` stays held, d1 shows 1
    for q in ["f1", "f2", "f5"] {
        let i = pending_gates().into_iter().find(|&i| GATES.with(|g| g.borrow()[i].q == q)).unwrap();
        resolve(i);
        h.idle_turn();
        h.check(q);
    }
    // the stream's first item is still out too
    let i = pending_gates().into_iter().find(|&i| GATES.with(|g| g.borrow()[i].q == "fs")).unwrap();
    resolve(i);
    h.idle_turn();
    h.check("fs");
    assert_eq!(h.w.shown(A), Some(0), "the selection is still held by the cascade");
    assert_eq!(h.w.shown(D1), Some(1), "d1 shows the old answer while its own dependent loads");
    let i = pending_gates().into_iter().find(|&i| GATES.with(|g| g.borrow()[i].q == "f3")).unwrap();
    resolve(i);
    h.idle_turn();
    h.check("f3");
    assert_eq!((h.w.shown(A), h.w.shown(D1), h.w.shown(D3)), (Some(2), Some(21), Some(213)));
}

#[test]
fn netting_back_cancels_and_releases() {
    let mut h = Harness::new();
    while let Some(&i) = pending_gates().first() {
        resolve(i);
        h.idle_turn();
    }
    h.turn(|w| w.write(A, Some(1)));
    assert!(h.w.held.contains(&A));
    let asked = GATES.with(|g| g.borrow().len());
    h.turn(|w| w.write(A, Some(0)));
    h.check("netted");
    assert!(!h.w.held.contains(&A), "back to the shown input releases at once");
    let dropped = GATES.with(|g| g.borrow()[..asked].iter().filter(|x| x.dropped).count());
    assert!(dropped >= 2, "the requests for 1 were dropped");
    // a non-stream question asked nothing new; the stream restarted quietly
    let new: Vec<&'static str> = GATES.with(|g| g.borrow()[asked..].iter().map(|x| x.q).collect());
    assert_eq!(new, vec!["fs"]);
}
