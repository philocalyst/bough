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
