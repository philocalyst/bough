//! Compile-cost probe: one row type and nothing else (`bench/r8/lay_compile.sh`).
#[path = "../scenes/rows.rs"]
mod rows;
#[allow(unused_imports)]
use bough_expr::U;
#[allow(unused_imports)]
use bough_lay::*;
fn main() {
    let t = [(13.2f32, 0.0f32, 3.8f32); rows::MAX_TOK]; let r = rows::ide_line(std::env::args().count() + 3, &t, 26.4); let mut o = [[0i32; 4]; 10]; println!("{:?}", place_row(&r, 2.0, &U::default(), 1860.0, &mut o));
}
