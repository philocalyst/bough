//! 11. A retry with backoff: transient failures are retried after 250 ms, 500 ms, 1 s, 2 s; the panel says
//! which attempt is running and when the next one starts; a refusal or the fifth failure is final.
//!
//! The retry loop is ordinary Rust in an `async gen` block, and each `yield` is one answer the screen shows
//! (a status, then the document). Opening another document drops the whole loop wherever it is, a backoff
//! sleep included: no retry fires after the user has moved on.
//!
//! Not written: an attempt counter in the view's state, a timer per backoff and its cancellation, a check that
//! the retry is still for the open document, a state machine for "loading / waiting / failed / done".
use crate::api::{self, Doc};
use bough::prelude::*;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Attempt {
    Trying(u32),
    Waiting { failed: u32, next_in: Duration, why: String },
    Done(Result<Doc, String>),
}

pub fn with_retry(id: u64) -> impl std::async_iter::AsyncIterator<Item = Attempt> {
    async gen move {
        let mut wait = Duration::from_millis(250);
        for n in 1..=5u32 {
            yield Attempt::Trying(n);
            match api::doc(id).await {
                Ok(d) => {
                    yield Attempt::Done(Ok(d));
                    return;
                }
                Err(e) if e.is_transient() && n < 5 => {
                    yield Attempt::Waiting { failed: n, next_in: wait, why: e.to_string() };
                    sleep(wait).await;
                    wait *= 2;
                }
                Err(e) => {
                    yield Attempt::Done(Err(e.to_string()));
                    return;
                }
            }
        }
    }
}

pub struct Loader {
    pub id: Signal<u64>,
    pub status: Signal<Attempt>,
}

impl Loader {
    pub fn new(id: u64) -> Loader {
        Loader { id: Signal::new(id), status: Signal::new(Attempt::Trying(0)) }
    }
    pub fn open(&mut self, id: u64) {
        *self.id = id;
    }
}

impl View for Loader {
    fn view(&self) -> impl Element<Self> + '_ {
        div().child((
            text(sig!(self.status).map(|s| match s {
                Attempt::Trying(n) => format!("loading (attempt {n})"),
                Attempt::Waiting { failed, next_in, why } => format!("attempt {failed} failed ({why}); retrying in {} ms", next_in.as_millis()),
                Attempt::Done(Ok(d)) => d.body.clone(),
                Attempt::Done(Err(e)) => format!("gave up: {e}"),
            })),
            // every status replaces the last; a new id drops the loop
            follow(sig!(self.id), with_retry).into(sig!(self.status)),
        ))
    }
}
