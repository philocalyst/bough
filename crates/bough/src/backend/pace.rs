//! The interactive host's input queue (docs/lanes/pacing/cards/2026-11-11-display-paced-input.md).
//!
//! The event monitor pushes; the host drains once AppKit has delivered everything already queued (a run-loop
//! observer, before the main thread waits) and again at a frame's latch. Draining runs one input turn per
//! queued item, in order. Only adjacent items whose sequential application is equivalent to one merged item
//! coalesce:
//!
//! * pointer moves: the last position (hover and drag routes see the final position; intermediate hover
//!   crossings and drag steps are not replayed, as browsers coalesce `pointermove`);
//! * wheel deltas of the same kind (line or precise), at the same point and of the same sign: their sum
//!   (same-sign clamps compose: `clamp(clamp(a + d1) + d2) == clamp(a + d1 + d2)`, and a line glide aimed
//!   twice before a tick aims at the summed target from the same start).
//!
//! Downs, ups, keys and leaves never coalesce: their count and order are the input's.
//!
//! Platform-free (the storm test drives it headless); the host converts AppKit events into [`In`].
use crate::app::{App, View};
use crate::input::{Button, Key, Mods};
use bough_metal::framelog as fl;
use std::collections::VecDeque;

/// One input, in window points (top-left origin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum In {
    Move {
        x: f32,
        y: f32,
    },
    Down {
        x: f32,
        y: f32,
        button: Button,
    },
    Up {
        x: f32,
        y: f32,
        button: Button,
    },
    Leave,
    /// `dy` already in the app's direction (positive scrolls the content up); `precise`: device points
    /// (trackpad), otherwise lines (a glide).
    Wheel {
        x: f32,
        y: f32,
        dy: f32,
        precise: bool,
    },
    Key {
        key: Key,
        mods: Mods,
    },
}

impl In {
    /// The frame log's turn kind.
    pub fn kind(&self) -> u64 {
        match self {
            In::Move { .. } => fl::K_MOVE,
            In::Down { .. } => fl::K_DOWN,
            In::Up { .. } => fl::K_UP,
            In::Leave => fl::K_LEAVE,
            In::Wheel { .. } => fl::K_WHEEL,
            In::Key { .. } => fl::K_KEY,
        }
    }
}

/// A queued input: the item, the oldest event time it stands for (host ns; 0 when unknown) and how many
/// events it absorbed.
#[derive(Clone, Copy, Debug)]
pub struct Queued {
    pub input: In,
    pub t: u64,
    pub n: u32,
}

#[derive(Default)]
pub struct InputQueue {
    q: VecDeque<Queued>,
    /// Events absorbed by coalescing so far.
    pub coalesced: u64,
    /// Items applied so far.
    pub applied: u64,
    #[cfg(feature = "sabotage_coalesce")]
    skip_up: bool,
}

impl InputQueue {
    pub fn new() -> InputQueue {
        InputQueue::default()
    }

    /// Queue `input` (event time `t`, host ns), merging it into the last item when that is exact.
    pub fn push(&mut self, input: In, t: u64) {
        if let Some(last) = self.q.back_mut() {
            match (&mut last.input, input) {
                (In::Move { x, y }, In::Move { x: nx, y: ny }) => {
                    (*x, *y) = (nx, ny);
                    last.n += 1;
                    self.coalesced += 1;
                    return;
                }
                (
                    In::Wheel { x, y, dy, precise },
                    In::Wheel {
                        x: nx,
                        y: ny,
                        dy: ndy,
                        precise: np,
                    },
                ) if *x == nx
                    && *y == ny
                    && *precise == np
                    && *dy != 0.0
                    && ndy != 0.0
                    && (*dy > 0.0) == (ndy > 0.0) =>
                {
                    *dy += ndy;
                    last.n += 1;
                    self.coalesced += 1;
                    return;
                }
                _ => {}
            }
        }
        // the negative control (gate G2): a "coalescer" that also merges a click into an adjacent identical
        // click, which loses clicks; the storm test must fail with it
        #[cfg(feature = "sabotage_coalesce")]
        {
            let n = self.q.len();
            if let In::Down { x, y, button } = input {
                if n >= 2
                    && matches!((self.q[n - 2].input, self.q[n - 1].input), (In::Down { x: a, y: b, button: c }, In::Up { x: d, y: e, button: f }) if (a, b, c) == (x, y, button) && (d, e, f) == (x, y, button))
                {
                    self.skip_up = true;
                    return;
                }
            }
            if self.skip_up && matches!(input, In::Up { .. }) {
                self.skip_up = false;
                return;
            }
        }
        self.q.push_back(Queued { input, t, n: 1 });
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn pop(&mut self) -> Option<Queued> {
        self.q.pop_front()
    }
}

/// Run one input's turn.
pub fn apply<V: View>(a: &mut App<V>, input: In) {
    match input {
        In::Move { x, y } => a.pointer_move(x, y),
        In::Down { x, y, button } => a.pointer_down(x, y, button),
        In::Up { x, y, button } => a.pointer_up(x, y, button),
        In::Leave => a.pointer_leave(),
        In::Wheel {
            x,
            y,
            dy,
            precise: true,
        } => a.wheel_points(x, y, dy),
        In::Wheel {
            x,
            y,
            dy,
            precise: false,
        } => a.wheel(x, y, 0.0, dy),
        In::Key { key, mods } => a.key_down(key, mods),
    }
}

/// Drain the queue into `a`: one turn per item, in order (a frame-log row per turn). The queue is not
/// borrowed while a turn runs (an event delivered meanwhile is queued behind). Returns the turns run.
pub fn drain<V: View>(q: &std::cell::RefCell<InputQueue>, a: &mut App<V>) -> usize {
    let mut n = 0;
    loop {
        let Some(it) = q.borrow_mut().pop() else {
            break;
        };
        fl::turn_begin(it.input.kind(), it.t);
        apply(a, it.input);
        if fl::ON {
            fl::or(fl::M_FLAGS, ((a.has_damage() || a.animating()) as u64) << 3);
        }
        fl::turn_end();
        n += 1;
    }
    q.borrow_mut().applied += n as u64;
    n
}
