#!/usr/bin/env python3
"""Measurement-only overlay for the R6 predecessor. Run from repo root after saving originals.
Usage: instrument.py timing|stats. Never use the stats build for GPU-time claims.
Restoration is explicit from the saved predecessor files, not this script.
"""
from pathlib import Path
import sys
stats = sys.argv[1] == 'stats'
p = Path('crates/bough-metal/src/fused.rs')
s = p.read_text()
def replace(a,b):
    global s
    assert s.count(a) == 1, (a[:80], s.count(a))
    s = s.replace(a,b)
replace('    pub bin_cap: u32,', '    pub bin_cap: u32,\n    r6_draw: usize,\n    r6_enabled: bool,')
replace('            bin_cap,\n            surfs,', '            bin_cap,\n            r6_draw: 0,\n            r6_enabled: std::env::var_os("R6_PROBE").is_some(),\n            surfs,')
replace('        self.release(k, nt);', '''        self.r6_draw += 1;
        if self.r6_enabled && [60, 120, 180].contains(&self.r6_draw) {
            self.r6_probe(target, rows, full, n_dir, copy);
        }
        self.release(k, nt);''')
probe = '''    // Measurement-only overlay: called after sync, before releasing this frame.
    fn r6_probe(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, copy: &SlotCopy) {
        eprintln!("R6 frame={} size={}x{} full={} copy={:?}", self.r6_draw, self.w, self.h, full, copy);
        let reps = 100;
        for cycle in 0..5 {
            let mut cases = vec![("whole", 0, false, true), ("repair", 0, false, false), ("stage", 1, false, false), ("bin", 2, false, false), ("copy", 3, false, true), ("empty", 3, true, false),
                // floors: tile kernel with an empty bin (lookup + 1024 writes), tile dispatch that returns at once,
                // copy that writes a constant (no read), copy dispatch that returns at once
                ("tilefloor", 4, false, false), ("tilenop", 7, false, false), ("copyfill", 5, false, true), ("copynop", 6, false, true)];
            if cycle % 2 != 0 { cases.reverse(); }
            for (name, debug, empty, use_copy) in cases {
                let rr: &[u128] = if empty { &[] } else { rows };
                let cp = if use_copy { copy } else { &SlotCopy::NONE };
                let ms = self.bench_frames(target, rr, !empty && full, n_dir, reps, debug, cp);
                eprintln!("R6_TIME frame={} cycle={} case={} ms={:.6}", self.r6_draw, cycle, name, ms);
            }
        }
        // Restore the intended frame's pixels before normal submission proceeds.
        self.bench_frames(target, rows, full, n_dir, 1, 0, copy);
    }

'''
if stats:
    # Fixed diagnostic stride: all valid tile x coordinates fit the existing 128-bit damage row.
    s = s.replace('(ty + 1) * 4, MTLResourceOptions::StorageModeShared', '(ty + 1 + ty * 128 * 8) * 4, MTLResourceOptions::StorageModeShared')
    probe = '''    // Measurement-only counters; never use this build for timing.
    fn r6_probe(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, copy: &SlotCopy) {
        self.bench_frames(target, rows, full, n_dir, 1, 0, copy);
        eprintln!("R6 frame={} size={}x{} full={} copy={:?}", self.r6_draw, self.w, self.h, full, copy);
        let got = self.r6_tiles("R6_TILE", target, rows, full);
        // The diagnostic keeps the original accumulator/output code: compare retained to full redraw.
        let reference = self.reference_pixels(n_dir);
        let bad = got.iter().zip(&reference).filter(|(a,b)| a != b).count();
        eprintln!("R6_PIXEL frame={} different_bytes={}", self.r6_draw, bad);
        assert_eq!(bad, 0, "diagnostic retained/full mismatch");
        // Full viewport: the same counters for every tile (the population a scroll copy moves).
        self.bench_frames(target, &[], true, n_dir, 1, 0, &SlotCopy::NONE);
        let got2 = self.r6_tiles("R6_FULL", target, &[], true);
        assert!(got2 == reference, "full redraw differs from reference");
    }

    fn r6_tiles(&mut self, tag: &str, target: usize, rows: &[u128], full: bool) -> Vec<u8> {
        let ty = self.lay.tiles_y() as usize;
        // SAFETY: new_bins/resize allocate (ty+1+ty*128*8) shared words in this overlay;
        // bench_frames waited for completion and no stream frame has been released.
        let counters = unsafe { std::slice::from_raw_parts(self.counts.contents().as_ptr() as *const u32, ty + 1 + ty * 128 * 8) };
        let got = read_io(&self.surfs[target].io, self.w, self.h);
        for y in 0..ty {
            for x in 0..self.lay.tiles_x() as usize {
                if !full && rows.get(y).copied().unwrap_or(0) & (1u128 << x) == 0 { continue; }
                let q = &counters[ty + 1 + (y * 128 + x) * 8..][..8];
                let first = &got[(y * 32 * self.w + x * 32) * 4..][..4];
                let uniform = (y * 32..((y + 1) * 32).min(self.h)).all(|py| {
                    (x * 32..((x + 1) * 32).min(self.w)).all(|px| &got[(py * self.w + px) * 4..][..4] == first)
                });
                assert!(q[4] == 0 || uniform, "opaque certificate disagrees with pixels");
                eprintln!("{} frame={} x={} y={} scan={} hit={} raw_layers={} executed_layers={} uniform={} pixels={} pixel_uniform={} glob_layers={}", tag, self.r6_draw, x, y, q[0], q[1], q[2], q[3], q[4], q[5], u32::from(uniform), q[6]);
            }
        }
        got
    }

'''
replace('    /// Did a row bin overflow in the last frame', probe + '    /// Did a row bin overflow in the last frame')
replace('            self.encode(&cmd, 2, nr, nt, false);', '            self.encode(&cmd, 2, if matches!(debug, 3 | 5 | 6) { 0 } else { nr }, if matches!(debug, 2 | 3 | 5 | 6) { 0 } else { nt }, false);')
p.write_text(s)
p = Path('crates/bough-metal/src/shader.metal')
s = p.read_text()
if not stats:
    def rs(a, b):
        global s
        assert s.count(a) == 1, (a[:80], s.count(a))
        s = s.replace(a, b)
    rs('    if (h.debug == 2u) return;   // debug 2: the bin pass alone', '    if (h.debug == 2u || h.debug == 7u) return;')
    rs('    uint nb = atomic_load_explicit(fb.counts + tile.y, memory_order_relaxed);\n    for (uint b0 = 0; b0 < nb; b0 += 128) {\n        bool rh = false, cover = false;\n        FRec r;',
       '    uint nb = h.debug == 4u ? 0u : atomic_load_explicit(fb.counts + tile.y, memory_order_relaxed);\n    for (uint b0 = 0; b0 < nb; b0 += 128) {\n        bool rh = false, cover = false;\n        FRec r;')
    rs('    device const FSlot& fs = *(device const FSlot*)(pool + slot.x);\n    device const FBind& fb = *(device const FBind*)(pool + FBIND_OFF);\n    device const FRow* frows = (device const FRow*)(pool + slot.x + FSLOT_ROWS);\n    uint i = tgid2.x, tw',
       '    uint dbg = ((device const FHdr*)pool)->debug;\n    if (dbg == 6u) return;\n    device const FSlot& fs = *(device const FSlot*)(pool + slot.x);\n    device const FBind& fb = *(device const FBind*)(pool + FBIND_OFF);\n    device const FRow* frows = (device const FRow*)(pool + slot.x + FSLOT_ROWS);\n    uint i = tgid2.x, tw')
    rs('        if (all(px >= r0) && all(px < r1)) dst.write(src.read(uint2(int2(px) - d)), px);',
       '        if (all(px >= r0) && all(px < r1)) { if (dbg == 5u) dst.write(float4(0.25, 0.5, 0.75, 1.0), px); else dst.write(src.read(uint2(int2(px) - d)), px); }')
    p.write_text(s)
    sys.exit(0)
start = s.index('kernel void fs_tile(')
end = s.index('// Scroll by copy (RESULTS', start)
head, t, tail = s[:start], s[start:end], s[end:]
def tr(a,b):
    global t
    assert t.count(a) == 1, (a[:80], t.count(a))
    t = t.replace(a,b)
tr('    for (uint b0 = 0; b0 < nb; b0 += 128) {', '''    device atomic_uint* stats = fb.counts + h.tiles_y + 1u + (tile.y * 128u + tile.x) * 8u;
    if (li < 8u) atomic_store_explicit(stats + li, 0u, memory_order_relaxed);
    threadgroup_barrier(mem_flags::mem_device);
    uint raw_layers = 0u, exec_layers = 0u, glob_layers = 0u, pixels = 0u;
    for (uint j = 0; j < 4u; j++) {
        uint2 px = o + 2u * lt + uint2(j & 1u, j >> 1);
        pixels += uint(px.x < uint(h.viewport.x) && px.y < uint(h.viewport.y));
    }
    for (uint b0 = 0; b0 < nb; b0 += 128) {''')
tr('        k0 = k0 > 0 ? k0 - 1u : 0u;', '''        if (li == 0u) {
            atomic_fetch_add_explicit(stats + 1, nr, memory_order_relaxed);
            if (nr != 0u) atomic_store_explicit(stats + 4, uint(k0 == nr), memory_order_relaxed);
        }
        if (k0 > 0u) glob_layers = 0u;   // a cover in this batch hides every earlier batch (global floor)
        k0 = k0 > 0 ? k0 - 1u : 0u;
        for (uint k = 0; k < nr; k++) {
            for (uint j = 0; j < 4u; j++) {
                uint2 px = o + 2u * lt + uint2(j & 1u, j >> 1);
                if (px.x >= uint(h.viewport.x) || px.y >= uint(h.viewport.y)) continue;
                half4 sh = f_shade_x(rec[k], c00 + float2(float(j & 1u), float(j >> 1)), atlas, img);
                raw_layers += uint(sh.a > 0.0h);
                exec_layers += uint(k >= k0 && sh.a > 0.0h);
                glob_layers += uint(k >= k0 && sh.a > 0.0h);
            }
        }''')
tr('    for (uint j = 0; j < 4; j++) {\n        uint2 px = o + 2u * lt', '''    uint raw_sum = simd_sum(raw_layers), exec_sum = simd_sum(exec_layers), pixel_sum = simd_sum(pixels), glob_sum = simd_sum(glob_layers);
    if (lane == 0u) {
        atomic_fetch_add_explicit(stats + 2, raw_sum, memory_order_relaxed);
        atomic_fetch_add_explicit(stats + 3, exec_sum, memory_order_relaxed);
        atomic_fetch_add_explicit(stats + 5, pixel_sum, memory_order_relaxed);
        atomic_fetch_add_explicit(stats + 6, glob_sum, memory_order_relaxed);
    }
    if (li == 0u) atomic_store_explicit(stats, nb, memory_order_relaxed);
    for (uint j = 0; j < 4; j++) {
        uint2 px = o + 2u * lt''')
p.write_text(head + t + tail)
