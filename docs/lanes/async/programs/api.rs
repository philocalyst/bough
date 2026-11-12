//! The application's own client library, as the programs see it: plain `async fn`s and plain data.
//!
//! Nothing here knows bough. In a real application these would be HTTP calls (any client: the futures
//! only have to be `'static`; bough polls them on the UI thread, so they need not be `Send`). For the
//! tests the same signatures are backed by a *controllable fake server*: every call registers itself as
//! pending and completes only when the test answers it, so a test can order completions any way it
//! likes and can see which requests were dropped (cancelled) before they were answered.
#![allow(dead_code)]

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

// =============================================================================== data

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub number: u32,
    pub rows: Vec<String>,
    pub more: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Story {
    pub id: u64,
    pub title: String,
    pub author: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub visitors: u64,
    pub revenue: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub id: u64,
    pub body: String,
}

/// Decoded pixels (the media lane owns the real image type; this is width, height and one colour).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The network failed; asking again may work.
    Transient(String),
    /// The server refused; asking again will not help.
    Refused(String),
}

impl Error {
    pub fn is_transient(&self) -> bool {
        matches!(self, Error::Transient(_))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Transient(m) => write!(f, "network: {m}"),
            Error::Refused(m) => write!(f, "refused: {m}"),
        }
    }
}

/// One token of a streamed completion.
#[derive(Clone, Debug, PartialEq)]
pub struct Token(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirror {
    East,
    West,
}

// =============================================================================== calls

pub async fn search(q: String) -> Vec<Hit> {
    call("search", q).await
}
pub async fn page(n: u32) -> Page {
    call("page", n.to_string()).await
}
pub async fn story(id: u64) -> Story {
    call("story", id.to_string()).await
}
pub async fn user(id: u64) -> User {
    call("user", id.to_string()).await
}
pub async fn try_user(id: u64) -> Result<User, Error> {
    call("try_user", id.to_string()).await
}
pub async fn comments(story: u64) -> Vec<Comment> {
    call("comments", story.to_string()).await
}
pub async fn todos() -> Vec<Todo> {
    call("todos", String::new()).await
}
pub async fn add_todo(title: String) -> Result<Todo, Error> {
    call("add_todo", title).await
}
pub async fn set_done(id: u64, done: bool) -> Result<Todo, Error> {
    call("set_done", format!("{id}={done}")).await
}
pub async fn toggle_done(id: u64) -> Result<Todo, Error> {
    call("toggle_done", id.to_string()).await
}
pub async fn stats() -> Result<Stats, Error> {
    call("stats", String::new()).await
}
pub async fn doc(id: u64) -> Result<Doc, Error> {
    call("doc", id.to_string()).await
}
pub async fn doc_from(m: Mirror, id: u64) -> Result<Doc, Error> {
    call("doc_from", format!("{m:?}/{id}")).await
}
pub async fn fetch(url: String) -> Vec<u8> {
    call("fetch", url).await
}
pub async fn inbox() -> Vec<String> {
    call("inbox", String::new()).await
}
pub async fn thread(id: u64) -> Vec<String> {
    call("thread", id.to_string()).await
}
pub async fn prefs() -> Vec<(String, bool)> {
    call("prefs", String::new()).await
}
pub async fn profile(id: u64) -> (User, Vec<String>) {
    call("profile", id.to_string()).await
}

/// A streamed completion: one call, many answers (the fake answers it with `server::stream`).
pub fn complete(prompt: String) -> impl std::async_iter::AsyncIterator<Item = Token> {
    async gen move {
        let id = server::open("complete", prompt);
        let _g = StreamGuard(id);
        while let Some(t) = server::next::<Token>(id).await {
            yield t;
        }
    }
}

/// Live prices of a symbol (one tick per server push).
pub fn ticks(symbol: String) -> impl std::async_iter::AsyncIterator<Item = f64> {
    async gen move {
        let id = server::open("ticks", symbol);
        let _g = StreamGuard(id);
        while let Some(p) = server::next::<f64>(id).await {
            yield p;
        }
    }
}

// =============================================================================== the fake server

/// A call the test has not answered yet.
pub struct Call {
    pub name: &'static str,
    pub arg: String,
    pub id: u64,
}

struct Slot {
    name: &'static str,
    arg: String,
    id: u64,
    /// The answer (set by the test), taken by the future.
    answer: Option<Box<dyn std::any::Any>>,
    /// Stream items not yet taken; `closed` once the test ends the stream.
    items: std::collections::VecDeque<Box<dyn std::any::Any>>,
    closed: bool,
    waker: Option<Waker>,
    /// The future (or stream) was dropped before it finished: cancelled.
    dropped: bool,
    done: bool,
}

#[derive(Default)]
struct Server {
    next: u64,
    slots: Vec<Slot>,
}

thread_local! {
    static SERVER: RefCell<Server> = RefCell::new(Server::default());
}

pub mod server {
    use super::*;

    /// Forget every call (a fresh test).
    pub fn reset() {
        SERVER.with(|s| *s.borrow_mut() = Server::default());
    }

    /// Calls in flight (asked, not answered, not dropped), oldest first.
    pub fn pending() -> Vec<Call> {
        SERVER.with(|s| s.borrow().slots.iter().filter(|c| !c.done && !c.dropped).map(|c| Call { name: c.name, arg: c.arg.clone(), id: c.id }).collect())
    }

    /// Every call ever made of `name`, with whether it was dropped before its answer (cancelled).
    pub fn history(name: &str) -> Vec<(String, bool)> {
        SERVER.with(|s| s.borrow().slots.iter().filter(|c| c.name == name).map(|c| (c.arg.clone(), c.dropped)).collect())
    }

    /// Answer the oldest pending call `name(arg)` with `v`. Panics if there is none.
    pub fn answer<T: 'static>(name: &str, arg: &str, v: T) {
        SERVER.with(|s| {
            let mut s = s.borrow_mut();
            let c = s.slots.iter_mut().find(|c| c.name == name && c.arg == arg && !c.done && !c.dropped && c.answer.is_none()).unwrap_or_else(|| panic!("no pending call {name}({arg})"));
            c.answer = Some(Box::new(v));
            if let Some(w) = c.waker.take() {
                w.wake();
            }
        })
    }

    /// Is a call `name(arg)` pending?
    pub fn is_pending(name: &str, arg: &str) -> bool {
        pending().iter().any(|c| c.name == name && c.arg == arg)
    }

    /// Push one item into the open stream `name(arg)`.
    pub fn push<T: 'static>(name: &str, arg: &str, v: T) {
        SERVER.with(|s| {
            let mut s = s.borrow_mut();
            let c = s.slots.iter_mut().rev().find(|c| c.name == name && c.arg == arg && !c.closed && !c.dropped).unwrap_or_else(|| panic!("no open stream {name}({arg})"));
            c.items.push_back(Box::new(v));
            if let Some(w) = c.waker.take() {
                w.wake();
            }
        })
    }

    /// End the open stream `name(arg)`.
    pub fn close(name: &str, arg: &str) {
        SERVER.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(c) = s.slots.iter_mut().rev().find(|c| c.name == name && c.arg == arg && !c.closed) {
                c.closed = true;
                if let Some(w) = c.waker.take() {
                    w.wake();
                }
            }
        })
    }

    pub(super) fn open(name: &'static str, arg: String) -> u64 {
        SERVER.with(|s| {
            let mut s = s.borrow_mut();
            let id = s.next;
            s.next += 1;
            s.slots.push(Slot { name, arg, id, answer: None, items: Default::default(), closed: false, waker: None, dropped: false, done: false });
            id
        })
    }

    /// The next item of stream `id` (None: closed). Dropping the returned future mid-wait is a cancel.
    pub(super) fn next<T: 'static>(id: u64) -> impl Future<Output = Option<T>> {
        Next::<T> { id, _t: std::marker::PhantomData }
    }

    struct Next<T> {
        id: u64,
        _t: std::marker::PhantomData<T>,
    }
    impl<T: 'static> Future for Next<T> {
        type Output = Option<T>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
            SERVER.with(|s| {
                let mut s = s.borrow_mut();
                let c = s.slots.iter_mut().find(|c| c.id == self.id).expect("stream slot");
                if let Some(v) = c.items.pop_front() {
                    return Poll::Ready(Some(*v.downcast::<T>().expect("item type")));
                }
                if c.closed {
                    c.done = true;
                    return Poll::Ready(None);
                }
                c.waker = Some(cx.waker().clone());
                Poll::Pending
            })
        }
    }

    /// Mark stream `id` dropped (its consumer went away).
    pub(super) fn drop_stream(id: u64) {
        SERVER.with(|s| {
            if let Some(c) = s.borrow_mut().slots.iter_mut().find(|c| c.id == id && !c.done) {
                c.dropped = true;
            }
        })
    }
}

/// One request: pending until the test answers it; dropping it unanswered records a cancellation.
struct Reply<T> {
    id: u64,
    _t: std::marker::PhantomData<T>,
}

fn call<T: 'static>(name: &'static str, arg: String) -> Reply<T> {
    Reply { id: server::open(name, arg), _t: std::marker::PhantomData }
}

impl<T: 'static> Future for Reply<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        SERVER.with(|s| {
            let mut s = s.borrow_mut();
            let c = s.slots.iter_mut().find(|c| c.id == self.id).expect("call slot");
            match c.answer.take() {
                Some(v) => {
                    c.done = true;
                    Poll::Ready(*v.downcast::<T>().unwrap_or_else(|_| panic!("answer of the wrong type for {}", c.name)))
                }
                None => {
                    c.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
    }
}

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        SERVER.with(|s| {
            if let Ok(mut s) = s.try_borrow_mut() {
                if let Some(c) = s.slots.iter_mut().find(|c| c.id == self.id && !c.done) {
                    c.dropped = true;
                }
            }
        })
    }
}

/// Held by a stream for its whole life: dropping the stream before it closed marks the call cancelled.
pub struct StreamGuard(pub u64);
impl Drop for StreamGuard {
    fn drop(&mut self) {
        server::drop_stream(self.0);
    }
}
