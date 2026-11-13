//! Text of any length as plain state: a [`Line`] is a field a handler edits; a record names it by a handle.
//!
//! A record is words, so it cannot own a string. Short text sits in the record itself ([`crate::kinds::Text`]);
//! text of any length lives in a pool, and the record holds `(slot, version)`. Every edit bumps the version,
//! so re-describing the row that shows the line finds different words, stores them and damages the record:
//! the framework notices by comparing, as with any other operand. A handle whose version is stale, or whose
//! line is gone, draws nothing; it is never a dangling pointer.
use bough_expr::Words;
use std::cell::RefCell;

struct Slot {
    ver: u32,
    live: bool,
    s: String,
}
thread_local! {
    static POOL: RefCell<(Vec<Slot>, Vec<u32>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

/// A line of text owned by application state.
pub struct Line {
    slot: u32,
    /// The text lives in this thread's pool: the line is neither `Send` nor `Sync`.
    _here: core::marker::PhantomData<*const ()>,
}

/// What a record holds of a [`Line`].
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineRef {
    slot: u32,
    ver: u32,
    /// A handle is resolved in the pool of the thread that made it: neither `Send` nor `Sync`.
    _here: core::marker::PhantomData<*const ()>,
}
// SAFETY: two words; every bit pattern is a (possibly stale) handle.
unsafe impl Words for LineRef {}

impl Line {
    pub fn new(s: &str) -> Line {
        POOL.with_borrow_mut(|(slots, free)| {
            let slot = match free.pop() {
                Some(i) => {
                    let sl = &mut slots[i as usize];
                    sl.ver = sl.ver.wrapping_add(1);
                    sl.live = true;
                    sl.s.clear();
                    sl.s.push_str(s);
                    i
                }
                None => {
                    slots.push(Slot { ver: 1, live: true, s: s.to_owned() });
                    slots.len() as u32 - 1
                }
            };
            Line { slot, _here: core::marker::PhantomData }
        })
    }
    /// Edit the text. The version moves whether or not the closure changed anything.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut String) -> R) -> R {
        POOL.with_borrow_mut(|(slots, _)| {
            let sl = &mut slots[self.slot as usize];
            sl.ver = sl.ver.wrapping_add(1);
            f(&mut sl.s)
        })
    }
    pub fn with<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        POOL.with_borrow(|(slots, _)| f(&slots[self.slot as usize].s))
    }
    pub fn get(&self) -> String {
        self.with(str::to_owned)
    }
    pub fn len(&self) -> usize {
        self.with(str::len)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The handle a record holds, as of now.
    pub fn handle(&self) -> LineRef {
        POOL.with_borrow(|(slots, _)| LineRef { slot: self.slot, ver: slots[self.slot as usize].ver, _here: core::marker::PhantomData })
    }
}
impl Drop for Line {
    fn drop(&mut self) {
        // the pool may already be gone at thread exit
        let _ = POOL.try_with(|p| {
            if let Ok(mut p) = p.try_borrow_mut() {
                let sl = &mut p.0[self.slot as usize];
                sl.live = false;
                sl.ver = sl.ver.wrapping_add(1);
                sl.s = String::new();
                p.1.push(self.slot);
            }
        });
    }
}
impl LineRef {
    /// The text the handle names, if it is still that text.
    pub fn with<R>(&self, f: impl FnOnce(Option<&[u8]>) -> R) -> R {
        POOL.with_borrow(|(slots, _)| match slots.get(self.slot as usize) {
            Some(sl) if sl.live && sl.ver == self.ver => f(Some(sl.s.as_bytes())),
            _ => f(None),
        })
    }
}

/// ```compile_fail
/// fn send<T: Send>() {}
/// send::<bough_span::line::Line>();
/// ```
/// ```compile_fail
/// fn sync<T: Sync>() {}
/// sync::<bough_span::line::LineRef>();
/// ```
#[cfg(doctest)]
struct NotSendNotSync;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_stale_handle_names_nothing() {
        let mut l = Line::new("abc");
        let h = l.handle();
        assert!(h.with(|s| s == Some(b"abc".as_slice())));
        l.edit(|s| s.push('d'));
        assert!(h.with(|s| s.is_none()), "an edit makes older handles stale");
        let h2 = l.handle();
        assert!(h2.with(|s| s == Some(b"abcd".as_slice())));
        drop(l);
        assert!(h2.with(|s| s.is_none()), "a dropped line is gone");
        let m = Line::new("x");
        assert!(h2.with(|s| s.is_none()), "a reused slot is another line");
        assert!(m.handle().with(|s| s == Some(b"x".as_slice())));
    }
}
