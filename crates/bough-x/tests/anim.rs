//! The pixel gates on the animation fixture: time moves every step with no turn, rows in endless motion enter
//! and leave exposure, the pointer words move, and after every step
//!   retained (damaged tiles repaired) == full redraw from the stores == immediate-mode paint by the view
//!   == (every 8th step) a fresh mount at the same instant.
//! `--features mutant_sweep` (a frame damages nothing for records in motion) must fail this.
#[path = "../scenes/anim.rs"]
mod anim;
use anim::*;
use bough_x::prelude::*;
use bough_x::soft::{Immediate, Soft};

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n.max(1) as u64) as usize
    }
}
const H: i32 = 600;

fn check(step: usize, a: &Anim, world: &mut World<Soft>, retained: &mut Soft, fresh: bool) {
    retained.repair(&world.tiles, |s| world.paint(s));
    world.tiles.clear();
    let mut full = Soft::new(W, H);
    world.paint(&mut full);
    let diff = |a: &[u32], b: &[u32]| a.iter().zip(b).filter(|(x, y)| x != y).count();
    assert!(retained.px == full.px, "step {step}: retained != full redraw ({} pixels)", diff(&retained.px, &full.px));
    let mut now = Soft::new(W, H);
    a.view(&mut Immediate::new(&mut now, &world.shown));
    assert!(full.px == now.px, "step {step}: stores != immediate ({} pixels)", diff(&full.px, &now.px));
    if fresh {
        let mut w2 = World::<Soft>::new(W, H, NWORDS);
        w2.shown = world.shown;
        w2.mount(a);
        let mut f = Soft::new(W, H);
        w2.paint(&mut f);
        assert!(full.px == f.px, "step {step}: retained != fresh mount ({} pixels)", diff(&full.px, &f.px));
    }
}

#[test]
fn animation_matches_every_oracle() {
    let mut a = Anim::new(500, H);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    world.mount(&a);
    let mut retained = Soft::new(W, H);
    let mut rng = Rng(0xa11e | 1);
    check(0, &a, &mut world, &mut retained, true);
    let mut t = 0u32;
    for step in 1..=300 {
        t += if rng.below(5) == 0 { 500 } else { 16 + rng.below(30) as u32 };
        world.tick(t);
        match rng.below(8) {
            0 => a.scroll_by(rng.below(400) as i32 - 150),
            1 => a.scroll_by(rng.below(4000) as i32 - 1500),
            2 => a.toggle(world.now()),
            3 | 4 => world.pointer_words(rng.below(W as usize) as i32, rng.below(H as usize) as i32),
            _ => {}
        }
        world.settle(&a);
        check(step, &a, &mut world, &mut retained, step % 8 == 0);
    }
}

/// With nothing endless on screen a frame at rest leaves no damage and no sweep behind.
#[test]
fn a_finished_transition_leaves_nothing() {
    let mut a = Anim::new(0, H);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    // long after the fade's start word (0), so the banner is at rest when mounted
    world.tick(1000);
    world.mount(&a);
    world.tiles.clear();
    assert!(world.tiles.sweeps.is_empty());
    world.tick(1100);
    a.toggle(world.now());
    world.settle(&a);
    assert_eq!(world.tiles.sweeps.len(), 1);
    world.tiles.clear();
    world.tick(1150);
    assert!(world.tiles.any() && world.tiles.sweeps.len() == 1);
    world.tiles.clear();
    world.tick(1300);
    assert!(world.tiles.any(), "the frame in which it comes to rest is damaged");
    assert!(world.tiles.sweeps.is_empty());
    world.tiles.clear();
    world.tick(1316);
    assert!(!world.tiles.any());
}
