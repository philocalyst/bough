//! The independent review's probes for this crate (recent-20261112, batch 6), as tests: what a safe caller
//! can no longer do, and what the admission checks refuse.
use bough_kernel::cpu::{self, Pool};
use bough_paint::expr::*;
use bough_paint::paint::*;
use bough_paint::plan;
use bough_paint::scene::{Desc, Scene};

type Box0 = Solid<Lane, Pm<Rec>>;
type Card = Over<Shadow<Lane, I<Rec>, I<Rec>, Pm<Rec>, Tab>, RRect<Lane, I<Rec>, Pm<Rec>>>;

fn scene() -> Scene {
    let mut s = Scene::new(96, 64, 1 << 12);
    let c = s.column::<Box0>(4);
    s.push(c, [0, 0, 96 * 256, 64 * 256], Solid { b: Lane, c: Pm { c: Rec(0xff30_6090) } });
    s
}

/// R02: the tile list of the safe `shade` is checked to be a set before workers share the surface.
#[test]
#[should_panic(expected = "listed twice")]
fn shade_refuses_a_duplicated_tile() {
    let mut s = scene();
    let u = U::default();
    s.update(&u);
    let mut out = vec![0u32; 96 * 64];
    s.shade(&u, &[0, 1, 0], &mut out, 96, &Pool::new(2), 2);
}
#[test]
#[should_panic(expected = "stride")]
fn shade_refuses_a_stride_under_the_width() {
    let mut s = scene();
    let u = U::default();
    s.update(&u);
    let mut out = vec![0u32; 96 * 64];
    s.shade(&u, &[0], &mut out, 64, &Pool::new(0), 0);
}
#[test]
#[should_panic(expected = "tile")]
fn shade_refuses_a_tile_outside_the_frame() {
    let mut s = scene();
    let u = U::default();
    s.update(&u);
    let mut out = vec![0u32; 96 * 64];
    s.shade(&u, &[999], &mut out, 96, &Pool::new(0), 0);
}

/// R04: a box far larger than the surface that crosses it is drawn exactly (its window is clamped by the
/// stager before it is packed to 16 bits); one that lies beyond the admitted range is refused, not wrapped.
#[test]
fn a_huge_box_across_the_surface_is_exact() {
    let (fw, fh) = (200u32, 120u32);
    let mut s = Scene::new(fw, fh, 1 << 14);
    let c = s.column::<Card>(4);
    let far = 30_000 * 256;
    for (rect, r, blur, col) in [([-far, -far, far, far], 40 * 256, 24 * 256, 0xff20_50a0u32), ([-far, 30 * 256, 90 * 256, far], 12 * 256, 16 * 256, 0xc0f0_8020), ([150 * 256, -far, far, 70 * 256], 0, 30 * 256, 0xff10_9040)] {
        s.push(c, rect, Over(Shadow { b: Lane, r: I { a: Rec(r as u32) }, blur: I { a: Rec(blur as u32) }, c: Pm { c: Rec(0x8000_0000) }, tab: Tab::NONE }, RRect { b: Lane, r: I { a: Rec(r as u32) }, c: Pm { c: Rec(col) } }));
    }
    let u = U::default();
    let pool = Pool::new(1);
    let stride = fw as usize;
    let mut out = vec![0u32; stride * fh as usize];
    s.frame(&u, &mut out, stride, &pool, 1);
    let mut oracle = vec![0u32; stride * fh as usize];
    cpu::naive(&s.stage(&u), fw, fh, &cpu::Atlas::new(&s.atlas, s.aw), 0, &mut oracle, stride);
    let d = out.iter().zip(&oracle).filter(|(a, b)| a != b).count();
    let mut spec = vec![0u32; stride * fh as usize];
    s.spec(&u, &mut spec, stride);
    let ds = spec.iter().zip(&oracle).filter(|(a, b)| a != b).count();
    assert_eq!((d, ds), (0, 0), "differing words: fast kernel {d}, specification {ds}");
    assert!(out.iter().any(|&p| p != out[0]), "the scene drew one flat colour: the boxes' edges are not on the surface");
}
#[test]
#[should_panic(expected = "rect lane beyond")]
fn a_rect_beyond_the_admitted_range_is_refused() {
    let mut s = Scene::new(96, 64, 1 << 12);
    let c = s.column::<Box0>(4);
    s.push(c, [0, 0, 40_000 * 256, 64 * 256], Solid { b: Lane, c: Pm { c: Rec(0xff30_6090) } });
}
#[test]
#[should_panic(expected = "does not fit the window lane")]
fn a_surface_wider_than_the_window_lane_is_refused() {
    let _ = Scene::new(40_000, 64, 1 << 12);
}

/// R05: a plan names archetypes by bit in a 32-bit word; a longer list is refused rather than aliased.
#[test]
#[should_panic(expected = "more than 32 archetypes")]
fn a_plan_over_33_archetypes_is_refused() {
    let d: Vec<&'static Desc> = (0..33).map(|_| Desc::of::<Box0>()).collect();
    let _ = plan::plan_dyn(&d);
}
#[test]
fn a_plan_over_32_archetypes_stands() {
    let d: Vec<&'static Desc> = (0..32).map(|_| Desc::of::<Box0>()).collect();
    let (a, _, deps) = plan::plan_dyn(&d);
    assert_eq!(a.len(), 32);
    assert!(deps.iter().all(|&m| m == 0));
}
