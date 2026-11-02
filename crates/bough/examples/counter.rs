//! The design's counter (`designs/reactivity/final/examples/progressive.rs`, rung 1): state is a plain struct,
//! the route is a method, layout is a recipe, and nothing takes a coordinate.
//!
//!   cargo run --release -p bough --example counter        # a window; click "+"
//!   cargo test --release -p bough --test counter          # headless: real clicks, the count, the pixels
use bough::prelude::*;

const BAR: Flex = Flex::row().align(Align::Center).gap(12.0);

pub struct Counter {
    pub count: Signal<u32>,
}

impl Counter {
    pub fn inc(&mut self) {
        *self.count += 1; // a publish: the label's port patches, `view` does not rerun
    }
}

impl View for Counter {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .layout(BAR)
            .p(24.0)
            .child(text(sig!(self.count).map(|n| format!("Count: {n}"))))
            .child(button("+").on_click(Counter::inc))
    }
}

#[allow(dead_code)]
fn main() {
    Application::run(Window::document(Counter { count: Signal::new(0) }).title("Counter"));
}
