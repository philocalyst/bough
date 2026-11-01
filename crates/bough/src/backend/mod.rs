//! Platform backends: the Metal presenter (IOSurface ring, completion-handler publish, stencil
//! damage, one instanced draw) and the AppKit window host. Everything platform-specific lives here
//! so it can be re-pointed at the reworked bough-metal later.
#[cfg(target_os = "macos")]
pub mod metal;
#[cfg(target_os = "macos")]
pub mod host;
