//! The exactness gate: three executors, one kernel source, equal bytes.
//! (a) `cpu::naive`: the kernel's definition, pixel by pixel, every command, no binning;
//! (b) `cpu::raster`: tile programs with the cut, cores, spans, flat rows, on one and on several cores;
//! (c) `metal::Gpu`: naga's MSL of the same source (macOS; skipped where there is no Metal device), or
//!     `vulkan::Gpu`: naga's SPIR-V of the same source (Linux; skipped where there is no Vulkan adapter).
//! `BOUGH_EXACT_SEEDS` sets the number of scenes (default 24; 125 scenes are 10,000 tiles).
use bough_kernel::cpu::{self, Atlas, Binner, Damage, Opt, Pool, Prog};
use bough_kernel::{k, scenes, stage};

const FW: u32 = 320;
const FH: u32 = 250; // not a multiple of 32: the last band is partial
const STRIDE: usize = 328;

fn seeds() -> u64 {
    std::env::var("BOUGH_EXACT_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(24)
}

fn first_diff(a: &[u32], b: &[u32]) -> Option<(usize, usize, u32, u32)> {
    (0..FH as usize).flat_map(|y| (0..FW as usize).map(move |x| (x, y))).find_map(|(x, y)| {
        let i = y * STRIDE + x;
        (a[i] != b[i]).then_some((x, y, a[i], b[i]))
    })
}

#[test]
fn executors_agree() {
    let atlas_px = scenes::atlas();
    let img_px = scenes::images();
    let atlas = Atlas::new(&atlas_px, scenes::ATLAS_W).with_images(&img_px, scenes::IMG_W);
    let pool = Pool::new(3);
    let words = STRIDE * FH as usize;
    #[cfg(target_os = "macos")]
    let mut gpu = bough_kernel::metal::Gpu::new(bough_kernel::KERNEL_MSL);
    #[cfg(target_os = "linux")]
    let mut gpu = bough_kernel::vulkan::Gpu::new(bough_kernel::KERNEL_SPIRV);
    #[cfg(target_os = "linux")]
    if let Some(g) = gpu.as_ref() {
        eprintln!("exact: the GPU executor is Vulkan on {} ({:?})", g.info.name, g.info.device_type);
    }
    let (mut bin, mut prog) = (Binner::default(), Prog::default());
    let (mut tiles, mut gpu_tiles) = (0u64, 0u64);
    let mut h = 0u64;
    for seed in 0..seeds() {
        let scene = scenes::adversarial(seed, FW, FH, 110);
        let now = [0u32, 37, 90, 400][(seed % 4) as usize];
        let mut spec = vec![0u32; words];
        cpu::naive(&scene, FW, FH, &atlas, now, &mut spec, STRIDE);
        h ^= bough_kernel::hash(&spec).rotate_left(seed as u32);
        // every tile, with and without each executor shortcut, on 1 and 4 threads
        for (cut, flat, extra) in [(true, true, 0), (true, true, 3), (false, false, 0), (true, false, 2), (false, true, 1)] {
            bin.bin(&scene, FW, FH, STRIDE as u32, &atlas, now, &Damage::All, cut, &mut prog);
            let mut out = vec![0u32; words];
            let st = cpu::raster(&scene, &prog, &atlas, &mut out, STRIDE, &pool, extra, Opt::flat(flat));
            tiles += st.tiles;
            if let Some((x, y, a, b)) = first_diff(&out, &spec) {
                panic!("seed {seed} now {now} cut {cut} flat {flat} threads {}: pixel ({x},{y}) fast {a:08x} spec {b:08x}", extra + 1);
            }
        }
        // the per-tile specification (the oracle large scenes use)
        let mut out = vec![0u32; words];
        cpu::reference(&scene, FW, FH, &atlas, now, &mut out, STRIDE, &pool);
        assert!(first_diff(&out, &spec).is_none(), "seed {seed}: reference != naive");
        // a damaged subset: its tiles equal the specification, every other word is untouched
        let mut r = scenes::Rng(seed + 99);
        let sub: Vec<(u16, u16)> = (0..8u16).flat_map(|y| (0..10u16).map(move |x| (x, y))).filter(|_| r.u(3) == 0).collect();
        bin.bin(&scene, FW, FH, STRIDE as u32, &atlas, now, &Damage::Tiles(&sub), true, &mut prog);
        let mut out = vec![0x5a5a_5a5au32; words];
        cpu::raster(&scene, &prog, &atlas, &mut out, STRIDE, &pool, 2, Opt::default());
        let check_subset = |out: &[u32], who: &str| {
            for y in 0..FH as usize {
                for x in 0..STRIDE {
                    let i = y * STRIDE + x;
                    let want = if x < FW as usize && sub.contains(&((x / 32) as u16, (y / 32) as u16)) { spec[i] } else { 0x5a5a_5a5a };
                    assert_eq!(out[i], want, "seed {seed} {who} subset: word ({x},{y})");
                }
            }
        };
        check_subset(&out, "cpu");
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if let Some(g) = gpu.as_mut() {
            g.upload(&scene, &atlas, &prog);
            let sentinel = vec![0x5a5a_5a5au32; words];
            g.set_out(&sentinel);
            g.render(prog.tiles(), words);
            check_subset(g.out(words), "gpu");
            for cut in [true, false] {
                bin.bin(&scene, FW, FH, STRIDE as u32, &atlas, now, &Damage::All, cut, &mut prog);
                g.upload_prog(&prog);
                g.clear_out(words);
                g.render(prog.tiles(), words);
                gpu_tiles += prog.tiles() as u64;
                if let Some((x, y, a, b)) = first_diff(g.out(words), &spec) {
                    panic!("seed {seed} now {now} cut {cut}: pixel ({x},{y}) gpu {a:08x} spec {b:08x}");
                }
            }
        }
    }
    eprintln!("exact: {} scenes, {tiles} cpu tiles, {gpu_tiles} gpu tiles, 0 differing words; spec hash {h:016x}", seeds());
}

/// A GPU whose kernel rounds one product differently must be caught (the gate's negative control on the
/// GPU side; the CPU side's are the `mutant_*` features).
#[cfg(target_os = "macos")]
#[test]
fn gpu_rounding_mutant_is_caught() {
    let msl = bough_kernel::KERNEL_MSL;
    let needle = "16384u";
    assert!(msl.contains(needle), "the mutation site moved: update the needle");
    let Some(mut gpu) = bough_kernel::metal::Gpu::new(&msl.replacen(needle, "16383u", 1)) else { return };
    let atlas_px = scenes::atlas();
    let img_px = scenes::images();
    let atlas = Atlas::new(&atlas_px, scenes::ATLAS_W).with_images(&img_px, scenes::IMG_W);
    let words = STRIDE * FH as usize;
    let (mut bin, mut prog) = (Binner::default(), Prog::default());
    let mut caught = 0;
    for seed in 0..4 {
        let scene = scenes::adversarial(seed, FW, FH, 110);
        let mut spec = vec![0u32; words];
        cpu::naive(&scene, FW, FH, &atlas, 0, &mut spec, STRIDE);
        bin.bin(&scene, FW, FH, STRIDE as u32, &atlas, 0, &Damage::All, true, &mut prog);
        gpu.upload(&scene, &atlas, &prog);
        gpu.clear_out(words);
        gpu.render(prog.tiles(), words);
        caught += first_diff(gpu.out(words), &spec).is_some() as u32;
    }
    // the mutant differs on products whose low 15 bits are exactly 16384 and whose effect survives the 8-bit
    // store: rare per pixel, so a single scene may miss it; the gate as a whole must not
    eprintln!("gpu rounding mutant: caught in {caught} of 4 scenes");
    assert!(caught >= 1, "a GPU kernel with a different rounding constant passed the gate");
}

#[test]
fn arithmetic_identities() {
    for t in 0..256 {
        assert_eq!(k::ch8(k::q15(t)), t);
        assert!(t == 0 || k::q15(t) > k::q15(t - 1));
    }
    assert_eq!(k::q15(255), k::ONE);
    for a in (0..=k::ONE).step_by(7).chain([k::ONE]) {
        assert_eq!(k::mul15(a, k::ONE), a);
        assert_eq!(k::mul15(a, 0), 0);
        for b in (0..=k::ONE).step_by(1021) {
            assert!(k::mul15(a, b) <= b && k::mul15(a, b) <= a);
        }
    }
    // isqrt: every square boundary below 2^30, and a sweep
    for r in 0..32768u32 {
        let s = r * r;
        assert_eq!(k::isqrt(s), r);
        if r > 0 {
            assert_eq!(k::isqrt(s - 1), r - 1);
        }
        assert_eq!(k::isqrt(s + 1), if r == 0 { 1 } else { r });
    }
    for s in (0..1u32 << 30).step_by(4099) {
        let r = k::isqrt(s);
        assert!(r * r <= s && (r + 1) * (r + 1) > s);
    }
    // the spline CDF: ends exact, monotone, symmetric to one unit
    assert_eq!((k::cdf3(-384), k::cdf3(0), k::cdf3(384), k::cdf3(9999)), (0, k::HALF, k::ONE, k::ONE));
    for u in -384..384 {
        assert!(k::cdf3(u + 1) >= k::cdf3(u));
        assert!((k::cdf3(u) + k::cdf3(-u)).abs_diff(k::ONE) <= 1);
    }
    // blend: identity on a zero source, replacement by an opaque one, bounded
    let mut r = scenes::Rng(5);
    for _ in 0..200_000 {
        let acc = k::premul(r.next() as u32);
        let sh = k::premul(r.next() as u32);
        assert_eq!(k::blend(acc, k::splat(0)), acc);
        let o = k::premul(r.next() as u32 | 0xff00_0000);
        assert_eq!(k::blend(acc, o), o);
        let b = k::blend(acc, sh);
        assert!(b.x <= b.w && b.y <= b.w && b.z <= b.w && b.w <= k::ONE);
    }
}

/// Identity 5: a scene moved by whole pixels renders the same pixels, moved.
#[test]
fn translation_invariance() {
    let atlas_px = scenes::atlas();
    let img_px = scenes::images();
    let atlas = Atlas::new(&atlas_px, scenes::ATLAS_W).with_images(&img_px, scenes::IMG_W);
    let (w, h) = (160u32, 128u32);
    for seed in 0..6 {
        let a = scenes::adversarial(seed, w, h, 40);
        let (dx, dy) = (17i32, 9i32);
        let mut b = a.clone();
        for c in b.words.chunks_exact_mut(stage::WORDS) {
            for i in [4, 5, 6, 7] {
                let (lo, hi) = ((c[i] as u16 as i16) as i32, ((c[i] >> 16) as u16 as i16) as i32);
                // an empty core stays empty: leave the all-zero rectangle alone
                if i >= 6 && c[6] == 0 && c[7] == 0 {
                    continue;
                }
                c[i] = ((lo + dx) as u32 & 0xffff) | ((hi + dy) as u32) << 16;
            }
            c[8] = (c[8] as i32 + dx * 256) as u32;
            c[10] = (c[10] as i32 + dx * 256) as u32;
            c[9] = (c[9] as i32 + dy * 256) as u32;
            c[11] = (c[11] as i32 + dy * 256) as u32;
            if c[0] & k::F_XF != 0 {
                // the pivot and the translated pivot move with the scene; the matrix does not
                for i in [24, 26] {
                    c[i] = (c[i] as i32 + dx * 256) as u32;
                    c[i + 1] = (c[i + 1] as i32 + dy * 256) as u32;
                }
            }
        }
        let (fw, fh) = (w + 32, h + 32);
        let stride = fw as usize;
        let mut pa = vec![0u32; stride * fh as usize];
        let mut pb = pa.clone();
        cpu::naive(&a, fw, fh, &atlas, 20, &mut pa, stride);
        cpu::naive(&b, fw, fh, &atlas, 20, &mut pb, stride);
        // compare where both frames hold the unclipped scene (commands clipped by the frame edge differ by
        // construction: the frame is not part of the scene)
        for y in 0..(fh as i32 - dy) {
            for x in 0..(fw as i32 - dx) {
                let (i, j) = (y as usize * stride + x as usize, (y + dy) as usize * stride + (x + dx) as usize);
                assert_eq!(pa[i], pb[j], "seed {seed}: ({x},{y}) moved by ({dx},{dy})");
            }
        }
    }
}

/// The Vulkan executor's negative control: the same rounding constant changed in the SPIR-V (an `OpConstant`
/// of 16384, shared by every use, changed to 16383) must make the gate fail.
#[cfg(target_os = "linux")]
#[test]
fn vulkan_rounding_mutant_is_caught() {
    let mut words = bough_kernel::vulkan::spirv_words(bough_kernel::KERNEL_SPIRV);
    let mut i = 5;
    let mut site = None;
    while i < words.len() {
        let (wc, op) = ((words[i] >> 16) as usize, words[i] & 0xffff);
        if op == 43 && wc == 4 && words[i + 3] == 16384 {
            site = Some(i + 3);
            break;
        }
        i += wc.max(1);
    }
    let site = site.expect("the mutation site moved: no OpConstant 16384 in the kernel's SPIR-V");
    words[site] = 16383;
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let Some(mut gpu) = bough_kernel::vulkan::Gpu::new(&bytes) else { return };
    let atlas_px = scenes::atlas();
    let img_px = scenes::images();
    let atlas = Atlas::new(&atlas_px, scenes::ATLAS_W).with_images(&img_px, scenes::IMG_W);
    let words = STRIDE * FH as usize;
    let (mut bin, mut prog) = (Binner::default(), Prog::default());
    let mut caught = 0;
    for seed in 0..4 {
        let scene = scenes::adversarial(seed, FW, FH, 110);
        let mut spec = vec![0u32; words];
        cpu::naive(&scene, FW, FH, &atlas, 0, &mut spec, STRIDE);
        bin.bin(&scene, FW, FH, STRIDE as u32, &atlas, 0, &Damage::All, true, &mut prog);
        gpu.upload(&scene, &atlas, &prog);
        gpu.clear_out(words);
        gpu.render(prog.tiles(), words);
        caught += first_diff(gpu.out(words), &spec).is_some() as u32;
    }
    eprintln!("vulkan rounding mutant: caught in {caught} of 4 scenes");
    assert!(caught >= 1, "a Vulkan kernel with a different rounding constant passed the gate");
}

/// `wrap` (the window's pages as the output buffer): the kernel's words land in the caller's memory, equal to the
/// definition's; and a target may outlive the executor that made it (an owner drops them in any order).
#[cfg(target_os = "linux")]
#[test]
fn vulkan_writes_wrapped_memory() {
    let Some(mut gpu) = bough_kernel::vulkan::Gpu::new(bough_kernel::KERNEL_SPIRV) else { return };
    let atlas_px = scenes::atlas();
    let img_px = scenes::images();
    let atlas = Atlas::new(&atlas_px, scenes::ATLAS_W).with_images(&img_px, scenes::IMG_W);
    let words = STRIDE * FH as usize;
    let bytes = (words * 4).next_multiple_of(4096);
    let layout = std::alloc::Layout::from_size_align(bytes, 4096).unwrap();
    // SAFETY: a non-zero layout; freed below after the target is gone.
    let mem = unsafe { std::alloc::alloc_zeroed(layout) };
    assert!(!mem.is_null());
    // SAFETY: page aligned, `bytes` long, alive until after the target drops.
    let Some(target) = (unsafe { gpu.wrap(mem, bytes) }) else {
        eprintln!("vulkan: no host-pointer import on {}; wrap untested", gpu.info.name);
        unsafe { std::alloc::dealloc(mem, layout) };
        return;
    };
    let (mut bin, mut prog) = (Binner::default(), Prog::default());
    for seed in 0..3 {
        let scene = scenes::adversarial(seed, FW, FH, 110);
        let mut spec = vec![0u32; words];
        cpu::naive(&scene, FW, FH, &atlas, 0, &mut spec, STRIDE);
        bin.bin(&scene, FW, FH, STRIDE as u32, &atlas, 0, &Damage::All, true, &mut prog);
        gpu.upload(&scene, &atlas, &prog);
        gpu.render_to(prog.tiles(), &target);
        // SAFETY: the GPU finished (render_to waits); `words` u32s of the allocation.
        let out = unsafe { std::slice::from_raw_parts(mem as *const u32, words) };
        if let Some((x, y, a, b)) = first_diff(out, &spec) {
            panic!("seed {seed}: wrapped pixel ({x},{y}) gpu {a:08x} spec {b:08x}");
        }
    }
    drop(gpu);
    drop(target);
    // SAFETY: allocated above with this layout; nothing references it now.
    unsafe { std::alloc::dealloc(mem, layout) };
}
