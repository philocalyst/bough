//! The façade's sheet on this core, held to the same oracles as the core's fixture: after every step of a
//! random script, in mount-once mode and in the façade's default (described again after every turn),
//!   retained (damaged tiles repaired) == full redraw from the stores == immediate-mode paint by the view
//!   == (every 16th step) a fresh mount at the same instant.
#[path = "../scenes/sheet_el.rs"]
mod sheet_el;
use bough_x::el::App;
use bough_x::prelude::*;
use bough_x::soft::{Immediate, Soft};
use sheet_el::*;

const W: i32 = 2560;
const H: i32 = 1600;
const SCALE: f32 = 2.0;

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n.max(1) as u64) as usize
    }
}

fn check(tag: &str, step: usize, app: &mut App<Sheet, Soft>, retained: &mut Soft, fresh: bool) {
    let world = &mut app.world;
    retained.repair(&world.tiles, |s| world.paint(s));
    world.tiles.clear();
    let mut full = Soft::new(W, H);
    world.paint(&mut full);
    let diff = |a: &[u32], b: &[u32]| a.iter().zip(b).filter(|(x, y)| x != y).count();
    assert!(retained.px == full.px, "{tag} step {step}: retained != full redraw ({} pixels)", diff(&retained.px, &full.px));
    let mut now = Soft::new(W, H);
    bough_x::sink::View::view(&app.v, &mut Immediate::new(&mut now, &world.shown));
    assert!(full.px == now.px, "{tag} step {step}: stores != immediate ({} pixels)", diff(&full.px, &now.px));
    if fresh {
        // a fresh mount of the same state at the same instant shows the same, except a transition in flight
        // (a fresh mount is at rest): compared only when nothing is moving
        if world.tiles.sweeps.is_empty() {
            let mut w2 = World::<Soft>::new(W, H, 48);
            w2.shown = world.shown;
            w2.mount(&app.v);
            let mut f = Soft::new(W, H);
            w2.paint(&mut f);
            assert!(full.px == f.px, "{tag} step {step}: retained != fresh mount ({} pixels)", diff(&full.px, &f.px));
        }
    }
}

fn script(rows: usize, steps: usize, once: bool) {
    let sheet = *Sheet::with_rows(rows);
    let mut app = App::<Sheet, Soft>::new(sheet, W, H, SCALE);
    app.once = once;
    let mut retained = Soft::new(W, H);
    let mut rng = Rng(0x5ee7 | 1);
    let tag = if once { "once" } else { "redescribe" };
    check(tag, 0, &mut app, &mut retained, true);
    let viewport = 800.0 - TOOLBAR - HEADER - CELL_H;
    let mut t = 1u32;
    for step in 1..=steps {
        t += if rng.below(4) == 0 { 400 } else { 16 + rng.below(40) as u32 };
        app.frame(t);
        match rng.below(8) {
            0 => {
                let c = rng.below(COLS);
                app.pointer_move((GUTTER + c as f32 * CELL_W + 25.0) * SCALE, (TOOLBAR + 5.0) * SCALE);
            }
            1 => app.pointer_move((GUTTER + 150.0) * SCALE, 10.0 * SCALE),
            2 => app.turn(|s| s.toggle_theme()),
            3 => {
                let top = (*app.v.scroll / CELL_H) as usize;
                let r = if rng.below(2) == 0 { (top + rng.below(30)).min(rows - 1) } else { rng.below(rows) };
                let (c, v) = (rng.below(COLS), rng.below(1_000_000) as f32 / 100.0);
                app.turn(|s| s.edit(r * COLS + c, v));
            }
            4 => {
                let top = (*app.v.scroll / CELL_H) as usize;
                let i = ((top + rng.below(30)).min(rows - 1) * COLS + rng.below(COLS)) as u32;
                app.turn(|s| s.select(i));
            }
            5 => app.turn(|s| s.scroll_by(if rng.below(2) == 0 { 66.0 } else { -66.0 }, viewport)),
            6 => {
                let dy = rng.below(2000) as f32 - 750.0;
                app.turn(|s| s.scroll_by(dy, viewport))
            }
            _ => {
                let dy = rng.below(45) as f32 - 20.0;
                app.turn(|s| s.scroll_by(dy, viewport))
            }
        }
        check(tag, step, &mut app, &mut retained, step % 16 == 0);
    }
}

#[test]
fn facade_sheet_once() {
    script(10_000, 300, true);
}
#[test]
fn facade_sheet_redescribing() {
    script(10_000, 200, false);
}
#[test]
fn facade_sheet_short() {
    script(20, 150, true);
}

#[test]
fn nothing_unexposed_exists() {
    let app = App::<Sheet, Soft>::new(*Sheet::with_rows(10_000), W, H, SCALE);
    let (records, _, bytes) = app.world.root.stats();
    // 35 exposed rows of 22 records and under 60 fixed records
    assert!(records < 36 * 22 + 60, "{records} records for a window of 35 rows");
    assert!(bytes < 320 * 1024, "{bytes} bytes of columns");
}
