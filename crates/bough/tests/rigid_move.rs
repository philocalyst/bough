//! A splice is a rigid move (round 7): when a row leaves or arrives, the rows after it are displaced by a
//! whole number of pixels with nothing inside them changed. The layout write-back hands the core one move
//! for their records (`Bough::move_begin` / `move_end`) instead of repainting each row: the frame copies the
//! pixels and repairs the exposed strip. The gates are the usual ones: the presented frame equals a full
//! redraw, and the retained records equal a fresh mount.
use bough::layout::prelude::*;
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Table {
    rows: Vec<(u32, String)>,
}
impl View for Table {
    fn view(&self) -> impl Element<Self> + '_ {
        let line = |r: &(u32, String)| {
            row((block(18.0, 18.0).bg(rgb(0x203040 + r.0 * 0x030507)), label(r.1.as_str()).font(12.0, 16.0)))
                .gap(6.0)
                .h(24.0)
                .w(260.0)
                .bg(rgb(if r.0 % 2 == 0 { 0xf5f5f5 } else { 0xe6e6e6 }))
        };
        column((block(260.0, 10.0).bg(rgb(0x808080)), column(each(self.rows.iter().map(line))), block(260.0, 14.0).bg(rgb(0x2f6bff)))).w(300.0).bg(rgb(0xffffff))
    }
}

fn rows(n: u32) -> Vec<(u32, String)> {
    (0..n).map(|i| (i, format!("row number {i}"))).collect()
}

type Item7 = (u8, [f32; 4], u32, u32, f32, f32, [u16; 2]);
fn dump(app: &App<impl View>) -> Vec<(u8, u16, [i16; 4], Vec<Item7>)> {
    app.rt.bough.records().iter().map(|r| (r.kind, r.depth, r.ink.0, if r.ink.is_empty() { Vec::new() } else { app.rt.expand_payload(r.payload) })).collect()
}

#[test]
fn a_removed_row_moves_the_rows_below_it_as_one_run() {
    let cfg = || Config::new(320.0, 800.0, 1.0, FONT.to_vec());
    let mut a = App::new(Table { rows: rows(25) }, cfg(), false);
    a.frame();
    let before = a.rt.lay.stats.rigid_moves;
    a.turn(|t| drop(t.rows.remove(4)));
    assert_eq!(a.rt.lay.stats.rigid_moves, before + 1, "the rows below the removed one are one rigid move");
    let moves = a.rt.bough.moves(0).to_vec();
    assert_eq!(moves.len(), 1);
    assert_eq!((moves[0].dx, moves[0].dy), (0, -24));
    // what is damaged: the strip the move exposed and the edges of what stayed, not the rows that moved
    let marked = a.rt.bough.plane(0).count();
    assert!(marked <= 30, "{marked} tiles marked for one removed row (a repaint of the 20 rows below it is 200)");
    a.frame();
    let mut t = Table { rows: rows(25) };
    t.rows.remove(4);
    let mut f = App::new(t, cfg(), false);
    f.frame();
    assert_eq!(dump(&a), dump(&f), "retained != fresh mount");
}

#[test]
fn splices_present_what_a_full_redraw_draws() {
    for ring in [1usize, 2, 4] {
        let mut cfg = Config::new(320.0, 800.0, 1.0, FONT.to_vec());
        cfg.ring = ring;
        let mut a = App::new(Table { rows: rows(22) }, cfg, true);
        a.frame();
        let mut r = 0x1234_5678_9abc_def1u64 ^ ring as u64;
        let mut rnd = move |n: usize| {
            r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (r >> 33) as usize % n.max(1)
        };
        let mut next = 100u32;
        let mut moves = 0;
        for step in 0..40 {
            let n = a.owner.rows.len();
            let grow = n < 6 || (n < 30 && rnd(2) == 0);
            let at = rnd(n + grow as usize);
            let id = next;
            next += 1;
            let before = a.rt.lay.stats.rigid_moves;
            a.turn(|t| {
                if grow {
                    t.rows.insert(at, (id, format!("arrived {id}")));
                } else {
                    t.rows.remove(at);
                }
            });
            moves += a.rt.lay.stats.rigid_moves - before;
            a.frame();
            let (px, full) = (a.presented_pixels(), a.reference_pixels());
            if px != full {
                let i = px.iter().zip(&full).position(|(x, y)| x != y).unwrap();
                let w = a.rt.win_w as usize;
                panic!("ring {ring} step {step} ({} at {at} of {n}): presented != full redraw at ({}, {})", if grow { "insert" } else { "remove" }, (i / 4) % w, (i / 4) / w);
            }
        }
        assert!(moves > 20, "ring {ring}: {moves} rigid moves in 40 splices");
    }
}
