//! The host layer (round 8, e2e lane).
//!
//! * [`pace`] (portable): which buffer a frame is drawn into, what has to be repaired in it, what the compositor
//!   is told changed, and when a commit may go out. The rule: **an input commits at once, whether or not a frame
//!   callback is outstanding; the newest state wins; continuous animation is paced by frame callbacks.**
//! * [`event`] (portable): what a window reports.
//! * [`keymap`] (portable): the subset of an xkb text keymap a host needs to turn key codes into characters.
//! * [`wl`] (Linux): a Wayland toplevel on the wire.
//! * [`run`] (Linux): the loop.
pub mod event;
pub mod keymap;
pub mod pace;
#[cfg(target_os = "linux")]
pub mod run;
#[cfg(target_os = "linux")]
pub mod wl;

/// A set of 32-pixel tiles, one bit each, `stride` words per tile row; `lo..hi` are the rows that may hold a
/// bit. The geometry `bough-x` uses for damage, as borrowed plain data so this crate depends on nothing.
#[derive(Clone, Copy)]
pub struct TilesRef<'a> {
    pub bits: &'a [u64],
    pub stride: usize,
    pub lo: usize,
    pub hi: usize,
}

/// What the loop drives. Sizes are buffer pixels.
pub trait App {
    /// The window is `w` x `h` buffer pixels at `scale` buffer pixels per logical pixel. Called before the
    /// first frame and whenever either changes; everything is damaged afterwards.
    fn resize(&mut self, w: i32, h: i32, scale: i32);
    /// One input event (positions in buffer pixels).
    fn input(&mut self, e: &event::Event);
    /// The period of the application's own clock, if it has one.
    fn period(&self) -> Option<core::time::Duration> {
        None
    }
    /// The application's clock fired.
    fn clock(&mut self) {}
    /// Time is `ms`: a frame of continuous animation is about to be drawn.
    fn advance(&mut self, ms: u32);
    /// Whether anything is in continuous motion (a frame callback is worth asking for).
    fn animating(&self) -> bool;
    /// Bring the scene up to date with the state and return what is damaged since the last `painted`.
    fn settle(&mut self) -> TilesRef<'_>;
    /// Draw into `px` (`stride` pixels per row), everything or only the tiles of `repair`.
    ///
    /// # Safety
    /// `px` is valid for writes of the window's rows at `stride` pixels each, and nothing else touches it.
    unsafe fn paint(&mut self, px: *mut u32, stride: usize, repair: Option<TilesRef<'_>>);
    /// The damage returned by the last `settle` is now in a buffer: forget it.
    fn painted(&mut self);
}
