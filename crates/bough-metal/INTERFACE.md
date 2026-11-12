# bough-metal GPU input contract

**Fused layout (what the façade runs, `src/fused.rs`, kernels `fs_copy` + `fs_bin` + `fs_tile`).** Sections 1-8 below are the
round-2 prototype contract (1920x1440 compiled in, a mirror of bough-core pages); the fused presenter reads the real
structures, at a runtime window size, in ONE `storageModeShared` buffer whose contents ARE the façade's `Region`
(`bough_mem`): every column below is carved once at a fixed offset and written in place by its owner. Nothing is
mirrored.

| column | owner (Rust type) | GPU read |
|---|---|---|
| header `FHdr` @0 (256 B reserved) | presenter | `viewport f32x2, tiles_x, tiles_y, inst_off, spaces_off, clips_off, dir_id_off, dir_ink_off, n_dir, pages_off, gx_off, gent_off, runs_off, toks_off, tok_stride, bin_cap, debug` (u32), then the capacity masks `m_inst, m_page, m_glyph, m_run, m_space, m_clip, m_strike` (capacity − 1; every capacity a power of two) |
| bind table `FBind` @128 | presenter (`write_binds`) | `t[4]` target surfaces and `atlas` (Metal 3 resource ids), `bins`, `counts` (GPU addresses), then (**round 3c, appended**) `r[4]`: the same 4 surfaces as `access::read` handles (the scroll copy's source). 88 B, ends at 216 of the 256 reserved. Encoders bind only the region (buffer 0) and the slot offset (buffer 1). **Append-only:** new entries go after `r[4]` |
| 3 frame slots | presenter (`write_slot`) | `FSlot` (**64 B since round 3c**): `bin_tg[3]` @0, `tile_tg[3]` @12, `target` @24, `nrows` @28, `copy_tg[3]` @32 (scroll copy: tiles of the destination's tile span, or 0), `src` @44 (source surface), `dst0, dst1` @48 (destination rect `[x0, y0, x1, y1)` as u16 pairs), `d` @56 (the move `(dx, dy)` as an i16 pair: destination p reads source p − d), `cspan` @60 (copy tile span `x0 \| y0 << 8 \| width << 16`); then `FRow {ty, span = x0 \| x1 << 8, prefix, pad, bits: u32x4}` per damaged tile row **at +64** (was +32) |
| quad instances | façade `Col<Instance>` (48 B, section 4 layout; `ext` unused) | payload instance index / instance run |
| pages | bough-core `PagePool` arena = `Col<Page>` (**1472 B**) | `ink` @128 (**page-relative**: absolute = ink + `offl`), `gink` @640, `payload` @768, `clip` (u16) @1280, `offl` (i16x4 = dx, dy, -dx, -dy) @1436 |
| directory | bough-core `Col<u32>` page ids, `Col<NRect>` absolute page unions | L1 cull |
| clips | bough-core `Col<NRect>` (id 0 = whole domain) | every record is clamped to `clips[page.clip[slot]]` |
| spaces | bough-text `GlyphPool::spaces`, `Col<[i32; 2]>` (integral px) | added to instances and runs |
| glyph slots | bough-text `GlyphPool::{x: Col<i32>, ent: Col<u16>}` | pens and token ids |
| run headers | bough-text `GlyphPool::runs: Col<RunHeader>` (32 B, section 5) | windows onto slots |
| strike token tables | bough-text `GlyphCache` `Col<TokStrike>` (stride `size_of::<TokStrike>()`, `entry[256]` first) | glyph geometry, atlas rect |

* **Payload** (record → what is drawn): bit 31 set = text, runs `[p & 0xFFFFF, + ((p >> 20) & 0x7FF) + 1)` (a
  multi-line text is one record over consecutive run headers); bit 30 set = an instance run
  `[p & 0x3FFFF, + ((p >> 18) & 0xFFF) + 1)` (bars, glyph-instance text); otherwise one instance index.
* **Cull:** directory union (absolute) → `gink` against `subsat(th, offl)` → record `ink` against the same page-relative
  threshold; a hit's absolute ink = `ink + offl`. Clip-free: unclipped inks are a superset, the clamp is exact.
* **Scroll by copy (round 3c, RESULTS §15):** each frame's encoder runs `fs_copy` (indirect args @32, 32 x 8
  threads) before `fs_bin`. A threadgroup per tile of the copy's destination span writes the target's pixels in
  `[dst0, dst1)` from surface `src` at `p − d`, skipping tiles set in the frame's `FRow` bits (the tile pass writes
  those whole), so copy and repair write disjoint tiles and no barrier is needed. 0 threadgroups without a scroll.
  The source is always the surface showing the previous frame, never the target.
* **Row certificates (round 6):** the bind table gains `cert[4]` @216 (GPU addresses; the table now fills the
  256-byte header): per surface a plane of one 72-byte `FCert` per tile (`ty * tiles_x + tx`): `c[8]` (u32x2: the
  bits of a half4 colour per 32 x 4 px band, 0 = none) and `m[8]` (u8: which of the band's 4 pixel rows hold it).
  Invariant: a set bit means all 32 pixels of that row hold, in that surface, the bytes `write(float4(c))`
  produces. `fs_tile` certifies each band it shades (all 128 accumulators bit-equal); `fs_copy` derives every
  destination row from its source row and then SKIPs it (destination already certified the same colour), FILLs
  it (32 writes, no read) or MOVEs it, and stores the destination's new certificates. The planes are shared
  storage, zeroed by the presenter when a surface is first drawn or comes back from a trim; a zero plane is all
  MOVE. `FSlot.src` bit 8 (`CERT_FROZEN`) makes the copy leave the destination's certificates alone (kernel
  timing). The frozen original kernels (`src/oracle.metal`, compiled lazily by `reference_pixels`) mask
  `src & 3` and ignore the planes; they read the same region, slots and bind table, which is why every layout
  change is an append.
* **The damage is the dispatch:** the CPU copies the OR-ed tile rows into `FRow` records (O(damaged rows)); the bin
  pass runs one threadgroup per record (indirect args `bin_tg`), the tile pass one per damaged tile (`tile_tg`), each
  finding its tile by a binary search over `prefix` and a select of the k-th set bit of `bits`.
* **Bins:** `tiles_y × bin_cap × 16 B` (private), entries `(payload or instance, ink.xy, ink.zw, clip << 20 | slot)`;
  `counts[tiles_y]` records the largest overflowing row; the CPU grows the bins and redraws that frame before it is
  published.
* **Bounds by construction (round 3):** every index the kernels read from region data is ANDed with its column's
  mask and every loop bound read from it is min-ed with a capacity; a corrupted region renders garbage, in bounded
  time, inside its columns.
* **Submission (round 3):** every frame is a pre-armed, event-released frame of the stream (RESULTS Part IV); the
  slot's `target` names the surface. Anything added to the residency set (a surface's first use, a resize, bin or
  atlas growth) drains the pending frames and re-arms, because a submitted batch does not see later additions.
* **Write rule:** the CPU writes the region only after `Fused::sync()` (one frame in flight; in the pre-armed stream,
  before the frame's event release).

**bough-gpu (round 4, wgpu-hal):** the same region, header, bind table and slots. Kernels are wgsl-rs
(`crates/bough-gpu/src/shaders`) with one layout: group 0 = `POOL` (region, binding 0), `BINS` (1), `COUNTS` (2), and
textures `T0..T3` (3..6), `R0..R3` (7..10), `ATLAS` (11), which build.rs's transform reads from `FBind` instead of
binding them; group 1 = `SLOT` (uniform: x = the slot's byte offset). Metal indices: buffers 0, 1, 2, slot 3, naga's
sizes buffer 4 (crates/bough-gpu/RESULTS.md).

---

This is what the presenter's GPU passes read, byte for byte. bough-core (pages, directory, damage) and bough-text
(glyph pool, run windows) write these structures **in place**: no translation pass, no copy.

Two GPU passes read it each frame, in one compute encoder (`cs_bin32` then `cs_tile32b`, see RESULTS.md 13):

* the **bin pass** runs once per damaged 32-px tile row;
* the **tile pass** runs once per damaged 32x32 tile.

Everything lives in **one `storageModeShared` MTLBuffer, the pool**. All offsets below are bytes from the pool base
unless marked "within". All integers are little-endian.

**Write rule:**

* The CPU writes the pool only while no GPU frame reads it: after `Presenter::sync()`, so one frame is in flight.
* In pre-armed mode, writes must also come *before* `MTLSharedEvent.signaledValue = 2f−1`.

## 1. Header (pool offset 0, 84 bytes used, 256 reserved)

| offset | type | field | meaning |
|---|---|---|---|
| 0 | f32 x2 | viewport | surface size in px (1920, 1440) |
| 8 | u32 | spaces_off | -> `f32x2` per space |
| 12 | u32 | clips_off | -> `f32x4` per clip |
| 16 | u32 | rects_off | (stencil path only) |
| 20 | u32 | inst_off | -> 48-B `Instance` array |
| 24 | u32 | list_off | (render paths only) |
| 28 | u32 | n_inst | instances in use |
| 32 | u32 | plane_off | (vclip path only) |
| 36 | u32 | pages_off | (16-px kernel only) |
| 40 | u32 | dir_page_off | -> `u32` page id per directory entry |
| 44 | u32 | dir_ink_off | -> `NRect` page union per directory entry |
| 48 | u32 | n_dir | directory entries |
| 52 | u32 | pagepool_off | -> page 0 of the page pool |
| 56 | u32 | debug | 0 (diagnostics only) |
| 60 | u32 | stats_off | diagnostics |
| 64 | u32 | gx_off | -> `i32` pen per glyph slot |
| 68 | u32 | gent_off | -> `u16` entry id per glyph slot |
| 72 | u32 | runs_off | -> 32-B `RunHeader` per run |
| 76 | u32 | entries_off | -> 32-B `Entry`, `strike * 256 + ent` |
| 80 | u32 | space_clip_off | -> `u32` clip index per space (clip for text runs) |

## 2. NRect (8 bytes)

* bough-core's `NRect`: `i16 x 4 = (x0, y0, −x1, −y1)`, device pixels, half-open.
* EMPTY = `(32767, 32767, 32767, 32767)`.
* The GPU tests "ink overlaps tile/band `[X0, X1) x [Y0, Y1)`" exactly as `Thresh::matches` does:
  `all(ink < (X1, Y1, −X0, −Y0))`. EMPTY fails by itself, so padding needs no length check.

## 3. Directory and pages: bough-core's own layout

* **Directory** (paint order), one entry per page:
  * `dir_page[i]: u32` = the page's id;
  * `dir_ink[i]: NRect` = the page's union ink (core `d_ink`).
* **Page pool:** page `id` lives at `pagepool_off + id * 1344` and is **exactly `bough_core::page::Page`**
  (`repr(C, align(64))`, 1344 B):

| within page | type | column | GPU use |
|---|---|---|---|
| 0 | u16 x 64 | depth | – |
| 128 | NRect x 64 | ink | record cull (tile/band test) |
| 640 | NRect x 8 | gink (union of slots 8g..8g+8) | group cull |
| 704 | u8 x 64 | kind | – (dispatch is by payload bit) |
| 768 | u32 x 64 | payload | what to draw (below) |
| 1024 | u32 x 64 | handle | – |
| 1280 | u32 x 3 | version, id, dir | – |

* **Paint order** = directory order, then slot order. Slots `[len, 64)` are core padding: ink EMPTY. A record's
  `ink` must cover every pixel the record can change, including the 1-px AA margin, already clipped. Group and page
  unions may be supersets.
* **payload:**
  * bit 31 clear: index into the `Instance` array (§4);
  * **bit 31 set: text run index** (§5); `kind` = 4 (RUN) by convention.
* **Integration:**
  * `PagePool` allocates its pages from an arena inside the pool buffer (it is already type-stable and never frees).
  * The `Bough` keeps `dir_page` (page id = `(ptr − arena) / 1344`) and `d_ink` as pool-resident columns.
  * The cull hierarchy the GPU uses is the core's own: directory union → `gink` → `ink`.
  * For > 256 directory entries, the core's chunk unions (`c_ink`) would be the extra first level (not needed at
    this scale).

## 4. Instance (48 bytes, `instance.rs` / `shader.metal`)

| offset | type | field |
|---|---|---|
| 0 | f32 x2 | pos (px, relative to the space) |
| 8 | f32 x2 | size (px) |
| 16 | u32 | color, RGBA8 (r = low byte), straight alpha |
| 20 | u32 | color2 (border fill) |
| 24 | f32 | radius |
| 28 | f32 | border width |
| 32 | u16 x4 | atlas texels x, y, w, h (glyph kind) |
| 40 | u16 | space |
| 42 | u8 | kind: 0 solid, 1 rounded rect, 2 border+fill, 3 glyph |
| 43 | u8 | flags |
| 44 | u32 | clip index |

* Space `s` adds `spaces[s]` (f32x2 px). Clip `c` = `clips[c]` (f32 x0, y0, x1, y1 px); the quad is clamped to it
  (an exact clip).
* A space move or scroll is one 8-byte write plus the core's ink update. No instance is rewritten.

## 5. Text: bough-text's GPU format (iteration 2), expanded by the bin pass

| structure | where | layout |
|---|---|---|
| glyph pool, pen | `gx_off + 4 * slot` | `i32` pen in font units (run-relative) |
| glyph pool, entry | `gent_off + 2 * slot` | `u16` strike entry id (= the token; for ASCII the byte) |
| `RunHeader` (32 B) | `runs_off + 32 * run` | origin f32x2 @0 (px, space-relative; origin.y integral), scale f32 @8 (px per font unit), color u32 @12, space u16 @16, strike u16 @18, base u32 @20, len u32 @24, x0 i32 @28 |
| `Entry` (32 B) | `entries_off + 32 * (strike * 256 + ent)` | geo f32x4 @0 = [left, −top, w, h] px, uv u16x4 @16 (atlas texels), state u32 @24, pad @28 |
| space -> clip | `space_clip_off + 4 * space` | `u32` clip index applied to every glyph of runs in that space |

* A run draws slots `base .. base + len`. The glyph quad is
  `x = floor(fma(float(gx[slot] − x0), scale, origin.x + 0.5)) + geo.x`, `y = origin.y + geo.y`, size `geo.zw`,
  plus the space offset, clamped to the space's clip. Colour is the run's.
* `uv.w = 0` (empty bitmap, e.g. space) draws nothing.
* This is bough-text's formula, verified bit-exact there. Here the GPU-expanded run renders **byte-identically to
  the same text as 48-B glyph instances** (`examples/runs_check.rs`, heavy text load, 60 mixed steps, 0 differing
  bytes).
* **Windows are free:** a wrapped line, a stream line or a scrolled log line is a header write (`base`, `x0`,
  `origin`). A digit patch is one `u16` write per changed glyph.
* The core record of a run carries the run's union ink. The bin pass culls runs by it, then tests every glyph of a
  hit run against the row band (256 threads per step). Hit glyphs are appended to the row's bin in paint order, so
  each run is expanded at most once per damaged tile row.

## 6. Damage in, tiles out: the frame slot

* The GPU never reads a damage plane. The CPU turns `repair(age)` of the core's **32-px tile ring** (`[u128; rows]`,
  bit x of row y = tile (x, y)) into a frame slot.
* Slots `f % 2` at `FRAME_OFF + slot * 65536`, 64 KB each:

| within slot | type | field |
|---|---|---|
| 0 | u32 x3 | tile-pass indirect args: (ntiles, 1, 1) |
| 12 | u32 | target surface index (0..3), pre-armed mode |
| 16 | u32 | ntiles |
| 20 | u32 | full: 1 = all 60 x 45 tiles, list not written |
| 32 | u32 x ntiles | tiles, `(ty << 16) \| tx`, **row-major** (as a scan of the u128 rows emits them) |
| 65280 | u32 x3 | bin-pass indirect args: (nrows, 1, 1) |
| 65296 | u32 x nrows | rows, `ty \| tx0 << 8 \| tx1 << 16`: the damaged x-span of each damaged row |

* Cost on the CPU: one bit scan of the OR-ed rows (O(damaged tiles)), or nothing for a full redraw.
* This crate's history is 16-px, so it carries a 16→32 adapter (`gpu::tiles32`). The integration feeds the core's
  32-px planes directly and the adapter disappears.

## 7. Surfaces, atlas, bins

* 4 IOSurface-backed `BGRA8Unorm` textures (`ShaderWrite`); target choice in RESULTS.md 12.6 and 13.
* Glyph atlas: R8, read with `atlas.read(uv + t)` (texture 0).
* Bins (GPU-private, 45 rows x 8192 x 16 B) and their counts: owned by the presenter, never touched by the CPU.
* All of these plus the pool sit in one `MTLResidencySet` attached to the queue (RESULTS.md 13.3).

## 8. Domain limits (bent deliberately; widen by widening the field)

* i16 coordinates in the window (as bough-core).
* 64 directory pages and 4096 records in this prototype pool, 8192 glyph slots, 1024 runs, 256 entries per strike,
  256 spaces, 16 clips.
* Tile rows: x span < 256 tiles (8-bit fields).
* A row bin holds ≤ 8192 entries (each record or glyph at most once per row, so it cannot overflow while records +
  glyph slots ≤ 8192).

## Transforms (round 3c/4, input agent): page affine + per-instance affine

A transformed subtree's records live in pages that carry an affine (bough-core `Page::xm`, `Page::xv`, at
byte offset 1448 = `FPG_XF`, 24 bytes in what was padding): the stored inks are the records' untransformed
inks, and absolute = `M s + xv + off` with `M = I + xm` (row-major `[a, b, c, d]`; all zero = identity, so an
untransformed page is unchanged). `off` (the page offset, scrolls) still composes after the affine.

* **Bin pass** (`fs_bin`): for a page with a non-zero affine, the group test and the record test use the
  transformed bounding box of the stored rect (`f_pg_xink`: four corners through `M`, plus `xv + off`, floor /
  ceil, grown by 1 px), compared with the absolute tile threshold. Untransformed pages take the offset path
  exactly as before.
* **Per instance**: every instance and glyph run names a space; the header's `xforms_off` points at a column
  of `FXf { float4 dm; float2 c; float2 t; }` per space (`dm = M - I`, pivot `c` in space-local px, translation
  `t`; zeroed = identity). A point `u` (untransformed absolute = space-local + T) is drawn at `K + t + M (u - K)`,
  `K = T + c`. Staging (`f_stage_rec`, `f_stage_glyph`) computes the coverage window as the transformed box
  grown by 1 px and stores `M^-1`, `K`, `K + t` and `sqrt|det M|` in the record (`FRec.xi/xk/xt/xs`);
  shading maps the pixel centre back (`u = K + M^-1 (c - K - t)`) and scales SDF distances by `xs`; glyphs are
  sampled bilinearly when transformed. Index `space & m_space` as every other column.
* The CPU keeps both halves equal: `Rt::transform_subtree` writes the space's `FXf` and calls
  `Bough::set_run_xform` on the subtree's records (pages isolated, the same affine with the pivot in absolute
  px). Damage is the exact tile cover of each transformed ink (per 32-px row, the quad's x extent); hit testing
  maps the point through the inverse and tests the untransformed inks.
