# The CPU repair backend: API, inputs, guarantees (for lane gpu's per-frame executor choice)

Lane memory owns the CPU raster and the cost model; lane gpu owns the shared per-tile data both executors read
(lead's coordination, round 6). This is what exists on `lane/memory/round6`.

## Entry point

```rust
// crates/bough/src/cpu_raster.rs
pub struct CpuRaster { pub stats: CpuStats, .. }        // one per App (App::cpu); reusable buffers, no steady allocation
impl CpuRaster {
    /// Repair the damaged tiles (`rows`: the frame's OR-ed 32 px tile rows, bit x of row y = tile (x, y); or every
    /// tile when `full`) of a w x h px surface at `dst` (BGRA8, `bpr` bytes per row) from the runtime's tables.
    /// None: the frame holds something the model does not cover; nothing was written.
    pub unsafe fn repair(&mut self, rt: &Rt, rows: &[u128], full: bool, dst: *mut u8, bpr: usize) -> Option<u32>;
}
// crates/bough-metal/src/fused.rs: the surface, CPU-writable, GPU done with it
Fused::write_begin(i) -> (ptr, bytes_per_row, purged);  Fused::write_end(i);  Fused::show_held::<PACED>(i)
Fused::make_nonvolatile(i) -> purged;  Fused::gpu_used() -> bool
// crates/bough/src/repair.rs
pub fn cpu_exact(font: &'static [u8]) -> bool          // the cached self-check
```

`App::draw` is the only caller today: after `repair_rows` and `copy_plan`, a frame without a scroll copy whose
backend choice says CPU locks the target surface (`write_begin`), calls `repair`, unlocks, and publishes with
`show_held`; `None` falls through to the GPU path unchanged. A CPU frame never makes the surface resident (a first
residency drains the pre-armed stream, which is GPU work).

## Inputs (all read in place, nothing copied)

* records: `Bough::cull_tiles_pos` (positions in paint order of records touching damaged tiles) and
  `Bough::record(pos)` (ink, payload, clip), `Bough::xform_at(pos)` (refuses transformed pages),
  `Bough::clip_rect(id)`;
* instances `Rt::inst`, spaces `GlyphPool::spaces`, transforms `Rt::xforms` (refuses non-identity), run headers,
  glyph slots (`x`, `ent`), token tables (`GlyphCache::tok_strike`), the CPU copy of the glyph atlas;
* payload bits and masks as `fs_bin` reads them; any index out of range refuses the frame.

## What it models, and the arithmetic (each step as shader.metal writes it)

Kinds: solid (0), rounded (1), border (2), glyph instance (3), glyph runs (text records), untransformed, under clips.
Not: images, gradients, shadows (4+), transforms. Colours: `float(u8) * f32(1/255)`, RTNE to half, `rgb * a` in half.
Glyph coverage `half(texel / 255)`. `f_sd`: `precise::sqrt(fma(m.y, m.y, m.x * m.x)) + min(max(q.x, q.y), 0) - r`.
Border `fma(pm, h1, pm2 * h2)`. Blend `fma(acc, 1 - sh.a, sh)` (one half FMA). Store `RTNE(f32(v) * 255)`. The 2 x 2
block core test of `fs_tile` is reproduced (a block wholly inside a record's core takes `cv`).

## Guarantees and gates

* `crates/bough/tests/cpu_repair.rs`: adversarial scene, CPU forced, every frame equal to the GPU's full redraw;
  `--features sabotage_blend` (two roundings) fails it.
* `repair::cpu_exact`: a fixed probe scene both ways, cached per CPU model + OS build + shader source + model
  version (`$TMPDIR/bough-cpu-repair-<hash>`); `Repair::Auto` uses the CPU only after a pass.
* `BOUGH_REPAIR=cpu` on every pixel suite holds the CPU path to `reference_pixels` (GPU).
* Exactness depends on shader.metal keeping the written-out operations above: a change there must bump
  `repair::MODEL_VERSION` and the CPU model together.

## What lane gpu's shared per-tile data should give the CPU executor

Today the CPU stages from the record tables per frame (cull + expand runs + stage, then per tile the entries whose
pixel box meets it). If the GPU path grows a per-tile list (bins written by the CPU, formulation 7/9 of
IDEAS-OPUS), the CPU executor can take the same list: per tile, staged records `{q, b, core, pm, pm2, cv, uv, kind,
radius, bw}` in paint order. The executor's cost per tile is then shading only.
