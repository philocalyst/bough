//! wgsl-rs module (src/kernel.rs) -> WGSL -> naga -> MSL text in OUT_DIR. No shader compiler runs at build time:
//! Metal compiles the text when the GPU executor is created (this machine class has no `xcrun metal`), and a
//! host without Metal still builds the crate and runs the CPU executors.
#[path = "src/kernel.rs"]
#[allow(dead_code, unused)]
mod kernel_src;

use naga::back::msl;
use std::collections::BTreeMap;

fn main() {
    println!("cargo:rerun-if-changed=src/kernel.rs");
    let out = std::env::var("OUT_DIR").unwrap();
    let src = &kernel_src::kernel::WGSL_SOURCE;
    let wgsl = src.wgsl_source().unwrap_or_else(|e| panic!("wgsl-rs: {e}"));
    std::fs::write(format!("{out}/kernel.wgsl"), &wgsl).unwrap();
    let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wgsl)));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("naga validation: {e:?}"));
    let mut m = BTreeMap::new();
    for b in 0..5u32 {
        m.insert(naga::ResourceBinding { group: 0, binding: b }, msl::BindTarget { buffer: Some(b as u8), mutable: b == 3, ..Default::default() });
    }
    let res = msl::EntryPointResources { resources: m, immediates_buffer: None, sizes_buffer: Some(5) };
    let options = msl::Options {
        lang_version: (3, 0),
        per_entry_point_map: [("fs_ktile".to_string(), res)].into_iter().collect(),
        fake_missing_bindings: false,
        // every index is produced by the binner or masked by the kernel's own window tests
        bounds_check_policies: naga::proc::BoundsCheckPolicies::default(),
        zero_initialize_workgroup_memory: false,
        force_loop_bounding: false,
        ..Default::default()
    };
    let (text, tr) = msl::write_string(&module, &info, &options, &msl::PipelineOptions::default()).unwrap_or_else(|e| panic!("naga msl: {e:?}"));
    for e in &tr.entry_point_names {
        if let Err(e) = e {
            panic!("naga msl entry point: {e:?}");
        }
    }
    std::fs::write(format!("{out}/kernel.metal"), text).unwrap();
}
