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
    // the gate mutants (Cargo features) are preprocessor symbols of the shader
    let mut cc = vec!["-sdk", "macosx", "metal", "-O2"];
    for (feature, define) in [("CARGO_FEATURE_MUTANT_CERT_SKIP", "-DMUTANT_CERT_SKIP=1"), ("CARGO_FEATURE_MUTANT_CERT_STALE", "-DMUTANT_CERT_STALE=1"), ("CARGO_FEATURE_MUTANT_CERT_TILE", "-DMUTANT_CERT_TILE=1")] {
        if std::env::var_os(feature).is_some() {
            cc.push(define);
        }
    }
    cc.extend(["-c", "src/shader.metal", "-o", &air]);
    if ok(&cc) && ok(&["-sdk", "macosx", "metallib", &air, "-o", &lib]) {
        return;
    }
    println!("cargo:warning=no Metal compiler (install Xcode for a prebuilt metallib); shaders compile from source at startup");
    std::fs::write(&lib, []).unwrap();
    println!("cargo:rustc-cfg=bough_msl_runtime");
}
