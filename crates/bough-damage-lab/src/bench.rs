//! Timing: replay a trace through one formulation, single-threaded, counting
//! instructions (`ri_instructions`) and time over the presented frames.
//! Stage A = tree update only (scene apply), B = A + record (track),
//! C = B + repair (region + cull). Damage track = B − A, cull = C − B.

use crate::forms::{Form, Out};
use crate::measure::{counters, ticks, ticks_to_ns};
use crate::scene::Scene;
use crate::trace::Trace;

#[derive(Clone, Copy, Default, Debug)]
pub struct Stage {
    pub instr: u64,
    pub ns: f64,
}

pub fn run(tr: &Trace, ring: usize, form: Option<&mut (dyn Form + 'static)>, stage: u8, sc: &mut Scene, out: &mut Out, overhead: u64) -> Stage {
    sc.reset(tr);
    let mut form = form;
    if let Some(f) = form.as_deref_mut() {
        f.reset(tr, sc, ring);
        f.full(0, sc);
    }
    let nf = tr.frames.len();
    for n in 1..ring.min(nf + 1) {
        let fr = &tr.frames[n - 1];
        sc.apply(fr);
        if let Some(f) = form.as_deref_mut() {
            f.record(fr, sc);
            f.full(n % ring, sc);
        }
    }
    let mut sink = 0usize;
    let (i0, _) = counters();
    let t0 = ticks();
    match form {
        None => {
            for n in ring..=nf {
                sc.apply(&tr.frames[n - 1]);
            }
        }
        Some(f) => {
            for n in ring..=nf {
                let fr = &tr.frames[n - 1];
                sc.apply(fr);
                f.record(fr, sc);
                if stage >= 2 {
                    out.clear();
                    f.repair(n % ring, ring as u32, sc, out);
                    sink = sink.wrapping_add(out.list.len()).wrapping_add(out.batches.len());
                }
            }
        }
    }
    let t1 = ticks();
    let (i1, _) = counters();
    std::hint::black_box(sink);
    std::hint::black_box(&out);
    Stage { instr: i1 - i0 - overhead, ns: ticks_to_ns(t1 - t0) }
}

/// Min over `reps` runs (instructions are near-deterministic; time is noisy).
pub fn best(tr: &Trace, ring: usize, mut form: Option<&mut (dyn Form + 'static)>, stage: u8, reps: usize, overhead: u64) -> Stage {
    let mut sc = Scene::new(tr);
    let mut out = Out::default();
    let mut b = Stage { instr: u64::MAX, ns: f64::MAX };
    for _ in 0..reps {
        let s = run(tr, ring, form.as_deref_mut(), stage, &mut sc, &mut out, overhead);
        b.instr = b.instr.min(s.instr);
        b.ns = b.ns.min(s.ns);
    }
    b
}

pub fn presented(tr: &Trace, ring: usize) -> usize {
    (tr.frames.len() + 1).saturating_sub(ring)
}
