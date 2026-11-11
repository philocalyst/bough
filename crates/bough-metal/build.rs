// Ahead-of-time Metal compile: shader.metal -> shader.air -> shader.metallib in OUT_DIR.
//
// Without Xcode's Metal compiler (Command Line Tools only), the library is built from source at runtime
// instead: shader.metallib is left empty and `gpu::load_library` compiles the embedded source. That source is
// `shader.min.metal` in OUT_DIR: shader.metal without comments, indentation or blank lines (80 KB -> 51 KB of the
// executable), the same program.
use std::process::Command;
fn main() {
    println!("cargo:rerun-if-changed=src/shader.metal");
    println!("cargo::rustc-check-cfg=cfg(bough_msl_runtime)");
    let out = std::env::var("OUT_DIR").unwrap();
    let src = std::fs::read_to_string("src/shader.metal").unwrap();
    std::fs::write(format!("{out}/shader.min.metal"), minify(&src)).unwrap();
    let air = format!("{out}/shader.air");
    let lib = format!("{out}/shader.metallib");
    let ok = |args: &[&str]| {
        Command::new("xcrun")
            .args(args)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    if ok(&[
        "-sdk",
        "macosx",
        "metal",
        "-O2",
        "-c",
        "src/shader.metal",
        "-o",
        &air,
    ]) && ok(&["-sdk", "macosx", "metallib", &air, "-o", &lib])
    {
        return;
    }
    println!(
        "cargo:warning=no Metal compiler (install Xcode for a prebuilt metallib); shaders compile from source at startup"
    );
    std::fs::write(&lib, []).unwrap();
    println!("cargo:rustc-cfg=bough_msl_runtime");
}

/// MSL without `//` comments, leading and trailing blanks, runs of blanks or empty lines. Line breaks stay (the
/// preprocessor reads them). Wrong for string or character literals and block comments, which shader.metal does
/// not have; the build stops if one appears.
fn minify(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let code = line.find("//").map_or(line, |i| &line[..i]);
        assert!(
            !code.contains(['"', '\'', '\\']) && !code.contains("/*"),
            "shader.metal: minify cannot keep this line: {line}"
        );
        let mut words = code.split_whitespace();
        if let Some(w) = words.next() {
            out.push_str(w);
            for w in words {
                out.push(' ');
                out.push_str(w);
            }
            out.push('\n');
        }
    }
    out
}
