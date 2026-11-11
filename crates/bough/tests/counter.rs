//! The counter example, driven headless: the button is found by its interactive record's layout box, clicked
//! with real pointer events through hit testing, and the presented pixels are compared with a full redraw.
#[path = "../examples/counter.rs"]
mod counter;

use bough::input::Button;
use bough::raw::{App, Config};
use bough::signal::Signal;
use counter::Counter;

const SCALE: f32 = 2.0;

fn app() -> App<Counter> {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut app = App::new(Counter { count: Signal::new(0) }, Config::new(480.0, 320.0, SCALE, font), true);
    app.frame();
    app
}

/// The centre of the button (the one interactive record), in points.
fn button_centre(app: &App<Counter>) -> (f32, f32) {
    let r = (0..app.record_count()).map(|p| app.record_view(p)).find(|r| r.kind & 0x80 != 0).expect("an interactive record");
    let [x, y, w, h] = r.layout.expect("the button is a layout box");
    ((x + w / 2.0) / SCALE, (y + h / 2.0) / SCALE)
}

fn count(app: &mut App<Counter>) -> u32 {
    let mut n = 0;
    app.turn(|c| n = *c.count);
    n
}

#[test]
fn three_clicks_count_to_three_and_repaint_exactly() {
    let mut app = app();
    let (x, y) = button_centre(&app);
    for _ in 0..3 {
        app.pointer_down(x, y, Button::Primary);
        app.pointer_up(x, y, Button::Primary);
        app.frame();
    }
    assert_eq!(count(&mut app), 3);
    assert_eq!(app.presented_pixels(), app.reference_pixels(), "the repaired frame equals a full redraw");
}

#[test]
fn a_click_beside_the_button_does_nothing() {
    let mut app = app();
    app.pointer_down(470.0, 310.0, Button::Primary);
    app.pointer_up(470.0, 310.0, Button::Primary);
    app.frame();
    assert_eq!(count(&mut app), 0);
}
