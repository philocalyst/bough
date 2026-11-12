//! The runtime's clock: `sleep`, `timeout` and the engine's own deadlines (`every`).
//!
//! Time is whatever the engine was last told (`Engine::poll(now)`): the host passes the run loop's clock, a
//! test passes a fake one, so a test that sleeps for 30 s runs in microseconds and in a fixed order. A `Sleep`
//! registers its deadline with the engine that polls it (through a thread-local set only while the engine
//! polls); it is a UI-thread timer and panics if polled anywhere else.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

/// Deadlines of the sleeps of one engine.
#[derive(Default)]
pub struct Timers {
    pub(crate) now: Cell<Duration>,
    next: Cell<u64>,
    /// (deadline, key) -> waker, in deadline order.
    due: RefCell<BTreeMap<(Duration, u64), Waker>>,
    /// key -> deadline (to unregister).
    keys: RefCell<HashMap<u64, Duration>>,
}

impl Timers {
    pub fn now(&self) -> Duration {
        self.now.get()
    }
    /// The earliest deadline, if any sleep waits.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.due.borrow().keys().next().map(|k| k.0)
    }
    /// Advance to `now` and wake every sleep whose deadline has passed.
    pub(crate) fn advance(&self, now: Duration) {
        if now > self.now.get() {
            self.now.set(now);
        }
        loop {
            let first = {
                let mut d = self.due.borrow_mut();
                match d.first_key_value() {
                    Some((k, _)) if k.0 <= self.now.get() => {
                        let k = *k;
                        d.remove(&k).map(|w| (k, w))
                    }
                    _ => None,
                }
            };
            let Some((k, w)) = first else { break };
            self.keys.borrow_mut().remove(&k.1);
            w.wake();
        }
    }
    fn register(&self, deadline: Duration, w: &Waker, key: Option<u64>) -> u64 {
        let key = key.unwrap_or_else(|| {
            let k = self.next.get();
            self.next.set(k + 1);
            k
        });
        let mut keys = self.keys.borrow_mut();
        if let Some(old) = keys.insert(key, deadline) {
            self.due.borrow_mut().remove(&(old, key));
        }
        self.due.borrow_mut().insert((deadline, key), w.clone());
        key
    }
    fn unregister(&self, key: u64) {
        if let Some(d) = self.keys.borrow_mut().remove(&key) {
            self.due.borrow_mut().remove(&(d, key));
        }
    }
    /// Sleeps waiting (diagnostics, tests).
    pub fn waiting(&self) -> usize {
        self.keys.borrow().len()
    }
}

thread_local! {
    /// The timers of the engine polling on this thread (null outside its polls).
    pub(crate) static CURRENT: Cell<*const Timers> = const { Cell::new(std::ptr::null()) };
}

/// Run `f` with `t` as this thread's timers (the engine's polls).
pub(crate) fn with_timers<R>(t: &Timers, f: impl FnOnce() -> R) -> R {
    let prev = CURRENT.with(|c| c.replace(t as *const Timers));
    struct Restore(*const Timers);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|c| c.set(self.0));
        }
    }
    let _r = Restore(prev);
    f()
}

fn current<R>(f: impl FnOnce(&Timers) -> R) -> R {
    let p = CURRENT.with(|c| c.get());
    assert!(!p.is_null(), "bough's sleep/timeout polled outside bough's executor (they are UI-thread timers)");
    // SAFETY: set by `with_timers` for the duration of an engine poll; the engine (and its timers) outlive it.
    f(unsafe { &*p })
}

/// The engine's time now (inside a future polled by bough).
pub fn now() -> Duration {
    current(|t| t.now())
}

/// Completes `d` after its first poll.
pub fn sleep(d: Duration) -> Sleep {
    Sleep { d, deadline: None, key: None, timers: std::ptr::null() }
}

pub struct Sleep {
    d: Duration,
    deadline: Option<Duration>,
    key: Option<u64>,
    timers: *const Timers,
}

impl Future for Sleep {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        current(|t| {
            let d = self.d;
            let deadline = *self.deadline.get_or_insert(t.now() + d);
            if t.now() >= deadline {
                if let Some(k) = self.key.take() {
                    t.unregister(k);
                }
                return Poll::Ready(());
            }
            let k = t.register(deadline, cx.waker(), self.key);
            self.key = Some(k);
            self.timers = t as *const Timers;
            Poll::Pending
        })
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        if let Some(k) = self.key {
            // dropped on the UI thread by the engine (cancellation) or by the future that owns it
            let p = CURRENT.with(|c| c.get());
            let t = if p.is_null() { self.timers } else { p };
            if !t.is_null() {
                // SAFETY: a registered sleep's timers belong to an engine that drops its flights before itself.
                unsafe { (*t).unregister(k) };
            }
        }
    }
}

/// `f`, or `None` if it has not finished `d` after the first poll (and `f` is dropped).
pub async fn timeout<F: Future>(d: Duration, f: F) -> Option<F::Output> {
    crate::combin::race(async { Some(f.await) }, async {
        sleep(d).await;
        None
    })
    .await
}
