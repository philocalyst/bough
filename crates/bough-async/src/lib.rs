#![feature(async_iterator)]
//! bough's async core, with no platform and no GUI in it: what a question, an action and a flight are, how the
//! questions one turn asks are published together, the clock, and work for other cores. The façade (`bough`)
//! binds it to cells, wires and descriptions; the model checker (`tests/model.rs`) binds it to plain integers
//! and checks the publication rules over random and exhaustive schedules.
pub mod combin;
pub mod compute;
pub mod engine;
pub mod time;

pub use combin::{race, yield_now};
pub use compute::{compute, compute_stream, Cancel, Report};
pub use engine::{AId, Engine, Landing, Out, QId, QOpts, Question, Step, Work};
pub use time::{now, sleep, timeout};
