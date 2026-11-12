//! The site's scroll workload: a non-virtualized list of `SCROLL_ROWS` rows (default 1,000), each 960 x 48
//! logical pixels with 12 px padding, black 14 px `Item N`, alternating #f5f5f5 / #e6e6e6, in a 1000 x 700
//! window. The list is mounted whole; the wheel scrolls it.
fn row_count() -> usize { 1000 }
use bough::prelude::*;

pub struct ScrollApp {
    rows: usize,
    y: Signal<f32>,
}

fn row(i: usize) -> impl Element<ScrollApp> {
    let bg = if i % 2 == 0 { rgb(0xf5f5f5) } else { rgb(0xe6e6e6) };
    div().w(960.0).h(48.0).p(12.0).bg(bg).child(text(format!("Item {}", i + 1)))
}

impl View for ScrollApp {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(each((0..self.rows).map(row)))
    }
}

pub fn main() {
    Application::run(Window::document(ScrollApp { rows: row_count(), y: Signal::new(0.0) }).title("bough Scroll").size(1000.0, 700.0));
}

pub fn make() -> ScrollApp { ScrollApp { rows: 1000, y: Signal::new(0.0) } }
pub fn step(v: &mut ScrollApp) { let y = *v.y; v.y.set(if y == 0.0 { 16.0 } else { 0.0 }); }
