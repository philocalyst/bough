//! Round 6 (surface-api): what a state-flag flip costs (`flags.rs`). One mechanism, ten situations: every flip
//! below is a flag write read by a variant table, through `Rt::flags_write`. Instructions on the calling thread
//! for the turn (handler or input event, settle, effects, re-description if any) and for the frame after it
//! (headless: up to the tile list), median of 64 flips and their undo, `tinstr` overhead removed.
//!
//!     cargo run --release -p bough --example flags_cost
use bough::input::Button;
use bough::prelude::*;
use bough::raw::{App, Caps, Config};
use bough_metal::perf::{tinstr, tinstr_overhead};
use std::cell::Cell;

const ROW: f32 = 16.0;
thread_local! {
    static NAME: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

struct Line {
    id: u32,
}
impl View for Line {
    fn view(&self) -> impl Element<Self> + '_ {
        row((block(12.0, 12.0).bg(rgb(0xbbbbbb)).when(Flag::CHECKED, |s| s.bg(rgb(0x22aa55))), text(self.id).font(10.0, 12.0).color(rgb(0x202020)).when(Flag::SELECTED, |s| s.color(rgb(0xffffff)))))
            .w(400.0)
            .h(ROW)
            .bg(rgb(0xf5f5f5))
            .when(Flag::SEARCH_HIT, |s| s.bg(rgb(0xffd400)))
            .when(Flag::HOVERED, |s| s.bg(rgb(0xdddddd)))
            .when(Flag::SELECTED, |s| s.bg(rgb(0x2f6bff)))
    }
}

struct Page {
    lines: List<Line>,
    y: Signal<f32>,
    locked: bool,
    frozen: Signal<bool>,
    sent: u32,
    hand: Signal<f32>,
    views: Cell<u32>,
}
impl View for Page {
    fn view(&self) -> impl Element<Self> + '_ {
        self.views.set(self.views.get() + 1);
        row((
            column((
                column((
                    block(100.0, 20.0).bg(rgb(0x3366cc)).border(2.0, rgb(0x3366cc)).focusable().focus(|s| s.border(rgb(0x7ee3ff))).disabled(|s| s.bg(rgb(0x999999))),
                    block(100.0, 20.0).bg(rgb(0x3366cc)).border(2.0, rgb(0x3366cc)).focusable().focus(|s| s.border(rgb(0x7ee3ff))).disabled(|s| s.bg(rgb(0x999999))),
                    block(100.0, 20.0).bg(rgb(0x226622)).on_click(|p: &mut Page| p.sent += 1).active(|s| s.bg(rgb(0x113355))).disabled(|s| s.bg(rgb(0x999999))),
                ))
                .gap(4.0)
                .p(6.0)
                .bg(rgb(0xffffff))
                .when(Flag::FOCUS_WITHIN, |s| s.bg(rgb(0xeef3ff)))
                .when(Flag::user(0), |s| s.bg(rgb(0xccddee)))
                .flag(Flag::DISABLED, self.locked)
                .flag_by(Flag::user(0), sig!(self.frozen)),
                row((
                    block(30.0, 30.0).bg(rgb(0xaa3333)).on_drag(|p: &mut Page, e| *p.hand = e.dx),
                    block(30.0, 30.0).margin(80.0, 0.0, 0.0, 0.0).bg(rgb(0xcccccc)).when(Flag::DRAG_OVER, |s| s.bg(rgb(0x55aa55))),
                )),
            ))
            .w(240.0),
            div().w(420.0).h(780.0).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(list(sig!(self.lines)).w(400.0)),
        ))
        .bg(rgb(0xf0f0f0))
    }
}

fn app(n: usize) -> App<Page> {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(700.0, 800.0, 1.0, font);
    cfg.caps = Caps { instances: 1 << 16, pages: 1 << 12, glyphs: 1 << 18, runs: 1 << 16, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    let page =
        Page { lines: List::from_iter_in((0..n as u32).map(|id| Line { id })), y: Signal::new(0.0), locked: false, frozen: Signal::new(false), sent: 0, hand: Signal::new(0.0), views: Cell::new(0) };
    let mut a = App::new(page, cfg, false);
    a.frame();
    a
}

/// Median (turn, frame) instructions of `flip(a, k)` for k in 0..2*reps (a flip and its undo alternate).
fn cell(a: &mut App<Page>, flip: &mut dyn FnMut(&mut App<Page>, usize)) -> (u64, u64) {
    if std::env::var("CELL").is_ok_and(|c| c != "all" && !NAME.with(|n| n.borrow().contains(&c))) {
        return (0, 0);
    }
    let o = tinstr_overhead();
    for k in 0..4 {
        flip(a, k);
        a.frame();
    }
    let (mut ts, mut fs) = (Vec::new(), Vec::new());
    // profiling: SPIN=n repeats every cell n times
    let reps = std::env::var("SPIN").ok().and_then(|v| v.parse().ok()).unwrap_or(128usize);
    for k in 0..reps {
        let t0 = tinstr();
        flip(a, k);
        let t1 = tinstr();
        a.frame();
        let t2 = tinstr();
        ts.push((t1 - t0).saturating_sub(o));
        fs.push((t2 - t1).saturating_sub(o));
    }
    ts.sort();
    fs.sort();
    (ts[ts.len() / 2], fs[fs.len() / 2])
}

fn main() {
    let n = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let mut a = app(n);
    let row_y = |k: usize| k as f32 * ROW + ROW / 2.0;
    let x = 400.0;
    let v0 = a.owner.views.get();
    println!("flip,turn,frame   ({n} rows; one code path: Rt::flags_write)");
    let mut out = |name: &str, c: (u64, u64)| println!("{name},{},{}", c.0, c.1);
    // (the next cell's name, for CELL=part-of-a-name: profile one cell)
    macro_rules! named {
        ($n:expr, $c:expr) => {{
            NAME.with(|x| *x.borrow_mut() = String::from($n));
            out($n, $c)
        }};
    }
    named!("row hover crossing", cell(&mut a, &mut |a, k| a.pointer_move(x, row_y(3 + k % 2))));
    a.pointer_leave();
    a.frame();
    named!("pressed (down, then up)", cell(&mut a, &mut |a, k| if k % 2 == 0 { a.pointer_down(56.0, 64.0, Button::Primary) } else { a.pointer_up(56.0, 64.0, Button::Primary) }));
    a.pointer_move(600.0, 790.0);
    named!("keyboard focus ring + focus-within (Tab)", cell(&mut a, &mut |a, _| a.key_down(Key::Tab, Mods::default())));
    named!("disabled subtree (a plain field: the form re-describes)", cell(&mut a, &mut |a, k| a.turn(|p| p.locked = k % 2 == 0)));
    named!("a subtree flag from a signal (flag_by: no description)", cell(&mut a, &mut |a, k| a.turn(|p| drop(p.frozen.set(k % 2 == 0)))));
    named!(
        "single selection (the previous row off, one on)",
        cell(&mut a, &mut |a, k| a.turn(|p| {
            p.lines.set_flag_at((k + 6) % 7, Flag::SELECTED, false);
            p.lines.set_flag_at(k % 7, Flag::SELECTED, true);
        }))
    );
    named!(
        &format!("clear a flag on all {n} rows"),
        cell(&mut a, &mut |a, k| a.turn(|p| {
            if k % 2 == 0 {
                p.lines.clear_flag(Flag::SELECTED);
            } else {
                p.lines.set_flag_at(3, Flag::SELECTED, true);
            }
        }))
    );
    named!(
        &format!("shift-range selection over {n} rows"),
        cell(&mut a, &mut |a, k| a.turn(|p| {
            let n = p.lines.len();
            p.lines.set_flag_range(0..n, Flag::SELECTED, k % 2 == 0);
        }))
    );
    named!("checked toggle", cell(&mut a, &mut |a, k| a.turn(|p| p.lines.set_flag_at(4, Flag::CHECKED, k % 2 == 0))));
    named!(
        &format!("find in page: {} hits", n / 10),
        cell(&mut a, &mut |a, k| a.turn(|p| {
            let n = p.lines.len();
            for i in (3..n).step_by(10) {
                p.lines.set_flag_at(i, Flag::SEARCH_HIT, k % 2 == 0);
            }
        }))
    );
    a.pointer_move(15.0, 95.0);
    a.pointer_down(15.0, 95.0, Button::Primary);
    named!("drag over a target (with the drag route)", cell(&mut a, &mut |a, k| a.pointer_move(if k % 2 == 0 { 125.0 } else { 60.0 }, 95.0)));
    a.pointer_up(60.0, 95.0, Button::Primary);
    println!("descriptions of the page during all flips: {} (the disabled toggles only)", a.owner.views.get() - v0);
}
