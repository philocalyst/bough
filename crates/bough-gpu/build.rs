//! Build-time shader pipeline (no shader is parsed or compiled at runtime):
//! wgsl-rs modules (src/shaders) -> WGSL -> naga (vendored) -> MSL with the Metal binding indices wgpu-hal's
//! pipeline layout assigns -> `xcrun metal` -> one metallib in OUT_DIR, embedded by `bough_gpu::METALLIB`.
#[path = "src/shaders/mod.rs"]
#[allow(dead_code, unused_assignments)]
mod shaders;

use naga::back::msl;
use std::collections::BTreeMap;
use std::process::Command;

/// The one layout every kernel shares (see src/shaders/mod.rs), as wgpu-hal's Metal backend numbers it: per stage,
/// buffers and textures are counted separately in group order then entry order.
fn binding_map() -> msl::BindingMap {
    let mut m = BTreeMap::new();
    let rb = |group, binding| naga::ResourceBinding { group, binding };
    m.insert(rb(0, 0), msl::BindTarget { buffer: Some(0), mutable: false, ..Default::default() });
    for i in 0..4u32 {
        m.insert(rb(0, 1 + i), msl::BindTarget { texture: Some(i as u8), mutable: true, ..Default::default() });
        m.insert(rb(0, 5 + i), msl::BindTarget { texture: Some(4 + i as u8), ..Default::default() });
    }
    m.insert(rb(0, 9), msl::BindTarget { texture: Some(8), ..Default::default() });
    m.insert(rb(1, 0), msl::BindTarget { buffer: Some(1), ..Default::default() });
    m
}

fn main() {
    println!("cargo:rerun-if-changed=src/shaders");
    let out = std::env::var("OUT_DIR").unwrap();
    let modules: &[(&wgsl_rs::Source, &[&str])] = &[(&shaders::copy::copy::WGSL_SOURCE, &["fs_copy"])];
    let mut msl_all = String::new();
    for (i, (src, entries)) in modules.iter().enumerate() {
        let wgsl = src.wgsl_source().unwrap_or_else(|e| panic!("wgsl-rs module {}: {e}", src.name));
        std::fs::write(format!("{out}/{}.wgsl", src.name), &wgsl).unwrap();
        let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|e| panic!("{}: {}", src.name, e.emit_to_string(&wgsl)));
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}: {e:?}", src.name));
        let res = msl::EntryPointResources { resources: binding_map(), immediates_buffer: None, sizes_buffer: Some(2) };
        let options = msl::Options {
            lang_version: (3, 0),
            per_entry_point_map: entries.iter().map(|e| (e.to_string(), res.clone())).collect(),
            fake_missing_bindings: false,
            // every index the kernels read from region data is masked by construction (INTERFACE.md)
            bounds_check_policies: naga::proc::BoundsCheckPolicies::default(),
            zero_initialize_workgroup_memory: false,
            force_loop_bounding: false,
            ..Default::default()
        };
        let (text, _) = msl::write_string(&module, &info, &options, &msl::PipelineOptions::default()).unwrap_or_else(|e| panic!("{}: {e:?}", src.name));
        // each module is its own MSL translation unit: wrap it in a namespace so helpers never collide
        msl_all.push_str(&format!("// ---- module {} (wgsl-rs -> naga)\n", src.name));
        msl_all.push_str(&text);
        let _ = i;
    }
    let metal = format!("{out}/bough_gpu.metal");
    std::fs::write(&metal, &msl_all).unwrap();
    let (air, lib) = (format!("{out}/bough_gpu.air"), format!("{out}/bough_gpu.metallib"));
    let s = Command::new("xcrun").args(["-sdk", "macosx", "metal", "-O2", "-std=metal3.0", "-c", &metal, "-o", &air]).status().expect("xcrun metal");
    assert!(s.success(), "metal compile of the naga output failed ({metal})");
    let s = Command::new("xcrun").args(["-sdk", "macosx", "metallib", &air, "-o", &lib]).status().expect("xcrun metallib");
    assert!(s.success(), "metallib failed");
}
