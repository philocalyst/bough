//! Compile-cost probe: one row type and nothing else (`bench/r8/lay_compile.sh`).
#[path = "../scenes/rows.rs"]
mod rows;
#[allow(unused_imports)]
use bough_expr::U;
#[allow(unused_imports)]
use bough_lay::*;
fn main() {
    let r = rows::sheet_row(std::env::args().count()); let mut o = [[0i32; 4]; 22]; println!("{:?}", place_row(&r, 2.0, &U::default(), 2560.0, &mut o));
}
