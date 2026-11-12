//! Compile-cost probe: one row type and nothing else (`bench/r8/lay_compile.sh`).
#[path = "../scenes/rows.rs"]
mod rows;
#[allow(unused_imports)]
use bough_expr::U;
#[allow(unused_imports)]
use bough_lay::*;
fn main() {
    let m = rows::msg(std::env::args().count() as u64); let r = rows::chat_row(&m); let mut o = [[0i32; 4]; 24]; println!("{:?}", place_row(&r, 2.0, &U::default(), 1960.0, &mut o[..]));
}
