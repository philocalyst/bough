//! The standing workloads (round 7, bench lane; `docs/lanes/bench/SCENES.md`): five application scenes the user
//! asked for, each driven through the same phases, so every lane measures its change against the same work.
//!
//! * [`sheet`]: a spreadsheet of 10 x 10,000 = 100,000 cells; hover on a column header highlights its 10,000
//!   cells, a theme switch recolours all of them.
//! * [`ide`]: file tree, tabs, an editor with a minimap, a terminal.
//! * [`dash`]: a dashboard of 200 live charts.
//! * [`canvas`]: a design canvas of 5,000 layered nodes with shadows, clips and blurred shadows, animated layers.
//! * [`chat`]: a chat with images.
//!
//! A scene is a [`View`] plus a [`Scene`] impl: how to build its model at a size, and what one step of each phase
//! does. [`harness`] runs the phases and prints one TSV row per phase. The crate itself has no platform code
//! outside `sys.rs` (process memory) and the GPU calls the harness makes only with `RENDER=1` on macOS, so it
//! builds wherever `bough` does.
//!
//! Scenes are written as an application author would write them with today's API (`bough::prelude` for flow
//! layout, `bough::raw` where the prelude has no spelling: shadows, clips, absolutely placed layers). What a
//! scene wanted and could not express is listed at the top of its module: that list is a result.
pub mod harness;
pub mod sys;

#[cfg(feature = "canvas")]
pub mod canvas;
#[cfg(feature = "chat")]
pub mod chat;
#[cfg(feature = "dash")]
pub mod dash;
#[cfg(feature = "ide")]
pub mod ide;
#[cfg(feature = "sheet")]
pub mod sheet;

use bough::raw::{App, Caps};

pub use harness::{run_all, Phase};

/// What the harness needs from a scene.
pub trait Scene: bough::app::View + Sized + 'static {
    const NAME: &'static str;
    /// The window the scene opens at (points).
    const SIZE: (f32, f32) = (1280.0, 800.0);

    /// Build the model. `scale` in (0, 1] shrinks the scene's counts (rows, nodes, charts, messages) for a
    /// quick run; 1.0 is the size the brief specifies. Deterministic: the same scale gives the same model.
    fn build(scale: f32) -> Self;

    /// Region capacities for this model (instances, pages, glyphs, runs, spaces, clips, strikes).
    fn caps(&self) -> Caps;

    /// A one-line description of the built model's size (counts that matter: cells, nodes, ...).
    fn describe(&self) -> String;

    /// How many elements depend on the hover target (the fan-out the `hover` phase exercises).
    fn fanout(&self) -> usize;

    /// One step of steady interaction (k = 0, 1, 2, ...): what a user or the app does in one turn (typing,
    /// editing a value, a data tick, dragging a node, a message arriving). Leaves the frame to the harness.
    fn steady(app: &mut App<Self>, k: usize);

    /// One step of the fan-out hover: even k moves the pointer onto the element with thousands of
    /// dependents, odd k moves it off. Pointer input only (`app.pointer_move`), as a user would.
    fn hover(app: &mut App<Self>, k: usize);

    /// One theme switch: even k to the dark theme, odd k back.
    fn theme(app: &mut App<Self>, k: usize);

    /// Start this scene's transitions (cross-fades, animated layers) for step k; the harness then advances the
    /// frame clock (`App::tick`). Called by the `anim` phase and, every few steps, inside the `resize_anim` sweep.
    fn animate(app: &mut App<Self>, k: usize);
}

/// The scene names in their default order (`workload all`); a scene whose feature is off reports as unknown.
pub const SCENES: [&str; 5] = ["sheet", "ide", "dash", "canvas", "chat"];

/// Run the named scene's phases (`harness::run` with the scene's type).
pub fn run_scene(name: &str, cfg: &harness::RunCfg) -> bool {
    match name {
        #[cfg(feature = "sheet")]
        "sheet" => harness::run::<sheet::Sheet>(cfg),
        #[cfg(feature = "ide")]
        "ide" => harness::run::<ide::Ide>(cfg),
        #[cfg(feature = "dash")]
        "dash" => harness::run::<dash::Dash>(cfg),
        #[cfg(feature = "canvas")]
        "canvas" => harness::run::<canvas::Canvas>(cfg),
        #[cfg(feature = "chat")]
        "chat" => harness::run::<chat::Chat>(cfg),
        _ => return false,
    }
    true
}

/// The font every scene draws with (zgx's embedded face, so a run reads the same bytes on every host).
pub fn font() -> Vec<u8> {
    zgx_workload::FONT_BYTES.to_vec()
}

/// A small deterministic generator (xorshift64*), so scenes need no dependency and every host builds the same
/// model.
#[derive(Clone)]
pub struct Rng(pub u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    #[inline]
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in `0..n` (n > 0).
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    /// Uniform in `[0, 1)`.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// `n * scale`, at least `min`.
pub fn scaled(n: usize, scale: f32, min: usize) -> usize {
    ((n as f32 * scale).round() as usize).max(min)
}
