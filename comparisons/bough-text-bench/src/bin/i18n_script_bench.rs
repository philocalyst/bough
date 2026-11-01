//! zgx-script's `i18n_bench` (crates/zgx-script/examples/i18n_bench.rs) on bough: `ROWS` (200) texts showing one
//! count, changed `TICKS` times (300); `MODE=message` shows the plural message, `template` `"{count} messages"`,
//! `switch` the message with the locale switched (English, French) every tick instead. zgx counts this whole
//! process's instructions (it prints only the mode line): measure with `/usr/bin/time -l`.
use bough::prelude::*;
use bough_text_bench::i18n::*;
use bough_text_bench::{env, font_bytes};

fn main() {
    let (rows, ticks): (usize, u32) = (env("ROWS", 200), env("TICKS", 300));
    let mode = std::env::var("MODE").unwrap_or_else(|_| "message".into());
    let view = Shared { count: Signal::new(1), rows, template: mode == "template" };
    let mut app = App::new(view, Config::new(1200.0, 900.0, 2.0, font_bytes()), env("RENDER", 0) != 0);
    app.frame();
    for tick in 0..ticks {
        app.turn(|v| match mode.as_str() {
            "switch" => {
                LOCALE.with(|l| l.set(if tick % 2 == 0 { 1 } else { 0 }));
                let _ = &mut *v.count;
            }
            _ => {
                v.count.set(tick + 2);
            }
        });
        app.frame();
    }
    println!("{mode}: {rows} rows, {ticks} ticks");
}
