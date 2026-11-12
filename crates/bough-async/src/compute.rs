//! Work for other cores: `compute(f)` runs `f` on a worker and is the only place `Send` is asked for.
//!
//! A job is submitted at the future's first poll (a future dropped before it is polled costs nothing) to a
//! pool of `available_parallelism - 1` threads (at least one) started on first use. Dropping the future raises
//! the job's [`Cancel`] flag; `f` checks it where it can stop. A panic in `f` is caught on the worker and
//! resumed on the thread that polls the future, as if `f` had run there.
use std::any::Any;
use std::async_iter::AsyncIterator;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::task::{Context, Poll, Waker};

/// Raised when the future that owns the job is dropped.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    fn set(&self) {
        self.0.store(true, Ordering::Relaxed)
    }
}

type Job = Box<dyn FnOnce() + Send>;

struct Pool {
    q: Mutex<VecDeque<Job>>,
    cv: Condvar,
}

fn pool() -> &'static Pool {
    static P: OnceLock<&'static Pool> = OnceLock::new();
    P.get_or_init(|| {
        let p: &'static Pool = Box::leak(Box::new(Pool { q: Mutex::new(VecDeque::new()), cv: Condvar::new() }));
        let n = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1);
        for i in 0..n {
            std::thread::Builder::new()
                .name(format!("bough-worker-{i}"))
                .spawn(move || loop {
                    let job = {
                        let mut q = p.q.lock().unwrap();
                        loop {
                            if let Some(j) = q.pop_front() {
                                break j;
                            }
                            q = p.cv.wait(q).unwrap();
                        }
                    };
                    job();
                })
                .expect("a worker thread");
        }
        p
    })
}

fn submit(j: Job) {
    let p = pool();
    p.q.lock().unwrap().push_back(j);
    p.cv.notify_one();
}

struct Shared<T> {
    out: Mutex<Option<std::thread::Result<T>>>,
    waker: Mutex<Option<Waker>>,
}

/// `f(&cancel)` on a worker; its result here.
pub fn compute<T, F>(f: F) -> Compute<T, F>
where
    T: Send + 'static,
    F: FnOnce(&Cancel) -> T + Send + 'static,
{
    Compute { f: Some(f), shared: None, cancel: Cancel::default() }
}

pub struct Compute<T, F> {
    f: Option<F>,
    shared: Option<Arc<Shared<T>>>,
    cancel: Cancel,
}

impl<T, F> Unpin for Compute<T, F> {}

impl<T: Send + 'static, F: FnOnce(&Cancel) -> T + Send + 'static> Future for Compute<T, F> {
    type Output = T;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        if let Some(f) = self.f.take() {
            let sh = Arc::new(Shared { out: Mutex::new(None), waker: Mutex::new(Some(cx.waker().clone())) });
            self.shared = Some(sh.clone());
            let cancel = self.cancel.clone();
            submit(Box::new(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&cancel)));
                *sh.out.lock().unwrap() = Some(r);
                if let Some(w) = sh.waker.lock().unwrap().take() {
                    w.wake();
                }
            }));
            return Poll::Pending;
        }
        let sh = self.shared.clone().expect("polled after completion");
        if let Some(r) = sh.out.lock().unwrap().take() {
            self.shared = None;
            return match r {
                Ok(v) => Poll::Ready(v),
                Err(p) => std::panic::resume_unwind(p),
            };
        }
        *sh.waker.lock().unwrap() = Some(cx.waker().clone());
        // the worker may have finished between the two locks
        if sh.out.lock().unwrap().is_some() {
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    }
}

impl<T, F> Drop for Compute<T, F> {
    fn drop(&mut self) {
        if self.shared.is_some() {
            self.cancel.set();
        }
    }
}

/// Sends a worker's items back to the stream that runs it.
pub struct Report<T> {
    sh: Arc<StreamShared<T>>,
}

impl<T> Report<T> {
    pub fn send(&self, v: T) {
        self.sh.items.lock().unwrap().push_back(v);
        if let Some(w) = self.sh.waker.lock().unwrap().take() {
            w.wake();
        }
    }
}

struct StreamShared<T> {
    items: Mutex<VecDeque<T>>,
    done: AtomicBool,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
    waker: Mutex<Option<Waker>>,
}

/// `f(&cancel, &report)` on a worker; every `report.send(v)` is an item of the returned stream, which ends when
/// `f` returns.
pub fn compute_stream<T, F>(f: F) -> ComputeStream<T, F>
where
    T: Send + 'static,
    F: FnOnce(&Cancel, &Report<T>) + Send + 'static,
{
    ComputeStream { f: Some(f), sh: None, cancel: Cancel::default() }
}

pub struct ComputeStream<T, F> {
    f: Option<F>,
    sh: Option<Arc<StreamShared<T>>>,
    cancel: Cancel,
}

impl<T, F> Unpin for ComputeStream<T, F> {}

impl<T: Send + 'static, F: FnOnce(&Cancel, &Report<T>) + Send + 'static> AsyncIterator for ComputeStream<T, F> {
    type Item = T;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        if let Some(f) = self.f.take() {
            let sh = Arc::new(StreamShared { items: Mutex::new(VecDeque::new()), done: AtomicBool::new(false), panic: Mutex::new(None), waker: Mutex::new(Some(cx.waker().clone())) });
            self.sh = Some(sh.clone());
            let cancel = self.cancel.clone();
            submit(Box::new(move || {
                let rep = Report { sh: sh.clone() };
                if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&cancel, &rep))) {
                    *sh.panic.lock().unwrap() = Some(p);
                }
                sh.done.store(true, Ordering::Release);
                if let Some(w) = sh.waker.lock().unwrap().take() {
                    w.wake();
                }
            }));
            return Poll::Pending;
        }
        let Some(sh) = self.sh.as_ref() else { return Poll::Ready(None) };
        if let Some(v) = sh.items.lock().unwrap().pop_front() {
            return Poll::Ready(Some(v));
        }
        if sh.done.load(Ordering::Acquire) {
            // items sent before `done` are visible: check once more
            if let Some(v) = sh.items.lock().unwrap().pop_front() {
                return Poll::Ready(Some(v));
            }
            if let Some(p) = sh.panic.lock().unwrap().take() {
                std::panic::resume_unwind(p);
            }
            self.sh = None;
            return Poll::Ready(None);
        }
        *sh.waker.lock().unwrap() = Some(cx.waker().clone());
        if !sh.items.lock().unwrap().is_empty() || sh.done.load(Ordering::Acquire) {
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    }
}

impl<T, F> Drop for ComputeStream<T, F> {
    fn drop(&mut self) {
        if self.sh.is_some() {
            self.cancel.set();
        }
    }
}
