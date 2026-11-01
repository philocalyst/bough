// Ahead-of-time Metal compile: shader.metal -> shader.air -> shader.metallib in OUT_DIR.
use std::process::Command;
fn main() {
    println!("cargo:rerun-if-changed=src/shader.metal");
    let out = std::env::var("OUT_DIR").unwrap();
    let air = format!("{out}/shader.air");
    let lib = format!("{out}/shader.metallib");
    let s = Command::new("xcrun").args(["-sdk", "macosx", "metal", "-O2", "-c", "src/shader.metal", "-o", &air]).status().expect("xcrun metal");
    assert!(s.success(), "metal compile failed");
    let s = Command::new("xcrun").args(["-sdk", "macosx", "metallib", &air, "-o", &lib]).status().expect("xcrun metallib");
    assert!(s.success(), "metallib failed");
}
