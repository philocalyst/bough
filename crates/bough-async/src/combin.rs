//! Future combinators the programs need. Losing is being dropped: nothing here cancels anything by hand.
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// The output of whichever of `a` and `b` finishes first (`a` on a tie); the other is dropped with the race.
pub fn race<A, B>(a: A, b: B) -> Race<A, B>
where
    A: Future,
    B: Future<Output = A::Output>,
{
    Race { a: Box::pin(a), b: Box::pin(b) }
}

pub struct Race<A, B> {
    a: Pin<Box<A>>,
    b: Pin<Box<B>>,
}

impl<A: Future, B: Future<Output = A::Output>> Future for Race<A, B> {
    type Output = A::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<A::Output> {
        if let Poll::Ready(v) = self.a.as_mut().poll(cx) {
            return Poll::Ready(v);
        }
        self.b.as_mut().poll(cx)
    }
}

/// Hand control back once: the engine polls this flight again at its next turn (after a frame).
pub fn yield_now() -> YieldNow {
    YieldNow(false)
}

pub struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
