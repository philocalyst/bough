//! 12. A long computation that yields: counting primes below a limit the user types, with a progress bar.
//!
//! Two forms of the same computation. `on_ui`: runs on the UI thread in slices; each `yield` hands control
//! back (the progress is one answer, and a frame can be drawn after it), so the window stays responsive with no
//! thread at all; typing a new limit drops the generator at its current slice. `on_worker`: the same loop on a
//! worker through `compute`, reporting progress through the closure's `Report`; dropping the question raises
//! the cancel flag, and the worker stops at its next check.
//!
//! Not written: a thread, a channel, a progress callback plumbed into the view, a cancel token threaded
//! through, a frame-budget scheduler, a guard against a stale result for an older limit.
use bough::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Idle,
    Working(f32),
    Done(u64),
}

fn is_prime(k: u64) -> bool {
    k >= 2 && (2..).take_while(|d| d * d <= k).all(|d| k % d != 0)
}

/// Slices of 20,000 candidates on the UI thread.
pub fn on_ui(limit: u64) -> impl std::async_iter::AsyncIterator<Item = Progress> {
    async gen move {
        let mut count = 0;
        for k in 0..limit {
            count += is_prime(k) as u64;
            if k % 20_000 == 19_999 {
                yield Progress::Working(k as f32 / limit as f32);
            }
        }
        yield Progress::Done(count);
    }
}

/// The same loop on a worker; `report` sends progress back (each report is one answer on the UI thread).
pub fn on_worker(limit: u64) -> impl std::async_iter::AsyncIterator<Item = Progress> {
    compute_stream(move |cancel: &Cancel, report: &Report<Progress>| {
        let mut count = 0;
        for k in 0..limit {
            count += is_prime(k) as u64;
            if k % 20_000 == 19_999 {
                if cancel.is_set() {
                    return;
                }
                report.send(Progress::Working(k as f32 / limit as f32));
            }
        }
        report.send(Progress::Done(count));
    })
}

pub struct Primes {
    pub limit: Signal<u64>,
    pub ui: Signal<Progress>,
    pub worker: Signal<Progress>,
}

impl Primes {
    pub fn new(limit: u64) -> Primes {
        Primes { limit: Signal::new(limit), ui: Signal::new(Progress::Idle), worker: Signal::new(Progress::Idle) }
    }
    pub fn set(&mut self, limit: u64) {
        *self.limit = limit;
    }
}

fn bar(p: &Progress) -> f32 {
    match p {
        Progress::Idle => 0.0,
        Progress::Working(f) => *f,
        Progress::Done(_) => 1.0,
    }
}

impl View for Primes {
    fn view(&self) -> impl Element<Self> + '_ {
        div().child((
            text(sig!(self.ui).map(|p| match p { Progress::Done(n) => format!("{n} primes"), _ => "counting\u{2026}".to_string() })),
            block(200.0, 6.0).bg(rgb(0x2f6bff)).w_op(sig!(self.ui).map(|p| 200.0 * bar(p))),
            text(sig!(self.worker).map(|p| match p { Progress::Done(n) => format!("{n} primes (worker)"), _ => "counting on a worker\u{2026}".to_string() })),
            // progress is information, not a reason to hold the limit: deferred questions hold nothing
            follow(sig!(self.limit), on_ui).deferred().into(sig!(self.ui)),
            follow(sig!(self.limit), on_worker).deferred().into(sig!(self.worker)),
        ))
    }
}
