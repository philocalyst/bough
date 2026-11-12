//! bough-metal: direct-Metal presenter prototype (see RESULTS.md). macOS only: on every other target the crate
//! is empty (the frame contract it implements is bough-frame, the counters bough-sys).
#![cfg(target_os = "macos")]
pub mod damage;
pub mod fused;
pub mod gpu;
pub mod instance;
pub mod perf;
pub mod present;
pub mod scene;
pub mod threads;
pub mod app;
pub mod v2;
