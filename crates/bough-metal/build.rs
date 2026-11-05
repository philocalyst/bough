// Ahead-of-time Metal compile: shader.metal -> shader.air -> shader.metallib in OUT_DIR.
//
// Without Xcode's Metal compiler (Command Line Tools only), the library is built from source at runtime
// instead: shader.metallib is left empty and `gpu::load_library` compiles `shader.metal`.
use std::process::Command;
fn main() {
    println!("cargo:rerun-if-changed=src/shader.metal");
    println!("cargo::rustc-check-cfg=cfg(bough_msl_runtime)");
    let out = std::env::var("OUT_DIR").unwrap();
    let air = format!("{out}/shader.air");
    let lib = format!("{out}/shader.metallib");
    let ok = |args: &[&str]| Command::new("xcrun").args(args).status().map(|s| s.success()).unwrap_or(false);
    if ok(&["-sdk", "macosx", "metal", "-O2", "-c", "src/shader.metal", "-o", &air]) && ok(&["-sdk", "macosx", "metallib", &air, "-o", &lib]) {
        return;
    }
    println!("cargo:warning=no Metal compiler (install Xcode for a prebuilt metallib); shaders compile from source at startup");
    std::fs::write(&lib, []).unwrap();
    println!("cargo:rustc-cfg=bough_msl_runtime");
}
