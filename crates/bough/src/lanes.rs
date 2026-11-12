//! `lanes(n, sig!(self.t), f)`: `n` quads whose geometry and colour are one function of a signal and the
//! lane index, evaluated as **one pass over a column**.
//!
//! The representation change: an animation that moves many elements by one clock is not `n` reactive
//! nodes each re-deriving its style, but one record owning a run of `n` GPU instances (the GPU expands the
//! run) and one wire whose patch is a monomorphic loop `for i in 0..n { inst[i] = f(t, i) }` (the
//! closure is inlined into it, so the loop body is the style computation itself, written straight into
//! the instances the GPU reads) plus one union reduction for the damage: old and new bounds of the run,
//! two rects however many lanes moved. Nothing is allocated, nothing is looked up per lane.
use crate::color::Color;
use crate::elem::{Element, Mounter};
use crate::rt::*;
use crate::signal::{Signal, Wired};
use bough_core::RecordInit;
use bough_frame::pay_irun;
use bough_frame::Instance;

/// One lane's quad (points, relative to the element's origin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lane {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Color,
}

pub struct Lanes<'a, T, V, F> {
    n: usize,
    sig: Wired<'a, T, V>,
    f: F,
    radius: f32,
}

/// `n` lanes (at most 4,096 per element) driven by `sig`: lane `i` is `f(&value, i)`.
pub fn lanes<'a, T, V, F: Fn(&T, usize) -> Lane + Copy + 'static>(n: usize, sig: Wired<'a, T, V>, f: F) -> Lanes<'a, T, V, F> {
    assert!((1..=4096).contains(&n), "lanes: 1..=4096 per element");
    Lanes { n, sig, f, radius: 0.0 }
}
impl<T, V, F> Lanes<'_, T, V, F> {
    pub fn rounded(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
}

/// Write every lane's instance; returns the union of their ink (absolute device px).
#[inline(always)]
fn write_lanes<T, F: Fn(&T, usize) -> Lane>(rt: &mut Rt, node: u32, v: &T, f: F) -> bough_core::nrect::WRect {
    let (first, n, ox, oy, space, s) = {
        let nd = &rt.nodes[node as usize];
        (rt.runs[nd.run as usize].first as usize, rt.runs[nd.run as usize].len as usize, nd.x, nd.y, nd.space, rt.scale)
    };
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let inst = &mut rt.inst[first..first + n];
    for (i, p) in inst.iter_mut().enumerate() {
        let l = f(v, i);
        // on the geometry grid (`snap`), as every quad: a lane draws what the same box as a `quad` draws
        let (x, y, w, h) = (snap(ox + l.x * s), snap(oy + l.y * s), snap(l.w * s), snap(l.h * s));
        p.pos = [x, y];
        p.size = [w, h];
        p.color = l.color.0;
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + w);
        y1 = y1.max(y + h);
    }
    let margin = rt.nodes[node as usize].margin;
    rt.ink_rect(space, x0, y0, x1 - x0, y1 - y0, margin)
}

/// # Safety
/// Wire contract: `sig` is a `Signal<T>`; the recipe holds an `F`.
unsafe fn patch_lanes<T: 'static, F: Fn(&T, usize) -> Lane + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _o: *const u8) {
    // SAFETY: the contract.
    let v = unsafe { &*(sig as *const Signal<T>) };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr() as *const F) };
    let ink = write_lanes(rt, w.node, &**v, f);
    let h = rt.nodes[w.node as usize].handle;
    // the lanes changed inside the old union too: old and new ink, even when the union is the same
    rt.bough.touch_handle(h);
    rt.bough.set_ink_handle_wide(h, ink);
}

fn recipe_of<F: Copy>(f: F) -> [u8; 16] {
    const { assert!(size_of::<F>() <= 16, "lanes closure: at most 16 bytes of captures") };
    let mut r = [0u8; 16];
    // SAFETY: size checked at compile time.
    unsafe { std::ptr::write_unaligned(r.as_mut_ptr() as *mut F, f) };
    r
}

impl<'a, T: 'static, V: 'static, F: Fn(&T, usize) -> Lane + Copy + 'static> Element<V> for Lanes<'a, T, V, F> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        let s = m.rt.scale;
        let node = m.rt.alloc_node();
        let run = m.rt.alloc_run(self.n);
        m.rt.runs[run as usize].len = self.n as u32;
        let first = m.rt.runs[run as usize].first;
        let kind = if self.radius > 0.0 { KIND_RRECT } else { KIND_SOLID };
        for i in 0..self.n {
            *m.rt.inst_mut(first + i as u32) = Instance { space: m.space, kind, radius: snap(self.radius * s), ..Default::default() };
        }
        {
            let n = &mut m.rt.nodes[node as usize];
            n.run = run;
            n.space = m.space;
            n.clip = m.clip;
            n.kind = NK_BARS;
            n.x = m.ox * s;
            n.y = m.oy * s;
            n.margin = if self.radius > 0.0 { 1.0 } else { 0.0 };
        }
        let ink = write_lanes(m.rt, node, self.sig.get(), self.f);
        m.wire(&self.sig, patch_lanes::<T, F>, node, recipe_of(self.f));
        let hd = m.rt.bough.new_handle();
        m.rt.nodes[node as usize].handle = hd;
        m.scope.nodes.push(node);
        m.scope.runs.push(run);
        m.tape_push(node);
        m.recs.push(RecordInit { depth: m.depth, ink: ink.to_nrect(), kind: REC_QUAD, payload: pay_irun(first, self.n as u32), handle: hd.idx, clip: m.clip });
    }
    /// The lanes are wire-driven; a re-description steps over the record.
    fn update(self, m: &mut Mounter<'_, V>) {
        m.tape_next();
        m.pos += 1;
    }
}
