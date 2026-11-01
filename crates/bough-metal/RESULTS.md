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
$X/bench --strategy bh --secs 2 --threads     # a | b | bs | bh | c ; add --profile for phase instr, --sweep for damage modes
$X/bench --startup metallib|runtime
$X/probe 240 0 1 3                            # (c) preservation: frames, framebufferOnly, stride, maxDrawables
$X/latency c 300 3 1                          # presentedTime latency: kind frames maxDrawables maxOutstanding
$X/floor                                      # bare Metal submission floor
```
Note: building adds the objc2 dependency tree to the workspace `Cargo.lock` (untracked, not committed here).
