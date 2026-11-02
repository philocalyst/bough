//! The counter example, driven headless: real pointer events through hit testing, then the presented pixels
//! compared with a full redraw.
#[path = "../examples/counter.rs"]
mod counter;

use bough::prelude::*;
use counter::{config, Counter};

#[test]
fn three_clicks_count_to_three_and_repaint_exactly() {
    let mut app = App::new(Counter { count: Signal::new(0) }, config(2.0), true);
    app.frame();
    for _ in 0..3 {
        app.pointer_down(60.0, 80.0, Button::Primary);
        app.pointer_up(60.0, 80.0, Button::Primary);
        app.frame();
    }
    let mut n = 0;
    app.turn(|c| n = *c.count);
    assert_eq!(n, 3);
    assert_eq!(app.presented_pixels(), app.reference_pixels(), "the repaired frame equals a full redraw");
}

#[test]
fn a_click_outside_the_button_does_nothing() {
    let mut app = App::new(Counter { count: Signal::new(0) }, config(2.0), true);
    app.frame();
    app.pointer_down(200.0, 20.0, Button::Primary);
    app.pointer_up(200.0, 20.0, Button::Primary);
    app.frame();
    let mut n = -1;
    app.turn(|c| n = *c.count);
    assert_eq!(n, 0);
}
