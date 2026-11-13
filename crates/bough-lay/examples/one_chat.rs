//! Compile-cost probe: one row type and nothing else (`bench/r8/lay_compile.sh`). `one_none` lays out a single
//! box, so the layout function itself is in every probe and the difference is the row type.
#[path = "../scenes/rows.rs"]
mod rows;
use bough_expr::U;
use bough_lay::*;
fn main() {
    let msg = rows::msg(std::env::args().count() as u64); let mut m = Memo::new(); let mut o = [[0i32; 4]; 24]; println!("{:?}", m.row(&rows::chat_row(&msg), 2.0, &U::default(), 1960.0, (0, 0), &mut o[..]));
}
