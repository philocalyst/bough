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
    m.insert(rb(0, 1), msl::BindTarget { buffer: Some(1), mutable: true, ..Default::default() });
    m.insert(rb(0, 2), msl::BindTarget { buffer: Some(2), mutable: true, ..Default::default() });
    // textures (bindings 3..): never bound; the bindless transform reads them from the bind table
    for i in 0..4u32 {
        m.insert(rb(0, 3 + i), msl::BindTarget { texture: Some(i as u8), mutable: true, ..Default::default() });
        m.insert(rb(0, 7 + i), msl::BindTarget { texture: Some(4 + i as u8), ..Default::default() });
    }
    m.insert(rb(0, 11), msl::BindTarget { texture: Some(8), ..Default::default() });
    m.insert(rb(1, 0), msl::BindTarget { buffer: Some(3), ..Default::default() });
    m
}

/// The bind table every kernel reads at region offset 128 (`FBind` of INTERFACE.md, byte for byte): Metal 3
/// resource ids of the 4 surfaces (write), the atlas, the bins and counts addresses, the 4 surfaces (read).
const BIND_TABLE: &str = "struct BoughBind {
    metal::texture2d<float, metal::access::write> t[4];
    metal::texture2d<float, metal::access::sample> atlas;
    device metal::uint4* bins;
    device metal::atomic_uint* counts;
    metal::texture2d<float, metal::access::sample> r[4];
};
";

/// Mechanical transform of naga's MSL. Bindless (WGSL cannot name a texture held in a buffer): every texture
/// parameter of an entry point (`T0..T3`, `R0..R3`, `ATLAS`) is dropped from the signature and read from the bind
/// table in the region instead. So an encoder binds only the region and the slot: a pre-armed frame names no
/// surface (no per-frame `setTexture`, and IOSurfaceIsInUse reports only the compositor), as on the direct path.
fn bindless(msl: &str) -> String {
    let mut out = String::new();
    let mut fetch = String::new();
    let mut in_sig = false;
    for line in msl.lines() {
        if line.starts_with("kernel void ") {
            in_sig = true;
            fetch.clear();
        }
        if in_sig && line.starts_with(", metal::texture2d<") && line.contains("[[texture(") {
            let ty = &line[2..line.find('>').unwrap() + 1];
            let name = line[line.find('>').unwrap() + 1..].split_whitespace().next().unwrap();
            let field = match name.trim_end_matches('_') {
                n @ ("T0" | "T1" | "T2" | "T3") => format!("t[{}]", &n[1..]),
                n @ ("R0" | "R1" | "R2" | "R3") => format!("r[{}]", &n[1..]),
                "ATLAS" => "atlas".to_string(),
                n => panic!("texture parameter {n} has no bind-table slot"),
            };
            fetch.push_str(&format!("    {ty} {name} = __fb.{field};\n"));
            continue;
        }
        // workgroup memory as kernel-scope `threadgroup` variables (static size, so the encoder sets no
        // threadgroup-memory length: a passthrough metallib tells wgpu-hal nothing about it)
        if in_sig && line.starts_with(", threadgroup ") && line.ends_with('&') == false && line.contains('&') {
            let decl = line[2..].replace('&', "");
            fetch.push_str(&format!("    {decl};\n"));
            continue;
        }
        if in_sig && line == ") {" {
            in_sig = false;
            out.push_str(line);
            out.push('\n');
            if fetch.contains("__fb.") {
                out.push_str("    const device BoughBind& __fb = *reinterpret_cast<const device BoughBind*>(reinterpret_cast<const device char*>(&POOL) + 128);\n");
            }
            out.push_str(&fetch);
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if line.starts_with("#include <simd/simd.h>") {
            out.push_str(BIND_TABLE);
        }
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=src/shaders");
    let out = std::env::var("OUT_DIR").unwrap();
    let modules: &[(&wgsl_rs::Source, &[&str])] = &[(&shaders::copy::copy::WGSL_SOURCE, &["fs_copy"]), (&shaders::compact::compact::WGSL_SOURCE, &["fs_compact_test"]), (&shaders::raster::raster::WGSL_SOURCE, &["fs_bin", "fs_tile"])];
    let mut airs = Vec::new();
    let mut no_compiler = false;
    for (src, entries) in modules.iter() {
        let wgsl = src.wgsl_source().unwrap_or_else(|e| panic!("wgsl-rs module {}: {e}", src.name));
        std::fs::write(format!("{out}/{}.wgsl", src.name), &wgsl).unwrap();
        let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|e| panic!("{}: {}", src.name, e.emit_to_string(&wgsl)));
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}: {e:?}", src.name));
        let res = msl::EntryPointResources { resources: binding_map(), immediates_buffer: None, sizes_buffer: Some(4) };
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
        // one translation unit per module (naga's helpers and types are per module), linked into one metallib
        let metal = format!("{out}/{}.metal", src.name);
        // naga's helpers (`naga_div`, ...) have external linkage: give each module its own names
        let text = text.replace("naga_", &format!("naga_{}_", src.name));
        std::fs::write(&metal, format!("// module {} (wgsl-rs -> naga -> bindless transform)\n{}", src.name, bindless(&text))).unwrap();
        let air = format!("{out}/{}.air", src.name);
        match Command::new("xcrun").args(["-sdk", "macosx", "metal", "-O2", "-std=metal3.0", "-c", &metal, "-o", &air]).status() {
            Ok(s) if s.success() => airs.push(air),
            // No Metal compiler (Command Line Tools only): the per-module MSL stays in OUT_DIR and the metallib
            // is empty; the runtime MSL path for the hal presenter is still to be built (BRIEF §7.2).
            _ => no_compiler = true,
        }
    }
    let lib = format!("{out}/bough_gpu.metallib");
    if no_compiler {
        println!("cargo:warning=no Metal compiler (install Xcode): bough_gpu::METALLIB is empty");
        std::fs::write(&lib, []).unwrap();
        return;
    }
    let s = Command::new("xcrun").args(["-sdk", "macosx", "metallib"]).args(&airs).args(["-o", &lib]).status().expect("xcrun metallib");
    assert!(s.success(), "metallib failed");
}
