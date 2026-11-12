//! The leaves (`src/ops.rs` over the round-7 kernel's arithmetic) -> WGSL (wgsl-rs) -> MSL (naga), as text in
//! OUT_DIR. This is the only time naga runs: the per-archetype functions are rendered at run time as plain
//! calls of these leaves, valid in both languages, and appended (`src/emit.rs`).
#[path = "src/ops.rs"]
#[allow(dead_code, unused)]
mod ops_src;

fn main() {
    println!("cargo:rerun-if-changed=src/ops.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::env::var("OUT_DIR").unwrap();
    let wgsl = ops_src::ops::WGSL_SOURCE.wgsl_source().unwrap_or_else(|e| panic!("wgsl-rs: {e}"));
    std::fs::write(format!("{out}/leaves.wgsl"), &wgsl).unwrap();
    let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wgsl)));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap_or_else(|e| panic!("naga validation: {e:?}"));
    let options = naga::back::msl::Options { lang_version: (3, 0), fake_missing_bindings: false, zero_initialize_workgroup_memory: false, force_loop_bounding: false, ..Default::default() };
    let (text, _) = naga::back::msl::write_string(&module, &info, &options, &naga::back::msl::PipelineOptions::default()).unwrap_or_else(|e| panic!("naga msl: {e:?}"));
    std::fs::write(format!("{out}/leaves.metal"), text).unwrap();
}
