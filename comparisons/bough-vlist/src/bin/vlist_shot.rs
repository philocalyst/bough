//! Render the vlist scene offscreen for `TICKS` ticks of `ZGX_MODE` and write the last presented frame
//! as raw BGRA (`OUT`, default vlist.bgra, 1440 x 1600); also compares it with a full redraw of the same
//! retained state and prints the number of differing bytes.
use bough::prelude::*;
use bough_vlist::*;
use zgx_workload::vlist::{self, Mode};

fn main() {
    let ticks: usize = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
    let out = std::env::var("OUT").unwrap_or("vlist.bgra".into());
    let mode = Mode::from_env();
    let mut app = App::new(VList::new(vlist::ROWS), config(2.0), true);
    app.frame();
    for t in 1..=ticks {
        step(&mut app, mode, t as u64);
        app.frame();
    }
    let px = app.presented_pixels();
    let reference = app.reference_pixels();
    let diff = px.iter().zip(&reference).filter(|(a, b)| a != b).count();
    println!("{mode:?} after {ticks} ticks: scroll {:.1} pt, {} differing bytes vs a full redraw, mounted rows {:?}", app.rt.mregion_scroll_pt(0), diff, app.rt.mregions[0].range);
    std::fs::write(out, px).unwrap();
}
