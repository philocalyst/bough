//! The AppKit window host over bough-metal's fused presenter (`bough_metal::fused`).
#[cfg(target_os = "macos")]
pub mod host;
/// In-process synthetic input for the frame log's probes (feature `framelog`).
#[cfg(all(target_os = "macos", feature = "framelog"))]
pub mod drive;
