//! Compile-cost probe: one row type and nothing else (`bench/r8/lay_compile.sh`). `one_none` lays out a single
//! box, so the layout function itself is in every probe and the difference is the row type.
#[path = "../scenes/rows.rs"]
mod rows;
use bough_expr::U;
use bough_lay::*;
fn main() {
    let t = [(13.2f32, 0.0f32, 3.8f32); rows::MAX_TOK]; let mut m = Memo::new(); let mut o = [[0i32; 4]; 10]; println!("{:?}", m.row(&rows::ide_line(std::env::args().count() + 3, &t, 26.4), 2.0, &U::default(), 1860.0, (0, 0), &mut o[..]));
}
