# Lane memory, round 6: results per lever

Machine: M3 Pro, 36 GiB, macOS 15 (Darwin 24.6), built-in display at 2x; direct Metal. The ledger these levers
answer to is `LEDGER.md`. Every number below comes from a command named beside it; raw output is under
`bench/results/memory/` or quoted.

## 1. The GPU driver's block (76 MiB of an AWAGY window): what creates it, what returns it

`crates/bough-metal/examples/drvprobe.rs`, one variant per process, phys_footprint MiB:

| variant | after the submission | then |
|---|---:|---|
| device + queue + buffer, nothing submitted | 5.4–6.5 | |
| a committed command buffer with no encoder | 5.7 | 5.8 after 2 s |
| a compute encoder with nothing dispatched | 6.1 | 6.5 |
| a command buffer that only signals a shared event | 6.0 | 6.2 |
| a 4-byte blit (private buffer) | 75.1 | 14.9 about 1.17 s after (60.8 uncharged, 8 stays) |
| the same on a queue with `maxCommandBufferCount = 1` | 75.4 | 15.2 after 1.12 s |
| the same on a shared buffer | 74.9 | 14.7 after 1.12 s |
| blit, then the queue (and its command buffers) released | 75.4 | **48.6 at release, 7.0 within 55 ms** |
| blit, then queue, buffer and device released | 75.0 | 43.0, 6.5 within 55 ms |
| a second, idle queue alive while the first is released | 75.2 | 51.3, 7.0 within 200 ms |
| a process that only writes an IOSurface from the CPU | – | 6.5 |

* It is created by the first command buffer that runs work, not by the queue, an empty buffer, an empty encoder or
  an event; queue depth, storage mode and residency sets do not change it (`floorprobe`, round 5's probe, rerun:
  compute, 32 KiB threadgroup memory, an event-released pre-armed buffer all +60.8 + 8.2; a second queue's first
  submission +0.2; a render pass +30 more).
* It belongs to the queue that ran the work: releasing that queue returns all of it, the persistent 8 MiB included.
* Getting it back (`drvprobe recreate`, 3 cycles x 2 runs): a new queue and its first blit cost 3.0–3.3 ms and
  10.9–11.1 M process instructions. On a kept queue: a blit after 300 ms idle 0.32–0.63 ms / 0.18–0.21 M; after 2 s
  idle (the GPU powered down) 2.4–2.7 ms / 8.4–8.6 M.

## 2. Is the fused kernel reproducible on the CPU, bit for bit?

Yes for untransformed solid boxes, rounded boxes, borders, glyph instances and glyph runs, once two things are
known (`comparisons/bough-awagy/examples/cpu_tiles.rs`: a CPU model compared with the GPU's full redraw tile by
tile; `crates/bough-metal/examples/mathprobe.rs`: single operations over 4 M inputs):

* **the blend is one half-precision FMA**: `sh + acc * (1 - sh.a)` is contracted by the MSL compiler (fast math).
  Adversarial scene (3,000 translucent quads at quarter-pixel x, 1,200 rounded boxes and borders, 400 translucent
  labels; 4 seeds x 2,772 tiles): one rounding, 0 mismatching tiles; two roundings, 2,683–2,710 tiles differ;
* **the border mix `pm * h1 + pm2 * h2` is `fma(pm, h1, pm2 * h2)`**: without it 3–5 tiles per seed differ;
* **fast-math `length` cannot be reproduced**: the GPU's `sqrt(x)` is `x * rsqrt(x)` exactly (0 mismatches of 4 M),
  and its `rsqrt` is not correctly rounded (171,275 of 4 M), so `sqrt` differs from IEEE in 1,221,178 of 4 M; the dot
  inside `length` is exactly `fma(y, y, x * x)`. One border pixel in three seeds of the CPU-repair test was off by
  one LSB because of it;
* the store's rounding (nearest even vs half up) was never distinguished (no ties occur).

So shader.metal (lane gpu's file, prototyped here) now writes these steps out: `f_sd` uses
`precise::sqrt(fma(m.y, m.y, m.x * m.x))` (IEEE: 0 mismatches), and the blend and the border mix are explicit
`fma`s (the contractions the compiler already chose). The kernel's arithmetic no longer depends on the compiler's
choices for the modelled kinds. GPU pixels change only where the fast square root was off (SDF edge pixels).

### The GPU cost of writing the arithmetic out

Saturated-GPU bench (`Fused::bench_frames`, REPS = 400, median of 5 per process; the shader chosen at run time on one
binary with `BOUGH_METAL_SRC`; old and new alternating; `runtime` logs `kernel_bench-*.txt`, `kernel_ab-sdf.txt`):

| scene | old shader (ms) | written-out shader (ms) | ratio |
|---|---:|---:|---:|
| lab stream, full redraw (5 rounds) | 0.2787–0.2788 | 0.2783–0.2784 | 0.999 |
| lab stream, repair 499 tiles | 0.0952–0.0953 | 0.0951–0.0952 | 0.999 |
| lab scroll, full redraw | 0.2294–0.2295 | 0.2293 | 0.999 |
| lab scroll, repair 174 tiles | 0.0234–0.0237 | 0.0234–0.0239 | 1.00 |
| SDF-dense adversarial (300 rounded boxes / borders, 600 translucent quads, 61 labels), seed 3, full redraw | 0.3110–0.3112 | 0.3147–0.3149 | 1.012 |
| the same, seed 11 | 0.3140–0.3143 | 0.3180–0.3181 | 1.013 |

Variants, lab stream full redraw: explicit fmas alone 0.2782–0.2784; `precise::sqrt(fma(..))` without a branch
0.2783–0.2784; a branch that skips the root beside edges (the root only at corners) 0.2914–0.2925 with a precise root
and 0.2914–0.2915 with the fast one: the branch, not the root, costs (+4.6 %); rejected. Not measured with this
instrument: heavy, animate, vlist (no saturated bench exists for them). The headless per-frame GPU-slope instrument
(`bench/memory/gpu_ab.py`, 10x spread, 9 pairs) could not resolve 2 % at load 40–56: its A/A controls gave medians
0.87–1.00 with spreads up to 0.63–1.91.

## 3. The region as address space: per-column views (card `2026-11-11-region-views`)

**Mechanism.** The region's CPU memory is an anonymous mmap that reserves the record format's own limits (2^18
instances, 2^15 pages, 2^22 glyph slots, 2^20 run headers, 2^8 strikes; spaces and clips as configured), every
GPU-read column on a page boundary. The buffer bound to the encoders covers the header and frame slots only; each
column has its own no-copy view over the page-rounded prefix that holds its mask group's capacity (a power of two,
at least doubling), named in a column table in the header that the kernels read instead of `region + offset`.
Before a GPU frame `App` hands the presenter each group's high-water mark (seven compares); a growth makes new views,
rewrites the table and masks, and drains the pre-armed stream (as a bin growth does). CPU addresses never move.
`Caps` stops being a limit: a mount reaches only the format's limits.

**Gates.** `tests/capacity.rs`: 1,000 quads, then a turn to 70,000 (past APP_CAPS's 65,536 instances) in a live
window; each quad's centre pixel is its own colour and the frame equals a full redraw: passes; with
`--features sabotage_views` (views never grow) 68,976 of 70,000 quads are wrong. `bough-mem`
`heap_region_is_lazy`: 256 MiB reserved, nothing resident; one element written, one page resident. Instruction
slopes (release builds, `bench/memory/slopes_ab.py`, 10x spread, median of 5, A = d892f974da, B = a1873e83fa with
`BOUGH_REPAIR=gpu`), process instructions per frame:

| cell | RENDER=1: A | B | B/A | RENDER=0: A | B | B/A |
|---|---:|---:|---:|---:|---:|---:|
| heavy stream | 263,340 | 264,634 | 1.005 | 159,087 | 158,920 | 0.999 |
| heavy scroll | 136,797 | 138,261 | 1.011 | 34,342 | 34,208 | 0.996 |
| heavy both | 309,477 | 310,335 | 1.003 | 203,028 | 203,275 | 1.001 |
| lab both | 134,134 | 133,035 | 0.992 | 29,722 | 29,853 | 1.004 |
| animate | 121,532 | 121,822 | 1.002 | 13,873 | 14,049 | 1.013 |
| vlist scroll | 117,121 | 117,195 | 1.001 | 12,755 | 12,798 | 1.003 |
| vlist jump | 136,447 | 136,259 | 0.999 | 31,186 | 31,206 | 1.001 |

Every cell within 1.02 (B is the tree at a1873e83fa's code, built before its commit). Every repetition is in the
runtime log `slopes-views.txt`.

(Sections 4 onward: the CPU repair backend's measured cells, the backend choice, the queue release, memory after.)
