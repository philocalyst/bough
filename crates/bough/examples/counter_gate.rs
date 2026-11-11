//! The counter's pixel gate as an executable, so it runs on a binary built with any profile (the size lane's
//! profiles rebuild std with `panic_immediate_abort`, which the test harness cannot use). It does what
//! `tests/counter.rs` does: the button is found by its interactive record's layout box, clicked three times
//! with real pointer events through hit testing, and the presented frame is compared with a full redraw.
//! It prints the count and the differing bytes, writes the presented frame (raw BGRA) to `OUT` when set, and
//! exits nonzero when the count is not 3 or any byte differs.
//!
//!   ZGX_FONT_FILE=<DejaVuSans.ttf> OUT=counter.bgra target/<profile>/examples/counter_gate
#[path = "counter.rs"]
mod counter;

use bough::input::Button;
use bough::raw::{App, Config};
use bough::signal::Signal;
use counter::Counter;

const SCALE: f32 = 2.0;

fn main() {
    let path = std::env::var("ZGX_FONT_FILE")
        .unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into());
    let font = std::fs::read(path).expect("a font");
    let mut app = App::new(
        Counter {
            count: Signal::new(0),
        },
        Config::new(480.0, 320.0, SCALE, font),
        true,
    );
    app.frame();
    let r = (0..app.record_count())
        .map(|p| app.record_view(p))
        .find(|r| r.kind & 0x80 != 0)
        .expect("an interactive record");
    let [x, y, w, h] = r.layout.expect("the button is a layout box");
    let (x, y) = ((x + w / 2.0) / SCALE, (y + h / 2.0) / SCALE);
    for _ in 0..3 {
        app.pointer_down(x, y, Button::Primary);
        app.pointer_up(x, y, Button::Primary);
        app.frame();
    }
    let mut n = 0;
    app.turn(|c| n = *c.count);
    let px = app.presented_pixels();
    let reference = app.reference_pixels();
    let diff = px.iter().zip(&reference).filter(|(a, b)| a != b).count();
    println!("counter: count {n}, {diff} differing bytes vs a full redraw");
    if let Ok(out) = std::env::var("OUT") {
        std::fs::write(out, &px).expect("write OUT");
    }
    std::process::exit(if n == 3 && diff == 0 { 0 } else { 1 });
}
