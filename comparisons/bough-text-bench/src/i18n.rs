//! zgx's two `i18n_bench`es as bough views: rows of plural messages (`{n, plural, one {# message} other {#
//! messages}}`, English plural rules and plain numbers, as zgx builds them without its `i18n` feature) against
//! the same rows as templates (`format!`). The locale is app state read by the message (zgx's `I18n` service);
//! a switch re-renders every message.
use bough::raw::*;
use std::cell::Cell;

pub const MAX_ROWS: usize = 1000;
pub const LINE: f32 = 17.0;

thread_local! {
    /// 0: en, 1: fr.
    pub static LOCALE: Cell<u8> = const { Cell::new(0) };
}

/// The two catalogs of zgx's Rust bench (`FR` of the script bench: `script_fr`).
pub fn message(n: u32) -> String {
    match (LOCALE.with(|l| l.get()), n == 1) {
        (_, true) => format!("{n} message"),
        (0, false) => format!("{n} messages"),
        (_, false) => format!("{n} messages reçus"),
    }
}
pub fn script_message(n: u32) -> String {
    match (LOCALE.with(|l| l.get()), n == 1) {
        (0, true) => format!("{n} message"),
        (0, false) => format!("{n} messages"),
        (_, true) => format!("{n} courriel"),
        (_, false) => format!("{n} courriels"),
    }
}
pub fn template_text(n: u32) -> String {
    if n == 1 { format!("{n} message") } else { format!("{n} messages") }
}

/// zgx's `i18n_bench` (crates/zgx/examples): one count per row.
pub struct Inbox {
    pub counts: [Signal<u32>; MAX_ROWS],
    pub rows: usize,
    pub template: bool,
}

impl View for Inbox {
    fn view(&self) -> impl Element<Self> + '_ {
        let tpl = self.template;
        each((0..self.rows).map(move |i| {
            let w = sig!(self.counts).at(i);
            let f: fn(&u32) -> String = if tpl { |n| template_text(*n) } else { |n| message(*n) };
            text(w.map(f)).at(0.0, i as f32 * LINE).size(14.0).line_height(LINE).color(rgb(0xe5edf7))
        }))
    }
}

/// zgx-script's `i18n_bench`: every row shows the one count.
pub struct Shared {
    pub count: Signal<u32>,
    pub rows: usize,
    pub template: bool,
}

impl View for Shared {
    fn view(&self) -> impl Element<Self> + '_ {
        let f: fn(&u32) -> String = if self.template { |n| format!("{n} messages") } else { |n| script_message(*n) };
        each((0..self.rows).map(move |i| text(sig!(self.count).map(f)).at(0.0, i as f32 * LINE).size(14.0).line_height(LINE).color(rgb(0xe5edf7))))
    }
}
