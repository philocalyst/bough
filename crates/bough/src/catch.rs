//! `catch(content, fallback)`: the nearest boundary for errors raised by questions inside it (`ok_into`), the
//! alternative to holding `Result`s in cells (`docs/lanes/async/programs/p09b_errors_catch.rs`).
//!
//! A question that lands through `ok_into(dst)` writes `Ok` answers into `dst`; an `Err` goes to the innermost
//! `catch` open when the question was mounted. The boundary then describes `fallback(&error, retry)` instead of its
//! content (the content is unmounted: its questions are dropped with it). `retry` is a route: it clears the error,
//! so the next description mounts the content again and its questions ask again (React's error boundary reset).
//! Errors are typed per boundary: a question whose error type is not the boundary's is a compile error at the
//! `catch` (the fallback takes `&E`) or a panic at the landing if they meet through nesting (named in the message).
use crate::describe::Arm2;
use crate::elem::{Element, Mounter};
use std::any::Any;
use std::cell::Cell;
use std::marker::PhantomData;

/// A boundary's state.
#[derive(Default)]
pub(crate) struct Catch {
    /// Questions mounted inside it (diagnostics; retry remounts the content instead).
    pub questions: Vec<u32>,
    /// The error shown in place of the content.
    pub error: Option<Box<dyn Any>>,
}

pub struct Catcher<E, C, F> {
    content: C,
    fallback: F,
    _e: PhantomData<fn(&E)>,
}

/// The nearest boundary for errors of type `E` raised inside `content` (see the module docs).
pub fn catch<E, C, F>(content: C, fallback: F) -> Catcher<E, C, F> {
    Catcher { content, fallback, _e: PhantomData }
}

/// The route a fallback's Retry button takes: clear the boundary's error (the content mounts again).
pub struct Retry<V>(u32, PhantomData<fn(&mut V)>);
impl<V> Clone for Retry<V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V> Copy for Retry<V> {}

impl<V> Retry<V> {
    /// The retry route of boundary `b` (the `b`-th `catch` mounted), for tests that drive it without a click.
    #[doc(hidden)]
    pub fn for_test(b: u32) -> Retry<V> {
        Retry(b, PhantomData)
    }
}

impl<V> FnOnce<(&mut V,)> for Retry<V> {
    type Output = ();
    extern "rust-call" fn call_once(self, _: (&mut V,)) {
        retry(self.0)
    }
}
impl<V> FnMut<(&mut V,)> for Retry<V> {
    extern "rust-call" fn call_mut(&mut self, _: (&mut V,)) {
        retry(self.0)
    }
}
impl<V> Fn<(&mut V,)> for Retry<V> {
    extern "rust-call" fn call(&self, _: (&mut V,)) {
        retry(self.0)
    }
}

const RETRY_CAP: usize = 8;
thread_local! {
    /// Boundaries a handler asked to retry, taken after the handler (no destructor: a plain thread-local).
    static RETRY_N: Cell<u32> = const { Cell::new(0) };
    static RETRY_AT: [Cell<u32>; RETRY_CAP] = const { [const { Cell::new(0) }; RETRY_CAP] };
    /// Errors raised by landings since the pump last looked: (boundary, error).
    pub(crate) static RAISED: std::cell::RefCell<Vec<(u32, Box<dyn Any>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn retry(b: u32) {
    let n = RETRY_N.with(|n| n.get());
    if (n as usize) < RETRY_CAP {
        RETRY_AT.with(|a| a[n as usize].set(b));
        RETRY_N.with(|c| c.set(n + 1));
    }
}

impl crate::rt::Rt {
    /// After a handler: the boundaries it retried show their content again (their next description mounts it).
    pub(crate) fn take_retries(&mut self) {
        let n = RETRY_N.with(|n| n.replace(0));
        for i in 0..n as usize {
            let b = RETRY_AT.with(|a| a[i].get());
            if let Some(c) = self.asks.catches.get_mut(b as usize) {
                c.error = None;
            }
        }
    }
    /// After a landing step: errors raised by `ok_into` landings go to their boundaries.
    pub(crate) fn take_raised(&mut self) {
        let r = RAISED.with(|r| std::mem::take(&mut *r.borrow_mut()));
        for (b, e) in r {
            if let Some(c) = self.asks.catches.get_mut(b as usize) {
                c.error = Some(e);
            }
        }
    }
}

impl<V: 'static, E: 'static, C: Element<V>, F: Fn(&E, Retry<V>) -> FB, FB: Element<V>> Element<V> for Catcher<E, C, F> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let b = m.rt.asks.catches.len() as u32;
        m.rt.asks.catches.push(Catch::default());
        m.tape_push(b);
        m.rt.asks.catching.push(b);
        let a: Arm2<C, FB> = Arm2::A(self.content);
        a.mount(m);
        m.rt.asks.catching.pop();
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        let b = m.tape_next();
        m.rt.asks.catching.push(b);
        let shown = m.rt.asks.catches[b as usize].error.as_ref().map(|e| (self.fallback)(e.downcast_ref::<E>().expect("an error of the boundary's type (a question inside raised another type)"), Retry(b, PhantomData)));
        let a: Arm2<C, FB> = match shown {
            Some(f) => Arm2::B(f),
            None => Arm2::A(self.content),
        };
        a.update(m);
        m.rt.asks.catching.pop();
    }
}
