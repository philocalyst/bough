//! Which f32 operations does the GPU compute exactly as IEEE, under the shader library's default compile options
//! (fast math)? (lane memory, round 6: the CPU repair backend must reproduce `f_sd`'s `length` bit for bit.)
//!
//!   mathprobe [n = 4000000]
//!
//! A kernel compiled from source exactly as bough-metal compiles `shader.metal` (`newLibraryWithSource`, options
//! `nil`) evaluates, for n random (x, y) drawn from the ranges `f_sd` sees (|q| < 4096, and near the rounding
//! boundaries of small values): `length(float2(x, y))`, `sqrt(x)` and `x * x + y * y`. Each result is compared with
//! CPU candidates and the number of mismatches per candidate printed.
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::*;

const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
kernel void k_math(device const float2* in [[buffer(0)]], device float4* out [[buffer(1)]], uint i [[thread_position_in_grid]]) {
    float2 q = in[i];
    float2 m = max(q, 0.0);
    out[i] = float4(length(m), sqrt(q.x), m.x * m.x + m.y * m.y, rsqrt(q.x));
    out[i + 0x4000000u] = float4(precise::sqrt(q.x), precise::sqrt(fma(m.y, m.y, m.x * m.x)), 0.0, 0.0);
}
"#;

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(4_000_000);
    let device = MTLCreateSystemDefaultDevice().unwrap();
    let queue = device.newCommandQueue().unwrap();
    let lib = device.newLibraryWithSource_options_error(&NSString::from_str(MSL), None).expect("msl");
    let pso = device.newComputePipelineStateWithFunction_error(&lib.newFunctionWithName(&NSString::from_str("k_math")).unwrap()).unwrap();
    // inputs: half uniform in [0, 4096) at 1/256 steps and arbitrary floats, half small values near 0..64
    let mut s = 0x1234_5678_9abc_def0u64;
    let mut rnd = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let inp: Vec<[f32; 2]> = (0..n)
        .map(|k| {
            let a = rnd();
            let f = |b: u64| match k % 3 {
                0 => (b % (4096 * 256)) as f32 / 256.0,
                1 => (b % (64 << 16)) as f32 / 65536.0,
                _ => f32::from_bits(0x3f80_0000 + (b % 0x0800_0000) as u32) - 1.0 + (b >> 40 & 63) as f32,
            };
            [f(a & 0xffff_ffff), f(a >> 32)]
        })
        .collect();
    let bin = device.newBufferWithLength_options(n * 8, MTLResourceOptions::StorageModeShared).unwrap();
    assert!(n <= 0x4000000);
    let bout = device.newBufferWithLength_options((0x4000000 + n) * 16, MTLResourceOptions::StorageModeShared).unwrap();
    // SAFETY: n * 8 bytes, the buffer's length.
    unsafe { std::ptr::copy_nonoverlapping(inp.as_ptr() as *const u8, bin.contents().as_ptr() as *mut u8, n * 8) };
    let c = queue.commandBuffer().unwrap();
    let e = c.computeCommandEncoder().unwrap();
    e.setComputePipelineState(&pso);
    unsafe {
        e.setBuffer_offset_atIndex(Some(&bin), 0, 0);
        e.setBuffer_offset_atIndex(Some(&bout), 0, 1);
    }
    e.dispatchThreads_threadsPerThreadgroup(MTLSize { width: n, height: 1, depth: 1 }, MTLSize { width: 256, height: 1, depth: 1 });
    e.endEncoding();
    c.commit();
    c.waitUntilCompleted();
    // SAFETY: n float4 results.
    let out = unsafe { std::slice::from_raw_parts(bout.contents().as_ptr() as *const [f32; 4], n) };
    // SAFETY: the second block of n float4 results.
    let out2 = unsafe { std::slice::from_raw_parts((bout.contents().as_ptr() as *const [f32; 4]).add(0x4000000), n) };
    let mut more = [0usize; 4];
    for (&[x, y], (o, p)) in inp.iter().zip(out.iter().zip(out2)) {
        let (mx, my) = (x.max(0.0), y.max(0.0));
        if x > 0.0 {
            // the GPU's sqrt as x * its rsqrt
            if (x * o[3]).to_bits() != o[1].to_bits() {
                more[0] += 1;
            }
            // its rsqrt correctly rounded?
            if ((1.0f64 / (x as f64).sqrt()) as f32).to_bits() != o[3].to_bits() {
                more[1] += 1;
            }
        }
        if x >= 0.0 && x.sqrt().to_bits() != p[0].to_bits() {
            more[2] += 1;
        }
        if my.mul_add(my, mx * mx).sqrt().to_bits() != p[1].to_bits() {
            more[3] += 1;
        }
    }
    println!(
        "sqrt == x * rsqrt (GPU's own): mismatches {}; rsqrt correctly rounded: mismatches {}; precise::sqrt == IEEE sqrt: mismatches {}; precise::sqrt(fma(y,y,x*x)) == CPU: mismatches {}",
        more[0], more[1], more[2], more[3]
    );
    let mut bad = [0usize; 8];
    let mut first: [Option<(f32, f32, f32, f32)>; 8] = [None; 8];
    let _keep: Retained<ProtocolObject<dyn MTLBuffer>> = bin;
    for (k, (&[x, y], o)) in inp.iter().zip(out).enumerate() {
        let _ = k;
        let (mx, my) = (x.max(0.0), y.max(0.0));
        let cands = [
            // length: dot with two roundings, then sqrt
            (mx * mx + my * my).sqrt(),
            // dot contracted: fma(x, x, y * y)
            mx.mul_add(mx, my * my).sqrt(),
            // dot contracted the other way: fma(y, y, x * x)
            my.mul_add(my, mx * mx).sqrt(),
            // the dot exact (f64), sqrt rounded once
            ((mx as f64) * (mx as f64) + (my as f64) * (my as f64)).sqrt() as f32,
        ];
        for (j, c) in cands.iter().enumerate() {
            if c.to_bits() != o[0].to_bits() {
                bad[j] += 1;
                first[j].get_or_insert((mx, my, o[0], *c));
            }
        }
        // sqrt(x) alone, correctly rounded; the dot alone: two roundings, fma either way
        if x.sqrt().to_bits() != o[1].to_bits() && x >= 0.0 {
            bad[4] += 1;
            first[4].get_or_insert((x, 0.0, o[1], x.sqrt()));
        }
        let dots = [mx * mx + my * my, mx.mul_add(mx, my * my), my.mul_add(my, mx * mx)];
        for (j, d) in dots.iter().enumerate() {
            if d.to_bits() != o[2].to_bits() {
                bad[5 + j] += 1;
                first[5 + j].get_or_insert((mx, my, o[2], *d));
            }
        }
    }
    let names = [
        "length: dot 2 roundings + sqrt",
        "length: fma(x,x,y*y) + sqrt",
        "length: fma(y,y,x*x) + sqrt",
        "length: exact dot + sqrt",
        "sqrt(x)",
        "dot: 2 roundings",
        "dot: fma(x,x,y*y)",
        "dot: fma(y,y,x*x)",
    ];
    for j in 0..8 {
        println!("{:34} mismatches {:9} of {n}  first {:?}", names[j], bad[j], first[j]);
    }
}
