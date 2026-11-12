//! The pixel gates of phase 1, on the sheet: after every step of a random script,
//!   retained (damaged tiles repaired) == full redraw from the stores == immediate-mode paint by the view
//!   == a fresh mount.
//! `--features mutant_delta` (the derivative skips odd records) must fail this.
#[path = "../scenes/sheet.rs"]
mod sheet;
use bough_x::prelude::*;
use bough_x::soft::{Immediate, Soft};
use sheet::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn check(tag: &str, step: usize, sheet: &Sheet, world: &mut World<Soft>, retained: &mut Soft, fresh: bool) {
    retained.repair(&world.tiles, |s| world.paint(s));
    world.tiles.clear();
    let mut full = Soft::new(W, H);
    world.paint(&mut full);
    assert!(retained.px == full.px, "{tag} step {step}: retained != full redraw ({} pixels)", diff(&retained.px, &full.px));
    let mut now = Soft::new(W, H);
    sheet.view(&mut Immediate::new(&mut now, &sheet.u));
    assert!(full.px == now.px, "{tag} step {step}: stores != immediate ({} pixels)", diff(&full.px, &now.px));
    if fresh {
        let mut w2 = World::<Soft>::new(W, H, NWORDS);
        w2.mount(sheet);
        let mut f = Soft::new(W, H);
        w2.paint(&mut f);
        assert!(full.px == f.px, "{tag} step {step}: retained != fresh mount ({} pixels)", diff(&full.px, &f.px));
    }
}
fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

fn script(rows: usize, steps: usize, redescribe: bool) {
    let mut sheet = Sheet::new(rows);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    world.mount(&sheet);
    let mut retained = Soft::new(W, H);
    let mut rng = Rng(0x5ee7 | 1);
    let tag = if redescribe { "redescribe" } else { "once" };
    check(tag, 0, &sheet, &mut world, &mut retained, true);
    for step in 1..=steps {
        let n = sheet.rows.len();
        match rng.below(10) {
            0 => {
                let c = rng.below(COLS);
                world.pointer(&mut sheet, GUTTER + c as i32 * CELL_W + 50, TOOLBAR + 10);
            }
            1 => {
                world.pointer(&mut sheet, GUTTER + 300, 20);
            }
            2 => sheet.toggle_theme(),
            3 => {
                // an exposed cell, or any cell
                let top = (sheet.u.get::<Scroll>() / CELL_H) as usize;
                let r = if rng.below(2) == 0 { (top + rng.below(30)).min(n - 1) } else { rng.below(n) };
                let (c, v) = (rng.below(COLS), rng.below(1_000_000) as f32 / 100.0);
                if redescribe { sheet.edit_opaque(r, c, v) } else { sheet.edit(r, c, v) }
            }
            4 => sheet.select(rng.below(n * COLS) as u32),
            5 => sheet.scroll_by(if rng.below(2) == 0 { 132 } else { -132 }),
            6 => sheet.scroll_by(rng.below(4000) as i32 - 1500),
            7 => sheet.scroll_by(rng.below(90) as i32 - 40),
            8 => {
                let top = (sheet.u.get::<Scroll>() / CELL_H) as usize;
                let at = (top + rng.below(36)).min(n);
                sheet.insert_row(at, std::array::from_fn(|c| (step * 13 + c) as f32), redescribe);
            }
            _ => {
                if n > 3 {
                    let top = (sheet.u.get::<Scroll>() / CELL_H) as usize;
                    sheet.remove_row((top + rng.below(36)).min(n - 1), redescribe);
                }
            }
        }
        if redescribe {
            world.redescribe(&sheet);
        } else {
            world.settle(&sheet);
        }
        check(tag, step, &sheet, &mut world, &mut retained, step % 16 == 0);
    }
}

#[test]
fn once_mode_matches_every_oracle() {
    script(10_000, 400, false);
}
#[test]
fn short_sheet() {
    script(20, 200, false);
}
#[test]
fn redescribing_mode_matches_every_oracle() {
    script(10_000, 300, true);
}

#[test]
fn nothing_unexposed_exists() {
    let sheet = Sheet::new(10_000);
    let mut world = World::<Soft>::new(W, H, NWORDS);
    world.mount(&sheet);
    let (records, _, bytes) = world.root.stats();
    // 34 exposed rows of 22 records and under 40 fixed records
    assert!(records < 34 * 22 + 40, "{records} records for a window of 34 rows");
    assert!(bytes < 256 * 1024, "{bytes} bytes of columns");
}
