# bough-metal: first prototype results

Apple M3 Pro, macOS 15.7.4, ProMotion panel (display link 120 Hz, 8.33 ms), window 960x720 pt @2x = 1920x1440 px
BGRA8. `release` profile, `nightly-2026-10-17`, `objc2` 0.6 family, no wgpu, no winit. Raw data is in `results/`.

All numbers below were taken with the harness in this crate (`examples/bench.rs`). Nothing here re-measures zgx; see
"Comparing with zgx" before quoting any ratio.

## 1. What was built

| piece | where | notes |
|---|---|---|
| 48-byte instance | `src/instance.rs`, `src/shader.metal` | pos, size, 2 x RGBA8, radius, border width, atlas uv (4 x u16), `space` (u16), `kind` (u8), `flags`, `ext` = clip index. `static_assert`ed to 48 B. |
| uber-shader | `src/shader.metal` | one vertex fn, one fragment fn, `switch(kind)`: solid, rounded rect (analytic AA via SDF), border (ring + fill), glyph (R8 atlas). Premultiplied source-over blend. Vertex stage adds `spaces[inst.space]`, clamps the quad to `clips[inst.ext]`. |
| pool | `src/gpu.rs` | ONE `storageModeShared` MTLBuffer holds header, spaces, clips, stencil rects, 4096 instances, draw list. The CPU writes instances in place; the shader gets the buffer once (`setVertexBuffer`) and finds everything from the header. Draw = `drawPrimitives(strip, 4, instanceCount: N, baseInstance: start)`; the vertex shader does `idx = list[instance_id]`. |
| scene | `src/scene.rs` | 1,470 instances: sidebar, header, 6 movable cards (a `space` each), 60-bar chart, 22-row scrollable table (a `space` + clip) of 8-glyph cells from a procedural 1024x128 R8 atlas. |
| damage | `src/damage.rs` | 16x16-px tile bitmap planes (120x90 tiles, rows padded to 128 bits), 16-epoch history ring, `repair(since) = OR of planes since`, tile plane -> rect list (horizontal runs merged vertically, allocation free). |
| repair modes | `src/gpu.rs` | `bbox` (1 scissored draw), `multi-shared` (1 scissored draw per rect, shared bbox-culled list), `multi-culled` (per-rect CPU-culled lists), `stencil` (rect list written to a memoryless Stencil8 attachment, then ONE draw; falls back to the plain path when there is one rect). |
| strategies | `src/present.rs` | (a) `Control`, (b) `IoRing` with three publish syncs: `Wait` (b), `Scheduled` (bs), `Handler` (bh), (c) `MetalAge`. |
| window | `src/app.rs` | **raw AppKit via objc2-app-kit, no winit.** `NSApplication` (Accessory policy, so no Dock icon and no focus steal), `NSWindow`, layer-hosting `NSView`, `NSView.displayLink(target:selector:)` (CADisplayLink, macOS 14+) driving a `define_class!` target. ~115 lines; it never blocked. |

Shader build: `build.rs` runs `xcrun -sdk macosx metal -O2` + `metallib` into `OUT_DIR`, embedded with `include_bytes!`;
the same MSL is also available for runtime compilation.

### Startup cost (headless, `bench --startup metallib|runtime`, 5 runs each)

| | device + queue | library | 3 pipelines | `Engine::new` total | first full frame (ring + 3 IOSurfaces + draw) |
|---|---|---|---|---|---|
| AOT metallib | 24.6 - 36.7 ms (one 134 ms outlier) | 0.03 - 0.5 ms | 1.4 - 2.8 ms | 28 - 40 ms | 8.4 - 11 ms (one 41 ms outlier) |
| runtime MSL, **cold** (first run of a new source hash, seen once) | 32 ms | **236 ms** | **38 ms** | **307 ms** | 7 ms |
| runtime MSL, warm (system shader cache hit) | 26 - 33 ms (one 82 ms) | 1.0 - 2.7 ms (one 15.6 ms) | 1.3 - 5.6 ms | 31 - 39 ms | 9 - 11 ms |

Winner: AOT metallib. Runtime compile is only ~equal once the system cache is warm; the cold case (first launch after an
update, new machine) costs ~270 ms of pure shader work. `MTLCreateSystemDefaultDevice` + queue (25 - 35 ms) dominates
both and is unavoidable.

## 2. Correctness: byte-exact against a full redraw

`examples/correctness.rs`: scripted random partial updates (idle, 1 colour, 2-80 scattered, card move, scroll,
resize, background colour, invalidate-all, combined). After EVERY presented frame the presented surface is read back
(IOSurface memory directly for (b); a blit of the drawable for (a) after the copy and for (c)) and compared byte for byte
with a from-scratch full redraw of the final scene through the same pipeline.

| run | strategies x modes | frames presented | mismatches |
|---|---|---|---|
| headless, 400 scripted frames each (detached layers) | a, b, bs, bh, c x {bbox, multi-shared, multi-culled, stencil} = 20 combos | 308 - 335 per combo | **0** |
| windowed, display-link paced, 300 frames each | a, b, bs, bh, c x {bbox, stencil} | 246 / 249 per combo | **0** |
| sabotage control (`BOUGH_SABOTAGE=1`: ages one epoch too young) | b, bbox, 100 frames | 79 | **56** (so the check does detect age-tracking bugs) |

Caveat: this proves the pixels in the surface are right when the GPU is finished. It cannot see tearing of a surface
the compositor reads while the GPU is still writing it (relevant to `b`/`bs`/`bh`, section 4).

## 3. Benchmarks (10 s per strategy, display-link rate, `--secs 2` x 5 scenarios, damage mode `stencil`)

One process per strategy (clean RSS). First 10 ticks of every scenario are discarded. Metrics per tick:

* **in-tick instr** = `ri_instructions` delta (via `proc_pid_rusage(RUSAGE_INFO_V4)`, snap cost subtracted) around
  scene update + `Engine::frame()` on the main thread. This is the "our own code + the Metal calls we make" number.
* **process instr/tick** = `ri_instructions` delta between consecutive tick starts, i.e. everything the process did
  (main thread, Metal/driver threads, completion handler, Core Animation, kernel on our behalf). Includes ~60 K/tick
  of display-link/run-loop overhead that exists even when idle (see idle row).
* **thread CPU** = sum over all threads of CPU time per tick (Mach `THREAD_EXTENDED_INFO`).
* GPU ms = `GPUEndTime - GPUStartTime`; tick->GPU = tick start to `GPUEndTime`.
* interval = display-link tick to tick (all runs: median 8.33 ms; refresh was never missed in the shown rows except where p99 > 8.4).

Scenarios: idle (no damage, tick still arrives), one colour change (random small instance), 80 scattered colour
changes (heavy-stream shape), 400-px-tall scrolled table viewport (1500x400 px repaint), full redraw (1920x1440).

### Idle

| | process instr/tick (link running) | CPU of all threads per tick | process instr/s with link **paused** | CPU ms/s paused |
|---|---|---|---|---|
| all five runs | 59.5 - 62.1 K (= 7.2 M instr/s at 120 Hz) | 43 - 57 us (~0.5 % of a core) | 0.29 - 0.43 M | 0.18 - 0.29 |

No command buffer, no allocation, no Metal message in the idle path (`Engine::frame` returns after one bool test), but
the display link itself costs ~60 K instr per tick. Pausing the link when nothing is damaged removes 95 % of it.

### (a) CONTROL: CAMetalLayer + retained private texture + whole-texture blit to drawable (zgx-like)

| scenario | in-tick instr | process instr/tick | thread CPU us | GPU ms med / p99 | tick->GPU ms | interval med / p99 | ObjC msgs | draws | repaired px |
|---|---|---|---|---|---|---|---|---|---|
| one colour | 166.8 K | 789.7 K | 467 | 0.502 / 1.313 | 0.80 | 8.33 / 8.36 | 16 | 1 | 2,204 |
| 80 scattered | 233.6 K | 858.5 K | 444 | 0.655 / 1.326 | 0.94 | 8.33 / 8.38 | 20 | 2 | 196,653 |
| scroll 400 px | 236.2 K | 860.9 K | 452 | 0.535 / 1.528 | 0.96 | 8.33 / 8.38 | 16 | 1 | 608,000 |
| full redraw | 375.7 K | 832.6 K | 433 | 0.923 / 1.985 | 2.07 | 8.33 / 8.36 | 16 | 1 | 2,764,800 |

Memory: RSS 36.6 MB, **phys_footprint 242 MB**, `MTLDevice.currentAllocatedSize` 47.0 MB (retained texture 11.1 MB +
3 system drawables). In an earlier 1-s-per-scenario run the same strategy showed p99 intervals of 15.7 - 16.4 ms on
scatter/scroll/full (missed vsyncs) with tick->GPU of 9 ms: `nextDrawable` blocking.

### (b) IOSurface ring (3 surfaces, `layer.contents = IOSurface` in a CATransaction), publish after `waitUntilCompleted`

| scenario | in-tick instr | process instr/tick | thread CPU us | GPU ms med / p99 | tick->GPU ms | interval med / p99 | ObjC msgs | draws | repaired px |
|---|---|---|---|---|---|---|---|---|---|
| one colour | 363.2 K | 434.9 K | 247 | 0.193 / 1.589 | 0.55 | 8.33 / 8.36 | 19 | 2 | 6,567 |
| 80 scattered | 437.1 K | 506.2 K | 340 | 0.496 / 2.126 | 0.82 | 8.33 / 8.37 | 19 | 2 | 492,367 |
| scroll 400 px | 408.8 K | 472.4 K | 294 | 0.490 / 1.383 | 0.69 | 8.33 / 8.36 | 15 | 1 | 608,000 |
| full redraw | 344.1 K | 406.1 K | 290 | 0.902 / 1.826 | 1.15 | 8.33 / 8.37 | 15 | 1 | 2,764,800 |

Memory: RSS 28.4 MB, phys_footprint 159 MB, currentAllocatedSize 35.5 MB (3 x 11.06 MB IOSurfaces = 33.2 MB + pool/atlas).

### (b-s) same, publish after `waitUntilScheduled`

| scenario | in-tick instr | process instr/tick | thread CPU us | GPU ms med / p99 | interval med / p99 | ObjC msgs |
|---|---|---|---|---|---|---|
| one colour | 331.8 K | 431.6 K | 222 | 0.373 / 1.093 | 8.33 / 8.38 | 19 |
| 80 scattered | 402.8 K | 502.8 K | 225 | 0.441 / 1.087 | 8.33 / 8.35 | 19 |
| scroll 400 px | 373.6 K | 470.2 K | 248 | 0.408 / 1.074 | 8.33 / 8.36 | 15 |
| full redraw | 304.3 K | 403.7 K | 254 | 0.866 / 1.713 | 8.33 / 8.36 | 15 |

### (b-h) same, publish from the GPU completion handler (**winner**)

| scenario | in-tick instr | process instr/tick | process net of idle tick (-60 K) | thread CPU us (main) | GPU ms med / p99 | tick->GPU ms | interval med / p99 | ObjC msgs | draws | repaired px |
|---|---|---|---|---|---|---|---|---|---|---|
| one colour | **83.3 K** | 418.9 K | 359 K | 223 (111) | 0.216 / 1.052 | 0.44 | 8.33 / 8.36 | 15 | 2 | 6,567 |
| 80 scattered | 153.6 K | 491.4 K | 431 K | 321 (130) | 0.442 / 1.550 | 0.68 | 8.33 / 8.35 | 15 | 2 | 492,367 |
| scroll 400 px | 122.9 K | 459.6 K | 400 K | 257 (114) | 0.317 / 1.745 | 0.58 | 8.33 / 8.37 | 11 | 1 | 608,000 |
| full redraw | 57.5 K | 392.9 K | 333 K | 289 (101) | 0.864 / 1.846 | 1.07 | 8.33 / 8.36 | 11 | 1 | 2,764,800 |

Memory: RSS 26.9 MB, phys_footprint 159.5 MB, currentAllocatedSize 35.5 MB.

### (c) CAMetalLayer drawables, `loadAction = Load`, ages tracked per drawable (identified by IOSurface id), framebufferOnly = true

| scenario | in-tick instr | process instr/tick | thread CPU us | GPU ms med / p99 | tick->GPU ms | interval med / p99 | ObjC msgs | draws |
|---|---|---|---|---|---|---|---|---|
| one colour | 176.7 K | 764.1 K | 406 | 0.286 / 1.104 | 0.54 | 8.33 / 8.37 | 17 | 2 |
| 80 scattered | 266.5 K | 850.7 K | 395 | 0.463 / 1.075 | 0.77 | 8.33 / 8.36 | 17 | 2 |
| scroll 400 px | 426.5 K | 855.3 K | 382 | 0.308 / 1.072 | 1.51 | 8.33 / 8.60 | 13 | 1 |
| full redraw | 441.6 K | 808.1 K | 398 | 0.801 / 1.298 | 9.22 | 8.34 / 9.60 | 13 | 1 |

Memory: RSS 36.4 MB, phys_footprint 159.7 MB, 3 drawables = 33.2 MB. Run-to-run: in the other full 10-s run of (c) the
layer grew a **4th drawable** mid-run (new IOSurface id; our tracker repaints it in full, so it stays correct) and the
first scenario showed p99 17.9 ms / tick->GPU 8.5 ms. (c) is the noisiest strategy.

### Summary: one colour change

| | in-tick instr | process instr/tick | thread CPU us | main-thread CPU us | msgs |
|---|---|---|---|---|---|
| (a) control | 166.8 K | 789.7 K | 467 | 155 | 16 |
| (c) CAMetalLayer age | 176.7 K | 764.1 K | 406 | 142 | 17 |
| (b) IOSurface, wait completed | 363.2 K | 434.9 K | 247 | 188 | 19 |
| (b-s) IOSurface, wait scheduled | 331.8 K | 431.6 K | 222 | 165 | 19 |
| **(b-h) IOSurface, completion handler** | **83.3 K** | **418.9 K** | **223** | **111** | 15 (+4 on the Metal thread, included) |

## 4. Damage-repair modes (strategy b-h, 1 s per scenario, `results/sweep_bh.txt`)

| mode | one colour: in-tick instr / draws / GPU ms / repaired px | 80 scattered: in-tick instr / draws / GPU ms med (p99) / repaired px | scroll: in-tick instr, GPU ms | full: in-tick instr, GPU ms |
|---|---|---|---|---|
| bbox | 80 K / 1 / 0.427 / 607,372 | 149.6 K / 1 / 0.900 (1.29) / 2,432,393 | 124 K, 0.233 | 57 K, 0.730 |
| multi-shared | 84 K / 3 / 0.311 / 4,970 | **335 K** / **136** / 1.239 (2.23) / 457,567 | 123 K, 0.476 | 57 K, 0.892 |
| multi-culled | 84 K / 3 / 0.339 / 7,705 | **1,025 K** / 130 / 0.405 (1.10) / 506,166 | 123 K, 0.487 | 57 K, 0.876 |
| **stencil** | 83 K / 2 / 0.348 / 4,691 | 150.6 K / 2 / **0.451 (1.06)** / 515,903 | 123 K, 0.435 | 57 K, 0.877 |

* A ring of 3 means every repair is the union of the last 3 epochs' damage, so "one colour" repairs 3 glyph-sized tiles;
  bbox of three random spots is 607 K px (22 % of the screen) vs 4.7 K px as a rect list. The union-bbox is the loser.
* Per-rect draws scale badly: 130+ scissored draws cost 335 K instr (multi-shared, because the CPU encodes them) and
  up to 2.2 ms p99 GPU; per-rect culling is 1.0 M instr of pure CPU. The stencil mask costs the same CPU as bbox (150 K) and
  has the best GPU p99. **Stencil it is**: one rect -> plain path, many rects -> memoryless stencil + one draw.
* GPU time at display cadence is dominated by a ~0.2 - 0.4 ms floor (the GPU clocks down between 8-ms frames): the same
  tiny repair measures 0.014 ms median back-to-back (`results/floor.txt`). Compare GPU columns only within one run.
* Full redraw uses `loadAction = DontCare` (nothing loaded); partial repairs use `Load`.

## 5. Findings per question asked

### (b) IOSurface ring: latency, pacing, synchronization

* **Publish only after the GPU is done (or scheduled).** `layer.contents = surface` hands the compositor a surface
  whose pixels the GPU may still be writing. Three variants measured above: `waitUntilCompleted` inline (`b`),
  `waitUntilScheduled` inline (`bs`), completion-handler thread (`bh`). The total process cost is about the same (419 -
  435 K instr per frame); the difference is *where* it is paid: `b` burns 250 K+ instr / ~190 us on the main thread
  in the wait, `bh` keeps the main thread at 83 K / 111 us. `bs` relies on GPU in-order execution between our work and
  the compositor's reads, which I did **not** verify visually; `bh` is safe by construction.
* **The compositor holds the two newest surfaces.** `IOSurfaceIsInUse` sampled before each present (~985 frames per
  run, final runs): in 92 - 94 % of frames (`b`, `bh`) the surface about to be redrawn is free while the previous two
  are in use. **But in ~3 % of frames (28/986 `b`, 35/986 `bh`, 32/983 `bs`) the surface round-robin picked was still
  marked in use** (an earlier 1-s run had 0 of 500, so it is load dependent). We overwrote it anyway (`skip_in_use` is
  off); no corruption was observable through the CPU-side checks, but a compositor that is still scanning that
  surface would show a glitch. A ring of 3 is therefore the minimum and NOT enough to be safe by construction:
  add a 4th surface and pick by `!is_in_use()` (`IoRing::skip_in_use` implements the skip; it needs `n >= 4` to have
  a free candidate). With `bs` the compositor releases surfaces earlier (89 % of frames only the newest is held)
  because publication happens before GPU completion.
* **Age tracking is exact** because we own the ring: age = epochs since the surface was drawn; `repair(age)` is a few
  plane ORs. 0 mismatches over thousands of frames (section 2).
* **Pacing**: all `b*` runs held median 8.33 ms, p99 8.35 - 8.37 ms with no missed vsyncs. (a) and (c) show p99 up to
  16 - 18 ms in some scenarios and tick->GPU-done of 9 ms in full redraw (`nextDrawable` back-pressure).
* **Latency to photons**: not directly measurable for a plain `CALayer` (no presented callback). Measured for
  (c) with `drawable.presentedTime` (`examples/latency.rs`, `results/latency.txt`): tick start -> on screen is **16.7 ms
  (2 vsyncs) when at most one present is outstanding** (25 ms in an earlier run), **33 ms (4 vsyncs) unthrottled with
  3 drawables** (33 - 40 ms across runs), and 50 ms in one run with a throttle of 2 outstanding (unexplained);
  the display-link tick fires 8.3 ms before the deadline vsync, so the best case is 2 vsyncs through WindowServer. `layer.contents` commits through the same render-server pipeline, and our 1-in-flight discipline is the
  throttled case, so I expect ~16.7 ms, i.e. no worse than the best CAMetalLayer. The release phase of surfaces
  (poller thread on `IOSurfaceIsInUse`) clusters near the vsync target but with a second mode half a period later; I
  could not turn that into a trustworthy latency number, so treat the (b) latency as inferred, not measured.
* **`presentsWithTransaction` / display link**: `presentsWithTransaction` is a `CAMetalLayer` property (it makes a
  present land inside the current CATransaction, for resize sync); it does not exist for a plain layer and is not
  needed: our publish already *is* a CATransaction, so a size change plus the new surface can be committed
  atomically for free. A display link is **not required for correctness**, only for pacing: it fires 8.3 ms before the
  deadline vsync, giving one frame of budget; sparse updates could skip it entirely and commit on damage (the
  commit lands on the next vsync). Pausing it when idle is the single biggest idle win (section 3).
* Cost of the publish itself: `CATransaction` begin / disableActions / setContents / commit = 86 - 118 K instr per frame
  (`bench --strategy b --profile`; in (b-h) it moves to the completion thread), the largest single item in the frame.

### (c) CAMetalLayer drawables: are contents preserved across reuse?

`examples/probe.rs`: each acquisition reads the whole drawable back (needs `framebufferOnly = false`), compares with the
colour written at the last use of that drawable, then clears it to colour(counter), presents.

| config | drawables seen (by IOSurface id / by MTLTexture pointer) | re-acquisitions checked | preserved exactly | lost / partial |
|---|---|---|---|---|
| 3 drawables, every tick | 3 / 3 | 237 | **237** | 0 / 0 |
| 3 drawables, every 30th tick (0.25 s gaps) | 2 / 2 | 118 | **118** | 0 / 0 |
| 2 drawables, every tick | 2 / 2 | 198 | **198** | 0 / 0 |
| framebufferOnly = true | 3 / 3 (readback impossible; correctness harness covers it indirectly) | - | - | - |

* Contents were **always preserved**; the texture identity (`MTLTexture` pointer) and the IOSurface id are both
  stable per drawable and 1:1, so either identifies a drawable. Plus 0 mismatches in the full scripted correctness
  runs with `loadAction = Load` and per-drawable age.
* **Not reliable in principle**: the API does not promise it; the number of drawables varies (2 or 3 seen in the probe
  depending on framebufferOnly/timing; a 4th appeared mid-run in one bench). A new id is treated as age = None (full
  repaint) so growth is safe; the unsafe failure is the same id with discarded or modified contents (resize, display
  change, memory pressure, power events, compression changes), which I could not provoke here but also cannot rule
  out. I would not ship (c) as the only path, but it is a legitimate fast path *with a cheap validity check* (e.g.
  keep a 4x4 probe pixel per drawable, read via the same GPU pass, and fall back to full repaint on mismatch).
* The performance verdict on (c) is the real reason to prefer (b): `nextDrawable` alone costs **237 - 304 K instr**
  per frame (`results/profile_a.txt`), more than the entire in-tick budget of (b-h).

## 6. The exact Metal call sequence per frame for the winner (b-h, steady state)

Display-link callback `tick:` (main thread). Counted ObjC messages (retain/release and getters excluded):

```
 0  [cmd status]                     poll previous frame (complete? else waitUntilCompleted)  -- sync(), 1 msg
    CPU: scene writes (4 B per colour), damage plane OR, rect extraction, page-culled draw list into the pool
 1  [queue commandBuffer]
 2  [colorAttachment0 setTexture:surface[i].tex]       (descriptor + attachment cached; setLoadAction only when it flips Load/DontCare)
 3  [cmd renderCommandEncoderWithDescriptor:pd]
 4  [enc setRenderPipelineState:pso]
 5  [enc setVertexBuffer:pool offset:0 atIndex:0]       (the ONLY buffer: header, spaces, clips, instances, draw list)
 6  [enc setFragmentTexture:atlas atIndex:0]
 7  [enc setScissorRect:bbox]
 8  [enc drawPrimitives:TriangleStrip vertexStart:0 vertexCount:4 instanceCount:N baseInstance:start]
 9  [enc endEncoding]
10  [cmd addCompletedHandler:block]
11  [cmd commit]
                                                      => 11 messages on the render thread (+1 poll)
 GPU-completion thread, handler body:
12  [CATransaction begin]
13  [CATransaction setDisableActions:YES]
14  [layer setContents:iosurface]
15  [CATransaction commit]                            => 4 messages
```
Multi-rect repair replaces 4-5 by: `setRenderPipelineState(mask)`, `setDepthStencilState(write)`, `setStencilReferenceValue`,
`setVertexBuffer`, `setScissorRect`, `draw(rects)`, `setRenderPipelineState(main+stencil)`, `setDepthStencilState(test)`,
`setFragmentTexture`, `setScissorRect`, `draw`: 15 messages total as measured (`ObjC msgs` column). Idle = 0 messages.
Instruction split of the one-colour frame (`bench --profile`, mean): region build 9.5 K, cmd-buffer + draw-list cull 15 K,
encode 39 K, commit 14 K (= 77 K of the 83 K in-tick); the completion thread adds ~100 K (CA commit) plus Metal
completion/driver work, which with the 60 K idle tick makes the 419 K process total.

## 7. Floor: what bare Metal costs here (`examples/floor.rs`, headless, amortised over 2000 iterations)

| operation | instr | CPU us |
|---|---|---|
| `snap()` = `proc_pid_rusage` (measurement overhead!) | 5.7 K | 0.3 |
| empty command buffer: create + commit | 50.6 K | 13 |
| empty command buffer: create + commit + `waitUntilCompleted` | 112 K | 36 |
| 16x16 repair, 1 draw: encode + commit, no wait | 180 K | 60 |
| same + `waitUntilScheduled` | 228 K | 87 |
| same + `waitUntilCompleted` | 241 K | 164 |
| same + completed handler + wait | 239 K | 140 |
| `commandBufferWithUnretainedReferences` vs normal | 220.0 K vs 220.1 K | no difference |
| whole-texture 1920x1440 blit copy (cmd + wait) | 188 K (vs 112 K empty) | 89 |
| GPU ms back-to-back: tiny repair / 400-px viewport / full repaint / full-texture copy | 0.014 / 0.075 / 0.214 / 0.086 | |

## 8. What surprised me

1. **`nextDrawable` is the most expensive call in the frame**: 237 - 304 K instr, more than all of (b-h)'s in-tick
   work (83 - 154 K). The wgpu/CAMetalLayer path pays that on every present; the IOSurface path replaces it with a
   ~100 K CATransaction commit that runs off the main thread.
2. **Process-wide, (b) only halves (a)**: 419 - 435 K vs 790 K instr per colour change. The Metal submit (~180 K including
   completion) + the compositor commit (~100 K) + the idle display-link tick (60 K) set a floor of ~340 K that no CPU-side
   rewrite of *our* code removes. In-tick (main thread) the gap is much larger (83 K vs 167 K) but moving work to other
   threads does not remove it. Whole-texture blit is *cheap* on this GPU (0.07 - 0.09 ms, 25 - 30 K instr to encode).
3. **A display link tick with nothing to do costs ~60 K instr (7.2 M instr/s at 120 Hz).** Pausing it drops idle to
   0.3 - 0.4 M instr/s (the remaining floor is the process's other timers). "Idle does nothing" needs to mean "the link is
   off".
4. `proc_pid_rusage` costs 5.7 - 8 K instr per call, comparable to a whole encode. Bracketing a 10 K section with two
   snaps is hopeless without subtracting a calibrated overhead; the profile numbers are +-5 K.
5. The compositor holds **two** surfaces, and in ~3 % of frames the third (the one we are about to overwrite) was still
   in use too: a 3-ring has no slack at 120 Hz. `IOSurfaceIsInUse` is a free reuse oracle, so use a 4th surface.
6. CAMetalLayer unthrottled has a **standing queue**: 33 - 40 ms from tick to screen versus 16.7 - 25 ms when we hold one
   present outstanding. The IOSurface path gets the good number by construction (one frame in flight because the CPU
   writes the shared pool).
7. `GPUEndTime - GPUStartTime` once returned 2e8 ms (garbage); filtered. GPU time at a 120 Hz cadence is 5 - 10x
   the back-to-back figure (DVFS), so GPU ms are only comparable within a run.
8. phys_footprint of (a) is **243 MB vs 159 MB** for everything else (RSS is similar): non-`framebufferOnly` drawables +
   retained texture. (Cause not isolated.)
9. Cold runtime shader compile is 270 ms; AOT metallib is 2 ms.
10. A vertex-shader clip clamp (`clamp(p, clip.xy, clip.zw)` on an axis-aligned quad) is an exact clip and needed zero
    fragment cost; the "scroll" scenario relies on it.

## 9. Comparing with zgx

`bench/baselines/TARGETS.md` lists zgx on macOS: one colour write = 121 K instr (~24 K its own + ~83 K Metal), heavy
stream 8.30 M, p99 frame interval 17.6 ms. Those figures come from zgx's harness, whose scope (headless? present
included? process-wide or thread?) I did not reproduce. Our own *floor* for one render pass + commit is 180 K process
wide (section 7), which already exceeds 83 K, so zgx's number must be a narrower scope than process-wide
`ri_instructions` with a window and presentation. Comparable scopes here: **in-tick 83 K** (main thread: update + encode +
commit) vs zgx 121 K; **process-wide 419 K** vs an unknown zgx figure. Before claiming a win, run zgx's colour-write
scene under this crate's `snap()` wrapper (both scopes). Do not quote 83 K vs 121 K alone.

## 10. Five most promising structural changes (reformulation-method framing)

1. **Bend: "the CPU decides what to draw". Representation change: the damage tile plane lives in the pool and the vertex
   shader culls against it.** `instanceCount = N` always; a degenerate-triangle test against the plane bits replaces the
   per-frame region build (9 - 60 K), cull (15 - 22 K) and rect extraction. The command stream becomes constant, which
   enables change 2. Verify with the same byte-exact oracle; cost is N x vertex work on a GPU that is idle anyway.
2. **Bend: "a frame is encoded each time". Replace with a replayable frame (indirect command buffer / argument table;
   Metal 4 command allocators on macOS 26).** Today encode + commit = 53 K of 83 K in-tick and 11 messages. With 1
   executed ICB per frame, per-frame messages drop to ~4 (`commandBuffer`, setTexture on a cached pass, `execute`,
   `commit`). Floor test needed: does an ICB make the 50 K "empty commit" cheaper? (It will not; so measure.)
3. **Bend: "the link ticks, then we check for work". Representation change: damage -> `CFRunLoopSource`/`dispatch` wake +
   `CADisplayLink.paused`.** Idle 7.2 M -> 0.3 M instr/s; sparse updates (one colour per 500 ms) commit directly with
   no tick at all. Also coalesce several writes within a vsync into one epoch (already free: stamps, not flags).
4. **Bend: "publish needs a CA transaction + a completion thread" (100 K + a thread hop, 25-30 % of the process
   total).** Candidates, in order of risk: (i) verify `waitUntilScheduled`/in-order GPU ordering visually (screen
   capture of a slow-GPU frame) and publish from the main thread at no wait cost; (ii) publish via
   `MTLSharedEvent`-signalled main-thread commit at the next tick start (costs a frame of latency, saves the thread);
   (iii) one persistent layer per surface (3 layers, contents set once) and flip visibility, to see whether CA's per-commit
   cost depends on the contents change; (iv) add a 4th surface chosen by `!IOSurfaceIsInUse` so a slow compositor never
   forces a wait.
5. **Bend: "the CPU may only touch the pool when the GPU is idle" (currently 1 frame in flight). Representation change:
   epoch-versioned copy-on-write instance pages (the Bough's pages) so the CPU runs ahead and the GPU reads a frozen epoch.**
   Removes `sync()` from the hot path, allows 2-3 frames in flight without tearing the ring's age accounting, and is the
   precondition for encoding on a render thread (DESIGN section 9). Also try write-combined pool memory and shrinking the instance to 32 B
   (drop `color2`/`uv` into side tables for the 80 % of instances that are plain rects/glyphs).

## 11. Reproduce

```
export CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/metal
cargo build --release --examples   # via ~/.local/share/cargo/bin/cargo
X=$CARGO_TARGET_DIR/release/examples
$X/correctness 400 a,b,bs,bh,c                 # headless byte-exact check, 20 combos
$X/correctness 300 a,b,bs,bh,c stencil,bbox --window
BOUGH_SABOTAGE=1 $X/correctness 100 b bbox    # must FAIL
$X/metal_bench --strategy bh --secs 2 --threads     # a | b | bs | bh | c ; add --profile for phase instr, --sweep for damage modes
$X/metal_bench --startup metallib|runtime
$X/probe 240 0 1 3                            # (c) preservation: frames, framebufferOnly, stride, maxDrawables
$X/latency c 300 3 1                          # presentedTime latency: kind frames maxDrawables maxOutstanding
$X/floor                                      # bare Metal submission floor
```
Note: building adds the objc2 dependency tree to the workspace `Cargo.lock` (untracked, not committed here).

---

# Part II: Opus iteration (reformulation attempts, 2026-11-01)

Same machine, same scene and the same byte-exact oracle. The machine was **loaded** throughout: load average 40-60
from parallel agents, and ioreg "Device Utilization" ~50 % from other processes. Instruction counts are what to
trust; GPU ms and CPU us are noisy. Each process-wide figure was repeated (alternating runs) and moved ±1-2 %
between repeats of the same scenario order, but up to ±15 % between different scenario orders (see 12.9). New
code: `src/v2.rs` (presenter with switchable mechanisms), the `cs_repair` and `vs_vclip` shaders, Zobrist tile
hashes in `src/scene.rs`, and the examples `lab`, `publish`, `gpumodes`, `vclip_test`. Raw data: `results/v2/`.

## 12.0 Harness fixes (before any attempt)

The prototype's "process instr/tick" bracketed every tick with 3-4 `proc_pid_rusage` snaps (5-7 K each), which
inflated it by ~20 K. The prototype's 60 K "idle tick" was about one third measurement. `examples/lab.rs` measures
as follows instead:

* **process-wide instr / frame** = `ri_instructions` delta over a whole scenario / frames. The only snaps are at
  scenario boundaries. Everything is included: link, Metal threads, CA, kernel time on our behalf, and the tiny
  bookkeeping.
* **main-thread in-tick** = `thread_selfcounts` (libsystem_kernel; 1.45 K per call vs 5.1 K, overhead subtracted).
* **publish-thread instr** = `thread_selfcounts` inside the publish itself (whichever thread commits).
* **idle**: the window starts when the display link actually pauses, and the per-thread snapshot is taken before
  the snap.
* **present→commit latency**: `CACurrentMediaTime` at `present()` → after `CATransaction commit` of that frame.
* Scenarios: idle, one-colour, scatter-80, scroll-400, full, plus **toggle** (one instance alternates A/B every tick:
  caret, hover) and **sparse** (one colour change every 100 ms from a timer, i.e. input-like). 1.4 s each, one
  process per strategy, ≤ 11 s per window.
* **WindowServer CPU could not be measured.** `proc_pid_rusage(WindowServer)` returns EPERM; `ps -o time` and `top`
  need an entitlement (also outside the sandbox); system-wide CPU and GPU counters are swamped by the load above.
  Compositor cost is inferred from our own side only.

The prototype winner (b-h) re-measured in this harness is the **"before"** column everywhere below.

## 12.1 Attempt A: idle = zero (KEPT)

* **Rule bent:** "the display link ticks; each tick checks for work". **Representation:** damage *arms* the link.
  The tick that finds no damage and no pending frame calls `[link setPaused:YES]`; a damaging write (event, timer)
  calls `setPaused:NO`.

| idle, 8 s window (`results/v2/lab_idle8_*.txt`) | instr/s | wakeups/s | CPU ms/s |
|---|---|---|---|
| before: link running (b-h) | 5,250,103 | 120.8 | 3.8 |
| **after: link paused** | **104,804** | **1.5** | **0.1** |

* That is 50x fewer instructions and 80x fewer wakeups. 0.1 ms CPU/s = 0.01 % of a core, which shows as 0.0 %
  like zgx.
* The residue (1.5 wakeups/s) is AppKit's NSEventThread, one Metal housekeeping thread and the scenario-ending
  timer. Nothing of ours runs.
* 1.4-s windows read 0.15-0.39 M instr/s because fixed costs are amortised over less time.

## 12.2 Attempt B: present from the event, not from a tick (KEPT)

* **Rule bent:** "frames are produced in display-link callbacks". **Representation:** a sparse change runs the
  whole frame (sync, repair, commit) inside the event handler that caused it. The link stays paused, and the commit
  lands on the next vsync anyway.

| sparse: 1 colour change per 100 ms, cost per change (`results/v2/sparse_isolation.txt`) | process instr |
|---|---|
| before: b-h, link always running (~12 idle ticks per change) | 973,252 |
| link pause + wake the link per change (b-h `--pause`) | 665,213 |
| **direct present from the event** (`cs:h:free:4 --pause --direct`) | **358,872** |

Waking a paused CADisplayLink for one frame costs ~300 K (link restart, 2 ticks, 6.5 wakeups per change) compared
with presenting right away.

## 12.3 Attempt C: the Core Animation commit (floor measured; nothing below ~80 K exists in public API)

`examples/publish.rs` puts a *new* surface on screen once per tick with no GPU work at all. All surfaces hold
identical pixels, so nothing flickers. Process cost is net of a bare display-link tick (`results/v2/publish_*.txt`).

| publish mechanism | committing-thread instr | process instr / publish | verdict |
|---|---|---|---|
| bare CADisplayLink tick (reference) | - | 44.9-46.6 K (whole tick) | - |
| `CATransaction` + `setContents:` new IOSurface, **main thread** | 78-85 K | **+87-105 K** | cheapest real swap |
| same, implicit transaction (run-loop flush) | 7 K + flush | +102 K | same |
| same, background serial queue (what b-h does in its completion handler) | 79 K | +131 K | +30 K for the hop |
| `setContents:` the SAME surface (no change) | 34 K | +41-59 K | fixed cost of any commit |
| `setContentsChanged` (no swap) | 76 K | +97 K | no cheaper than a swap |
| hidden-flip between 4 layers with fixed contents | 56 K | +76 K | **REJECTED: unsafe** (below) |
| 8x6 grid of 240-px tile layers, change 1 tile | 102 K | +145-160 K | REJECTED |
| same grid, change all 48 | 1.78 M | +1.81 M | REJECTED (~37 K per changed layer) |
| CAMetalLayer `nextDrawable` + clear + `presentDrawable` | 104 K | +668 K | REJECTED |
| CAMetalDisplayLink (macOS 14) drawables | 63 K | +700 K | REJECTED |
| AVSampleBufferDisplayLayer + CMSampleBuffer(IOSurface), display-immediately | 27 K | +292 K + a CoreMedia converter thread (60 us) | REJECTED |
| borderless window (3 layers instead of 27) | 78 K | +99 K | no effect: the commit does not walk unchanged chrome |

* **Hidden-flip is unsafe by construction.** With 4 layers holding fixed contents, the compositor keeps *all four*
  surfaces `IOSurfaceIsInUse` permanently (in-use histogram 16/16 samples at 4/4). With a contents swap it holds
  2-3. So no public signal ever says when a hidden layer's surface may be redrawn, and the variant is rejected even
  though it is 25 K cheaper.
* **Multiple small layers for independent regions** (invention candidate, measured): the commit costs ~37 K per
  changed layer and +0.5 K per layer present. Damage spread over many tiles (scatter, scroll) becomes 0.5-1.8 M per
  frame. REJECTED. Its potential compositor-side win could not be measured (12.0).
* **Where to commit** (full pipeline, compute raster, 4-ring; `results/v2/lab_cs_*`):

| publish site | one-colour process instr | present→commit | verdict |
|---|---|---|---|
| CATransaction on Metal's completion thread (`h`) | 388-402 K | 0.34-0.40 ms | **KEPT** |
| completion thread → `dispatch_async(main)` → main commits (`hm`) | 439 K | 0.35 ms | rejected (+45 K) |
| main thread at the next tick, completion polled (`nt`) | 395 K | 8.33 ms | rejected (no saving, +1 frame) |
| main-queue one-shot 0.3 ms after submit, re-polls every 0.25 ms (`tp`) | 530 K | 0.6-2.3 ms | rejected (2-3 extra wakeups/frame) |

The microbench's "main is 30 K cheaper than a background commit" disappears in the real pipeline: Metal's
completion thread wakes for every command buffer anyway, so the handler adds only the commit. **Conclusion: a
publish costs ≥ ~80 K on the committing thread plus ~20 K elsewhere, whichever public route is used. Core
Animation's commit cannot be removed, only kept off the main thread and done once per frame.**

## 12.4 Attempt D: compute tile repair (KEPT as the raster)

* **Rule bent:** "pixels come from the rasterizer inside a render pass". **Representation:** the damage plane *is*
  the dispatch.
  * Each damaged 16x16 tile is one 256-thread threadgroup.
  * The kernel culls the instance array itself in two levels:
    1. one thread per 64-instance page tests the Bough's page ink summary (mirrored into the pool);
    2. the surviving pages are processed 4 at a time (256 instances), compacted in paint order with
       `simd_prefix_exclusive_sum`. The culling thread stages each hit instance's derived record (clip-clamped quad,
       centre, colours) in threadgroup memory.
  * Every pixel blends the staged records in registers and is written exactly once into the IOSurface texture: no
    load action, no store of untouched pixels, no stencil, no rasterizer.
  * The CPU no longer extracts rects, culls a draw list or writes stencil rects. It only lists tile indices into a
    frame slot, or sets a `full` flag.
* Iterations of the kernel (headless, GPU ms full / scroll):
  * v1 flat scan: 0.949 / 0.299
  * \+ page cull: 0.656 / 0.173
  * \+ staged records: **0.511 / 0.177** (`results/v2/gpumodes*.txt`)

Head-to-head on identical damage (`examples/gpumodes.rs`, headless, IOSurface target, modes interleaved per
iteration, 300 iterations; `results/v2/gpumodes_rec.txt`). Each cell is CPU instr (main thread: region build +
cull + encode + commit) / GPU ms median:

| damage (16-px tiles, age-3 union) | bbox | stencil (prototype) | vclip (12.5) | **compute** |
|---|---|---|---|---|
| one colour (18) | 85 K / 0.198 | 89 K / 0.065 | 104 K / 0.018 | **42 K / 0.011** |
| scatter-80 (2,332) | 141 K / 0.286 | 146 K / 0.175 | 211 K / 0.192 | **78 K / 0.159** |
| scroll 400 px (2,375) | 71 K / **0.106** | 71 K / 0.105 | 84 K / 0.139 | 78 K / 0.177 |
| full (10,800) | 53 K / **0.237** | 53 K / 0.236 | 53 K / 0.236 | **40 K** / 0.511 |

* Compute has the lowest CPU at every damage size, and the lowest GPU for small and medium damage.
* **It loses the GPU at large damage: 1.7x at scroll, 2.2x at full** (at display cadence and DVFS clocks: 0.5 vs
  0.35 ms and 1.2-1.4 vs 0.6 ms). Per the method this is a recorded losing regime, not a reason for a second path.
  The fix is structural (12.11 #1).
* Windowed (`cs:h:rr:3`, everything else as b-h): one colour 482 K → **388 K** process, in-tick 83 K → 47 K.
* **phys_footprint 159 → 122 MB**: no render pass is ever encoded. The cause was not isolated; most likely the
  tiler's parameter buffer is never allocated.

## 12.5 Attempt E: vertex-clipped instance stream, "format C on the GPU" (damage-lab follow-up; reference path)

* **Rule bent:** "the CPU splits primitives into damaged runs" (damage lab §5.4). **Representation:**
  * The 16-px plane (90 rows x 128 bits) goes into the pool.
  * The CPU emits only `(instance | first-row << 16)` entries, one per 8 tile rows of an instance that meet damage
    in its x-span.
  * One draw of `entries x 32` sub-quads. Sub-quad (r, k) = the instance quad ∩ tile row r ∩ the k-th damaged run
    of that row inside the instance's span. Runs are found by bit scan in `vs_vclip`; missing rows/runs are
    degenerate. `loadAction = Load`; no stencil, no scissor.
* **Two exactness rules found by the oracle** (`examples/vclip_test.rs` checks inside/outside the plane separately):
  1. **A repair superset must be a function of the tile, never of the instance.** The first version merged a
     row's extra runs per instance, so a panel repainted gap tiles that the button above it did not. That produced
     80,638 bad pixels outside the plane. Fix: the CPU canonicalises each *row* to ≤ 4 runs (fills the smallest
     gaps); the cull and the shader read the same canonical plane.
  2. **Interpolated varyings are not split-invariant.** The fragment shader's `local` was an interpolated varying.
     It is now `position.xy - centre` with `centre` flat, exact for any split, and all render modes use it.
* Result: the lab's prediction holds on this GPU. One-colour GPU time is **0.018 ms vs 0.065 (stencil) vs 0.198
  (bbox)**, because only damaged hardware tiles are touched.
* But the row walk costs CPU: 104-211 K vs 89-146 K for stencil. Compute beats it on both axes at small damage
  and on CPU everywhere. Windowed `vc:h:rr:3`: 492 K one colour, 546 K scatter (b-h: 479 K, 475 K).
* Verdict: **best render-pass path, not kept as the raster.** It stays in the code as the render-pipeline
  reference, and as the fallback design if compute's large-damage gap cannot be closed.

## 12.6 Attempt F: 4th surface chosen by `IOSurfaceIsInUse` (KEPT; ring safety)

* Rule: target ≠ `shown` (the surface last handed to the compositor) ∧ `!IOSurfaceIsInUse(target)`, else defer the
  frame and retry next tick.
* **Proof:**
  * Only `shown` can go from not-in-use to in-use, because we publish nothing else.
  * A surface that is neither `shown` nor in use at selection therefore stays unused until we publish it, and we
    publish it only after its GPU work completes.
  * Every other surface the compositor has seen was superseded by a later commit it has processed (it displayed
    it). If the compositor still reads such a surface, `IOSurfaceIsInUse` says so.
* **Measured:** over all v2 lab runs, **≈12,300 GPU frames + ≈1,300 GPU-skipped publishes: 0 drawn into an in-use
  surface, 0 deferred.** A free candidate was nearly always available (histograms typically
  `[0, 0, 570-720, 170-320, 1]`).
* The prototype's 3-ring round robin drew into an in-use surface in 0-3 of ~885 frames per run (`use_hist` bit 0).
* **Two confounders:**
  * During the completion handler, Metal itself still holds the surface, so a publish-time check reads "in use"
    (5-28 per run, Handler mode only; 0 with next-tick publish).
  * Pre-armed batches (12.8) hold use counts on all bound surfaces. That gives false positives, which are safe.
* Cost: +11 MB (44 MB of surfaces).

## 12.7 Attempt G (invention 1): content-addressed surfaces, Zobrist tile hashes (KEPT)

* **Rule bent:** "damage is a change history; a surface of age a must repaint OR(age)". **Representation:** the
  scene keeps, per 16-px tile, a 64-bit hash (XOR of a hash of each touching instance's full drawing state). Each
  surface keeps the hash vector it was drawn at.
  * If a free surface already holds every pixel of the current state, it is published and **the GPU is skipped**.
  * If the surface on screen holds it, **nothing at all happens**.
  * Colour changes are exactly reversible (XOR out the old state, XOR in the new).
* **Three iterations, recorded:**
  1. Hashing every moved instance on space changes, with a "fewest differing tiles" pick: toggle won, but the tax
     was +290 K/frame on scroll and +110 K on scatter, which breaks the "same mechanism cheapest in the simple
     case" rule.
  2. **Geometry is salted, not hashed.** A move XORs fresh random values into the damaged tiles: one value for the
     old bounds and a different one for the new, so tiles in both still change. Cost is O(damaged tiles), never
     O(instances). A move-and-back is no longer recognised; colour toggles still are. The pick became an
     early-exit "does any surface hold *all* of it".
  3. The target's repair set is the plain history union, with no per-tile hash filter (that filter cost ~35 instr
     per damaged tile). Keeping a surface's hashes in sync is a row-span `memcpy`: tiles in the span but outside
     the repair set already hold equal hashes, by invariant.
* After iteration 3 (full scenario order, 2 runs): **toggle 331 K → 159-162 K per frame** (GPU skipped in 164/168
  frames; −58 % vs b-h's 378 K). The other scenarios are within ±7 % of the same config without hashing
  (`results/v2/final_*`).
* Exact (0 mismatches) and sabotage-detected (63/79 frames fail with ages one epoch too young). Collision
  probability is 2^-64 per tile comparison.
* The damage lab's `sig32` note predicted exactly this niche (oscillation inside the age window). The salted form
  is how it stops taxing everything else.

## 12.8 Attempt H (invention 2): pre-armed command stream released by a shared event (KEPT as throughput mode)

* **Rule bent:** "a frame's command buffer is encoded and committed at frame time". **Representation:** the compute
  kernel reads its target index and tile count from the pool, so the command stream is constant.
  * K frames are pre-encoded into one command buffer, each as `wait(ev ≥ 2f−1)`, indirect dispatch from slot
    f%2, `signal(ev = 2f)`.
  * A frame is then: write the tile list and target into the slot, then `ev.signaledValue = 2f−1` (one message).
  * The next batch is encoded once per K frames.
* **Main thread:** in-tick 47 K → **22-24 K**.
* **Process-wide depends entirely on how GPU completion is observed:**

| completion observed by | one-colour process instr | present→commit |
|---|---|---|
| `notifyListener:atValue:` + commit on the listener thread (`arm8:h`) | 388 K (= compute + handler) | 0.41 ms |
| main-queue timer poll (`arm32:tp`) | 484 K | 0.6 ms |
| **the next display-link tick polls `signaledValue`** (`arm8/32:nt`) | **274-278 K** (short order), 343 K (full order) | 8.33 ms |

* **Finding:** Metal's per-frame floor is **per command buffer**. One of the two ~60-us Metal threads disappears
  from the per-thread profile with batching. Being told "the GPU is done" costs ~60 K process-wide whichever way it
  is delivered (completion thread, listener, timer). The only free observer is a tick that happens anyway, at +1
  frame of latency.
* So this is the right mode for continuous animations whose frames are not input-driven (−7..−18 % vs the
  interactive mode), and the wrong one for input.
* It also makes `IOSurfaceIsInUse` over-report (12.6).
* Metal never aborted a batch that waited on its event for seconds: 0 batch errors, including the 1.4-s idle
  phases.

## 12.9 Observed regime (not explained): the first GPU submissions after a GPU-silent stretch

* After the toggle scenario (1.4 s of CA-only frames, zero GPU submissions), the following sparse changes cost
  0.95-1.3 M each instead of ~360 K. A Metal driver thread spent 185-340 us per change instead of ~65, and in one
  run our own main-thread and commit work doubled too.
* Reproduced 4/4 times after toggle and 0/3 times after one-colour or full (`results/v2/sparse_isolation.txt`).
* It matters for "idle = zero": the price of idling may be paid at the next wake. **Next measurement:** cost and
  latency of the first 10 frames after N seconds of GPU silence.

## 12.10 Before / after (same harness, same scene; before = b-h, after = recommended design)

* **after** = compute repair + completion-handler commit + content-addressed 4-ring + link paused when idle +
  direct present from events (`lab --strategy cs:h:z:4 --pause --direct`).
* Two alternating runs each (`results/v2/final_{1..4}_*.txt`); ranges are the two runs. GPU ms are display-cadence
  medians.
* zgx-like control (a) in the same harness for scale (`final_5_a.txt`).

| scenario | before: process instr/frame (in-tick, GPU ms) | **after: process instr/frame** (in-tick, GPU ms) | Δ process | control (a) |
|---|---|---|---|---|
| **idle** (8 s) | 5.25 M instr/s, 120.8 wakeups/s, 3.8 ms CPU/s | **0.105 M instr/s, 1.5 wakeups/s, 0.1 ms CPU/s** | **−98 %** | 5.85 M/s |
| one colour | 482-488 K (83 K, 0.12-0.15) | **400-402 K** (51.5 K, 0.031-0.033) | −17 % | 926 K |
| 80 scattered | 474-475 K (155 K, 0.35-0.43) | **420-422 K** (137 K, 0.53-0.59) | −11 % | 888 K |
| scroll 400 px | 446-447 K (127 K, 0.34-0.35) | **434-435 K** (147 K, 0.51-0.57) | −3 % | 887 K |
| full redraw | 377-379 K (59 K, 0.61) | **348-350 K** (63 K, 1.20-1.37) | −8 % | 798 K |
| toggle A/B (caret, hover) | 378-379 K (60 K) | **159-160 K** (90 K = commit on main; GPU skipped) | **−58 %** | 804 K |
| sparse 10/s (per change, isolated order) | 973 K | **359-363 K** | **−63 %** | 1.43 M |
| phys_footprint | 159 MB | **133 MB** (4 surfaces; 122 MB with 3) | −16 % | 241 MB |
| frame interval (tick) med / p99 | 8.33 / 8.33 | 8.33 / 8.33 (commit interval p99 8.6-9.0) | = | 8.33 / 14.5 |
| present → commit | (prototype: tick → GPU done 0.44 ms) | 0.38-0.40 ms one colour, 1.5-1.7 ms full | | |

* Throughput mode (`arm32:nt:z:4`, +8.33 ms of latency), full order: 343 / 365 / 377 / 325 K and toggle 166 K;
  274-278 K one colour in a short order.
* **Where one colour change's 400 K goes now:**
  * our main thread 51 K (sync poll, scene write, pick, tile list, 10 messages)
  * the CA commit on the completion thread 84 K
  * the display-link tick ~45 K
  * two Metal driver threads, ~60 us CPU each, most of the remaining ~220 K
* **Our own code is 13 % of the frame. The rest is OS and driver**, which is why continuous scenarios moved only
  3-17 % while idle, toggle and sparse (where whole stages vanish) moved 58-98 %.

## 12.11 Recommended presenter design and its per-frame call sequence

* One mechanism for all damage sizes. A frame is triggered by a display-link tick while something animates, or
  directly by the event that damaged the scene; the link is paused otherwise.
* Surfaces: 4 IOSurface-backed BGRA8 textures (`RenderTarget|ShaderRead|ShaderWrite`) and one plain `CALayer`.

```
tick (or event handler), main thread
 0 [prevCmd status]                         poll; waitUntilCompleted only if the GPU is late (1 msg)
   scene writes: 4 B per colour + Zobrist delta XOR into <= 4 tile hashes; moves: one salt XOR per damaged tile
   epoch = hist.push(acc)                   (16-epoch ring of 16-px tile planes)
   pick: zero_diff(shown)?                  -> return: NOTHING else happens (no GPU, no CA)
         for s in free (s != shown && !IOSurfaceIsInUse(s)):    <= 3 IOSurfaceIsInUse calls
             zero_diff(s)? -> publish s (4 CA msgs on main), no GPU, done
         target = first free (none free -> defer, retry next tick)
   plane = OR(hist since target.epoch); write tile list (or `full` flag) into pool slot f%2
 1 [queue commandBuffer]
 2 [cmd computeCommandEncoder]
 3 [enc setComputePipelineState: cs_repair]
 4 [enc setBuffer: pool offset:0 atIndex:0]          (header, spaces, clips, pages, instances, slots)
 5 [enc setBytes: &slotOffset length:4 atIndex:1]
 6 [enc setTextures: {atlas, target x4} withRange:0..5]
 7 [enc dispatchThreadgroups:(ntiles,1,1) threadsPerThreadgroup:(16,16,1)]
 8 [enc endEncoding]
 9 [cmd addCompletedHandler: ^publish]
10 [cmd commit]                                      => 10 messages; in-tick 47-52 K instr (one colour)
   nothing damaged and nothing pending -> [link setPaused:YES]
Metal completion thread (^publish)
11 [CATransaction begin]  12 [CATransaction setDisableActions:YES]
13 [layer setContents: target.iosurface]  14 [CATransaction commit]   => ~84 K instr
```

Next structural steps, in order:

1. **Close compute's large-damage GPU gap** (scroll 1.7x, full 2.2x vs the rasterizer). Use 32x32 threadgroup
   tiles at 4 pixels per thread, which amortises the two-level cull 4x and matches the hardware tile (damage-lab
   §1.4), with a per-tile-row binning pass once per frame. The target is the render path's 0.24 ms full.
2. **One publish trigger that is free.** Use the throughput mode's observer (the next tick) only while the link is
   running anyway, and the completion handler otherwise. Make that a property of the scheduler, not a branch in
   the presenter. Measure input-to-photon with it.
3. **Explain 12.9** (the first submissions after GPU silence), and try Metal 4 command allocators and residency
   sets (macOS 26) against the ~60 K per-command-buffer floor that now dominates.

Reproduce (in addition to section 11):

```
X=$CARGO_TARGET_DIR/release/examples
$X/correctness 200 a,b,bs,bh,c,vc:h:rr:3,vc:nt:free:4,cs:nt:z:4,arm8:nt:free:4 bbox,stencil   # 0 mismatches
$X/correctness 120 cs:h:z:4,arm32:nt:z:4,vc:h:free:4 bbox --window                           # windowed, 0
BOUGH_SABOTAGE=1 $X/correctness 100 cs:h:z:4 bbox                                             # must FAIL
$X/vclip_test                                       # vclip inside/outside-plane proof, 6 cases
$X/gpumodes 300                                     # bbox | stencil | vclip | compute head-to-head
$X/publish 1.1 tick-only,main-explicit,bg-explicit,hidden-flip,grid-1,metal-layer,avsbdl   # CA floor
$X/lab --strategy bh                                # before
$X/lab --strategy cs:h:z:4 --pause --direct         # after (raster:publish:pick:n; rp|vc|cs|armK : h|hm|nt|tp : rr|free|z : 3|4)
$X/lab --strategy cs:h:z:4 --pause --scn idle --secs 8
```

---

# Part III: second Opus round (2026-11-01)

* **Same rules:** one mechanism; byte-exact against a full redraw through the same mechanism; sabotage must fail;
  losing results kept.
* **Same machine, same load:** other agents kept the GPU ~50 % busy. GPU ms are noisy run to run; ratios inside one
  interleaved run, and p10s, are what to read.
* **New code:**
  * `cs_repair32`, `cs_bin32` and `cs_tile32b` in `shader.metal`;
  * the bough-core page mirror and bough-text runs in `scene.rs`;
  * residency + keep-warm in `v2.rs`;
  * new examples `runs_check`, `wake`, `opcost`;
  * `INTERFACE.md`.
* Raw data: `results/v2/r2_*`.

## 13.1 Compute repair that wins GPU time at every damage size (KEPT: binned 32x32)

Round 1's 16-px compute kernel lost the GPU 1.7x on scroll and 2.2x on full redraw. Chain of attempts, each one a
representation change, all exact (correctness + sabotage after each):

| step | rule bent / representation | full / scroll GPU ms (stencil in the same run) | verdict |
|---|---|---|---|
| v3 | 32x32 threadgroup tile (the hardware tile and bough-core's damage unit), 4 px/thread, half-precision accumulation; cull reads **bough-core's own page layout**: directory union → `gink` (8-record groups) → record `ink`, NRect threshold tests | 0.414 / 0.206 (0.307 / 0.186) | progress |
| diag | `debug` modes: write-only 0.073-0.079, cull-only 0.19-0.22, i.e. shading ≈ 0.2 ms with only 3.5 records staged per tile | | shading cost per evaluation, then culling |
| v4 | colours premultiplied **once per record** (staged), thread owns a 2x2 block (one reject test for 4 px), **opaque floor**: shading starts at the last opaque solid record covering the whole tile (exact: blending over alpha 1 is a bitwise overwrite) | 0.370 (0.275) | same ratio |
| v5 | **core box**: per record, the box where coverage is provably 1 (solid: the quad; rrect/border: centre ± (half − max(r, 0.5 + bw) − 1)); blocks inside take the colour with no SDF. Staging capped at 64 records per step (~6 KB of threadgroup memory → occupancy) | 0.383 / 0.174 (0.348 / 0.182) | scroll won, full −10 % |
| **v6** | **row binning**: pass 1, one threadgroup per damaged 32-px tile ROW, runs the hierarchical cull once and appends hits (instance + ink) to the row's bin in paint order; pass 2, each tile scans only its row's bin (128 per step, 3 barriers). Two dispatches, one encoder, one buffer barrier | **0.188-0.229 / 0.082-0.119** (0.248-0.308 / 0.123-0.184) | **KEPT** |
| v7 | per-tile lists built in the bin pass (one thread per tile column scans the row bin sequentially) | 0.29 / 0.14 (0.31-0.35 / 0.18); text-heavy 1.5x stencil | REJECTED: the sequential scan serialises the bin pass |

Head-to-head, default scene (`gpumodes`, headless, modes interleaved; `r2_gpumodes_binned_*.txt`,
`r2_gpumodes_final.txt`). Cells are CPU instr (encode+commit, main thread) / GPU ms median:

| damage | stencil (render) | compute16 (round 1) | **binned32** |
|---|---|---|---|
| one colour | 89 K / 0.090-0.137 | 42 K / 0.015-0.026 | **46 K / 0.020-0.054** |
| scatter-80 | 146 K / 0.198-0.224 | 78 K / 0.234 | **67 K / 0.123-0.130** |
| scroll 400 px | 72 K / 0.123-0.184 | 78 K / 0.201-0.302 | **60 K / 0.082-0.119** |
| full | 53 K / 0.248-0.308 | 40 K / 0.667 | **44 K / 0.188-0.229** |

* **On the dashboard scene, binned32 now wins both CPU and GPU at all four damage sizes, with no damage-size
  switch.** The single exception is the 16-px kernel's one-colour GPU time: 32-px tiles repaint 4x the pixels of a
  glyph-sized damage.
* At display cadence (DVFS): one colour 0.066 vs 0.14-0.15 ms (b-h), scroll 0.24 vs 0.34, full 0.20-0.73 vs
  0.58-0.60.

## 13.2 Text runs expanded on the GPU (bough-text iteration 2 format) (KEPT)

* **Rule bent:** "the draw list holds glyph instances". **Representation:**
  * A text run is ONE core record: kind RUN, payload = run | bit 31, ink = the run's union.
  * Glyphs stay in bough-text's 6-byte SoA pool (`x: i32`, `ent: u16`) behind a 32-byte window header.
  * The bin pass culls runs by their record ink. It expands each hit run cooperatively (256 glyphs per step,
    bough-text's `floor(fma(float(x − x0), scale, origin.x + 0.5)) + geo` plus the space and the space's clip),
    tests every glyph against the row band, and appends hits to the row bin in paint order.
  * So a run is expanded at most once per damaged tile row and shared by all the row's tiles. The tile pass stages
    glyph entries like instances.
* **Heavy text load** (`BOUGH_HEAVYTEXT=1`):
  * 84 numeric labels of 8 glyphs (in the cards; a tick changes the last 3 digits of every label);
  * a 5-line log pane of 85 glyphs per line (a tick writes one new line into a ring of slots and re-points 5
    window headers);
  * plus all the dashboard's table cells and labels.
  * Total: 2,309 glyphs in 232 runs. The GPU directory holds **487 records instead of 2,564**.
* **Exactness:**
  * `runs_check`: the same scene state rendered with the mirror in instance mode and in runs mode is
    **byte-identical** (60 mixed steps).
  * Partial repairs in runs mode are exact (182 frames, 3 strategies), and sabotage fails.
* **Cost of the expansion: none measurable.** Runs mode vs instance mode through the same kernels
  (`r2_gpumodes_heavytext_*`), GPU ms:

| heavy-text damage | stencil (glyph instances) | binned32 (glyph instances) | **binned32 (runs, GPU-expanded)** |
|---|---|---|---|
| text tick (84 labels + log line) | 0.118-0.133 | 0.194-0.197 | **0.171-0.193** |
| one colour (a whole label) | 0.063-0.066 | 0.068 | 0.083 |
| scatter-80 | 0.199-0.200 | 0.183-0.188 | 0.185-0.189 |
| scroll 400 | 0.107-0.109 | 0.075-0.076 | 0.081-0.082 |
| full | 0.268 | 0.274-0.276 | 0.276-0.278 |

* CPU per encode+commit is identical in both modes (45-71 K, vs 54-143 K for stencil).
* **Losing regime (recorded):** on glyph-dense partial damage (the text tick), binned compute is ~1.45x stencil on
  the GPU, and full redraw only ties. A tile in a log-pane row scans ~230 row-bin entries to find ~5 glyphs. The
  tile-list fix (v7) failed. The next structural step is 2D binning with a *parallel* scatter: count + prefix +
  scatter per (row, column chunk), not a per-tile sequential scan.
* Windowed, heavy text (`r2_text_*`), process instr per frame:

| heavy text | b-h | binned design |
|---|---|---|
| text tick | 474-560 K | **456-458 K** |
| one colour | 417-423 K | **356-359 K** |
| full | 378-381 K | **357-360 K** |

  GPU at cadence: text tick 0.47 vs 0.31 ms, full 0.8-0.9 vs 0.65 ms (the losing regime above). In this prototype
  both the instance and the run representation are written on every text op (the render paths need instances), so
  the text tick's CPU includes both writers.

## 13.3 The post-idle wake cost: explained and (mostly) killed

**Headless reproduction** (`examples/wake.rs`): warm 2 s at 120 Hz, idle D, then 10 changes at 10 Hz
(`r2_wake.txt`).

| idle D | first change after idle | its present→commit | changes 2-10 |
|---|---|---|---|
| 0.1-0.5 s | 216-229 K | 0.3-0.5 ms | 226-234 K |
| **1.5-3 s** | **9.9-10.0 M** | **3.1-4.1 ms** | **565-650 K** |

* **Explanation:**
  * Between 0.5 and 1.5 s of no GPU work, the GPU powers down.
  * The first submission afterwards costs ~10 M instructions in our process. These are counted to our threads,
    presumably the kernel driver polling while the GPU powers back up.
  * It also adds ~3-4 ms of latency.
  * The driver then stays in a slow state at 10 Hz submission rates: 2.5x per frame for the following changes.
* Not Core Animation: no window, no CA commits involved.
* In round 1's lab this produced 0.95-1.3 M per sparse change after the GPU-silent toggle phase.

| remedy (headless, same protocol) | first change after 1.5 / 3 s | changes 2-10 | idle cost |
|---|---|---|---|
| none | 9.9 / 10.0 M | 565-650 K | ~0.05 M/s |
| `MTLResidencySet` (pool, atlas, bins, surfaces) on the queue | 7.6 / 8.5 M | **222 K** | 0 |
| empty command buffer every 100 / 500 ms | 10-16.5 M | 540-620 K | 0.3-1.1 M/s: **useless**, no GPU work runs |
| real work (4-byte blit) every 250 ms, no residency set | 2.3-2.6 M | 540-625 K | 0.4-0.8 M/s |
| pre-armed command stream (event release) | 11.9-12.8 M | 250 K | 0 |
| **residency set + real 4-byte blit every 800 ms** | **0.49 / 0.47 M** | **230 K** | ~0.3 M/s (headless) |

* **Kept:** a residency set, always on (it removes the lingering slow state for free), plus **bounded keep-warm**
  (`BOUGH_KEEPWARM=800,10`): a 4-byte blit whenever the GPU has been idle 0.8 s, but only within 10 s of the last
  real frame. After that the GPU is allowed to sleep and the next wake pays the 10 M once.
* Windowed (`r2_wake_lab.txt`), sparse changes after the GPU-silent stretch:
  * **416-460 K per change with keep-warm vs 949-971 K without** (one blit fired);
  * keep-warm's idle cost inside its window: **0.54 M instr/s** at 800 ms (1.29 M/s at 250 ms, 0.81 at 500), vs
    0.105-0.137 M/s idle floor.
* **Break-even:** keep-warm costs ~0.4 M/s above the floor, so it is worth it for idle gaps up to ~25 s. With a
  10-s window the worst case is +4 M per long idle period; the best case saves ~9.5 M and ~3 ms of input latency
  per short pause.

## 13.4 Final before / after (same harness, same scene code, alternating runs; `r2_final_*`)

* **Before** = b-h (round-0 winner).
* **After** = the recommended design: binned compute, completion-handler commit, content-addressed 4-ring, link
  paused when idle, direct present from events, residency set, bounded keep-warm (`lab --strategy cb:h:z:4 --pause
  --direct`, `BOUGH_KEEPWARM=800,10`).
* Note: this round's prototype scene costs more per scroll write (`op_scroll` ≈ 102 K vs 49 K; it recomputes 924
  clipped inks and mirrors them). Both columns pay it. In the real Bough a translation is a NEON pass over the ink
  column.

| scenario | before: process instr/frame (GPU ms at cadence) | **after** | change |
|---|---|---|---|
| idle (8 s) | 5.25-5.41 M instr/s, 121 wakeups/s, 3.8-4.7 ms CPU/s | **0.105-0.137 M/s**, 1.5 wakeups/s, 0.1 ms CPU/s (0.54 M/s inside the 10-s keep-warm window) | −97..−98 % |
| one colour | 481-491 K (0.14-0.15) | **346 K** (0.066) | **−29 %** |
| 80 scattered | 477 K (0.41-0.42) | **418 K** (0.37-0.39) | −12 % |
| scroll 400 px | 533 K (0.33-0.35) | **503-505 K** (0.24-0.25) | −5 % |
| full redraw | 379 K (0.58-0.60) | **357-359 K** (0.20-0.51) | −6 % |
| toggle A/B | 379 K | **163-164 K** (GPU skipped) | **−57 %** |
| sparse 10/s | 976-980 K | **386-401 K** | **−60 %** |
| phys_footprint | 159-160 MB | **140 MB** | −12 % |
| frame interval med / p99 | 8.33 / 8.33 | 8.33 / 8.33 (commit p99 8.6-9.1) | = |
| present → commit | (not instrumented) | 0.37-0.45 ms one colour, 0.55-0.95 full | |

## 13.5 Recommended presenter, round 2: call sequence

Same as 12.11 with two changes.

**The raster is two dispatches in one compute encoder:**

```
 1 [queue commandBuffer]
 2 [cmd computeCommandEncoder]
 3 [enc setBuffer:pool offset:0 atIndex:0]     4 [enc setBytes:&slotOffset length:4 atIndex:1]
 5 [enc setBuffer:bins atIndex:2]              6 [enc setBuffer:binCounts atIndex:3]
 7 [enc setComputePipelineState:cs_bin32]      8 [enc dispatchThreadgroups:(nrows) threadsPerThreadgroup:(256)]
 9 [enc memoryBarrierWithScope:Buffers]
10 [enc setComputePipelineState:cs_tile32b]   11 [enc setTextures:{atlas, target x4} withRange:0..5]
12 [enc dispatchThreadgroups:(ntiles) threadsPerThreadgroup:(16,16)]
13 [enc endEncoding]   14 [cmd addCompletedHandler:^publish]   15 [cmd commit]
```

* The CPU side of a frame: OR of the age window's 32-px planes → row-major tile list + row spans into the frame
  slot (O(damaged tiles); `full` flag for a redraw).
* **Setup gains** one `MTLResidencySet` (pool, atlas, bins, surfaces) on the queue.
* **Idle gains** a 0.8-s keep-warm blit while within 10 s of the last frame.
* The input contract (bough-core pages and directory, bough-text runs, frame slots) is `INTERFACE.md`.

Next structural steps:

1. 2D binning with a parallel count/prefix/scatter, for glyph-dense partial damage (13.2's losing regime).
2. 16-px sub-tile masks inside a 32-px tile (write only damaged quadrants) to recover the 16-px kernel's
   one-colour GPU time.
3. Map bough-core's `PagePool` arena and bough-text's `GlyphPool` into the pool buffer for real. The integration
   is then pointer arithmetic, and the prototype's mirror pass and double text representation disappear.

Reproduce (in addition to 11 and Part II):

```
$X/gpumodes 300                                           # default scene: stencil | vclip | compute16 | compute32 | binned32
BOUGH_HEAVYTEXT=1 $X/gpumodes 200                         # + text-heavy pattern, binned32 runs vs instances
BOUGH_HEAVYTEXT=1 $X/runs_check 60                        # GPU-expanded runs == glyph instances, byte for byte
BOUGH_HEAVYTEXT=1 BOUGH_RUNS=1 $X/correctness 200 cb:h:z:4,cb:h:free:4,ab8:nt:free:4 bbox
BOUGH_SABOTAGE=1 $X/correctness 100 cb:h:free:4 bbox      # must FAIL
$X/wake cb:h:free:4 none|residency|warm100|rblit800       # post-idle wake cost per remedy
BOUGH_KEEPWARM=800,10 $X/lab --strategy cb:h:z:4 --pause --direct
BOUGH_HEAVYTEXT=1 BOUGH_RUNS=1 $X/lab --strategy cb:h:z:4 --pause --direct --scn text,one-colour,full
$X/opcost                                                 # scene write costs (prototype scene)
```

---

# Part IV: the per-frame floor, GPU time and the unmeasured columns (round 3, branch `floor`)

Same M3 Pro (macOS 15.7.4), `dist` binaries, load average 15–70 from three other agents' builds throughout:
instruction counts are the figures to read, CPU % and GPU time are context. "Before" is `canonical` 9c61b75 (the
fused system of STATUS.md) built in this worktree; zgx is 06debe9d's `dist`, as in STATUS. Raw output:
`bench/results/floor/`.

```
export RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/floor
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo test --workspace --release                       # every gate, incl. present.rs, gpu_bounds.rs, edges.rs
cargo build --profile dist -p bough-heavy -p bough-scenes -p bough-vlist -p bough-zgui
bench/floor/build.sh                                   # gputime.dylib (headless GPU time), macsample (zgx's sampler)
bench/results/floor/run_slopes_bough.sh                # instructions per frame, R0/R1, every scene
bench/results/floor/run_gpu_slopes.sh                  # GPU ms and Kcycles per frame, headless, bough + zgx
AFTER=<dist dir> BEFORE=<dist dir> bench/results/floor/run_windowed_gpu.sh   # 10 s windows + GPU column (takes the lock)
target/floor/dist/drag_probe                           # zgx's SAME_WINDOW drag probe (take the lock)
target/floor/release/examples/fprobe 3000 <modes>      # the submission floor, per thread
ZGX_MODE=stream target/floor/release/kernel_bench      # kernel A/B at a saturated GPU clock (DEBUG=0,1,2)
```

## 14.1 GPU time for both frameworks, without Screen Recording or sudo (item 1)

* **What zgx's runner uses.** `bench/windowed/macsample.swift` (zgx's own sampler) reads the GPU time of a process
  from the I/O Registry: every `IOUserClient` of the accelerator has an `AppUsage` array whose
  `accumulatedGPUTime` (ns) is what Activity Monitor shows, keyed by `IOUserClientCreator` = "pid N". The GPU's
  mean active clock comes from IOReport ("GPU Stats", GPUPH residency × pmgr `voltage-states9` frequencies), and
  Mcycles/s = GPU seconds × MHz / wall. **None of it needs root or Screen Recording**: only zgx's frame-pacing
  capture (ScreenCaptureKit) and its preflight (System Events) do. Built with `swiftc -lIOReport`, it runs here.
* `task_info(TASK_POWER_INFO_V2).gpu_energy.task_gpu_utilisation` is **0 on Apple silicon** (checked), so it is
  not an option. `proc_pid_rusage` has no GPU field.
* **Headless:** `bench/floor/gputime.c` is a dylib injected with `DYLD_INSERT_LIBRARIES` into any Metal process
  (bough's or zgx's unmodified binaries): a thread polls the accelerator's children for the process's clients every
  2 ms (a client's accounting vanishes when the process releases its device, which wgpu does before exit, so the
  last value per client is kept) and prints GPU seconds and the run's mean GPU MHz at exit. `gpu_slope.py` turns it
  into GPU ms and Kcycles per frame by the slope method. The poller costs CPU, so instruction counts are never read
  from these runs.
* **Windowed:** `bench/floor/windowed_gpu.py` runs a window under the lock and samples it with macsample from 1.5 to
  9 s: CPU %, instructions, footprint, GPU ms/s, MHz, Mcycles/s, window server CPU/GPU %.
* **Caveat, measured:** GPU time is dominated by the clock. Back-to-back headless frames ran at 338–1,140 MHz here
  (other agents' GPU load moves it), so the same binary's GPU ms per frame moved 2× between repetitions; Kcycles
  normalise this only roughly (the MHz is the whole GPU's). At a saturated clock (`kernel_bench`) the same
  repairs cost a quarter of their cadence time.

GPU column, windowed (10 s, macsample; one repetition per cell, interleaved; `windowed_gpu.txt`), and headless
(slope, median of 3; `gpu_slopes_*.txt`):

| scene / mode | headless GPU Kcycles/frame zgx / **bough** | headless GPU ms/frame zgx / bough before / **bough after** | windowed GPU Mcycles/s zgx / before / **after** | windowed GPU ms/s zgx / before / **after** |
|---|---|---|---|---|
| heavy stream | 446 / **359** | 0.989 / 0.715 / **0.412** | 20.5 / 19.9 / **16.7** | 27.4 / 17.6 / **34.0** |
| heavy scroll | 268 / **303** | 0.783 / 0.648 / **0.351** | 12.9 / 30.4 / **21.0** | 32.6 / 31.9 / **30.5** |
| heavy both | 627 / **502** | 1.459 / 0.505 / **0.850** | 40.4 / 38.6 / **32.9** | 60.3 / 53.9 / **41.1** |
| lab stream | 63 / **110** | 0.289 / 0.334 / **0.451** | 17.5 / 21.2 / **23.3** | 32.7 / 31.4 / **34.3** |
| lab scroll | 123 / **128** | 0.352 / 0.376 / **0.354** | 22.2 / 25.9 / **25.8** | 38.7 / 48.8 / **41.2** |
| lab both | 134 / **223** | 0.398 / 0.476 / **0.365** | 22.7 / 30.2 / **26.1** | 36.4 / 51.9 / **75.6** |
| animate - | 97 / **72** | 0.272 / 0.253 / **0.217** | 14.5 / 26.4 / **26.3** | 37.9 / 69.4 / **36.8** |
| vlist scroll | 203 / **316** | 0.600 / 0.715 / **0.419** | 22.7 / 28.4 / **27.5** | 40.5 / 45.0 / **44.1** |
| vlist fling | 208 / **329** | 0.615 / 0.523 / **0.407** | 23.4 / 27.8 / **18.5** | 34.6 / 40.7 / **45.2** |
| vlist jump | 203 / **309** | 0.606 / 0.632 / **0.453** | 16.9 / 20.6 / **29.1** | 38.8 / 56.1 / **52.7** |
| blink - | – | – | 0.4 / 0.0 / **0.0** | 1.0 / 0.0 / **0.0** |

Reading (the first GPU column for either framework):
* **bough wins** heavy stream and both (359 vs 446 and 502 vs 627 Kcycles a frame headless; 16.7 vs 20.5 and 32.9
  vs 40.4 Mcycles/s windowed: zgx repaints 2.67 Mpx a tick, bough ~1.1 M) and animate headless (72 vs 97 Kcycles).
* **bough loses** glyph-dense and scroll-dense cells: lab stream 110 vs 63 Kcycles (1.7×), lab both 223 vs 134,
  vlist 309–329 vs 203–208 (1.5×); heavy scroll and lab scroll tie. Windowed, animate is 26.3 vs 14.5 Mcycles/s.
* Run-to-run spread under the external GPU load is ±30 % (the clock moved 338–1,143 MHz between runs of one binary),
  so only the ~1.5× gaps are clear. Where the time goes is 14.6: the tile pass's bin scan in text rows, the regime
  RESULTS 13.2 already recorded as losing.

## 14.2 The per-frame floor: one pre-armed, bindless stream for every frame, published off the input thread (item 2)

### What a frame cost, per thread (`fprobe`, headless, one damaged tile, empty scene; `threads::instr_snapshot`)

Per-thread instructions come from `proc_pidinfo(PROC_PIDTHREADCOUNTS)` (also from outside: `bench/floor/threads.py`).

| mechanism (per frame) | process | main | driver threads |
|---|---:|---:|---|
| command buffer + encoder + commit, wait next frame (before: every RENDER=1 frame) | 196 K | 60 K | 69 K + 68 K (submission thread in `IOGPUCommandQueueSubmitCommandBuffers`, completion thread) |
| pre-armed batch of 8, event-released, surfaces bound (before: animate's throughput mode) | 134 K | 56 K | 73 K |
| **pre-armed batch of 16, bindless encoders** | **101 K** | 45 K | 52 K |
| (nop kernel) batch of 8 encoders, no events | 26 K | 10 K | 16 K |
| (nop kernel) event-released, signal + wait per frame | 103–107 K | 42–45 K | 54 K |
| (nop kernel) event-released, no per-frame signal, no wait | 71–75 K | 30–33 K | 33 K |
| `setSignaledValue` with no waiter / `signaledValue` read / wait on a reached value | 2.8 K / 72 / 69 | | |

Findings (`sample` of the stream): a GPU wait on a CPU-signalled shared event is resolved **in the kernel, on the
submission thread**: `IOGPUCommandQueueSubmitCommandBuffers` blocks inside the trap until the value arrives and then
submits the next segment. So every event-released frame still pays one kernel segment submission (~50 K), and per
command buffer ~180 K. Binding IOSurface textures to the pre-armed encoders cost a further ~20 K a frame in that
path and made every pending frame hold every surface "in use".

### The mechanism kept

1. **Every frame goes through the pre-armed stream** (`fused.rs`): K = 16 frames encoded ahead into one command
   buffer, each `wait(ev ≥ 2f−1) → bin, tile → signal(ev = 2f)`. A frame is: write its slot, `ev.signaledValue =
   2f−1`. The GPU starts at the release: no encoder, commit or submission on the frame's critical path, which is
   lower latency than encoding.
2. **Bindless encoders.** A pre-armed frame binds only the region and its slot offset. Surfaces and the atlas are
   Metal 3 resource ids, bins and counts GPU addresses, in a bind table in the region header (`FBind`), all in the
   queue's residency set. The tile pass writes `t[target]`. Consequences: −25 % per frame (134 → 101 K), pending
   frames name no surface (`IOSurfaceIsInUse` now reports only the compositor: `armprobe`), and growing the bins is a
   header write (the old drain/re-arm on bin growth is gone).
3. **The publisher thread** (windowed). The input thread writes the slot, signals, and posts (surface, event value)
   into a one-job mailbox. The publisher waits for the event, checks the frame's bin-overflow word, and commits the
   surface in its own explicit Core Animation transaction (with the layer's new frame after a resize). It also keeps
   the stream armed (`Kern`, the encoder objects and the arming state under one lock), so **no encode, commit, GPU
   wait or CA transaction runs on the input thread**. A job posted while another is queued supersedes it. The
   surface queued, being committed, or committed last counts as in use for the ring (the compositor's own flag lags a
   commit by up to one server turn), which keeps RESULTS 12.6's proof intact. An overflowed frame is held back; the
   next `sync` grows the bins, redraws it and posts it again.
4. **Residency growth drains the stream.** Found by the resize gate (flaky black frames): **a residency set's new
   allocations do not reach command buffers already submitted**, and a pre-armed batch is submitted long before its
   frames run. So when a surface is first used, after a resize, or when the bins or atlas grow, the pending frames
   are released with empty slots and a new batch is armed (`residency_changed`, at most a few times a window life).
   The first version of this round had the race latent on a surface's first use.

### Results

Headless, per frame, slope method (`slopes_before.txt` / `slopes_after.txt`, same machine state, bough only; zgx
from STATUS §1.1):

| scene / mode | zgx R=0 | before R=0 | **after R=0** | zgx R=1 | before R=1 | **after R=1** | Metal floor (R1−R0) before → after | × zgx R=1 |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| heavy stream | 2,982,643 | 180,237 | **194,005** | 6,190,927 | 381,678 | **319,232** | 201,441 → 125,227 | 19.4 |
| heavy scroll | 161,932 | 26,510 | **26,237** | 1,443,768 | 229,956 | **134,454** | 203,446 → 108,217 | 10.7 |
| heavy both | 3,153,118 | 216,118 | **222,228** | 6,896,602 | 442,964 | **317,691** | 226,846 → 95,463 | 21.7 |
| lab stream | 754,172 | 26,687 | **27,819** | 1,061,953 | 231,749 | **130,623** | 205,062 → 102,804 | 8.1 |
| lab scroll | 84,489 | 10,112 | **9,777** | 649,136 | 212,062 | **99,554** | 201,950 → 89,777 | 6.5 |
| lab both | 813,004 | 35,058 | **33,779** | 1,408,099 | 258,022 | **146,138** | 222,964 → 112,359 | 9.6 |
| animate - | 226,715 | 13,912 | **13,808** | 816,343 | 154,267 | **124,263** | 140,355 → 110,455 | 6.6 |
| vlist scroll | 108,843 | 13,009 | **12,989** | 1,140,929 | 214,270 | **134,073** | 201,261 → 121,084 | 8.5 |
| vlist fling | 178,601 | 19,393 | **19,576** | 1,308,185 | 218,276 | **128,008** | 198,883 → 108,432 | 10.2 |
| vlist jump | 335,715 | 35,559 | **35,510** | 1,333,209 | 235,855 | **144,893** | 200,296 → 109,383 | 9.2 |

The Metal floor (R1 − R0) fell from ~200 K to **~105 K for every scene**: input-driven and stream scenes are now
at or below animate's old 142 K (lab stream 131 K, lab scroll 100 K, vlist 128–145 K, animate 124 K).

Windowed (10 s, macsample, one repetition, before/after interleaved; `windowed_gpu.txt`), instructions per second:

| scene / mode | instr/s zgx | before | **after** | CPU % zgx / before / **after** | footprint MB zgx / before / **after** |
|---|---:|---:|---:|---|---|
| heavy stream | 329.0 M | 36.3 M | **33.9 M** | 6.14 / 1.55 / **1.65** | 167 / 137 / **139** |
| heavy scroll | 58.0 M | 27.7 M | **23.6 M** | 3.00 / 1.85 / **1.89** | 163 / 138 / **124** |
| heavy both | 426.1 M | 38.9 M | **35.0 M** | 7.82 / 2.27 / **2.12** | 168 / 138 / **124** |
| lab stream | 73.9 M | 27.8 M | **24.3 M** | 2.46 / 1.85 / **1.87** | 148 / 121 / **124** |
| lab scroll | 44.4 M | 26.4 M | **22.6 M** | 2.80 / 1.88 / **1.48** | 148 / 121 / **125** |
| lab both | 86.6 M | 27.7 M | **23.5 M** | 3.61 / 2.03 / **2.08** | 149 / 104 / **113** |
| animate - | 100.4 M | 31.9 M | **35.5 M** | 4.40 / 2.24 / **3.21** | 137 / 120 / **134** |
| blink - | 1.8 M | 0.6 M | **0.6 M** | 0.13 / 0.04 / **0.04** | 144 / 51 / **53** |
| vlist scroll | 63.3 M | 26.8 M | **22.2 M** | 3.22 / 1.96 / **1.97** | 150 / 109 / **119** |
| vlist fling | 69.9 M | 27.5 M | **23.2 M** | 3.19 / 2.02 / **1.95** | 136 / 114 / **120** |
| vlist jump | 68.4 M | 28.2 M | **23.5 M** | 3.09 / 2.14 / **2.00** | 141 / 105 / **111** |
| heavy idle | 0.3 M | 0.1 M | **0.1 M** | 0.02 / 0.01 / **0.01** | 53 / 45 / **47** |
| lab idle | 7.7 M | 0.1 M | **0.1 M** | 0.12 / 0.01 / **0.01** | 43 / 41 / **41** |

**zgx's drag probe** (same-window synthetic drag, main-thread instructions per move, 3 drags × 81 moves; `drag_probe`
under the lock, the input branch's binary run alongside):

| | per move |
|---|---:|
| zgx (INPUT.md) | 182,576 / 211,352 / 219,862 |
| bough, canonical presenter (input branch binary, this session) | 216,690 – 225,819 |
| **bough, this branch** | **118,501 – 126,225** |
| bough, frames off (`BOUGH_NOFRAME`: AppKit dispatch + the turn) | 112,920 – 115,340 |

The frame now costs the input thread 5–12 K above AppKit's own dispatch (was ~105 K): the cell is won outright,
−35..−43 % against zgx.

Costs and trade-offs measured:
* **animate (windowed)** rose 31.9 → 35.5 M instr/s against the old throughput mode, which published at the next
  tick for free but one refresh late. The publisher thread's wake and event wait (~25–30 K a frame) is the price of
  publishing in the same refresh; the input thread does less (no CA commit).
* CA commit alternatives were measured in round 1 (12.3: IOSurface swap ≥ ~80 K on the committing thread; CAMetalLayer
  `nextDrawable` 237–304 K alone; CAMetalDisplayLink +700 K; `setContentsChanged` no cheaper than a swap; hidden flip
  unsafe). Nothing public avoids a transaction per published frame, so it was moved off the input thread instead.

### Window resize (lead's request)

`Fused::resize`: fresh IOSurfaces at the new size (unbacked until drawn), row bins and counts for the new tile rows,
the scratch reference surface, and the layer's new frame committed **in the same transaction as the first frame at
the new size** (no dropped frame, nothing shown stretched); `App::resized` calls it. The interactive host observes
`NSWindowDidResizeNotification` (synchronously, so live resize draws every step) and the windows are resizable.
Gates (`comparisons/bough-scenes/tests/present.rs`): every frame after growing past the creation size and shrinking
again equals a full redraw at ring ages 1–4 and in the windowed, published ring; `bough-zgui`'s resize relayout gate
now runs on resized surfaces.

## 14.3 Exact geometric damage: the edge-band rule (item 3)

* **The rule** (`Bough::damage_edges`): a box primitive's pixel depends on edge `e` only within the kind's band of
  it (solid: the pixel-centre test; rounded: radius + 1 px of anti-aliasing; border: radius or border width), so a
  geometry change damages only the bands around the edges that **moved**, across both inks' perpendicular extent —
  never old ∪ new. For a solid it is at most the symmetric difference plus slack; for a translation it is the four
  bands; for a bar that grows, the strip between its old and new top.
* **Making it exact (representation change):** "moved" must be decided exactly, and an unmoved edge must be bitwise
  the same edge on the GPU. Two changes do that:
  1. **Geometry on a 1/256-px grid.** Quad and bar positions, sizes, radii and border widths are snapped; every sum
     the kernels form (position + integral space offset + size, ± integral margins and clip edges) is then exact in
     f32 below 2^15 px, whatever the compiler's association, so `x + w` on the CPU is the GPU's edge.
  2. **Edge-form shading** in the fused tile pass: records are staged with their four edges and the rounded-box SDF
     is `max(c − b1, b0 − c) + r` per lane (equal to the centre/half form mathematically), so a pixel's value depends
     bit for bit only on the edges within its band.
* **Gates.** `bough-core/tests/edges.rs`: 4,000 random boxes (solid, rounded, bordered), random edge subsets moved by
  ¼ px to 40 px; every pixel whose shaded value changes lies in a damaged tile (band − 2: fails at case 0). The heavy
  pixel gates at ring ages 1–4 pass; sabotage (no bar damage, or band 0) fails them at tick 7.
* **Applied to:** `quad_geom` (every quad geometry write: width/span operands, layout placement, the text field
  caret) and the bars (one rect per sparkline: the bars' columns between the highest and lowest moved top).

| heavy, repaired tiles per frame (ring 2, `heavy_headless`) | stream | both |
|---|---:|---:|
| before (old ∪ new) | 1,209 | 2,059 |
| quads only (progress bars) | 1,133 | 1,982 |
| **kept: quads + one strip rect per sparkline** | **1,058** | **1,907** |
| per-column strips (each tile column's own top range) | 846 | 1,689 |

* **Rejected: per-column strips** (−30 % tiles). Every formulation tried cost 32–65 K instructions a tick (780 bars,
  240 tile columns; per-bar `damage_edges` 230 K, per-column `or_rows` 26 K, row masks 45 K): R0 heavy stream
  180 → 212–245 K. The kept form folds one min/max per bar in registers and marks one rect per sparkline: +10 K
  (R0 heavy stream 180 → 190–194 K, +5–8 %; both +3 %) for −12.5 % tiles; R1 still falls 382 → 319 K. Tiles above the top band are text (log pane, digit patches, table rows): the
  text agent's area.

## 14.4 Bounds safety on the GPU by construction (item 5)

* Every column the kernels read has a **power-of-two capacity** (`Caps::pow2`; the rounding is the phantom padding,
  zero-filled, and an all-zero instance / page / glyph / run header draws nothing). The header carries `capacity − 1`
  per column, and **every index read from region data is ANDed with its column's mask** (page ids, instance and run
  indices, glyph slots, strike and token ids, spaces, clip ids, the bin entries the tile pass re-reads); **every loop
  bound read from region data is min-ed with a capacity** (directory length, run lengths); atlas reads are clamped.
  No branch; ~1 ALU op per index.
* Gate (`comparisons/bough-heavy/tests/gpu_bounds.rs`): random payloads, page ids, clip ids, run headers with
  lengths > 2^24, glyph entries and spaces → the frame completes in bounded time. Opening the glyph mask (which also
  bounds the run loops) makes the frame run for > 5 s (ignored test, run alone).
* Measured limit of the evidence: out-of-column reads inside the GPU's address space **do not fault** on this GPU
  (opening the index masks rendered garbage without an error), and Metal's shader validation did not flag them, so
  the masks are a guarantee by construction; what a test can observe is the loop bound.

## 14.5 Visual parity with zgx (item 6)

Heavy on the current kernels (edge-form SDF, grid geometry), `heavy_shot` vs zgx's `render_parity` frames
(2560×1600, `frame_diff`):

| frame | differ at all | > 16/255 | > 64/255 | max | by region > 16: title / tiles / cards / table / log |
|---|---:|---:|---:|---:|---|
| t = 120 | 4.48 % | **2.68 %** | **0.758 %** | 209 | 0.52 / 2.36 / 1.65 / 5.27 / 3.41 % |
| t = 0 | 4.51 % | 2.70 % | 0.765 % | 209 | 0.52 / 2.42 / 1.66 / 5.29 / 3.43 % |

Identical to the pre-fusion figures (2.69 / 0.77 %, 2.68 / 0.76 %): the difference is glyph anti-aliasing (the table
and log text), not geometry. **Lab was not re-diffed**: zgx's `render_parity` lab sets no font family (default sans,
not DejaVu), so a text diff is meaningless; a DejaVu reference needs a patched zgx build, not made (disk at 35 GB
with four agents building).

## 14.6 Where bough's GPU time goes; the glyph-dense regime (item 4, not solved)

`kernel_bench` (lab, ring 2, saturated clock; `DEBUG` 1 = no shading, 2 = bin pass only):

| | lab stream repair (499 tiles) | full redraw (2,700) | lab scroll repair (1,508) |
|---|---:|---:|---:|
| whole frame | 0.079 ms | 0.234 ms | 0.120 ms |
| bin pass alone | 0.012 | 0.019 | 0.009 |
| + tile pass cull/stage, no shading | 0.038 | 0.149 | 0.078 |
| shading | 0.041 | 0.085 | 0.042 |

The tile pass's scan of its row bin and staging is the largest share (47–64 %), most in glyph-dense rows: every
tile of a text row tests every entry of the row's bin. 2D binning (per row and column chunk, count + prefix +
scatter) was designed but **not built** this round; the A/B harness (`Fused::bench_frames`, `kernel_bench`) is in
place for it. 16-px quadrant masks need a finer damage plane in the core and were not attempted.

## 14.7 Residency (item 7, not solved)

The residency set still holds the whole region (~2 MB). One structural fact for whoever does it: **allocations added
to a residency set do not reach command buffers already submitted** (14.2, item 4), so "residency follows use" in a
pre-armed stream needs either a drain per change (what surfaces now do) or the region split into separately resident
heaps whose membership changes only between batches.

## 14.8 Honest gaps

* GPU column: bough loses lab (1.7×) and vlist (1.5×) GPU cycles per frame headless, and animate windowed; it wins
  heavy. Windowed GPU numbers are one repetition under heavy external GPU load.
* Heavy R0 stream +5–8 % (180 → 190–194 K) for the edge-band damage of the bars, the one STATUS §1.1 cell that
  regressed; R1 and windowed heavy improved. Per-column precision (−30 % tiles) rejected on CPU cost.
* Animate windowed +11 % instructions/s against the old one-frame-late throughput mode.
* Glyph-dense binning, 16-px quadrants, residency-follows-use: not built. Lab parity: no DejaVu zgx reference.

## 14.9 Round 3b (after merging canonical ca6412f: input, text3, floor)

Raw: `bench/results/floor/{slopes_3b,gpu_slopes_bough_3b,windowed_3b}.txt`. The canonical figures quoted are the
lead's merged slopes.

**GPU time: headline (headless slope, Kcycles per frame, median of 3)**

| scene | zgx | bough (round 3) | **bough (3b)** |
|---|---:|---:|---:|
| heavy stream / scroll / both | 446 / 268 / 627 | 359 / 303 / 502 | **313 / 260 / 451** |
| lab stream / scroll / both | 63 / 123 / 134 | 110 / 128 / 223 | **141 / 147 / 206** |
| animate | 97 | 72 | **75** |
| vlist scroll / fling / jump | 203 / 208 / 203 | 316 / 329 / 309 | **522 / 522 / 495** |

* **Not won.** bough wins heavy (every mode, 3b) and animate; it loses lab and vlist. The kernels did not change
  between the two bough columns, yet vlist moved 1.6×. The Kcycles figure multiplies the process's GPU seconds by
  the whole GPU's mean clock, and other agents' GPU work moves that clock (338–1,143 MHz seen). So only gaps of 2× or
  more are real at this load; lab stream (2.2×) and vlist (2.4×) are.
* **Tried this round:** tile-pass staging 256 wide instead of 128 (`kernel_bench`, saturated clock): lab stream
  repair 0.106 → 0.104 ms, scroll 0.171 → 0.173 ms. No effect, so staging width is not the bottleneck; reverted.
  The measured split stands (14.6): bin scan + staging 47–64 %, shading the rest, worst in text rows.
* **Not built:** the GPU-driven raster path (compute cull → indirect draw of instances clipped to damaged tiles in
  one render pass), which is what the TBDR evidence (damage lab, 12.5's vclip: one-colour 0.018 ms vs compute's
  0.011–0.054) points to for glyph-dense rows; row-pre-binned glyph runs; 2D binning. These are the open item.

**Regressions fixed**

| | canonical (merged) | **3b** |
|---|---:|---:|
| heavy stream R0 | 160 K | **153.6 K** |
| heavy both R0 | 194 K | **188.3 K** |
| heavy stream / both R1 | 263 K / 300 K | 279 K / 294 K (R1 ranges 265–289 K: noise) |
| animate R1 (headless) | 135 K | 135.8 K |

* **Edge damage now costs about what the old union damage cost.** The strip union comes from the bars' samples, not
  from reading instances back: a shift by one keeps every old sample except the first. So the union is two selects
  per bar on values the write loop already has, plus one rect per sparkline. Control on the same code: old ∪ new
  147.7 K vs edge rect 152.0 K (+2.9 %). The gain is −7 % tiles against that control (1,133 → 1,058) and −12.5 %
  against round 2's 1,209.
* **Publication by the turn's source, at compile time.** `App::frame()` is for input turns: the publisher thread
  publishes in the same refresh. `App::frame_paced()` is for clock-driven turns: the host's timer and display-link
  callbacks use it, and the frame is committed at the next turn on the calling thread, one refresh later, with no
  thread wake and no completion wait. `frame_as::<const PACED: bool>` selects the policy and `Fused::draw::<PACED>`
  follows it. Commits on the calling thread wait for anything the publisher still holds, so commits stay in frame
  order. Gate: `present.rs::paced_and_input_frames_publish_exactly_in_order`, mixed frames, exact.
  Windowed (10 s, macsample, 3b vs round 3 interleaved in one session):

| cell | round 3 (publisher) | **3b (paced)** |
|---|---:|---:|
| animate | 29.8 M instr/s, 2.90 % CPU | **26.9 M, 2.19 %** |
| heavy stream | 30.7 M, 1.95 % | **27.3 M, 1.48 %** |
| lab stream | 22.7 M, 1.78 % | **20.1 M, 1.29 %** |
| vlist scroll | 21.4 M, 1.71 % | **19.4 M, 1.23 %** |

  The drag probe keeps its win: 117–125 K per move on the 3b build.

**Residency (item 7): measured, nothing to recover at this scale.** Heavy's footprint is the same whether the region
is in the queue's residency set (148.5 MB) or only bound to the encoders (148.8 MB); lab: 95.9 vs 96.0 MB. The
region's untouched capacity does not show in `phys_footprint` either way. So residency that follows use would buy
nothing here, and the residency set stays, since the bindless surfaces and atlas need it. The structural constraint
for a larger region stands (14.7: a pre-armed batch never sees later residency additions).

**Lab visual parity (item 6), with DejaVu.** zgx's `render_parity` was built from a scratch clone at 06debe9d with
`font_family("DejaVu Sans")` added to its lab root (the clone was deleted afterwards). The 1920×1440 frames were
compared with `scene_shot`, Mode::Both:

| frame | differ at all | > 16/255 | > 64/255 | max |
|---|---:|---:|---:|---:|
| t = 120 | 4.04 % | **2.92 %** | **1.05 %** | 219 |
| t = 0 | 1.67 % | 1.22 % | 0.43 % | 219 |

The difference sits in the text bands (the streamed paragraph at t = 120, the list rows' labels) and in the title/button
band, where `render_parity`'s lab differs in layout (button alignment, one fewer label; noted in round 2). It is the same
order as heavy's 2.68 % > 16.

## 15. Round 3c: scroll by copy (direct Metal; superseded by ROUND4's wgpu pipeline, kept as its reference)

Built, gated, measured, then stopped by the round-4 change of direction (the GPU path moves to vendored wgpu with
wgsl-rs shaders; scroll-by-copy is rebuilt there on this design). Load average 85–146 throughout.

**Mechanism.**
* **Core** (`Bough::scroll_run_copy`, `Plane::shift_within`, `DamageRing::{scroll, set_scroll}`, `ScrollRec`): a scroll
  of any clip region C by an integral d marks only (1) the exposed strip(s) C \ (C + d); (2) this epoch's earlier marks
  inside C, also at their moved place (kept at the old place too: a mark from a record that did not scroll stays
  true); (3) the **static content's bands**: every record outside the run whose visible ink meets C does not move but
  the copy moves its pixels, so for a box (the caller's `band(kind, payload) >= 0`) exactly `damage_edges(v + d, v)`
  of its visible ink v (its perpendicular edges' bands and their moved copies), and for anything else (text, images,
  gradients: band −1) v ∪ (v + d). It keeps one `ScrollRec {clip, dx, dy}` per epoch beside the plane. A second scroll
  in the same epoch with another clip or delta marks both clips whole and poisons the record (a bounds condition).
* **Façade** (`Rt::scroll_copy`, used by `virtual_list` and the measured list): the paint ring's earlier marks get the
  same `shift_within`; the band closure is `edge_band` for single box quads, −1 otherwise. Positions are integral
  device px by construction (spaces are `[i32; 2]`; scroll offsets are rounded to device px before the delta is
  taken), so the moved content's pixels equal the copied ones bit for bit.
* **App::copy_plan**: if any epoch in the target's age window has a record, the clip's pixels come from the surface
  showing epoch e − 1 (`shown`, always exact) by ONE copy whatever the age (move = epoch e's own d, or 0 if the scroll
  was earlier): no sum of deltas is needed because the source is age 1. Rows = repair(age) on tiles not wholly inside
  C, plus epoch e's own marks (both rings) on tiles meeting C, plus any other record's clip whole. No source (ring of
  1) marks the clips whole.
* **Kernel `fs_copy`**: third dispatch, first in the pre-armed encoder (indirect args in the slot). One threadgroup per
  tile of the destination's tile span, 32 × 8 threads × 4 rows, skipping tiles in the frame's `FRow` bits, so copy and
  repair write disjoint tiles and no barrier is needed. Source through `FBind.r[4]` (appended, table 216 B); slot
  header 32 → 64 B (INTERFACE.md).

**Gates (all green):** core `tests/scroll_copy.rs` (`shift_within` == a per-tile oracle over 3,000 random planes;
`scroll_run_copy`'s marks cover every pixel a copy gets wrong in a pixel model of 200 random scenes with static solid
and text records); the pixel gates presented == full redraw at ring ages 1–4 and the windowed ring for vlist
scroll/fling/jump/back-up, heavy scroll/both/jumps, lab scroll/both/jumps, micro vscroll; retained == fresh mount;
present.rs. The delta+1 sabotage is the `bough/sabotage_copy` feature (not run before the redirect).

**Measured.**

| | before (3b) | **copy** |
|---|---:|---:|
| heavy scroll, repaired tiles/frame (ring 4 headless) | 990 | **90** |
| heavy both | 1,952 | **1,201** |
| vlist scroll / fling / jump, tiles/frame | ~2,250 (whole viewport) | **101 / 169 / 173** |
| vlist scroll GPU Kcycles/frame, ABAB pairs in one run (3 pairs) | 522–583 | **322–359**, median ratio **0.58** [0.55 .. 0.69] |

**Finding that bounds the idea (kernel_bench, saturated clock, lab 1920 × 1440):** copying 1920 × 1412 px costs
**0.215 ms**, a full compute redraw of the whole window 0.249 ms. A copy reads and writes every pixel (8 B/px), a redraw
only writes (4 B/px), and simple content (solid rows) shades almost for free, so moving existing pixels costs about
what drawing them does: the copy saves only the shading of dense content (−36…−45 % on vlist scroll), and vlist stays
above zgx's 203 Kcycles. The bandwidth floor, not the bin scan, is what a full-viewport scroll pays; a smaller copy
needs fewer moved pixels (only tiles whose content differs from the target's own, or compositor-side scrolling of a
sublayer), not a faster copy.
