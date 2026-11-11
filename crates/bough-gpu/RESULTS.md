# bough-gpu: the GPU path on vendored wgpu-hal, shaders in wgsl-rs (round 4, branch `floor`)

ROUND4 §8 and the lead's steer: the frame path runs on **wgpu-hal** (Metal backend), not wgpu-core; objects are
created once from raw Metal objects through the hal's own wrappers; shaders are **wgsl-rs**, compiled at build time.
M3 Pro, macOS 15.7, load average 85–146 from other agents throughout (instruction counts are the figures to read).

```
export RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/floor
cargo build --release -p bough-gpu --example halprobe
target/floor/release/examples/halprobe 3000                 # exactness + per-frame instructions (hal)
target/floor/release/examples/halprobe startup-hal           # cold startup, fresh process (alternate with:)
target/floor/release/examples/halprobe startup-direct        # cold startup of bough_metal::fused::Fused
target/floor/release/examples/fprobe 3000                    # direct-Metal per-frame floor, same session
```

## 1. What exists

* **Vendored** (`vendor/`, provenance and local changes in `vendor/README.md`): wgpu, wgpu-core, wgpu-hal, wgpu-types,
  naga, wgpu-naga-bridge, wgpu-core-deps-apple 29.0.4 (gfx-rs e99f5305), winit 0.30.13, wgsl-rs b2de16a (the rev
  gpui-ce pins; its naga is 29). Root `[patch]` tables resolve every use to the copies; `exclude = ["vendor"]`.
* **Build-time shader pipeline** (`crates/bough-gpu/build.rs`): wgsl-rs module → WGSL → naga (validated) → MSL with
  the Metal indices wgpu-hal's pipeline layout assigns → `xcrun metal` per module → one `metallib` → loaded by the
  hal with `ShaderInput::MetalLib`. Nothing parses WGSL or runs naga at runtime. Three mechanical MSL transforms:
  1. **bindless**: every texture parameter of an entry point is dropped and read from the bind table in the region
     (`FBind` at offset 128, the direct path's table byte for byte), so an encoder binds only the region (and bins,
     counts, slot): a pre-armed frame names no IOSurface;
  2. **workgroup memory** becomes kernel-scope `threadgroup` variables (static size; a passthrough metallib tells
     the hal nothing about threadgroup memory, and the first attempt read unallocated memory: the compaction test
     returned garbage until this transform);
  3. naga's helpers (`naga_div`, ...) are renamed per module, since modules are separate translation units linked into
     one metallib (duplicate external symbols otherwise).
* **wgsl-rs subgroup operations** (vendored addition): `subgroup_exclusive_add`, `subgroup_add`, `subgroup_max`,
  `subgroup_ballot` → naga → `simd_prefix_exclusive_sum` / `simd_sum` / ... The binned kernels' ordered compaction
  (`shaders/compact.rs`) is written on them.
* **The hal path** (`src/hal.rs`): `MTLCreateSystemDefaultDevice` + `Device::device_from_raw` (no instance, no
  adapter enumeration, no validation layer), `Queue::queue_from_raw`, the region a `storageModeShared` MTLBuffer the
  CPU writes in place (`buffer_from_raw`), IOSurface-backed surfaces (`texture_from_raw`), one residency set added to
  the hal queue's raw queue, the pre-armed stream: K frames in one hal command buffer, each
  `encodeWaitForEvent(2f−1)` (through the hal's `raw_command_buffer`) → compute pass → `encodeSignalEvent(2f)`;
  a frame = slot write + `setSignaledValue(2f−1)`. No change to wgpu-hal was needed so far.
* **Kernels ported:** `fs_copy` (scroll by copy) and the compaction self-test. Not yet: `fs_bin`, `fs_tile`.

## 2. Measured

**Exactness:** `fs_copy` through the hal: 0 wrong pixels of 2,304,000 (a 1440 × 1600 copy moved 24 px, one marked
tile skipped). Subgroup compaction: 68 hits over 256 lanes, in invocation order, with the right total.

**Cold startup, fresh process each, alternating** (to a presenter ready to draw):

| run | wgpu-hal (`HalGpu::new`) | of which device | pipelines | direct Metal (`Fused::new`) |
|---|---:|---:|---:|---:|
| 1 | 41.9 ms | 37.7 | 3.1 | 29.6 ms |
| 2 | 26.6 | 22.4 | 1.9 | 29.2 |
| 3 | 27.4 | 23.2 | 2.2 | 49.8 |
| 4 | 25.1 | 23.0 | 1.1 | 29.7 |
| median | **27.0** | 23.1 | 2.0 | **29.7** |

Device creation is ~22–23 ms of it, and that is the OS's `MTLCreateSystemDefaultDevice` (the input agent measured
22–29 ms for it alone); the hal's capability query adds little. The hal side builds 2 pipelines, direct 3, so only
"no worse" is claimed. The cut that remains is structural, not a hal one (§3).

**Per-frame process instructions** (one-tile frame, headless; hal 1440 × 1600, fprobe 512 × 512; same session):

| shape | direct Metal (fprobe) | wgpu-hal |
|---|---:|---:|
| command buffer per frame, wait each | 107.9 K | 201–229 K |
| pre-armed stream, textures in a hal bind group | (114–134 K, RESULTS 14.2: surfaces bound) | 120.6 K |
| **pre-armed stream, bindless (build-time transform)** | **105.6 K** (16/32 per batch), 116.3 K (8) | **102.6–111.9 K** (5 runs; 106.6 median) |

* On the path every frame takes (the pre-armed stream), **wgpu-hal is at parity with direct Metal**.
* A hal command buffer per frame costs about 2× direct. The cost is hal's submit (a completion-handler block per
  submit, an autorelease pool, fence maintenance) and its bind-group state. The stream amortizes it over the batch.
  That is why the frame path never submits per frame. wgpu-core would add its tracking on top of this path, so it
  was not built for the comparison. The lead's steer is borne out by the hal-vs-direct gap alone.

## 3. What is left, as a concrete design

1. **`fs_bin` / `fs_tile` in wgsl-rs** (`shaders/bin.rs`, `tile.rs`). The building blocks are proven: compaction,
   workgroup memory, the bind table, indirect dispatch from the slot.
   * Region data is read as `RuntimeArray<u32>` words. An `NRect` is 2 words, `x0 | y0 << 16, −x1 | −y1 << 16`,
     unpacked by shifts. `subsat` is `clamp` on i32. A page is 368 words (1472 B), with the same offsets as
     INTERFACE.md.
   * Bins and counts are bound storage buffers (bindings 1, 2). Bin growth means a new buffer and bind group, plus
     the existing drain-and-re-arm.
   * Colour accumulation is f32 first. The direct kernel uses `half`; port it if the GPU-time pairs
     (`bench/floor/gpu_pairs.py`, `A=bough=<direct dist> B=bough`) show a gap. wgsl-rs's f16 support is unverified.
   * Gate: the existing pixel gates, run against the hal presenter.
2. **`bough_gpu::Fused`** with `bough_metal::fused::Fused`'s API, so `App` switches at compile time:
   * sync and draw::<PACED>, show_held, prepare_target, trim_where, resize, sync_atlas;
   * reference_pixels and bench_frames;
   * the publisher thread and paced commits (unchanged objc2 code), and residency changes (drain and re-arm).
   The Kern encode is one hal compute pass per frame: `fs_copy`, `fs_bin`, then `fs_tile`. The bin→tile ordering
   is a split into two passes or the hal's buffer barrier; measure both.
3. **Scroll by copy in this pipeline.** `fs_copy` is ported and exact. The core (`scroll_run_copy`, `ScrollRec`,
   `shift_within`) and `App::copy_plan` (branch commit 2b4d924, metal RESULTS §15) are backend-free and take any
   clip region, so they serve the api agent's virtualization-as-damage directly. The delta+1 sabotage is the
   `bough/sabotage_copy` feature.
4. **Startup below the device floor.** Create the MTLDevice on a thread started at the top of `App::new`, while the
   main thread mounts, lays out and shapes, and join before `set_layout`. The ~22 ms of driver loading then overlaps
   CPU work that has to happen anyway.
5. **winit host.** Port the host onto the vendored winit with Accessory activation and no focus stealing. Measure
   event-loop construction and window creation against the current objc2 host, then strip what shows.
6. **Measure everything against direct Metal and zgx** before the façade switches: R0/R1 slopes, drag probe, GPU
   pairs, footprint, cold first pixel.
