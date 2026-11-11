//! Damage formulation lab; see RESULTS.md.
//!
//! Workloads (`workloads`) produce deterministic change traces (`trace`); each
//! damage formulation (`forms`) consumes the write-time events, maintains its
//! own state, and per presented frame produces a repair region plus a
//! paint-ordered draw list (`forms::Out`). `verify` replays every trace into a
//! software hash framebuffer ring and checks that repairing only the computed
//! region reproduces the full redraw exactly; `bench` measures instructions and
//! time of track + cull with the tree-update baseline subtracted.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod asm;
pub mod bench;
pub mod forms;
pub mod geom;
pub mod measure;
pub mod render;
pub mod scene;
pub mod trace;
pub mod verify;
pub mod workloads;
