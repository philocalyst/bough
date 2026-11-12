//! Where one hover crossing's instructions go (hit test, hit cell, chain), over documents of widget_bench's kind:
//! `rows` (the bench), `column` (the same rows without the inputs), `fields` (callback_mount_bench's rows).
//! Run: hover_probe [rows] [doc]   (`HX`: the pointer's x, default 20)
use bough::layout::prelude::*;
use bough::raw::*;
use bough::sys::perf::{snap, snap_overhead};
use bough_widgets::*;

struct Col {
    n: usize,
}
impl View for Col {
    fn view(&self) -> impl Element<Self> + '_ {
        column(each((0..self.n).map(|i| row((button(BUTTON_BG, format!("Button {i}")), button(STYLED_BG, "Styled"))).gap(ROW_GAP))))
    }
}

fn probe<V: View>(app: &mut App<V>, doc: &str) {
    let hx: f32 = std::env::var("HX").ok().and_then(|v| v.parse().ok()).unwrap_or(20.0);
    app.frame();
    let oh = snap_overhead();
    let (mut top, mut cell, mut full) = (0u64, 0u64, 0u64);
    for step in 0..400 {
        let y = hover_y(step);
        let (px, py) = (hx as i32, y as i32);
        let i0 = snap().instrs;
        let t = app.rt.bough.hit_top(px, py);
        let i1 = snap().instrs;
        let c = app.rt.bough.hit_cell(px, py, t);
        let i2 = snap().instrs;
        std::hint::black_box(c);
        top += i1 - i0 - oh;
        cell += i2 - i1 - oh;
        let i3 = snap().instrs;
        app.pointer_move(hx, y);
        full += snap().instrs - i3 - oh;
    }
    println!("{doc}: {} records, {} pages; per step: hit_top {} instr, hit_cell {} instr, pointer_move {} instr", app.rt.bough.len(), app.rt.bough.num_pages(), top / 400, cell / 400, full / 400);
}

fn main() {
    let rows: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(500);
    let doc = std::env::args().nth(2).unwrap_or_else(|| "rows".to_string());
    let cfg = || config(1200.0, 800.0, rows, 12, 40);
    match doc.as_str() {
        "rows" => probe(&mut App::new(Widgets::new(rows, &field_xs(rows)), cfg(), false), &doc),
        "column" => probe(&mut App::new(Col { n: rows }, cfg(), false), &doc),
        "fields" => probe(&mut App::new(CallbackRows::new(rows), cfg(), false), &doc),
        _ => panic!("doc: rows | column | fields"),
    }
}
