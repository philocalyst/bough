//! Process-level counters: retired instructions, CPU time, memory. Moved to bough-sys (round 7, platform lane: one
//! API with a macOS and a Linux implementation); re-exported here under the old path.
pub use bough_sys::perf::*;
