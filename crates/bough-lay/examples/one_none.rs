//! Compile-cost probe: one row type and nothing else (`bench/r8/lay_compile.sh`).
#[path = "../scenes/rows.rs"]
mod rows;
#[allow(unused_imports)]
use bough_expr::U;
#[allow(unused_imports)]
use bough_lay::*;
fn main() {
    println!("{:?}", bough_lay::Uniform { n: std::env::args().count() as u32, h: 44 }.top(3));
}
