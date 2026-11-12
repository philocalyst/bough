//! zgx `callback_mount_bench` onto bough: 500 rows (argv) of a 160 pt text input in a row (gap 8) whose
//! height is `28 + width * 0`, window 1200 x 800: the time to mount (+ first frame).
//! `READS=1`: zgx's callbacks read the root's width. Here the height is the constant 28 (bough has no
//! reactive layout-size operand on this branch), so both variants run the same program.
//! Run: callback_mount_bench [rows]
use bough::raw::App;
use bough::sys::perf::snap;
use bough_widgets::*;
use std::time::Instant;

fn main() {
    let rows: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(500);
    let reads = std::env::var_os("READS").is_some_and(|v| v == "1");
    // zgx builds `Ui::new` and the signals before its clock starts
    let view = CallbackRows::new(rows);
    let cfg = config(1200.0, 800.0, rows, 8, 8);
    let (i0, t0) = (snap().instrs, Instant::now());
    let mut app = App::new(view, cfg, false);
    let (mount_i, mount_t) = (snap().instrs - i0, t0.elapsed());
    app.frame();
    let (all_i, all_t) = (snap().instrs - i0, t0.elapsed());
    println!("PHASE App::new (mount + layout) {:8.3} M instr {:8.3} ms", mount_i as f64 / 1e6, mount_t.as_secs_f64() * 1e3);
    println!("PHASE mount + first frame      {:8.3} M instr {:8.3} ms", all_i as f64 / 1e6, all_t.as_secs_f64() * 1e3);
    println!(
        "{rows} rows, callbacks {}: mount {:.2} ms ({} records)",
        if reads { "reading geometry" } else { "not reading geometry" },
        all_t.as_secs_f64() * 1e3,
        app.rt.bough.len()
    );
}
