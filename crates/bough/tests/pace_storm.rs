//! Gate G2 of the pacing lane's X4 card (docs/lanes/pacing/cards/2026-11-11-display-paced-input.md): the
//! interactive host's input queue loses and reorders nothing. A seeded random storm of pointer moves, wheel
//! deltas (runs of one sign, then mixed), Refresh-style clicks and typing is pushed through
//! `backend::pace::InputQueue` in random batches and drained into a headless App, exactly as the host's event
//! monitor and run-loop observer do; the final state must equal both an oracle and the same storm dispatched
//! event by event. Negative control: `--features sabotage_coalesce` (adjacent identical clicks merge) fails.
use bough::backend::pace::{In, InputQueue, apply, drain};
use bough::input::{Button, Key, Mods};
use bough::prelude::*;
use bough::raw::{App, Config};
use std::cell::RefCell;

struct Storm {
    clicks: Signal<u32>,
    text: Signal<String>,
    y: Signal<f32>,
}

impl Storm {
    fn new() -> Storm {
        Storm {
            clicks: Signal::new(0),
            text: Signal::new(String::new()),
            y: Signal::new(Y0),
        }
    }
    fn click(&mut self) {
        *self.clicks += 1;
    }
}

const ROWS: usize = 200;
const ROW_H: f32 = 30.0;
const PORT_H: f32 = 300.0;
const Y0: f32 = 2800.0;
// window points: the button, the text field (clicked at its right end: the caret goes to the text's end) and
// the middle of the scroll port (column: 40 + 40 + 300)
const BUTTON: (f32, f32) = (80.0, 20.0);
const FIELD: (f32, f32) = (350.0, 60.0);
const PORT: (f32, f32) = (240.0, 230.0);

impl View for Storm {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            button("Click").w(160.0).h(40.0).on_click(Storm::click),
            text_input(sig!(self.text)).size(360.0, 40.0),
            div().w_pct(1.0).h(PORT_H).scroll(sig!(self.y)).child(each(
                (0..ROWS).map(|i| div().h(ROW_H).child(text(format!("row {i}")))),
            )),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}

fn app() -> App<Storm> {
    let font = std::fs::read(
        std::env::var("ZGX_FONT_FILE")
            .unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into()),
    )
    .unwrap();
    let mut a = App::new(Storm::new(), Config::new(480.0, 400.0, 1.0, font), false);
    a.frame();
    a
}

fn state(a: &mut App<Storm>) -> (u32, String, f32) {
    let mut s = (0, String::new(), 0.0);
    a.turn(|v| s = (*v.clicks, (*v.text).clone(), *v.y));
    s
}

/// xorshift64*: the storm is a function of the seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The storm and its oracle: (events, clicks, text, scroll offset).
fn storm(seed: u64, units: usize) -> (Vec<In>, u32, String, f32) {
    let mut r = Rng(seed);
    let (mut ev, mut clicks, mut text, mut y) = (Vec::new(), 0u32, String::new(), Y0);
    let max = ROWS as f32 * ROW_H - PORT_H;
    let mut focused = false;
    let click = |ev: &mut Vec<In>, (x, y): (f32, f32)| {
        ev.push(In::Move { x, y });
        ev.push(In::Down {
            x,
            y,
            button: Button::Primary,
        });
        ev.push(In::Up {
            x,
            y,
            button: Button::Primary,
        });
    };
    for _ in 0..units {
        match r.below(5) {
            0 => {
                // a run of clicks on the button (it takes the focus from the field)
                for _ in 0..1 + r.below(6) {
                    click(&mut ev, BUTTON);
                    clicks += 1;
                }
                focused = false;
            }
            1 => {
                // focus the field, then type
                if !focused {
                    click(&mut ev, FIELD);
                    focused = true;
                }
                for _ in 0..1 + r.below(8) {
                    if text.len() > 16 || (!text.is_empty() && r.below(3) == 0) {
                        ev.push(In::Key {
                            key: Key::Backspace,
                            mods: Mods::default(),
                        });
                        text.pop();
                    } else {
                        let c = (b'a' + r.below(26) as u8) as char;
                        ev.push(In::Key {
                            key: Key::Char(c),
                            mods: Mods::default(),
                        });
                        text.push(c);
                    }
                }
            }
            2 => {
                // pointer moves (hover crossings over the button included)
                for _ in 0..1 + r.below(12) {
                    ev.push(In::Move {
                        x: r.below(480) as f32,
                        y: r.below(400) as f32,
                    });
                }
            }
            3 => {
                // precise wheel deltas over the port: a run of one sign (they coalesce), sometimes then the other
                let sign = if r.below(2) == 0 { 1.0 } else { -1.0 };
                for k in 0..1 + r.below(10) {
                    let s = if k > 3 && r.below(4) == 0 {
                        -sign
                    } else {
                        sign
                    };
                    let dy = s * (1 + r.below(24)) as f32;
                    ev.push(In::Wheel {
                        x: PORT.0,
                        y: PORT.1,
                        dy,
                        precise: true,
                    });
                    y = (y + dy).clamp(0.0, max);
                }
            }
            _ => {
                // the pointer leaves the window
                ev.push(In::Leave);
            }
        }
    }
    (ev, clicks, text, y)
}

#[test]
fn a_queued_storm_loses_and_reorders_nothing() {
    for seed in [0x9e37_79b9_7f4a_7c15u64, 7, 2026] {
        let (ev, clicks, text, y) = storm(seed, 400);
        // the same storm, event by event (the host before the queue)
        let mut direct = app();
        for &i in &ev {
            apply(&mut direct, i);
        }
        // queued in random batches, drained as the host's run-loop observer does, frames in between
        let mut queued = app();
        let q = RefCell::new(InputQueue::new());
        let mut r = Rng(seed ^ 0x5555);
        let mut k = 0;
        while k < ev.len() {
            let n = (1 + r.below(40) as usize).min(ev.len() - k);
            for &i in &ev[k..k + n] {
                q.borrow_mut().push(i, 0);
            }
            k += n;
            drain(&q, &mut queued);
            queued.frame();
        }
        let (coalesced, applied) = (q.borrow().coalesced, q.borrow().applied);
        assert!(
            coalesced > 0,
            "seed {seed:#x}: the storm exercised coalescing"
        );
        assert_eq!(
            applied + coalesced,
            ev.len() as u64,
            "seed {seed:#x}: every event applied or merged"
        );
        let d = state(&mut direct);
        let s = state(&mut queued);
        assert_eq!(
            d,
            (clicks, text.clone(), y),
            "seed {seed:#x}: direct dispatch equals the oracle"
        );
        assert_eq!(
            s.0, clicks,
            "seed {seed:#x}: every click delivered (queued)"
        );
        assert_eq!(
            s.1, text,
            "seed {seed:#x}: every key delivered in order (queued)"
        );
        assert_eq!(
            s.2, y,
            "seed {seed:#x}: the wheel deltas sum exactly (queued)"
        );
    }
}

#[test]
fn only_moves_and_same_sign_wheels_merge() {
    let mut q = InputQueue::new();
    let w = |dy: f32| In::Wheel {
        x: 1.0,
        y: 2.0,
        dy,
        precise: true,
    };
    let d = In::Down {
        x: 5.0,
        y: 5.0,
        button: Button::Primary,
    };
    let u = In::Up {
        x: 5.0,
        y: 5.0,
        button: Button::Primary,
    };
    let k = In::Key {
        key: Key::Char('a'),
        mods: Mods::default(),
    };
    for i in [
        In::Move { x: 1.0, y: 1.0 },
        In::Move { x: 2.0, y: 2.0 },
        w(3.0),
        w(4.0),
        w(-1.0),
        d,
        u,
        d,
        u,
        k,
        k,
        In::Leave,
        In::Leave,
    ] {
        q.push(i, 0);
    }
    let got: Vec<In> = std::iter::from_fn(|| q.pop().map(|x| x.input)).collect();
    assert_eq!(
        got,
        vec![
            In::Move { x: 2.0, y: 2.0 },
            w(7.0),
            w(-1.0),
            d,
            u,
            d,
            u,
            k,
            k,
            In::Leave,
            In::Leave
        ]
    );
}
