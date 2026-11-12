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

(Sections 3 onward: region views, the CPU repair backend, the backend choice, the queue release, measured cells;
filled in as they are measured.)
