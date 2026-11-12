//! `sig!(self.selected).select(key, on, off)`: a keyed fan-out operand.
//!
//! zgx's `function-prop` scene gives each of N boxes a reactive style `selected == i ? on : off`: every
//! change of `selected` re-evaluates N predicates to change two boxes. Here the domain is changed instead:
//! all `select` operands of one signal share **one** wire whose table maps a key to the destinations
//! keyed by it. A change from `a` to `b` patches the destinations of `a` (to `off`) and of `b` (to `on`)
//! and nothing else: O(changed), not O(N), and one patch call per destination that changes.
use crate::elem::{Apply, IntoProp, Mounter};
use crate::rt::*;
use crate::signal::{Signal, Wired};
use std::any::Any;
use std::collections::HashMap;
use std::hash::Hash;

struct Dest<P> {
    node: u32,
    /// The node's handle when the destination was recorded (a reused index has another).
    handle: bough_core::Handle,
    on: P,
    off: P,
    apply: fn(&mut Rt, u32, &P),
}

struct SelTable<T, P> {
    cur: T,
    by_key: HashMap<T, Vec<Dest<P>>>,
}

#[derive(Default)]
pub struct Selects {
    tables: Vec<Box<dyn Any>>,
    /// (signal head address, table) of every select wire.
    by_head: Vec<(usize, u32)>,
}

pub struct Select<'a, T, V, P> {
    w: Wired<'a, T, V>,
    key: T,
    on: P,
    off: P,
}

impl<'a, T, V> Wired<'a, T, V> {
    /// `on` while the signal equals `key`, else `off`; every `select` of one signal shares one keyed wire.
    pub fn select<P>(self, key: T, on: P, off: P) -> Select<'a, T, V, P> {
        Select { w: self, key, on, off }
    }
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; `w.node` a `SelTable<T, P>`.
unsafe fn patch_select<T: Copy + Eq + Hash + 'static, P: 'static>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    let new = **unsafe { &*(sig as *const Signal<T>) };
    let mut tb = std::mem::replace(&mut rt.selects.tables[w.node as usize], Box::new(()));
    {
        let t = tb.downcast_mut::<SelTable<T, P>>().expect("select table of the signal's type");
        let old = t.cur;
        if old != new {
            t.cur = new;
            for (k, on) in [(old, false), (new, true)] {
                if let Some(ds) = t.by_key.get_mut(&k) {
                    ds.retain(|d| {
                        if !(rt.node_live(d.node) && rt.handle(d.node) == d.handle) {
                            return false; // its subtree was released
                        }
                        (d.apply)(rt, d.node, if on { &d.on } else { &d.off });
                        true
                    });
                }
            }
        }
    }
    rt.selects.tables[w.node as usize] = tb;
}

impl<'a, T: Copy + Eq + Hash + 'static, V, P: Clone + 'static> IntoProp<P, V> for Select<'a, T, V, P> {
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        let head = self.w.sig.head() as *const _ as usize;
        let cur = *self.w.get();
        let ti = match m.rt.selects.by_head.iter().find(|x| x.0 == head) {
            Some(&(_, t)) => t,
            None => {
                let t = m.rt.selects.tables.len() as u32;
                m.rt.selects.tables.push(Box::new(SelTable::<T, P> { cur, by_key: HashMap::new() }));
                m.rt.selects.by_head.push((head, t));
                // SAFETY: `self.w` is an `offset_of!`-proven `Signal<T>`; the patch reads one and its table.
                unsafe { m.wire_cell(self.w.sig.head(), self.w.off, patch_select::<T, P>, t, [0; 16]) };
                t
            }
        };
        m.keep = true;
        let handle = m.rt.handle(node);
        let d = Dest { node, handle, on: self.on.clone(), off: self.off.clone(), apply: K::apply };
        let t = m.rt.selects.tables[ti as usize].downcast_mut::<SelTable<T, P>>().expect("one value type per selected signal");
        t.by_key.entry(self.key).or_default().push(d);
        if cur == self.key { self.on } else { self.off }
    }
    fn value(self) -> P {
        if *self.w.get() == self.key { self.on } else { self.off }
    }
}
