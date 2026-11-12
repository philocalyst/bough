//! Attempt 3: shaping on the GPU. The CPU writes only the string bytes and a run table; a
//! compute kernel (one SIMD group per run) does the dense-table gather and the prefix sum,
//! and also the vertex shader's pixel snap. Exactness: every run's pens, snapped pixels and
//! trigger flag are read back and compared with the CPU kernels on the whole workload corpus
//! plus random strings. Then cost: CPU instructions per frame (bytes + encode + commit) and
//! GPU time per dispatch.
//!
//!   cargo run --release --example gpu_shape

#[cfg(target_os = "macos")]
#[path = "common/corpus.rs"]
mod corpus;
#[cfg(target_os = "macos")]
#[path = "common/meter.rs"]
mod meter;

// Metal only: on other hosts the example says so (bench lane, round 7: bough-text's examples build on Linux).
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("gpu_shape needs Metal (macOS)");
}

#[cfg(target_os = "macos")]
fn main() {
    metal::main()
}

#[cfg(target_os = "macos")]
mod metal {
    use super::{corpus, meter};
    use bough_text::shape::scalar_slots;
    use bough_text::{Shaper, SimpleScript};
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_foundation::{ns_string, NSString};
    use objc2_metal::*;
    use std::ptr::NonNull;

    const MSL: &str = r#"
    #include <metal_stdlib>
    using namespace metal;
    struct Run { uint off; uint len; };
    struct Snap { float scale; float oxh; };

    // One SIMD group (32 lanes) per run: lane i of chunk c handles byte 32c + i.
    kernel void shape(device const uchar* bytes [[buffer(0)]],
                      device const ushort* dense [[buffer(1)]],
                      device const Run* runs [[buffer(2)]],
                      device int* xout [[buffer(3)]],
                      device uint* trig [[buffer(4)]],
                      device int* pxout [[buffer(5)]],
                      constant Snap& snap [[buffer(6)]],
                      uint run [[threadgroup_position_in_grid]],
                      uint lane [[thread_index_in_simdgroup]]) {
        Run r = runs[run];
        int carry = 0;
        uint t = 0;
        for (uint base = 0; base < r.len; base += 32) {
            uint i = base + lane;
            uint st = 0;
            if (i < r.len) {
                uint a = bytes[r.off + i];
                uint b = (i + 1 < r.len) ? bytes[r.off + i + 1] : 0u;
                t |= ((a | b) & 0x80u) << 9;    // non-ASCII: refuse (bit 16)
                st = dense[((a & 127u) << 7) | (b & 127u)];
            }
            t |= st;
            int s = int(st);
            int ex = simd_prefix_exclusive_sum(s);
            if (i < r.len) {
                int x = carry + ex;
                xout[r.off + i] = x;
                pxout[r.off + i] = int(floor(fma(float(x), snap.scale, snap.oxh)));
            }
            carry += simd_sum(s);
        }
        t = simd_or(t);
        if (lane == 0) trig[run] = t;
    }
    "#;

    type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;

    struct Gpu {
        device: Retained<ProtocolObject<dyn MTLDevice>>,
        queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
        pso: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    }

    impl Gpu {
        fn new() -> Self {
            let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
            let queue = device.newCommandQueue().expect("queue");
            let lib = device.newLibraryWithSource_options_error(&NSString::from_str(MSL), None).expect("msl compile");
            let f = lib.newFunctionWithName(ns_string!("shape")).unwrap();
            let pso = device.newComputePipelineStateWithFunction_error(&f).expect("pso");
            Gpu { device, queue, pso }
        }
        fn buf(&self, bytes: usize) -> Buf {
            self.device.newBufferWithLength_options(bytes.max(16), MTLResourceOptions::StorageModeShared).unwrap()
        }
    }

    fn ptr<T>(b: &Buf) -> *mut T {
        b.contents().as_ptr() as *mut T
    }

    struct Batch {
        bytes: Buf,
        runs: Buf,
        x: Buf,
        trig: Buf,
        px: Buf,
        n_runs: usize,
    }

    /// CPU side of a frame: write each string's bytes and its run entry into the shared buffers.
    #[inline(never)]
    fn write_batch(b: &Batch, strings: &[&[u8]]) -> usize {
        let bp: *mut u8 = ptr(&b.bytes);
        let rp: *mut [u32; 2] = ptr(&b.runs);
        let mut off = 0usize;
        for (k, s) in strings.iter().enumerate() {
            // SAFETY: the buffers were sized for these strings.
            unsafe {
                std::ptr::copy_nonoverlapping(s.as_ptr(), bp.add(off), s.len());
                *rp.add(k) = [off as u32, s.len() as u32];
            }
            off += s.len();
        }
        off
    }

    /// CPU side of a frame: encode and commit one dispatch (no wait).
    #[inline(never)]
    fn encode(g: &Gpu, b: &Batch, dense: &Buf, snap: [f32; 2]) -> Retained<ProtocolObject<dyn MTLCommandBuffer>> {
        let cmd = g.queue.commandBuffer().unwrap();
        encode_into(g, &cmd, b, dense, snap);
        cmd.commit();
        cmd
    }

    /// The marginal part: one compute encoder in a command buffer the frame already has.
    #[inline(never)]
    fn encode_into(g: &Gpu, cmd: &ProtocolObject<dyn MTLCommandBuffer>, b: &Batch, dense: &Buf, snap: [f32; 2]) {
        let enc = cmd.computeCommandEncoder().unwrap();
        enc.setComputePipelineState(&g.pso);
        unsafe {
            enc.setBuffer_offset_atIndex(Some(&b.bytes), 0, 0);
            enc.setBuffer_offset_atIndex(Some(dense), 0, 1);
            enc.setBuffer_offset_atIndex(Some(&b.runs), 0, 2);
            enc.setBuffer_offset_atIndex(Some(&b.x), 0, 3);
            enc.setBuffer_offset_atIndex(Some(&b.trig), 0, 4);
            enc.setBuffer_offset_atIndex(Some(&b.px), 0, 5);
            enc.setBytes_length_atIndex(NonNull::new(snap.as_ptr() as *mut _).unwrap(), 8, 6);
            enc.dispatchThreadgroups_threadsPerThreadgroup(
                MTLSize { width: b.n_runs, height: 1, depth: 1 },
                MTLSize { width: 32, height: 1, depth: 1 },
            );
        }
        enc.endEncoding();
    }

    pub fn main() {
        let sh = Shaper::new(SimpleScript::build(corpus::font_bytes(), 0).unwrap());
        let g = Gpu::new();
        let dense = g.buf(128 * 128 * 2);
        // SAFETY: 32 KiB buffer, 16384 u16.
        unsafe { std::ptr::copy_nonoverlapping(sh.dense().as_ptr(), ptr::<u16>(&dense), 128 * 128) };

        // ---------------------------------------------------------------- exactness
        let mut corpus: Vec<String> = corpus::all_workload_strings();
        let mut st = 0x9E3779B97F4A7C15u64;
        for _ in 0..50_000 {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            let len = (st % 300) as usize;
            corpus.push((0..len).map(|k| (0x20 + ((st >> (k % 50)) as u8 ^ k as u8) % 95) as char).collect());
        }
        let strings: Vec<&[u8]> = corpus.iter().map(|s| s.as_bytes()).collect();
        let total: usize = strings.iter().map(|s| s.len()).sum();
        let b = Batch { bytes: g.buf(total), runs: g.buf(strings.len() * 8), x: g.buf(total * 4), trig: g.buf(strings.len() * 4), px: g.buf(total * 4), n_runs: strings.len() };
        write_batch(&b, &strings);
        let snaps = [[14.0 / 2048.0, 12.5], [11.0 / 2048.0, 3.37 + 0.5], [20.0 / 2048.0, 0.5], [16.0 / 2048.0, 1001.73]];
        let (mut checked_runs, mut checked_glyphs, mut refused) = (0usize, 0usize, 0usize);
        for snap in snaps {
            let cmd = encode(&g, &b, &dense, snap);
            cmd.waitUntilCompleted();
            let gpu_ms = (cmd.GPUEndTime() - cmd.GPUStartTime()) * 1e3;
            let (xg, pg, tg): (*const i32, *const i32, *const u32) = (ptr(&b.x), ptr(&b.px), ptr(&b.trig));
            let mut off = 0usize;
            let longest = strings.iter().map(|s| s.len()).max().unwrap_or(0) + 16;
            let mut cx = vec![0i32; longest];
            let mut ce = vec![0u16; longest];
            for (k, s) in strings.iter().enumerate() {
                let n = s.len();
                // SAFETY: GPU wrote n entries at off for run k.
                let (gx, gp, gt) = unsafe { (std::slice::from_raw_parts(xg.add(off), n), std::slice::from_raw_parts(pg.add(off), n), *tg.add(k)) };
                let ascii = s.is_ascii();
                let (_, acc) = if ascii { unsafe { scalar_slots(sh.dense(), s, 0, 0, cx.as_mut_ptr(), ce.as_mut_ptr()) } } else { (0, 0x8000) };
                let cpu_ok = ascii && acc & 0x8000 == 0;
                let gpu_ok = gt & 0x18000 == 0;
                assert_eq!(cpu_ok, gpu_ok, "eligibility of {:?}", String::from_utf8_lossy(s));
                if cpu_ok {
                    assert_eq!(gx, &cx[..n], "pens of {:?}", String::from_utf8_lossy(s));
                    for i in 0..n {
                        let want = (cx[i] as f32).mul_add(snap[0], snap[1]).floor() as i32;
                        assert_eq!(gp[i], want, "snap of glyph {i} of {:?}", String::from_utf8_lossy(s));
                    }
                    checked_runs += 1;
                    checked_glyphs += n;
                } else {
                    refused += 1;
                }
                off += n;
            }
            println!("snap {snap:?}: GPU pass over {} runs / {} bytes took {gpu_ms:.3} ms", strings.len(), total);
        }
        println!("exactness: {checked_runs} runs, {checked_glyphs} glyph pens and snapped pixels equal to the CPU (4 snaps); {refused} refused by both\n");

        // ---------------------------------------------------------------- cost of one heavy tick
        let ticks: Vec<Vec<(String, f32)>> = (0..200u64).map(corpus::heavy_numeric).collect();
        let tick_strings: Vec<Vec<&[u8]>> = ticks.iter().map(|t| t.iter().map(|(s, _)| s.as_bytes()).collect()).collect();
        let tb = Batch { bytes: g.buf(4096), runs: g.buf(84 * 8), x: g.buf(4096 * 4), trig: g.buf(84 * 4), px: g.buf(4096 * 4), n_runs: tick_strings[0].len() };
        let ov = meter::overhead();
        let (mut w_best, mut e_best, mut gpu_best, mut lat_best) = (u64::MAX, u64::MAX, f64::MAX, f64::MAX);
        let mut gpu_all = vec![];
        for t in &tick_strings {
            let a = meter::instructions();
            std::hint::black_box(write_batch(&tb, t));
            let m = meter::instructions();
            let t0 = std::time::Instant::now();
            let cmd = encode(&g, &tb, &dense, snaps[0]);
            let e = meter::instructions();
            cmd.waitUntilCompleted();
            let lat = t0.elapsed().as_secs_f64() * 1e3;
            w_best = w_best.min((m - a).saturating_sub(ov));
            e_best = e_best.min((e - m).saturating_sub(ov));
            let gms = (cmd.GPUEndTime() - cmd.GPUStartTime()) * 1e3;
            gpu_best = gpu_best.min(gms);
            lat_best = lat_best.min(lat);
            gpu_all.push(gms);
        }
        gpu_all.sort_by(|a, b| a.partial_cmp(b).unwrap());
        // Marginal: the encoder alone, in a command buffer that exists anyway.
        let mut m_best = u64::MAX;
        for t in &tick_strings[..50] {
            write_batch(&tb, t);
            let cmd = g.queue.commandBuffer().unwrap();
            let a = meter::instructions();
            encode_into(&g, &cmd, &tb, &dense, snaps[0]);
            let e = meter::instructions();
            cmd.commit();
            cmd.waitUntilCompleted();
            m_best = m_best.min((e - a).saturating_sub(ov));
        }
        println!("## One heavy tick (84 numeric labels) shaped on the GPU\n");
        println!("| | value |");
        println!("|---|---|");
        println!("| CPU: write string bytes + run table | {w_best} instr ({:.0} / label) |", w_best as f64 / 84.0);
        println!("| CPU: command buffer + compute encoder + commit | {e_best} instr |");
        println!("| CPU: compute encoder only (frame's command buffer exists) | {m_best} instr |");
        println!("| GPU: kernel time (GPUEnd - GPUStart), best / median | {:.1} / {:.1} us |", gpu_best * 1e3, gpu_all[gpu_all.len() / 2] * 1e3);
        println!("| commit -> completed (CPU wall, includes scheduling), best | {:.1} us |", lat_best * 1e3);
    }
}
