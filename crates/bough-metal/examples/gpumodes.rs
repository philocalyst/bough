//! Head-to-head of the four repair encodings on identical damage (headless, IOSurface-backed target):
//! bbox (1 scissored draw), stencil (mask + 1 draw), vclip (vertex-clipped stream from the plane), compute (tile
//! kernel). Modes are interleaved per iteration so GPU contention from other processes hits all equally.
//! CPU = main-thread instructions for region build + cull + encode + commit (thread_selfcounts, no wait).
//!   gpumodes [iters=300]
use bough_metal::damage::{History, Plane, Region, TX, TY};
use bough_metal::gpu::*;
use bough_metal::perf::*;
use bough_metal::present::{make_iosurface, Engine};
use bough_metal::scene::Rng;
use objc2_metal::*;

fn main() {
    let iters: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(300);
    let mut e = Engine::new(ShaderSource::Metallib);
    let io = make_iosurface();
    let d = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::BGRA8Unorm, 1920, 1440, false) };
    d.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
    d.setStorageMode(MTLStorageMode::Shared);
    let tex = e.gpu.device.newTextureWithDescriptor_iosurface_plane(&d, &io, 0).unwrap();
    let ovh = tinstr_overhead();
    let mut rng = Rng(0xFEED);
    println!("tinstr overhead {ovh}; {iters} iterations per (pattern, mode); damage = union of 3 epochs (ring-of-3 age)\n");
    println!("| pattern | repair tiles (16 px) | mode | CPU instr encode+commit (med) | GPU ms med | GPU ms p10 | GPU ms p90 | draws/dispatch | entries |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for pat in ["one-colour", "scatter-80", "scroll-400", "full"] {
        // build an age-3 repair plane for this pattern
        let mut h = History::new();
        for _ in 0..3 {
            match pat {
                "one-colour" => e.scene.op_one_colour(&mut rng),
                "scatter-80" => e.scene.op_scattered(&mut rng, 80),
                "scroll-400" => e.scene.op_scroll(7.0),
                _ => e.scene.invalidate_all(),
            }
            h.push(&mut e.scene.acc);
        }
        let mut plane = Plane::new();
        h.repair(Some(h.epoch - 3), &mut plane);
        let ntiles: u32 = if plane.full { (TX * TY) as u32 } else { (plane.r0..=plane.r1).map(|y| plane.rows[y as usize][0].count_ones() + plane.rows[y as usize][1].count_ones()).sum() };
        let modes = ["bbox", "stencil", "vclip", "compute"];
        let mut cpu: Vec<Vec<f64>> = vec![vec![]; 4];
        let mut gpu: Vec<Vec<f64>> = vec![vec![]; 4];
        let mut entries = [0usize; 4];
        let mut tiles: Vec<u32> = Vec::with_capacity(TX * TY);
        for it in 0..iters + 20 {
            for (m, &mode) in modes.iter().enumerate() {
                let t0 = tinstr();
                let cmd = e.gpu.queue.commandBuffer().unwrap();
                let mut region = Region::default();
                match mode {
                    "bbox" | "stencil" => {
                        if plane.full { region.set_full() } else { region.from_plane(&plane) }
                        e.gpu.encode_repair(&cmd, &tex, &e.scene, &region, if mode == "bbox" { DamageMode::Bbox } else { DamageMode::Stencil });
                        entries[m] = region.rects.len();
                    }
                    "vclip" => {
                        if plane.full {
                            region.set_full();
                            e.gpu.encode_repair(&cmd, &tex, &e.scene, &region, DamageMode::Bbox);
                        } else {
                            e.gpu.encode_vclip(&cmd, &tex, &e.scene, &plane);
                            entries[m] = e.gpu.vclip_entries.get();
                        }
                    }
                    _ => {
                        tiles.clear();
                        if !plane.full {
                            for y in plane.r0 as usize..=plane.r1 as usize {
                                for w in 0..2 {
                                    let mut b = plane.rows[y][w];
                                    while b != 0 {
                                        let x = w * 64 + b.trailing_zeros() as usize;
                                        b &= b - 1;
                                        if x < TX { tiles.push((y as u32) << 16 | x as u32) }
                                    }
                                }
                            }
                        }
                        let off = e.gpu.write_slot_ex(it % 2, 0, &tiles, plane.full);
                        let nt = if plane.full { TX * TY } else { tiles.len() };
                        e.gpu.encode_compute(&cmd, off, [&tex, &tex, &tex, &tex], nt, false);
                        entries[m] = nt;
                    }
                }
                cmd.commit();
                let c = (tinstr() - t0).saturating_sub(ovh) as f64;
                cmd.waitUntilCompleted();
                if it >= 20 {
                    cpu[m].push(c);
                    let g = (cmd.GPUEndTime() - cmd.GPUStartTime()) * 1e3;
                    if g > 0.0 && g < 500.0 {
                        gpu[m].push(g);
                    }
                }
            }
        }
        for (m, mode) in modes.iter().enumerate() {
            println!(
                "| {pat} | {ntiles} | {mode} | {:.0} | {:.3} | {:.3} | {:.3} | {} | {} |",
                pct(&mut cpu[m], 0.5), pct(&mut gpu[m], 0.5), pct(&mut gpu[m], 0.1), pct(&mut gpu[m], 0.9),
                if *mode == "stencil" && entries[m] > 1 { 2 } else { 1 }, entries[m]
            );
        }
    }
}
