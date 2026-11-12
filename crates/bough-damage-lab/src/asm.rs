//! Out-of-line, unmangled instances of the hot loops so their assembly can be
//! read (`objdump --disassemble-symbols=_lab_...`). Not used on any measured
//! path (the measured code inlines the same generic functions).

use crate::forms::{Out, Plane, cull_plane, cull_plane_runs, exact_slot};
use crate::geom::R;
use crate::scene::Scene;
use crate::trace::PState;

#[unsafe(no_mangle)]
#[inline(never)]
pub fn lab_plane_mark(p: &mut Plane, r: R) {
    p.mark(r)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn lab_plane_or(p: &mut Plane, o: &Plane) {
    p.or_from(o)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn lab_exact_slot_mark(p: &mut Plane, o: &PState, n: &PState) {
    exact_slot(o, n, &mut |r| p.mark(r))
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn lab_cull_plane(sc: &Scene, p: &Plane, out: &mut Vec<u32>) {
    cull_plane(sc, p, out)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn lab_cull_plane_runs(sc: &Scene, p: &Plane, out: &mut Out) {
    cull_plane_runs(sc, p, out)
}

#[cfg(target_arch = "aarch64")]
#[unsafe(no_mangle)]
#[inline(never)]
pub fn lab_cull_hw_neon(sc: &Scene, p: &crate::forms::hw::Hw, bb: R, out: &mut Vec<u32>) {
    crate::forms::hw::cull_hw_neon(sc, p, bb, out)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn lab_hw_mark(p: &mut crate::forms::hw::Hw, r: R) {
    p.mark(r)
}
