//! First increment of the wgpu-hal port (ROUND4 §8), measured against direct Metal:
//!  1. startup: device + queue + region + surfaces + pipelines, wgpu-hal from raw objects vs `bough_metal::fused`;
//!  2. the wgsl-rs `fs_copy` kernel through the hal is pixel-exact (vs a CPU model; a marked tile is skipped);
//!  3. per-frame process instructions: a hal command buffer per frame, and the hal pre-armed stream (16 frames per
//!     command buffer, released by a shared event), vs the same two shapes on direct Metal (`examples/fprobe`).
//! Usage: halprobe [frames=3000]
use bough_gpu::hal::HalGpu;
use bough_metal::fused::{Layout, SlotCopy, write_slot};
use bough_metal::perf::snap;

fn main() {
    let frames: u64 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(3000);
    let (w, h) = (1440usize, 1600usize);
    let sb = Layout::slot_bytes(w as u32, h as u32) as u32;
    let slots = [256, 256 + sb, 256 + 2 * sb];
    let lay = Layout { w: w as u32, h: h as u32, slot: slots, ..Default::default() };
    let region_len = (256 + 3 * sb as usize).next_multiple_of(16384);

    // 1. startup (cold: `halprobe startup-hal` / `halprobe startup-direct` create one presenter in a fresh process)
    match std::env::args().nth(1).as_deref() {
        Some("startup-hal") => {
            let t = std::time::Instant::now();
            let g = HalGpu::new(w, h, region_len, 2, &slots);
            println!("cold startup wgpu-hal: {:.0} us {:?}", t.elapsed().as_secs_f64() * 1e6, g.startup);
            return;
        }
        Some("startup-direct") => {
            let t = std::time::Instant::now();
            let _f = bough_metal::fused::Fused::new(w, h, 2.0, region_len, 2, 1024);
            println!("cold startup direct Metal (Fused::new): {:.0} us", t.elapsed().as_secs_f64() * 1e6);
            return;
        }
        _ => {}
    }
    let t = std::time::Instant::now();
    let mut g = HalGpu::new(w, h, region_len, 2, &slots);
    let hal_us = t.elapsed().as_secs_f64() * 1e6;
    let t = std::time::Instant::now();
    let f = bough_metal::fused::Fused::new(w, h, 2.0, region_len, 2, 1024);
    let direct_us = t.elapsed().as_secs_f64() * 1e6;
    drop(f);
    println!("startup: wgpu-hal {hal_us:.0} us {:?} | direct Metal (Fused::new, second device in the process) {direct_us:.0} us", g.startup);

    // 2. correctness: surface 1 holds a pattern; copy it 24 px up into surface 0 except row 3's tile 5
    let pat = |x: usize, y: usize| [(x * 7) as u8, (y * 13) as u8, (x ^ y) as u8, 255];
    g.fill_surface(1, pat);
    g.fill_surface(0, |_, _| [1, 2, 3, 4]);
    let copy = SlotCopy { src: 1, rect: [0, 0, w as i32, h as i32 - 24], dx: 0, dy: -24 };
    let mut rows = vec![0u128; h.div_ceil(32)];
    rows[3] = 1 << 5;
    // SAFETY: the region is laid out by `lay` (header + 3 slots) and the GPU is idle.
    unsafe { write_slot(g.region_ptr, &lay, 0, 0, &rows, false, &copy) };
    g.frame_submit(0, slots[0], true);
    let px = g.surface_pixels(0);
    let mut bad = 0usize;
    for y in 0..h {
        for x in 0..w {
            let got = &px[(y * w + x) * 4..][..4];
            let marked = y / 32 == 3 && x / 32 == 5;
            let want = if marked || y >= h - 24 { [1, 2, 3, 4] } else { pat(x, y + 24) };
            if got != want {
                bad += 1;
            }
        }
    }
    println!("fs_copy (wgsl-rs -> naga -> metallib, through wgpu-hal): {bad} wrong pixels of {}", w * h);
    assert_eq!(bad, 0);

    // 2b. the subgroup compaction (wgsl-rs subgroup ops -> naga -> simd_prefix_exclusive_sum / simd_sum)
    let words = [0x8000_0001u32, 0, 0xFFFF_FFFF, 0x0F0F_0F0F, 0, 0x10, 0xAAAA_AAAA, 1];
    let got = g.compact_test(slots[2] + 64, words);
    let want: Vec<(u32, u32)> = (0..256u32).filter(|&i| (words[(i >> 5) as usize] >> (i & 31)) & 1 != 0).map(|i| (i, words.iter().map(|w| w.count_ones()).sum())).collect();
    println!("subgroup compaction (wgsl-rs): {} hits, in order and complete: {}", got.len(), got == want);
    assert_eq!(got, want);

    // 3. per-frame instructions: a one-tile copy (the floor, not the copy's GPU cost)
    let small = SlotCopy { src: 1, rect: [0, 0, 32, 32], dx: 0, dy: -1 };
    for k in 0..2 {
        // SAFETY: as above.
        unsafe { write_slot(g.region_ptr, &lay, k, 0, &[], false, &small) };
    }
    for _ in 0..50 {
        g.frame_submit(0, slots[0], true);
    }
    let a = snap();
    for f in 0..frames {
        g.frame_submit((f % 2) as usize, slots[(f % 2) as usize], true);
    }
    let b = snap();
    println!("hal: command buffer per frame, wait each: {:.0} instr/frame", (b.instrs - a.instrs) as f64 / frames as f64);
    g.arm(16, &slots);
    let mut armed = 16u64;
    for _ in 0..32 {
        if g.released + 8 > armed {
            g.arm(16, &slots);
            armed += 16;
        }
        g.release();
    }
    g.wait_idle();
    let a = snap();
    for _ in 0..frames {
        if g.released + 8 > armed {
            g.arm(16, &slots);
            armed += 16;
        }
        g.release();
    }
    g.wait_idle();
    let b = snap();
    println!("hal: pre-armed stream (16 per command buffer, event-released, wait each): {:.0} instr/frame", (b.instrs - a.instrs) as f64 / frames as f64);
}
