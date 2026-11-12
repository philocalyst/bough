//! Owner grain: a handler costs what it changed (round 6).
//!
//! The design's rung 0 is "a handler mutating `&mut self` reruns the owner's `view`". Rerunning it when nothing
//! the description depends on changed is pure cost, and every input event used to pay it for the whole root. An
//! owner is described again after a turn only if something its description can see changed:
//!
//! * **Its plain bytes.** A [`Plan`] of the owner's type, built at compile time from rustc's type reflection,
//!   lists where its plain state lives: scalar fields (compared as bytes), fieldless enums (their tag), `Option`s
//!   of such (the tag, then the payload's own plan while it is `Some`) and the plain parts of nested structs,
//!   tuples and arrays. A reactive cell (`Signal`, `List`, `Entities`, `Reader`) is not plain state: see below.
//!   A field the plan cannot see through (a `Vec`, a `String`, a `Box`, any pointer, an enum with fields that
//!   is not an `Option`) makes the plan inexact, and such an owner is described again after every handler, as
//!   before. The bytes the description was made from are kept (`snapshot`); after a handler they are compared
//!   (`same`). Padding is never read: only the bytes of scalar leaves are.
//! * **A cell it read plainly.** A description reads a cell either through `sig!` (a wire: its value is
//!   patched in place, the description needs no rerun) or plainly (`*self.count`, `&self.name` as a constant
//!   operand, `self.rows.len()`). A plain read, while an owner describes itself, is recorded by the cell's own
//!   accessor ([`read`]), and the owner subscribes to that cell with a *wake wire*: the cell's next write
//!   queues it like any wire, and the wire marks the owner to be described again. This is the dependency
//!   tracking of Ripple's and Solid's tracked reads, reduced to the one case Rust's `&self` cannot see.
//!
//! The recorder costs one load of a global and a branch per plain cell read when no owner is describing
//! (`DESCRIBING`); the runtime's own wire patches read cells through untracked accessors.
use std::cell::{Cell, RefCell};
use std::mem::type_info::TypeKind;
use std::ptr::DynMetadata;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// Byte ranges a plan holds at most (more makes it inexact).
pub const PLAN_RANGES: usize = 48;
/// `Option` fields a plan holds at most, and the ranges of all their payloads.
pub const PLAN_OPTS: usize = 8;
pub const PLAN_PAYLOAD: usize = 32;

/// Is an `Option<T>` `Some`? (a vtable found at compile time by the field's type).
pub(crate) trait IsSome {
    /// # Safety
    /// `self` points at a live `Option<T>`.
    unsafe fn is_some(self: *const Self) -> bool;
}
impl<T> IsSome for Option<T> {
    unsafe fn is_some(self: *const Self) -> bool {
        // SAFETY: the contract; `is_some` is `Option`'s own variant test.
        unsafe { (*self).is_some() }
    }
}

/// An `Option` field of a plan: its offset, where its snapshot starts (a tag byte, then its payload's bytes),
/// its payload's ranges (`Plan::pr[first..first + n]`, offsets from the `Option`'s start) and the vtable that
/// tells its variant.
#[derive(Clone, Copy)]
pub struct Opt {
    pub off: u32,
    pub at: u32,
    pub first: u16,
    pub n: u16,
    pub bytes: u32,
    vt: Option<DynMetadata<dyn IsSome>>,
}

/// Where a type's plain state lives (see the module docs). Built at compile time; `exact` false means a
/// change can be invisible to it.
#[derive(Clone, Copy)]
pub struct Plan {
    /// (offset, length) of scalar bytes, merged when adjacent, in field order.
    pub ranges: [(u32, u32); PLAN_RANGES],
    pub n: u32,
    pub opts: [Opt; PLAN_OPTS],
    pub no: u32,
    /// The payload ranges of the options.
    pub pr: [(u32, u32); PLAN_PAYLOAD],
    pub npr: u32,
    /// Bytes of a snapshot: the ranges, then per `Option` a tag byte and its payload's bytes.
    pub bytes: u32,
    pub exact: bool,
    /// The type holds at least one reactive cell (a description may read one plainly).
    pub cells: bool,
}

/// The plan of `T`, built once at compile time.
pub fn plan<T>() -> &'static Plan {
    const { &build(std::mem::type_info::of::<T>()) }
}

#[rustc_comptime]
fn blank() -> Plan {
    let o = Opt { off: 0, at: 0, first: 0, n: 0, bytes: 0, vt: None };
    Plan { ranges: [(0, 0); PLAN_RANGES], n: 0, opts: [o; PLAN_OPTS], no: 0, pr: [(0, 0); PLAN_PAYLOAD], npr: 0, bytes: 0, exact: true, cells: false }
}

#[rustc_comptime]
fn build(t: std::any::TypeId) -> Plan {
    let mut p = blank();
    walk(&mut p, t, 0, 0);
    // the options' snapshots follow the ranges
    let mut at = 0u32;
    let mut i = 0;
    while i < p.n as usize {
        at += p.ranges[i].1;
        i += 1;
    }
    let mut k = 0;
    while k < p.no as usize {
        p.opts[k].at = at;
        at += 1 + p.opts[k].bytes;
        k += 1;
    }
    p.bytes = at;
    p
}

/// A scalar range: appended, or merged with the previous one when adjacent.
#[rustc_comptime]
fn range(p: &mut Plan, off: usize, len: usize) {
    if len == 0 {
        return;
    }
    let n = p.n as usize;
    if n > 0 && (p.ranges[n - 1].0 + p.ranges[n - 1].1) as usize == off {
        p.ranges[n - 1].1 += len as u32;
        return;
    }
    if n == PLAN_RANGES {
        p.exact = false;
        return;
    }
    p.ranges[n] = (off as u32, len as u32);
    p.n += 1;
}

#[rustc_comptime]
fn is_cell(t: std::any::TypeId) -> bool {
    match t.info().kind {
        TypeKind::Struct => t.fields(0) >= 1 && t.field(0, 0).type_id() == std::mem::type_info::of::<crate::signal::WireHead>(),
        _ => false,
    }
}

#[rustc_comptime]
fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Tuples are recognised by a trait (this toolchain's `TypeId::info` ICEs on a tuple, see `reflect.rs`).
trait IsTuple {}
macro_rules! tuples {
    ($(($($t:ident),*))*) => {$(impl<$($t),*> IsTuple for ($($t,)*) {})*};
}
tuples!(()(A)(A, B)(A, B, C)(A, B, C, D)(A, B, C, D, E)(A, B, C, D, E, F)(A, B, C, D, E, F, G)(A, B, C, D, E, F, G, H)(A, B, C, D, E, F, G, H, I)(A, B, C, D, E, F, G, H, I, J)(
    A, B, C, D, E, F, G, H, I, J, K
)(A, B, C, D, E, F, G, H, I, J, K, L));

#[rustc_comptime]
fn walk(p: &mut Plan, t: std::any::TypeId, off: usize, depth: u32) {
    // (an inexact plan keeps walking: whether the type holds cells is still needed)
    let size = match t.size() {
        Some(z) => z,
        None => {
            p.exact = false;
            return;
        }
    };
    if size == 0 {
        return; // PhantomData, (), a fieldless struct: nothing to see
    }
    if depth > 16 {
        p.exact = false;
        return;
    }
    if t.trait_info_of::<dyn IsTuple>().is_some() {
        let mut k = 0;
        while k < t.fields(0) {
            let f = t.field(0, k);
            walk(p, f.type_id(), off + f.offset(), depth + 1);
            k += 1;
        }
        return;
    }
    match t.info().kind {
        TypeKind::Bool | TypeKind::Char | TypeKind::Int | TypeKind::Float | TypeKind::FnPtr => range(p, off, size),
        TypeKind::Struct => {
            if is_cell(t) {
                p.cells = true;
                return;
            }
            let mut k = 0;
            while k < t.fields(0) {
                let f = t.field(0, k);
                walk(p, f.type_id(), off + f.offset(), depth + 1);
                k += 1;
            }
        }
        TypeKind::Array => {
            let e = match t.element_ty() {
                Some(e) => e,
                None => {
                    p.exact = false;
                    return;
                }
            };
            let esz = match e.size() {
                Some(z) => z,
                None => 0,
            };
            if esz == 0 {
                return;
            }
            // the length from the sizes (`array_len` ICEs on a length that is a named constant on this toolchain)
            let n = size / esz;
            if is_cell(e) {
                p.cells = true;
                return;
            }
            // an element that is one scalar run: the whole array is one range
            let mut probe = blank();
            walk(&mut probe, e, 0, depth + 1);
            if probe.exact && probe.no == 0 && probe.n == 1 && probe.ranges[0].0 == 0 && probe.ranges[0].1 as usize == esz {
                range(p, off, n * esz);
                return;
            }
            if probe.exact && probe.n == 0 && probe.no == 0 {
                p.cells |= probe.cells;
                return; // nothing plain in an element
            }
            if n > 16 {
                p.exact = false;
                return;
            }
            let mut i = 0;
            while i < n {
                walk(p, e, off + i * esz, depth + 1);
                i += 1;
            }
        }
        TypeKind::Enum => {
            // a fieldless enum is its tag
            let mut fields = 0;
            let mut v = 0;
            while v < t.variants() {
                fields += t.fields(v);
                v += 1;
            }
            if fields == 0 {
                range(p, off, size);
                return;
            }
            // an `Option` of plain scalars: its variant, then its payload's ranges while it is `Some`
            let vt = match t.trait_info_of::<dyn IsSome>() {
                Some(vt) => vt.get_vtable(),
                None => {
                    p.exact = false;
                    return;
                }
            };
            let mut some = usize::MAX;
            let mut v = 0;
            while v < t.variants() {
                if str_eq(t.variant(v).name(), "Some") && t.fields(v) == 1 {
                    some = v;
                }
                v += 1;
            }
            if some == usize::MAX || p.no as usize == PLAN_OPTS {
                p.exact = false;
                return;
            }
            let f = t.field(some, 0);
            let mut inner = blank();
            walk(&mut inner, f.type_id(), f.offset(), depth + 1);
            if !inner.exact || inner.no != 0 || p.npr as usize + inner.n as usize > PLAN_PAYLOAD {
                p.exact = false;
                return;
            }
            p.cells |= inner.cells;
            let first = p.npr as usize;
            let mut bytes = 0;
            let mut i = 0;
            while i < inner.n as usize {
                p.pr[first + i] = inner.ranges[i];
                bytes += inner.ranges[i].1;
                i += 1;
            }
            p.npr += inner.n;
            p.opts[p.no as usize] = Opt { off: off as u32, at: 0, first: first as u16, n: inner.n as u16, bytes, vt: Some(vt) };
            p.no += 1;
        }
        // pointers, references, unions, slices, trait objects: what they reach is not in the owner's bytes
        _ => p.exact = false,
    }
}

impl Plan {
    /// Append the plain state of the value at `v` (a live value of the plan's type) to `out`.
    ///
    /// # Safety
    /// `v` points at a live value of the type this plan was built for, not mutably borrowed elsewhere.
    pub unsafe fn snapshot(&self, v: *const u8, out: &mut Vec<u8>) {
        for &(o, l) in &self.ranges[..self.n as usize] {
            // SAFETY: the contract; a range covers the initialised bytes of scalar fields of the value.
            out.extend_from_slice(unsafe { std::slice::from_raw_parts(v.add(o as usize), l as usize) });
        }
        for op in &self.opts[..self.no as usize] {
            // SAFETY: the field at `off` is the `Option` whose vtable this is.
            let base = unsafe { v.add(op.off as usize) };
            let o: *const dyn IsSome = std::ptr::from_raw_parts(base as *const (), op.vt.expect("option vtable"));
            // SAFETY: as above.
            if unsafe { o.is_some() } {
                out.push(1);
                for &(r, l) in &self.pr[op.first as usize..(op.first + op.n) as usize] {
                    // SAFETY: a `Some` payload's scalar bytes (offsets from the option's start).
                    out.extend_from_slice(unsafe { std::slice::from_raw_parts(base.add(r as usize), l as usize) });
                }
            } else {
                out.push(0);
                out.resize(out.len() + op.bytes as usize, 0);
            }
        }
    }

    /// Is the plain state of the value at `v` what `snap` (a snapshot of this plan) holds?
    ///
    /// # Safety
    /// As `snapshot`; `snap` is `self.bytes` long.
    pub unsafe fn same(&self, v: *const u8, snap: &[u8]) -> bool {
        let mut at = 0usize;
        // the gate's mutant: the plan's last range is not compared (a change there goes unseen)
        let n = if cfg!(feature = "mutant_grain_plan") { (self.n as usize).saturating_sub(1) } else { self.n as usize };
        for &(o, l) in &self.ranges[..n] {
            let l = l as usize;
            // SAFETY: as `snapshot`.
            let now = unsafe { std::slice::from_raw_parts(v.add(o as usize), l) };
            if now != &snap[at..at + l] {
                return false;
            }
            at += l;
        }
        for op in &self.opts[..self.no as usize] {
            // SAFETY: as `snapshot`.
            let base = unsafe { v.add(op.off as usize) };
            let o: *const dyn IsSome = std::ptr::from_raw_parts(base as *const (), op.vt.expect("option vtable"));
            let mut a = op.at as usize;
            // SAFETY: as `snapshot`.
            let some = unsafe { o.is_some() };
            if (snap[a] != 0) != some {
                return false;
            }
            a += 1;
            if some {
                for &(r, l) in &self.pr[op.first as usize..(op.first + op.n) as usize] {
                    let l = l as usize;
                    // SAFETY: as `snapshot`.
                    let now = unsafe { std::slice::from_raw_parts(base.add(r as usize), l) };
                    if now != &snap[a..a + l] {
                        return false;
                    }
                    a += l;
                }
            }
        }
        true
    }
}

// =============================================================================== tracked reads

/// Owners describing themselves right now (any thread): while non-zero, a cell's plain read is recorded.
static DESCRIBING: AtomicU32 = AtomicU32::new(0);

/// What a plain read of a cell asks of the owner describing itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Dep {
    /// Any write of the cell (a `Signal`'s value).
    Value,
    /// A list's length or order (`len`, `slots`, `position`): structural ops, not item edits.
    Shape,
    /// A list's items (`get`, `iter`): every op, item edits included.
    Items,
    /// A cell whose writes do not all queue its head (`Entities`, `Reader`): the owner re-describes after
    /// every turn while its description reads it.
    Always,
}

#[derive(Clone, Copy)]
pub(crate) struct Read {
    /// The cell's head.
    pub head: usize,
    pub dep: Dep,
    /// The wake wire's patch for this cell type (a list's must close its log at the turn's end).
    pub wake: crate::rt::PatchFn,
}

thread_local! {
    static READS: RefCell<Vec<Read>> = const { RefCell::new(Vec::new()) };
    /// More reads than kept: the owners describing now cannot know what they read.
    static OVERFLOW: Cell<bool> = const { Cell::new(false) };
}
const READS_CAP: usize = 1 << 16;

/// A plain read of the cell whose head is at `head` (called by the cells' read accessors).
#[inline(always)]
pub(crate) fn read(head: *const Cell<u32>, dep: Dep, wake: crate::rt::PatchFn) {
    if DESCRIBING.load(Relaxed) != 0 {
        read_cold(head as usize, dep, wake);
    }
}

#[cold]
#[inline(never)]
fn read_cold(head: usize, dep: Dep, wake: crate::rt::PatchFn) {
    if cfg!(feature = "mutant_grain_track") {
        return; // the gate's mutant: plain reads subscribe nothing
    }
    READS.with(|r| {
        let mut r = r.borrow_mut();
        if r.len() >= READS_CAP {
            r.clear();
            OVERFLOW.with(|o| o.set(true));
        }
        r.push(Read { head, dep, wake });
    });
}

/// An owner describes itself: plain reads are recorded until `end`.
pub(crate) struct Describing {
    start: usize,
}
impl Describing {
    pub(crate) fn begin() -> Describing {
        DESCRIBING.fetch_add(1, Relaxed);
        let start = READS.with(|r| r.borrow().len());
        if start == 0 {
            OVERFLOW.with(|o| o.set(false));
        }
        Describing { start }
    }
    /// The plain reads of cells inside `[lo, lo + len)` since `begin`, and whether some were lost.
    pub(crate) fn end(self, lo: usize, len: usize, out: &mut Vec<Read>) -> bool {
        let start = self.start;
        std::mem::forget(self);
        DESCRIBING.fetch_sub(1, Relaxed);
        READS.with(|r| {
            let mut r = r.borrow_mut();
            let s = start.min(r.len());
            out.extend(r[s..].iter().filter(|x| x.head.wrapping_sub(lo) < len));
            r.truncate(s);
        });
        OVERFLOW.with(|o| o.get())
    }
}
impl Drop for Describing {
    /// A description that panicked: stop recording.
    fn drop(&mut self) {
        DESCRIBING.fetch_sub(1, Relaxed);
        READS.with(|r| {
            let mut r = r.borrow_mut();
            let s = self.start.min(r.len());
            r.truncate(s);
        });
    }
}

/// The wake wire of a cell read by `Dep::Value` (or `Shape`/`Items` of a non-list): any write wakes the root.
///
/// # Safety
/// None needed (it reads nothing; the wire contract's pointers are unused).
pub(crate) unsafe fn wake_root(rt: &mut crate::rt::Rt, _w: &crate::rt::Wire, _sig: *const u8, _owner: *const u8) {
    rt.redesc_root = true;
}

/// What the root owner's description depends on beyond its tape: its plain bytes as described and its
/// subscriptions (App).
#[derive(Default)]
pub(crate) struct OwnerDeps {
    /// The plain bytes the retained description was made from (`Plan::snapshot`).
    pub described: Vec<u8>,
    /// Cells subscribed to (offset in the owner, dependency): one wake wire each.
    pub subs: Vec<(u32, Dep)>,
    /// The last description read a cell whose writes cannot be seen (`Dep::Always`), or lost track.
    pub always: bool,
    /// Scratch for `Describing::end`.
    pub reads: Vec<Read>,
}
