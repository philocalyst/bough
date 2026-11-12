//! The memory-ledger harness drives the AppKit host and reads macOS footprint counters (see `mac.rs`); the example
//! exists on every platform so the workspace builds there.
#[cfg(target_os = "macos")]
#[path = "../../../../crates/bough/examples/counter.rs"]
#[allow(dead_code)]
mod counter;
#[cfg(target_os = "macos")]
#[path = "../../src/dashboard.rs"]
#[allow(dead_code)]
mod dashboard;
#[cfg(target_os = "macos")]
#[path = "../../src/scroll.rs"]
#[allow(dead_code)]
mod scroll;
#[cfg(target_os = "macos")]
#[path = "../../src/scroll_config.rs"]
mod scroll_config;
#[cfg(target_os = "macos")]
mod mac;

fn main() {
    #[cfg(target_os = "macos")]
    mac::main();
    #[cfg(not(target_os = "macos"))]
    eprintln!("ledger: macOS only");
}
