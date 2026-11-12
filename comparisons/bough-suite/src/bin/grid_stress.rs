//! A virtual grid of `ROWS` rows × 12 cells (`index_list` with a viewport, headless): what memory and work
//! follow the row count. Mounts, scrolls `TICKS` steps of 37 pt, jumps to 8 spread rows, then prints the
//! mounted rows, the records, the phys footprint and the instructions per scroll tick.
use bough::layout::prelude::*;
use bough::raw::*;
use bough_suite::*;

const COLS: usize = 12;
struct Grid {
    rows: usize,
    scroll: Signal<f32>,
}
fn grid_row(_: &Grid, i: usize, m: &mut Mounter<'_, Grid>) {
    m.mount(row((
        label(format!("{i}")).font(10.0, 14.0).w(56.0),
        row([0usize; COLS].iter().enumerate().map(|(j, _)| block(14.0, 14.0).bg(rgb(((i * COLS + j) as u32).wrapping_mul(0x9E37_79B9) >> 8))).collect::<Vec<_>>()).gap(2.0),
    )));
}
impl View for Grid {
    fn view(&self) -> impl Element<Self> + '_ {
        index_list(self.rows, grid_row).w(300.0).h(600.0).scroll(sig!(self.scroll)).estimate(14.0).gap(2.0)
    }
}

fn mb(b: u64) -> f64 {
    b as f64 / 1048576.0
}

fn main() {
    let rows = env_usize("ROWS", 1_000_000);
    let ticks = env_usize("TICKS", 600);
    let f0 = bough::sys::perf::snap().footprint;
    let mut a = App::new(Grid { rows, scroll: Signal::new(0.0) }, Config::new(320.0, 640.0, 2.0, font()).once(), false);
    a.frame();
    let i0 = instructions();
    for _ in 0..ticks {
        a.turn(|v| *v.scroll += 37.0);
        a.frame();
    }
    let per_tick = (instructions() - i0) as f64 / ticks.max(1) as f64;
    for k in 0..8 {
        let y = (rows / 8 * k) as f32 * 16.0;
        a.turn(|v| *v.scroll = y);
        a.frame();
    }
    let s = bough::sys::perf::snap();
    println!(
        "{rows} rows: {} mounted, {} records, footprint {:.1} MB (+{:.1} MB over the empty process), {:.0} instructions per scroll tick",
        a.rt.list_window(0).len(),
        a.rt.bough.records().len(),
        mb(s.footprint),
        mb(s.footprint.saturating_sub(f0)),
        per_tick
    );
}
