//! The publication engine: questions, holds, publications, actions, and the flights they own.
//!
//! It knows nothing of cells, wires or records. A *cell* is an opaque `usize` (bough passes a head address),
//! an *owner* an opaque pointer handed back to the question's own `land`. The façade tells the engine when a
//! question's source was written (`written`), lands what the engine says is ready (`step`), and applies the
//! holds and releases the engine lists (`hold_out`, `release_out`, `status_out`). See `DESIGN.md` of lane
//! `async` for the rules; the names below follow it.
//!
//! * A **flight** is a future or an async iterator, owned by exactly one question or action run. Dropping a
//!   flight is its cancellation. Flights are polled on the thread that owns the engine: once when started, then
//!   whenever their waker rang (at the next `poll`, never as a test inside an idle turn).
//! * A **publication** is the questions one turn asked loudly, with the source cells they hold. It commits when
//!   none of its questions is still in the air: the façade lands every answer and settles in one turn; questions
//!   fired by that settle join the same publication (a cascade keeps the hold); if none did, its cells are
//!   released.
use crate::time::{with_timers, Timers};
use std::any::Any;
use std::async_iter::AsyncIterator;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

/// A type-erased answer or input.
pub type Out = Box<dyn Any>;

/// A flight's work: one answer, or many.
pub enum Work {
    Once(Pin<Box<dyn Future<Output = Out>>>),
    Many(Pin<Box<dyn AsyncIterator<Item = Out>>>),
}

impl Work {
    pub fn once<F>(f: F) -> Work
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        Work::Once(Box::pin(async move { Box::new(f.await) as Out }))
    }
    pub fn many<S>(s: S) -> Work
    where
        S: AsyncIterator + 'static,
        S::Item: 'static,
    {
        Work::Many(Box::pin(Erase(Box::pin(s))))
    }
}

struct Erase<S>(Pin<Box<S>>);
impl<S: AsyncIterator> AsyncIterator for Erase<S>
where
    S::Item: 'static,
{
    type Item = Out;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Out>> {
        self.0.as_mut().poll_next(cx).map(|o| o.map(|v| Box::new(v) as Out))
    }
}

/// A question, type-erased (built by the façade from the author's closures).
pub trait Question {
    /// The input, read from the owner's cells now.
    ///
    /// # Safety
    /// `owner` is the live owner the question was mounted for.
    unsafe fn input(&self, owner: *mut u8) -> Out;
    fn same(&self, a: &dyn Any, b: &dyn Any) -> bool;
    fn clone_input(&self, a: &dyn Any) -> Out;
    /// The work for `input`.
    fn start(&self, input: &dyn Any) -> Work;
    /// Land one answer (`first`: the first since the input changed).
    ///
    /// # Safety
    /// As `input`.
    unsafe fn land(&self, owner: *mut u8, out: Out, first: bool);
    fn is_stream(&self) -> bool;
}

/// Where an action's answers go (built by the façade; it knows the action and its owner).
pub trait Landing {
    /// # Safety
    /// Called only while the action is mounted (its scope not released).
    unsafe fn land(&self, out: Out);
    /// One run is over: its input leaves the action's in-flight list.
    ///
    /// # Safety
    /// As `land`.
    unsafe fn finished(&self);
}

/// How a question holds.
#[derive(Clone, Copy, Debug, Default)]
pub struct QOpts {
    /// Hold its sources while it is out (false: `deferred`).
    pub hold: bool,
    /// Ask again quietly at this interval.
    pub every: Option<Duration>,
    /// Its landing writes only its `about` cell (`into`). A landing handler (`then`) may write anything, so the
    /// engine treats it as feeding other questions (it lands in a commit's first phase).
    pub lands_into: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AId(pub u32);

/// What `step` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing ready.
    Idle,
    /// Answers outside any publication landed (first answers, deferred and quiet questions, later stream items,
    /// action answers): settle, and step again.
    Landed,
    /// Publication `p`'s answers landed: settle (questions fired now join `p`), then `end_commit(p)`.
    Committing(u32),
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Stats {
    pub asked: u64,
    pub quiet: u64,
    pub netted: u64,
    pub dropped: u64,
    pub landed: u64,
    pub commits: u64,
    pub cascades: u64,
    pub merges: u64,
    pub polls: u64,
    pub action_runs: u64,
}

// ------------------------------------------------------------------------------------------------- wakers

struct Bell {
    woken: Mutex<Vec<(u32, u32)>>,
    ring: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

struct FWaker {
    bell: Arc<Bell>,
    id: u32,
    generation: u32,
    armed: AtomicBool,
}

impl Wake for FWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref()
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if self.armed.swap(false, Ordering::AcqRel) {
            self.bell.woken.lock().unwrap().push((self.id, self.generation));
            if let Some(r) = self.bell.ring.lock().unwrap().as_ref() {
                r();
            }
        }
    }
}

#[derive(Clone, Copy)]
enum FOwner {
    Q(u32),
    A(u32, u64),
}

struct Flight {
    work: Work,
    waker: Arc<FWaker>,
    owner: FOwner,
}

enum Polled {
    Pending,
    Item(Out),
    Done(Option<Out>),
}

// ------------------------------------------------------------------------------------------------- slots

struct QSlot {
    q: Option<Box<dyn Question>>,
    owner: *mut u8,
    sources: Vec<usize>,
    about: usize,
    opts: QOpts,
    seq: u64,
    /// Input of the flight in the air or of its staged answer (None: idle).
    asked: Option<Out>,
    /// Input whose answer has landed (None: never answered).
    shown: Option<Out>,
    flight: Option<u32>,
    staged: VecDeque<Out>,
    /// The flight ended (a stream may end with nothing).
    done: bool,
    /// The next landing is the first since the input changed.
    first: bool,
    /// Its flight was asked loudly (it holds its sources); a quiet member of a publication only waits in it.
    loud: bool,
    pubn: Option<u32>,
    due: Option<Duration>,
    /// `about` is shown as loading (status published to the façade).
    loading: bool,
}

struct Run {
    seq: u64,
    flight: Option<u32>,
    items: VecDeque<Out>,
    done: bool,
}

struct ASlot {
    land: Option<Box<dyn Landing>>,
    runs: VecDeque<Run>,
    next: u64,
}

struct Pub {
    members: Vec<u32>,
    /// While committing: staged members that feed no other question, landed in the second phase (or kept for
    /// the continuation if the first phase started a cascade).
    later: Vec<u32>,
    cells: Vec<usize>,
    /// Being formed by the current turn (or committing): not ready to commit yet.
    open: bool,
}

/// The engine of one runtime (see the module docs).
pub struct Engine {
    bell: Arc<Bell>,
    timers: Box<Timers>,
    flights: Vec<Option<Flight>>,
    fgen: Vec<u32>,
    ffree: Vec<u32>,
    qs: Vec<QSlot>,
    qfree: Vec<u32>,
    seq: u64,
    acts: Vec<ASlot>,
    afree: Vec<u32>,
    pubs: Vec<Option<Pub>>,
    pfree: Vec<u32>,
    /// Held cell -> the publication holding it.
    held: HashMap<usize, u32>,
    /// Source cell -> how many live questions read it.
    sourced: HashMap<usize, u32>,
    /// The publication the current turn's loud questions join (None: none yet).
    group: Option<u32>,
    /// Questions with answers to land outside any publication.
    alone: Vec<u32>,
    /// Actions whose first run has something to land.
    acts_ready: Vec<u32>,
    /// Flights in the air, live publications, questions with an interval: `quiet` is three compares.
    nflights: u32,
    npubs: u32,
    nevery: u32,
    /// Cells newly held / released, and `about` cells whose loading status changed: the façade drains them.
    pub hold_out: Vec<usize>,
    pub release_out: Vec<usize>,
    pub status_out: Vec<usize>,
    pub stats: Stats,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // flights (and the sleeps inside them) go before the timers
        let timers: *const Timers = &*self.timers;
        // SAFETY: `timers` lives until the end of this function (it is a field dropped after this body).
        with_timers(unsafe { &*timers }, || self.flights.clear());
    }
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            bell: Arc::new(Bell { woken: Mutex::new(Vec::new()), ring: Mutex::new(None) }),
            timers: Box::default(),
            flights: Vec::new(),
            fgen: Vec::new(),
            ffree: Vec::new(),
            qs: Vec::new(),
            qfree: Vec::new(),
            seq: 0,
            acts: Vec::new(),
            afree: Vec::new(),
            pubs: Vec::new(),
            pfree: Vec::new(),
            held: HashMap::new(),
            sourced: HashMap::new(),
            group: None,
            alone: Vec::new(),
            acts_ready: Vec::new(),
            nflights: 0,
            npubs: 0,
            nevery: 0,
            hold_out: Vec::new(),
            release_out: Vec::new(),
            status_out: Vec::new(),
            stats: Stats::default(),
        }
    }

    /// Called (from any thread) when a flight wakes: the host asks for a turn.
    pub fn set_ring(&self, f: Arc<dyn Fn() + Send + Sync>) {
        *self.bell.ring.lock().unwrap() = Some(f);
    }

    pub fn now(&self) -> Duration {
        self.timers.now()
    }
    /// The earliest moment something is due (a sleep or an `every`), for the host's timer.
    pub fn next_wake(&self) -> Option<Duration> {
        if self.nevery == 0 {
            return self.timers.next_deadline();
        }
        let q = self.qs.iter().filter(|q| q.q.is_some()).filter_map(|q| q.due).min();
        match (self.timers.next_deadline(), q) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    /// Anything in the air?
    pub fn busy(&self) -> bool {
        self.nflights != 0
    }
    /// Nothing in the air, nothing staged, no publication, no interval and no sleep: a turn has nothing to ask
    /// of the engine (the cost of async to an idle turn is this test).
    #[inline]
    pub fn quiet(&self) -> bool {
        self.nflights == 0 && self.npubs == 0 && self.nevery == 0 && self.alone.is_empty() && self.acts_ready.is_empty()
    }
    /// Flights rung and not yet polled.
    pub fn woken(&self) -> bool {
        !self.bell.woken.lock().unwrap().is_empty()
    }
    pub fn flights(&self) -> usize {
        self.flights.iter().filter(|f| f.is_some()).count()
    }
    /// Cells held now.
    pub fn held_cells(&self) -> usize {
        self.held.len()
    }
    pub fn is_held(&self, cell: usize) -> bool {
        self.held.contains_key(&cell)
    }
    /// Live publications.
    pub fn publications(&self) -> usize {
        self.pubs.iter().filter(|p| p.is_some()).count()
    }
    /// Is `cell` the subject of a question in the air?
    pub fn loading(&self, cell: usize) -> bool {
        self.qs.iter().any(|q| q.q.is_some() && q.about == cell && q.loading)
    }

    // --------------------------------------------------------------------------------------------- flights

    fn start_flight(&mut self, work: Work, owner: FOwner) -> u32 {
        let id = match self.ffree.pop() {
            Some(i) => i,
            None => {
                self.flights.push(None);
                self.fgen.push(0);
                (self.flights.len() - 1) as u32
            }
        };
        self.fgen[id as usize] = self.fgen[id as usize].wrapping_add(1);
        let waker = Arc::new(FWaker { bell: self.bell.clone(), id, generation: self.fgen[id as usize], armed: AtomicBool::new(true) });
        self.flights[id as usize] = Some(Flight { work, waker, owner });
        self.nflights += 1;
        id
    }

    /// Drop a flight (its cancellation).
    fn drop_flight(&mut self, id: u32) {
        let f = self.flights[id as usize].take();
        if f.is_some() {
            self.nflights -= 1;
        }
        self.fgen[id as usize] = self.fgen[id as usize].wrapping_add(1);
        self.ffree.push(id);
        let timers: *const Timers = &*self.timers;
        // SAFETY: the timers are boxed and outlive this call.
        with_timers(unsafe { &*timers }, || drop(f));
    }

    /// Poll flight `id` once.
    fn poll_flight(&mut self, id: u32) -> Polled {
        let Some(f) = self.flights[id as usize].as_mut() else { return Polled::Pending };
        f.waker.armed.store(true, Ordering::Release);
        let waker = Waker::from(f.waker.clone());
        let mut cx = Context::from_waker(&waker);
        self.stats.polls += 1;
        let timers: *const Timers = &*self.timers;
        // SAFETY: the timers are boxed and outlive this call.
        let r = with_timers(unsafe { &*timers }, || match &mut f.work {
            Work::Once(fut) => match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => Polled::Done(Some(v)),
                Poll::Pending => Polled::Pending,
            },
            Work::Many(s) => match s.as_mut().poll_next(&mut cx) {
                Poll::Ready(Some(v)) => Polled::Item(v),
                Poll::Ready(None) => Polled::Done(None),
                Poll::Pending => Polled::Pending,
            },
        });
        match r {
            Polled::Done(_) => {
                let f = self.flights[id as usize].take();
                self.nflights -= 1;
                self.fgen[id as usize] = self.fgen[id as usize].wrapping_add(1);
                self.ffree.push(id);
                with_timers(unsafe { &*timers }, || drop(f));
            }
            Polled::Item(_) => {
                // one item per turn: the stream is polled again at the next turn (after a frame)
                if let Some(f) = self.flights[id as usize].as_ref() {
                    Waker::from(f.waker.clone()).wake_by_ref();
                }
            }
            Polled::Pending => {}
        }
        r
    }

    /// Route a poll result to the flight's owner.
    fn deliver(&mut self, owner: FOwner, r: Polled) {
        match owner {
            FOwner::Q(qi) => {
                let q = &mut self.qs[qi as usize];
                match r {
                    Polled::Pending => return,
                    Polled::Item(v) => q.staged.push_back(v),
                    Polled::Done(v) => {
                        q.flight = None;
                        q.done = true;
                        if let Some(v) = v {
                            q.staged.push_back(v);
                        }
                    }
                }
                if q.pubn.is_none() && !self.alone.contains(&qi) {
                    self.alone.push(qi);
                }
            }
            FOwner::A(ai, seq) => {
                let a = &mut self.acts[ai as usize];
                let Some(run) = a.runs.iter_mut().find(|r| r.seq == seq) else { return };
                match r {
                    Polled::Pending => return,
                    Polled::Item(v) => run.items.push_back(v),
                    Polled::Done(v) => {
                        run.flight = None;
                        run.done = true;
                        if let Some(v) = v {
                            run.items.push_back(v);
                        }
                    }
                }
                if !self.acts_ready.contains(&ai) {
                    self.acts_ready.push(ai);
                }
            }
        }
    }

    /// Advance the clock to `now`, run what is due, and poll every flight that rang. Returns whether anything
    /// may be ready to land (`step`).
    pub fn poll(&mut self, now: Duration) -> bool {
        self.timers.advance(now);
        // `every`: quiet re-asks that are due
        let due: Vec<u32> = (0..self.qs.len() as u32).filter(|&i| self.qs[i as usize].q.is_some() && self.qs[i as usize].due.is_some_and(|d| d <= now)).collect();
        for qi in due {
            let q = &mut self.qs[qi as usize];
            q.due = q.opts.every.map(|e| now + e);
            self.reask(qi);
        }
        let woken = std::mem::take(&mut *self.bell.woken.lock().unwrap());
        for (id, g) in woken {
            if self.fgen.get(id as usize) != Some(&g) {
                continue; // dropped (and maybe reused) since it rang
            }
            let Some(owner) = self.flights[id as usize].as_ref().map(|f| f.owner) else { continue };
            let r = self.poll_flight(id);
            self.deliver(owner, r);
        }
        !self.alone.is_empty() || !self.acts_ready.is_empty() || self.ready_pub().is_some()
    }

    // --------------------------------------------------------------------------------------------- questions

    /// Mount a question (its first request is `first`).
    pub fn mount(&mut self, q: Box<dyn Question>, owner: *mut u8, sources: Vec<usize>, about: usize, opts: QOpts) -> QId {
        self.seq += 1;
        let slot = QSlot { q: Some(q), owner, sources, about, opts, seq: self.seq, asked: None, shown: None, flight: None, staged: VecDeque::new(), done: false, first: true, loud: false, pubn: None, due: None, loading: false };
        let i = match self.qfree.pop() {
            Some(i) => {
                self.qs[i as usize] = slot;
                i
            }
            None => {
                self.qs.push(slot);
                (self.qs.len() - 1) as u32
            }
        };
        if let Some(e) = opts.every {
            self.qs[i as usize].due = Some(self.timers.now() + e);
            self.nevery += 1;
        }
        for s in self.qs[i as usize].sources.clone() {
            *self.sourced.entry(s).or_default() += 1;
        }
        QId(i)
    }

    /// The first request of a just-mounted question: asked with the current input, holding nothing.
    pub fn first(&mut self, id: QId) {
        let qi = id.0;
        let q = &self.qs[qi as usize];
        let Some(qq) = q.q.as_ref() else { return };
        // SAFETY: the owner is the live owner the façade mounted the question for.
        let input = unsafe { qq.input(q.owner) };
        self.ask(qi, input, false);
    }

    /// A source cell of question `id` was written: compare its input and ask, net, or do nothing.
    pub fn written(&mut self, id: QId) {
        let qi = id.0;
        let q = &self.qs[qi as usize];
        let Some(qq) = q.q.as_ref() else { return };
        // SAFETY: as `first`.
        let input = unsafe { qq.input(q.owner) };
        if q.asked.as_ref().is_some_and(|a| qq.same(&*input, &**a)) {
            return; // the same question is in the air (or its answer is staged)
        }
        if q.shown.as_ref().is_some_and(|s| qq.same(&*input, &**s)) {
            if q.asked.is_none() {
                return; // idle, and its answer is the one shown
            }
            // back to the input whose answer is shown: drop what is in the air
            let stream = qq.is_stream();
            self.stats.netted += 1;
            self.cancel_q(qi);
            self.leave(qi);
            self.set_loading(qi, false);
            if stream {
                // a stream's answer is live: ask the shown input again, quietly
                self.ask(qi, input, false);
            }
            return;
        }
        let loud = q.opts.hold && q.shown.is_some();
        self.ask(qi, input, loud);
    }

    /// Ask again with the same input, holding nothing (`refresh`, `every`).
    fn reask(&mut self, qi: u32) {
        let q = &self.qs[qi as usize];
        if q.flight.is_some() || q.asked.is_some() {
            return; // an answer for the current input is coming
        }
        let Some(qq) = q.q.as_ref() else { return };
        // SAFETY: as `first`.
        let input = unsafe { qq.input(q.owner) };
        self.stats.quiet += 1;
        self.ask(qi, input, false);
    }

    /// Re-ask every question about `cell` (`refresh`). Returns how many were asked.
    pub fn refresh(&mut self, cell: usize) -> usize {
        let ids: Vec<u32> = (0..self.qs.len() as u32).filter(|&i| self.qs[i as usize].q.is_some() && self.qs[i as usize].about == cell).collect();
        for &qi in &ids {
            self.reask(qi);
        }
        ids.len()
    }

    /// Start `input`'s work for question `qi`, dropping whatever was in the air. `loud`: join the turn's
    /// publication and hold the sources.
    fn ask(&mut self, qi: u32, input: Out, loud: bool) {
        self.cancel_q(qi);
        self.stats.asked += 1;
        let work = {
            let q = &self.qs[qi as usize];
            q.q.as_ref().unwrap().start(&*input)
        };
        let q = &mut self.qs[qi as usize];
        q.asked = Some(input);
        q.done = false;
        q.first = true;
        q.loud = loud;
        let f = self.start_flight(work, FOwner::Q(qi));
        self.qs[qi as usize].flight = Some(f);
        if loud {
            self.join(qi);
        } else {
            self.leave(qi);
            if let Some(p) = self.holder_of(qi) {
                self.enter(qi, p);
            }
        }
        self.set_loading(qi, true);
        // polled once at once: an answer ready on its first poll lands in this turn
        let r = self.poll_flight(f);
        self.deliver(FOwner::Q(qi), r);
    }

    fn set_loading(&mut self, qi: u32, on: bool) {
        let q = &mut self.qs[qi as usize];
        if q.loading != on {
            q.loading = on;
            if q.about != 0 {
                self.status_out.push(q.about);
            }
        }
    }

    /// Drop question `qi`'s flight and staged answers.
    fn cancel_q(&mut self, qi: u32) {
        let q = &mut self.qs[qi as usize];
        q.staged.clear();
        q.asked = None;
        q.done = false;
        if let Some(f) = q.flight.take() {
            self.stats.dropped += 1;
            self.drop_flight(f);
        }
        self.alone.retain(|&x| x != qi);
    }

    /// Join the turn's publication (making one if needed), carrying the question's old publication along and
    /// merging any publication that holds one of its sources; hold its sources.
    fn join(&mut self, qi: u32) {
        let g = match self.group {
            Some(g) if self.pubs[g as usize].is_some() => g,
            _ => {
                let g = self.new_pub();
                self.group = Some(g);
                g
            }
        };
        if let Some(p) = self.qs[qi as usize].pubn {
            if p != g {
                self.merge(p, g);
            }
        }
        self.qs[qi as usize].pubn = Some(g);
        let pb = self.pubs[g as usize].as_mut().unwrap();
        if !pb.members.contains(&qi) {
            pb.members.push(qi);
        }
        let sources = self.qs[qi as usize].sources.clone();
        for s in sources {
            match self.held.get(&s).copied() {
                Some(p) if p == g => {}
                Some(p) => self.merge(p, g),
                None => {
                    self.held.insert(s, g);
                    self.pubs[g as usize].as_mut().unwrap().cells.push(s);
                    self.hold_out.push(s);
                }
            }
        }
    }

    /// The publication holding one of question `qi`'s sources, if any.
    fn holder_of(&self, qi: u32) -> Option<u32> {
        self.qs[qi as usize].sources.iter().find_map(|s| self.held.get(s).copied())
    }

    /// Enter publication `p` as a member that holds nothing (its answer waits for `p`'s commit). Every other
    /// publication holding one of its sources is merged into `p`.
    fn enter(&mut self, qi: u32, p: u32) {
        let sources = self.qs[qi as usize].sources.clone();
        for s in sources {
            if let Some(o) = self.held.get(&s).copied() {
                if o != p {
                    self.merge(o, p);
                }
            }
        }
        let p = self.held.get(&self.qs[qi as usize].sources[0]).copied().unwrap_or(p);
        self.qs[qi as usize].pubn = Some(p);
        let pb = self.pubs[p as usize].as_mut().unwrap();
        if !pb.members.contains(&qi) {
            pb.members.push(qi);
        }
    }

    /// Leave its publication (a netted, dropped or now-quiet question).
    fn leave(&mut self, qi: u32) {
        if let Some(p) = self.qs[qi as usize].pubn.take() {
            if let Some(pb) = self.pubs[p as usize].as_mut() {
                pb.members.retain(|&m| m != qi);
            }
        }
    }

    fn new_pub(&mut self) -> u32 {
        self.npubs += 1;
        let p = Pub { members: Vec::new(), later: Vec::new(), cells: Vec::new(), open: true };
        match self.pfree.pop() {
            Some(i) => {
                self.pubs[i as usize] = Some(p);
                i
            }
            None => {
                self.pubs.push(Some(p));
                (self.pubs.len() - 1) as u32
            }
        }
    }

    /// Merge publication `a` into `b`.
    fn merge(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        let Some(pa) = self.pubs[a as usize].take() else { return };
        self.pfree.push(a);
        self.npubs -= 1;
        self.stats.merges += 1;
        for &m in &pa.members {
            self.qs[m as usize].pubn = Some(b);
        }
        for &c in &pa.cells {
            self.held.insert(c, b);
        }
        for &m in &pa.later {
            self.qs[m as usize].pubn = Some(b);
        }
        let pb = self.pubs[b as usize].as_mut().unwrap();
        pb.later.extend(pa.later);
        for m in pa.members {
            if !pb.members.contains(&m) {
                pb.members.push(m);
            }
        }
        pb.cells.extend(pa.cells);
        if self.group == Some(a) {
            self.group = Some(b);
        }
    }

    /// A question's scope was released: it asks nothing more, its flight is dropped, it leaves its publication.
    pub fn revoke(&mut self, id: QId) {
        let qi = id.0;
        if self.qs[qi as usize].q.is_none() {
            return;
        }
        self.cancel_q(qi);
        self.leave(qi);
        self.set_loading(qi, false);
        let q = &mut self.qs[qi as usize];
        q.q = None;
        q.shown = None;
        if q.due.take().is_some() {
            self.nevery -= 1;
        }
        for s in std::mem::take(&mut q.sources) {
            if let Some(n) = self.sourced.get_mut(&s) {
                *n -= 1;
                if *n == 0 {
                    self.sourced.remove(&s);
                    // no live question reads it: nothing on screen can wait on it any more
                    if let Some(p) = self.held.remove(&s) {
                        if let Some(pb) = self.pubs[p as usize].as_mut() {
                            pb.cells.retain(|&c| c != s);
                        }
                        self.release_out.push(s);
                    }
                }
            }
        }
        self.qfree.push(qi);
    }

    // --------------------------------------------------------------------------------------------- turns

    /// A turn's loud questions start a new publication from here (`App::turn` calls it before the handler).
    pub fn begin_turn(&mut self) {
        self.close_group();
    }

    /// The current turn's publication is complete in membership.
    pub fn close_group(&mut self) {
        if let Some(g) = self.group.take() {
            if let Some(p) = self.pubs[g as usize].as_mut() {
                p.open = false;
            }
        }
    }

    fn member_ready(&self, qi: u32) -> bool {
        let q = &self.qs[qi as usize];
        !q.staged.is_empty() || q.done
    }

    /// A closed publication whose members are all answered, or with no loud member left (nothing it holds is
    /// waited on: its quiet members lag, which is their contract, and cannot lead once the cells are released).
    fn ready_pub(&self) -> Option<u32> {
        (0..self.pubs.len() as u32).find(|&p| {
            self.pubs[p as usize].as_ref().is_some_and(|pb| !pb.open && (pb.members.iter().all(|&m| self.member_ready(m)) || !pb.members.iter().any(|&m| self.qs[m as usize].loud)))
        })
    }

    /// The engine's state, for failure messages.
    pub fn dump(&self) -> String {
        use std::fmt::Write;
        let mut o = String::new();
        for (i, p) in self.pubs.iter().enumerate() {
            if let Some(p) = p {
                let _ = writeln!(o, "pub {i}: members {:?} later {:?} cells {:?} open {}", p.members, p.later, p.cells, p.open);
            }
        }
        for (i, q) in self.qs.iter().enumerate() {
            if q.q.is_some() {
                let _ = writeln!(o, "q {i}: flight {:?} staged {} done {} loud {} pub {:?} asked {} shown {}", q.flight, q.staged.len(), q.done, q.loud, q.pubn, q.asked.is_some(), q.shown.is_some());
            }
        }
        let _ = writeln!(o, "group {:?} held {:?} alone {:?}", self.group, self.held, self.alone);
        o
    }

    /// Land what is ready, one unit at a time (see [`Step`]). The façade settles after each.
    pub fn step(&mut self) -> Step {
        // the questions fired by the settle that followed the previous step form their own publication
        self.close_group();
        if let Some(p) = self.ready_pub() {
            self.stats.commits += 1;
            let mut members = std::mem::take(&mut self.pubs[p as usize].as_mut().unwrap().members);
            members.sort_by_key(|&m| self.qs[m as usize].seq);
            // quiet members still in the air leave (a publication with no loud member left is released)
            let (members, out): (Vec<u32>, Vec<u32>) = members.into_iter().partition(|&m| self.member_ready(m));
            for m in out {
                self.qs[m as usize].pubn = None;
            }
            // first phase: the members whose answers feed other questions (a cascade can only start from them)
            let (feed, later): (Vec<u32>, Vec<u32>) = members.into_iter().partition(|&m| self.feeds(m));
            let pb = self.pubs[p as usize].as_mut().unwrap();
            pb.open = true;
            pb.later = later;
            // a cascade fired by the landings joins this publication: it is the turn's group while committing
            self.group = Some(p);
            for m in feed {
                self.qs[m as usize].pubn = None;
                self.land_q(m);
            }
            return Step::Committing(p);
        }
        if !self.alone.is_empty() {
            let mut list = std::mem::take(&mut self.alone);
            list.sort_by_key(|&m| self.qs[m as usize].seq);
            let mut landed = false;
            for m in list {
                if self.qs[m as usize].q.is_none() || self.qs[m as usize].pubn.is_some() {
                    continue;
                }
                // an answer computed from a held cell's intent must not show before that cell does: it waits in
                // the publication that holds the cell
                if let Some(p) = self.holder_of(m) {
                    self.enter(m, p);
                    continue;
                }
                self.land_q(m);
                landed = true;
            }
            if landed {
                return Step::Landed;
            }
            return self.step();
        }
        if !self.acts_ready.is_empty() {
            for ai in std::mem::take(&mut self.acts_ready) {
                self.land_a(ai);
            }
            return Step::Landed;
        }
        Step::Idle
    }

    fn land_q(&mut self, qi: u32) {
        let q = &mut self.qs[qi as usize];
        let Some(qq) = q.q.as_ref() else { return };
        let items: Vec<Out> = q.staged.drain(..).collect();
        if items.is_empty() && !q.done {
            return;
        }
        let owner = q.owner;
        // the input these answers answer is the one shown from now on
        if q.done {
            q.shown = q.asked.take();
        } else if let Some(a) = q.asked.as_ref() {
            q.shown = Some(qq.clone_input(&**a));
        }
        let mut first = q.first;
        let n = items.len();
        q.first &= n == 0;
        let done = q.done;
        // landings write cells (the façade's settle follows); the question table is not touched meanwhile
        let qq: *const dyn Question = &**q.q.as_ref().unwrap();
        for v in items {
            self.stats.landed += 1;
            // SAFETY: the question is alive (checked above) and nothing below drops it before the loop ends;
            // the owner is the live owner it was mounted for.
            unsafe { (*qq).land(owner, v, first) };
            first = false;
        }
        if done {
            self.set_loading(qi, false);
        }
    }

    /// Does question `m`'s landing (possibly) write a cell another question reads?
    fn feeds(&self, m: u32) -> bool {
        let q = &self.qs[m as usize];
        !q.opts.lands_into || (q.about != 0 && self.sourced.contains_key(&q.about))
    }

    /// After the settle that followed `Step::Committing(p)`. If questions fired by that settle joined `p` (a
    /// cascade), the hold goes on and the members not yet landed wait in `p` with it: returns false. Otherwise
    /// the remaining members land now and this returns true: the façade settles and calls `end_commit` again,
    /// which releases `p`'s cells (`release_out`) and returns false.
    pub fn end_commit(&mut self, p: u32) -> bool {
        if self.group == Some(p) {
            self.group = None;
        }
        let Some(pb) = self.pubs[p as usize].as_mut() else { return false };
        if !pb.members.is_empty() {
            // a cascade: what has not landed waits for it
            self.stats.cascades += 1;
            let later = std::mem::take(&mut pb.later);
            for m in later {
                if !pb.members.contains(&m) {
                    pb.members.push(m);
                }
            }
            pb.open = false;
            return false;
        }
        if !pb.later.is_empty() {
            let later = std::mem::take(&mut pb.later);
            self.group = Some(p);
            for m in later {
                self.qs[m as usize].pubn = None;
                self.land_q(m);
            }
            return true;
        }
        let pb = self.pubs[p as usize].take().unwrap();
        self.pfree.push(p);
        self.npubs -= 1;
        for c in pb.cells {
            self.held.remove(&c);
            self.release_out.push(c);
        }
        false
    }

    // --------------------------------------------------------------------------------------------- actions

    pub fn add_action(&mut self, land: Box<dyn Landing>) -> AId {
        let a = ASlot { land: Some(land), runs: VecDeque::new(), next: 0 };
        match self.afree.pop() {
            Some(i) => {
                self.acts[i as usize] = a;
                AId(i)
            }
            None => {
                self.acts.push(a);
                AId((self.acts.len() - 1) as u32)
            }
        }
    }

    /// Start one run of action `id`.
    pub fn run(&mut self, id: AId, work: Work) {
        let ai = id.0;
        if self.acts[ai as usize].land.is_none() {
            return;
        }
        self.stats.action_runs += 1;
        let a = &mut self.acts[ai as usize];
        let seq = a.next;
        a.next += 1;
        a.runs.push_back(Run { seq, flight: None, items: VecDeque::new(), done: false });
        let f = self.start_flight(work, FOwner::A(ai, seq));
        self.acts[ai as usize].runs.back_mut().unwrap().flight = Some(f);
        let r = self.poll_flight(f);
        self.deliver(FOwner::A(ai, seq), r);
    }

    /// Drop every run of action `id` (their inputs leave `inflight` through `finished`).
    pub fn cancel(&mut self, id: AId) {
        let ai = id.0;
        let runs = std::mem::take(&mut self.acts[ai as usize].runs);
        for r in runs {
            if let Some(f) = r.flight {
                self.stats.dropped += 1;
                self.drop_flight(f);
            }
            if let Some(l) = self.acts[ai as usize].land.as_ref() {
                // SAFETY: the action is mounted (`land` is Some until `remove_action`).
                unsafe { l.finished() };
            }
        }
        self.acts_ready.retain(|&x| x != ai);
    }

    /// The action's scope was released: its runs are dropped and nothing of it lands again.
    pub fn remove_action(&mut self, id: AId) {
        let ai = id.0;
        let runs = std::mem::take(&mut self.acts[ai as usize].runs);
        for r in runs {
            if let Some(f) = r.flight {
                self.drop_flight(f);
            }
        }
        self.acts[ai as usize].land = None;
        self.acts_ready.retain(|&x| x != ai);
        self.afree.push(ai);
    }

    /// Land action `ai`'s answers in the order of its runs: a later run's answers wait for the earlier runs.
    fn land_a(&mut self, ai: u32) {
        loop {
            let a = &mut self.acts[ai as usize];
            let Some(land) = a.land.as_ref() else { return };
            let land: *const dyn Landing = &**land;
            let Some(front) = a.runs.front_mut() else { return };
            let items: Vec<Out> = front.items.drain(..).collect();
            let done = front.done;
            if done {
                a.runs.pop_front();
            }
            for v in items {
                self.stats.landed += 1;
                // SAFETY: mounted (checked above); the landing does not reach the engine.
                unsafe { (*land).land(v) };
            }
            if !done {
                return;
            }
            // SAFETY: as above.
            unsafe { (*land).finished() };
        }
    }

    /// Runs of action `id` not yet landed.
    pub fn runs(&self, id: AId) -> usize {
        self.acts[id.0 as usize].runs.len()
    }
}
