//! The pixel gates with real text (text lane): after every step of a random script on `scenes/rows.rs`,
//!   retained (damaged tiles repaired) == full redraw from the stores == immediate-mode paint by the view
//!   == (every 16th step) a fresh mount,
//! in mount-once and in re-describing mode. Lines are runs of the shared glyph lanes, numbers are arithmetic on
//! their value, every colour reads the block. `--features mutant_line` must fail this.
#[path = "../scenes/rows.rs"]
mod rows;
use bough_x::prelude::*;
use bough_x::soft::{Immediate, Soft, SoftText};
use rows::*;

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n.max(1) as u64) as usize
    }
}

fn soft(v: &Rows) -> Soft {
    let mut s = Soft::new(W, H);
    s.text = Some(SoftText { faces: &FACES, glyphs: v.glyphs.clone() });
    s
}
fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

fn check(tag: &str, step: usize, v: &Rows, world: &mut World<Soft>, retained: &mut Soft, fresh: bool) {
    retained.repair(&world.tiles, |s| world.paint(s));
    world.tiles.clear();
    let mut full = soft(v);
    world.paint(&mut full);
    assert!(retained.px == full.px, "{tag} step {step}: retained != full redraw ({} pixels)", diff(&retained.px, &full.px));
    let mut now = soft(v);
    v.view(&mut Immediate::new(&mut now, &world.shown));
    assert!(full.px == now.px, "{tag} step {step}: stores != immediate ({} pixels)", diff(&full.px, &now.px));
    if fresh {
        let mut w2 = World::<Soft>::new(W, H, NWORDS);
        w2.tick(world.now());
        w2.mount(v);
        let mut f = soft(v);
        w2.paint(&mut f);
        assert!(full.px == f.px, "{tag} step {step}: retained != fresh mount ({} pixels)", diff(&full.px, &f.px));
    }
}

fn script(n: usize, steps: usize, redescribe: bool) {
    let mut v = Rows::new(n);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    world.mount(&v);
    let mut retained = soft(&v);
    world.paint(&mut retained);
    world.tiles.clear();
    let tag = if redescribe { "redescribe" } else { "once" };
    check(tag, 0, &v, &mut world, &mut retained, true);
    // text is on screen at all: dark ink over the row backgrounds
    let inked = retained.px.iter().filter(|&&p| p & 0xff < 0x80 && p != 0).count();
    assert!(inked > 2_000, "only {inked} dark pixels: the text did not paint");
    let mut r = Rng(0x7e47 + n as u64);
    let mut next_id = n as u32;
    for step in 1..=steps {
        let len = v.items.len();
        match r.below(9) {
            8 => v.toggle_tint(),
            0 | 1 => v.hover(if r.below(5) == 0 { NO_HOVER } else { r.below(len.min(40)) as u32 + (v.u.get::<Scroll>() / ROW_H) as u32 }),
            2 => v.scroll_by(r.below(400) as i32 - 150),
            3 => v.scroll_by(r.below(5) as i32 - 2),
            4 => v.toggle_suffix(r.below(len)),
            5 => v.set_value(r.below(len), r.below(1_000_000) as i32 - 500_000),
            6 if len > 30 => v.remove(r.below(len)),
            _ => {
                next_id += 1;
                v.insert(r.below(len + 1), next_id)
            }
        }
        if redescribe {
            world.redescribe(&v);
        } else {
            world.settle(&v);
        }
        check(tag, step, &v, &mut world, &mut retained, step % 16 == 0);
    }
    // the memo kept describing cheap: far fewer lines were shaped than were described
    let g = v.glyphs.borrow();
    assert!(g.found > 4 * g.shaped, "{} shaped, {} found", g.shaped, g.found);
}

#[test]
fn rows_with_text_mount_once() {
    script(3_000, 300, false);
    script(40, 150, false);
}
#[test]
fn rows_with_text_redescribing() {
    script(3_000, 200, true);
}
#[test]
fn nothing_unexposed_is_shaped() {
    let v = Rows::new(100_000);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    world.mount(&v);
    let g = v.glyphs.borrow();
    // "rows · sorted", "Row " and the labels of the exposed rows (twelve words, so few distinct labels)
    assert!(g.shaped <= 2 + 26, "{} lines shaped at mount", g.shaped);
    assert!(g.bytes() < 16 * 1024, "{} bytes of text state for 100,000 rows", g.bytes());
}
