//! The drag probe drives AppKit directly (synthetic NSEvents through the window): macOS only. On other platforms
//! the binary exists (the workspace builds everywhere) and says so.
#[cfg(target_os = "macos")]
#[path = "../drag_probe_mac.rs"]
mod imp;

#[cfg(target_os = "macos")]
fn main() {
    imp::main()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("drag_probe: macOS only");
}
