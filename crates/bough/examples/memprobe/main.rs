//! Metal memory probe (macOS only; the example exists everywhere so the workspace builds on every platform).
#[cfg(target_os = "macos")]
mod mac;

fn main() {
    #[cfg(target_os = "macos")]
    mac::main();
    #[cfg(not(target_os = "macos"))]
    eprintln!("memprobe: macOS only");
}
