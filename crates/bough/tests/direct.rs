//! Direct wires (`direct.rs`): a colour bound to one box through `sig!` is applied by `Signal::set` itself.
//!
//! After every step the presented frame equals the full redraw and the retained tree equals a fresh mount of
//! the same state, at ring sizes 1..4, and the expected colours are probed. Steps: single writes, a batch of
//! writes to one cell and to many cells in one turn, a `DerefMut` write (the ordinary path), a signal bound
//! twice (no longer direct), a box under a variant (never direct), writes before the first frame, a box that a
//! re-description removes and brings back. Mutants: `--features mutant_direct_damage` (no damage for a direct
//! write) and `mutant_direct_store` (the write does not store) each fail; `mutant_direct_off` (nothing is
//! direct) passes: the control that the gate does not depend on the mechanism.
use bough::prelude::*;
use bough::raw::{App, Config};

const W: usize = 200;

fn config(ring: usize) -> Config {
    let mut c = Config::new(W as f32, 160.0, 1.0, zgx_workload::FONT_BYTES.to_vec());
    c.ring = ring;
    c
}
fn compare(a: &[u8], b: &[u8], context: &str) {
    assert_eq!(a.len(), b.len(), "{context}: dimensions");
    let n = a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a != b).count();
    if n != 0 {
        let first = a.chunks(4).zip(b.chunks(4)).position(|(a, b)| a != b).unwrap();
        panic!("{context}: {n} unequal pixels, the first at ({}, {})", first % W, first / W);
    }
}
fn px(p: &[u8], x: f32, y: f32) -> u32 {
    let i = (y as usize * W + x as usize) * 4;
    (p[i + 2] as u32) << 16 | (p[i + 1] as u32) << 8 | p[i] as u32
}

struct Tiles {
    tint: [Signal<Color>; 6],
    /// bound twice: an ordinary chain
    both: Signal<Color>,
    /// under a variant: an ordinary wire
    held: Signal<Color>,
    show: bool,
}
impl Tiles {
    fn new() -> Tiles {
        Tiles { tint: std::array::from_fn(|i| Signal::new(rgb(0x202020 + 0x101010 * i as u32))), both: Signal::new(rgb(0x884400)), held: Signal::new(rgb(0x004488)), show: true }
    }
    fn copy(&self) -> Tiles {
        Tiles { tint: std::array::from_fn(|i| Signal::new(*self.tint[i])), both: Signal::new(*self.both), held: Signal::new(*self.held), show: self.show }
    }
}
impl View for Tiles {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row(each((0..6).map(|i| block(30.0, 30.0).bg(sig!(self.tint).at(i))))),
            row((block(40.0, 20.0).bg(sig!(self.both)), block(40.0, 20.0).bg(sig!(self.both)), block(40.0, 20.0).bg(sig!(self.held)).when(Flag::SELECTED, |s| s.bg(rgb(0xffffff))))),
            self.show.then(|| block(60.0, 20.0).bg(sig!(self.tint).at(5))),
        ))
        .bg(rgb(0xf0f0f0))
    }
}

fn gate(a: &mut App<Tiles>, ring: usize, what: &str) -> Vec<u8> {
    a.frame();
    let retained = a.reference_pixels();
    compare(&a.presented_pixels(), &retained, &format!("ring {ring} {what}: presented/full"));
    let mut f = App::new(a.owner.copy(), config(ring), true);
    f.frame();
    compare(&retained, &f.reference_pixels(), &format!("ring {ring} {what}: retained/fresh"));
    retained
}

#[test]
fn direct_writes_equal_a_fresh_mount() {
    for ring in 1..=4 {
        let mut a = App::new(Tiles::new(), config(ring), true);
        // a write before the first frame (the binding is not armed yet: the ordinary path)
        a.turn(|t| drop(t.tint[0].set(rgb(0x111111))));
        gate(&mut a, ring, "before the first frame");
        a.turn(|t| drop(t.tint[1].set(rgb(0xaa0000))));
        let p = gate(&mut a, ring, "one write");
        assert_eq!(px(&p, 45.0, 15.0), 0xaa0000);
        a.turn(|t| {
            for c in [0x00aa00, 0x0000aa, 0x00aaaa] {
                t.tint[2].set(rgb(c));
            }
        });
        let p = gate(&mut a, ring, "a batch of writes to one cell");
        assert_eq!(px(&p, 75.0, 15.0), 0x00aaaa);
        a.turn(|t| {
            for i in 0..6 {
                t.tint[i].set(rgb(0x400000 + 0x2000 * i as u32));
            }
        });
        gate(&mut a, ring, "a write to every cell");
        a.turn(|t| *t.tint[3] = rgb(0x123456));
        let p = gate(&mut a, ring, "a DerefMut write (the ordinary path)");
        assert_eq!(px(&p, 105.0, 15.0), 0x123456);
        a.turn(|t| drop(t.both.set(rgb(0x44ff44))));
        let p = gate(&mut a, ring, "a signal bound twice");
        assert_eq!((px(&p, 20.0, 40.0), px(&p, 60.0, 40.0)), (0x44ff44, 0x44ff44));
        a.turn(|t| drop(t.held.set(rgb(0x9999ff))));
        let p = gate(&mut a, ring, "a box under a variant");
        assert_eq!(px(&p, 100.0, 40.0), 0x9999ff);
        a.turn(|t| drop(t.tint[5].set(rgb(0xff8800))));
        let p = gate(&mut a, ring, "a signal of a box and of an optional box");
        assert_eq!((px(&p, 165.0, 15.0), px(&p, 30.0, 60.0)), (0xff8800, 0xff8800));
        a.turn(|t| t.show = false);
        gate(&mut a, ring, "the optional box removed");
        a.turn(|t| drop(t.tint[5].set(rgb(0x0088ff))));
        gate(&mut a, ring, "a write after the removal");
        a.turn(|t| t.show = true);
        a.turn(|t| drop(t.tint[5].set(rgb(0x8800ff))));
        let p = gate(&mut a, ring, "the box back, then a write");
        assert_eq!(px(&p, 30.0, 60.0), 0x8800ff);
    }
}
