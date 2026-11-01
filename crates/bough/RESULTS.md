# bough façade: first end-to-end vertical slice

`crates/bough` (the public façade) + `comparisons/bough-heavy` (zgx's *heavy* dashboard on it: windowed adapter, headless
adapter, correctness tests) on the `integrate` worktree. Apple M3 Pro, macOS 15.7, 120 Hz panel, `nightly-2026-10-17`,
`dist` profile (fat LTO, cgu 1) for every number below. zgx = 06debe9d `dist` binaries already built in `_ref/zgx/target/dist`
(measured here by the identical method, nothing taken from zgx's published tables).

```
export CARGO_TARGET_DIR=$PWD/target; RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo test --release -p bough -p bough-heavy              # repair-rect oracle + pixel-exactness suite (debug passes too)
cargo build --profile dist -p bough-heavy
ZGX_MODE=both ZGX_SECONDS=10 target/dist/heavy_workload   # windowed adapter (prints {"framework":"bough","scene":"heavy","ticks":N})
ZGX_MODE=stream RENDER=0 TICKS=600 PHASES=1 target/dist/heavy_headless
python3 bench/integrate/slope.py bough 0 ; python3 bench/integrate/slope.py zgx 0     # slope method, RENDER=0 / 1
python3 bench/integrate/windowed.py bough both ; python3 bench/integrate/windowed.py zgx both   # 10 s windowed CPU / memory
TICKS=120 ZGX_MODE=both target/release/heavy_shot         # offscreen frame -> heavy.bgra, compared with a full redraw
```

## 1. Headline results

**Per-frame instructions, slope method** (TICKS=600 minus TICKS=300, `/usr/bin/time -l` "instructions retired", process wide,
median of 3 runs; same method for both, zgx's `heavy_headless` run on this Mac with `ZGX_FONT_FILE`). `RENDER=0`: everything up to a
ready draw list + instance writes (bough) / layout + damage, no renderer (zgx). `RENDER=1`: plus the offscreen Metal render of every
damaged frame (bough: IOSurface ring, stencil repair, one draw, commit; zgx: wgpu `GpuRenderer`).

| mode | zgx RENDER=0 | **bough RENDER=0** | ratio | zgx RENDER=1 | **bough RENDER=1** | ratio |
|---|---:|---:|---:|---:|---:|---:|
| idle   | 2,253 | **~0** (65, noise +-10 K of startup) | - | 551,699 | **~0** (no frame is drawn) | - |
| stream | 2,981,696 | **403,212** | 7.4x | 6,175,782 | **651,730** | 9.5x |
| scroll | 162,633 | **35,117** | 4.6x | 1,479,260 | **261,383** | 5.7x |
| both   | 3,157,324 | **436,360** | 7.2x | 6,942,210 | **684,959** | 10.1x |

Run-to-run spreads: bough stream [401,217 .. 403,229]; zgx stream [2,976,954 .. 2,997,030] (RENDER=0); the RENDER=1 rows have 1-10 % spread
(Metal driver threads). Spread of every cell is in `slope.py`'s output (`[min .. max]`).

**Per-phase, bough, instructions per frame** (`PHASES=1`, `proc_pid_rusage` brackets with calibrated overhead subtracted, ticks 60..900):

| mode | update (handler + wires + `format!`) | text (shape + instances + ink) | damage (repair plane + rects) | cull + draw list | present (RENDER=1: encode + commit) | sum |
|---|---:|---:|---:|---:|---:|---:|
| idle   | 48 | 36 | 0 | 0 | 0 | ~84 (bracket noise; the frame returns before any phase) |
| stream | 207,248 | 140,252 | 27,145 | 22,865 | 36 (RENDER=1: 60,934) | 397,546 (RENDER=1: 466,550) |
| scroll | 15,755 | 25 | 8,178 | 12,362 | 20 (RENDER=1: 58,762) | 36,339 (RENDER=1: 101,429) |
| both   | 233,667 | 145,319 | 27,743 | 24,203 | 8 (RENDER=1: 61,448) | 430,939 (RENDER=1: 504,307) |

(zgx's own macOS per-phase profile, `bench/baselines/macos/SUMMARY.md`, stream: update 1,458 K, layout 750 K, damage 1,227 K, paint 1,150 K,
encode 634 K, submit 16 K.) About half of bough's `update` is the workload's own `format!` calls (the strings must come from the
workload functions); the text phase is shaping 94 labels/tick + their ink write-through. bough's per-phase sums run ~3-5 % under the slope
number because the slope also counts the loop, `Heavy::tick`, and `visible_log()`.

**Windowed, 10 s runs, from outside** (`bench/integrate/windowed.py`: `proc_pid_rusage` at t=1.5 s and t=9 s; CPU = user+system of the process,
all threads; GPU and WindowServer are *not* in these numbers):

| mode | zgx CPU % of a core | **bough CPU %** | zgx instr/s | bough instr/s | zgx phys_footprint | **bough phys_footprint (ring 4 / ring 3)** | wakeups/s (both) | ticks (zgx / bough) |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| idle   | 0.18 | **0.01** | 12.3 M | **0.10 M** | 50.5 MB | **46.4 MB** | 0 / 0 | 0 / 0 |
| stream | 6.65 | **1.70** | 383.7 M | **51.3 M** | 169.0 MB | 187.7 MB / ~172 MB | 0 / 0 | 584 / 600 |
| scroll | 2.86 | **1.35** | 63.0 M | **29.4 M** | 161.7 MB | 187.7 MB / ~172 MB | 0 / 0 | 599 / 600 |
| both   | 7.35 | **1.83** | 441.3 M | **53.4 M** | 169.3 MB | 188.3 MB / 172.0 MB | 0 / 0 | 599 / 600 |

(the ring-3 figure is `BOUGH_RING=3`, measured for `both` only: 172.0 MB, CPU 1.92 %.) bough hits 600 ticks in 10 s in every mode; zgx
dropped to 584 once in stream. **Memory is the one column bough loses**: the default ring of 4 IOSurfaces (4 x 15.6 MB, the size bough-metal's
RESULTS recommends for compositor safety) is 62.5 MB of IOSurface against zgx's 31.2 MB; with a ring of 3 bough is 172 MB, a tie within 2-3 MB.
The ~91 MB "owned unmapped" GPU-driver baseline is identical in both processes (vmmap).

## 2. Correctness

* `comparisons/bough-heavy/tests/correctness.rs` (5 tests, release and debug): per mode (stream 240 ticks, scroll 240, both 400, a scroll test with
  jumps of up to 123,456 pt, idle) **after every 7th/9th presented frame the surface the ring drew into is read back (IOSurface memory) and
  compared byte for byte with a from-scratch full redraw of the same retained state** (all records, one unscissored pass), 0 differing bytes;
  and **at the end the retained frame equals a fresh mount of the view at the same signal values** (this second check caught a real bug, below).
  The ring cycles 4 surfaces, so ages 1..4 are exercised.
  `heavy_shot` re-runs the first check for 300 ticks of stream/scroll/both: 0 bytes. `idle_is_zero_work` asserts that 100 idle turns run 0 wires,
  shape 0 texts, draw 0 frames, and that `live.set(same value)` is cut off by the signal's equality test.
* `crates/bough/tests/repair.rs`: `rects_from_plane` against a per-tile oracle on 400 random planes (2560x1600): disjoint, tile-aligned, cover exactly the
  set tiles.
* Bugs the suite found while building this: (a) cull must use the bounding box of the *tile-rounded repair rects*, not the exact damage bbox (the
  stencil repaints whole tiles, so records whose ink lies in the repainted fringe were missing: 3,793 bytes wrong); (b) a text node freed and
  reallocated in the same turn left a stale entry in the dirty list that shaped the *new* (quad) node's empty text into an EMPTY ink (pills vanished
  at the table edge; only visible against a fresh mount). Both are fixed and covered.
* The frozen crates' own suites were not re-run (their default features are unchanged).

### Visual parity with zgx (`crates/bough/results/`)

zgx's window cannot be captured here (`screencapture -l` fails and a region capture returns only the wallpaper: no Screen Recording permission for
this process). The zgx reference frames therefore come from zgx's own `render_parity` example (built in a scratch target dir; offscreen `GpuRenderer`
on Metal at SCALE=2, the same scene code as `heavy_workload`, mode both) at 2560x1600:

| file | what |
|---|---|
| `zgx_heavy_t0.png`, `zgx_heavy_t120.png` | zgx, tick 0 and tick 120 |
| `bough_heavy_t0.png`, `bough_heavy_t120.png` | bough (`heavy_shot`, offscreen, same state) |
| `diff_t0.png`, `diff_t120.png` | red = pixels differing by more than 16/255 in any channel |
| `bough_window_last_presented.png` | the surface the real window (`heavy_workload`, both, 3 s, `BOUGH_DUMP`) had published to its layer at exit |

`ZGX_INITIAL_TICKS` does not reach the heavy scene in zgx (`Heavy::from_env` only copies mode/seconds; the initial ticks advance the unrelated lab
`Workload`), so `ZGX_MODE=idle` is the tick-0 frame; bough honours the same contract. Pixel statistics (`frame_diff`, device px, 4.1 M pixels):
tick 0: 4.47 % of pixels differ at all, 2.69 % by more than 16, 0.77 % by more than 64 (max 209); tick 120: 4.44 / 2.68 / 0.76 %. Layout, geometry, colours,
strings, table rows, pills, sparklines, progress fills and the 5-line log are identical; the remaining differences are glyph anti-aliasing (swash alpha
coverage blended in gamma space vs zgx's pipeline), the fractional top edge of sparkline bars and progress ends, and ~14-px corner pixels where the table's
rows are rect-clipped instead of clipped by the rounded padding box.

## 3. What was built

| file | role |
|---|---|
| `crates/bough/src/signal.rs` | `Signal<T>`, `Map` |
| `src/elem.rs` | `Element<V>`, `Mounter`, `IntoProp`/`Apply` (operand lowering + monomorphic patch fns), `quad`, `text`, `at`, `clip`, `each`, `bars`, `virtual_list` |
| `src/rt.rs` | pool layout + size-classed instance-run allocator, nodes, wire arena + settle, text phase, spaces/clips, keyed virtual-list regions |
| `src/text.rs` | bough-text SimpleScript shaper + atlas + fused shape-to-instances; rustybuzz fallback for trigger strings |
| `src/app.rs` | `View`, `App<V>`: mount, `turn`, `frame` (surface ring + ages, repair, cull, draw list, present), phase profiler |
| `src/repair.rs` | tile plane -> rect list |
| `src/backend/metal.rs`, `shader.metal` | the winning presenter (see 4.2) |
| `src/backend/host.rs` | AppKit window host on bough-metal's `app::run_sized`, timer driven |
| `comparisons/bough-heavy` | `Dash` view (`lib.rs`), `heavy_workload`, `heavy_headless`, `heavy_shot`, `frame_diff`, `tests/correctness.rs` |
| `bench/integrate/` | `slope.py`, `windowed.py`, `rgba2png.py` |

### API as implemented

```rust
pub struct Signal<T> { head: Cell<u32>, value: T }          // #[repr(C)], head first: bit 31 QUEUED, low bits = first wire index
impl Deref/DerefMut (DerefMut: one OR sets QUEUED iff the signal has wires);  Signal::set(v)->bool (PartialEq cut-off);  sig.map(f)  // f: Fn(&T)->R + Copy, <= 16 B captures

pub trait View: Sized + 'static { const FIXED: bool = false; fn view(&self) -> impl Element<Self> + '_; }
pub trait Element<V> { fn mount(self, m: &mut Mounter<'_, V>); }        // tuples (1..8), Vec<E>, ()
pub trait IntoProp<P> { fn lower<K: Apply<P = P>, V>(self, m: &mut Mounter<V>, node: u32) -> P; }
//   operands: constants (f32, (f32,f32), Color, String, &'static str), &Signal<T: Clone>, Map<T, F>

quad(x, y, w, h).bg(op).width(op).span(op).rounded(r).border(w, c).children(el)   // absolute quads; op: IntoProp<Color / f32 / (y,h)>
text(op).at(x, y).size(pt).line_height(pt).color(c)         // op: IntoProp<String>; '\n' = explicit lines
at(x, y, el), clip(x, y, w, h, el), each(iter)
bars(x, step, w, count, base, max_h, min_h, &Signal<u64>, |t| -> f32)                    // series run (see 4.3)
virtual_list(&Signal<f32>, row_h, [vx,vy,vw,vh], visible: fn(f32)->Range<usize>, row: fn(&V, usize, &mut Mounter<V>))

App::new(view, Config{width_pt,height_pt,scale,font,ring}, gpu: bool) -> App<V>
app.turn(|v: &mut V| ..)   // sync GPU -> handler -> settle queued signals' wires -> text phase
app.frame() -> bool        // false (and nothing at all happens) when the epoch has no damage
backend::host::run_window(title, make_cfg, view, tick, period, seconds, report) -> !      // windowed runner
```

Lowering, as the API spec says: `&self.field` operands become wires by the owner-relative-offset trick
(`offset = addr(field) - addr(owner)`, installed if inside `size_of::<V>()`, otherwise the operand is read once as a constant); a wire is a 64-byte
arena record `{next, node, patch fn, 16 B recipe}` whose `patch` is monomorphic in (property, signal type, closure type); `settle` runs the QUEUED
signals of the owner's wired-field list (O(wired fields)); dead wires of unmounted rows are unlinked lazily during the walk. Each drawable is one
Bough record (kind, `payload` = instance-run id) and one run of 48-byte instances in a size-classed pool (`storageModeShared` MTLBuffer, so CPU
writes are GPU-visible; nothing is uploaded). A text wire only formats and marks the node; the text phase shapes (fused kernel, per-line reuse for
multi-line text), writes instances and writes the glyph-quad union ink through `set_ink_handle` (old and new support to damage). Quads write 4 bytes
(`touch`) for a colour, or size + `set_ink_handle` for geometry. The virtual list is a region of keyed rows: each row is mounted by the user's
`fn(&V, row, &mut Mounter)` into its own **space** (spaces table, one origin per row), unmounted when it leaves `visible(scroll)`; a scroll writes
the space entries of the survivors and calls `translate_run` on their contiguous ink columns, then splices the rows that entered/left; instances are
never rewritten. A turn is: sync -> handler -> settle -> text -> (`frame`) repair(age) -> rects -> cull -> draw list -> encode -> commit; completion
handler publishes the surface. `frame` is the only thing that touches the GPU, and it returns before doing anything if the epoch has no damage.

## 4. Deviations, with reasons

### 4.1 From API.md
* **Signal layout**: `{head, value}` `#[repr(C)]` rather than `{value, head}`, so the runtime can find the head of any `Signal<_>` without knowing `T`.
* **`FIXED = false` re-description + diff is not implemented**; `view` is mounted once (everything in heavy is `FIXED` anyway). `changed`, `List`, `Entity`,
  `Reader`/async, `Source` routes (`COUNT.update(..)`), typed routes, dynamic projections/height heap and `FIXED` re-describe are **not built**; the only turn
  form is the arbitrary-handler one (`app.turn(|v| ..)`). Layout is absolute positioning only (no cephal).
* **`Element` is public**, not sealed, and `IntoProp::lower` is generic over the target property (`Apply`) so one operand impl serves every property; the
  `&Signal<T>` operand clones `T` for the initial value (`T: Clone`) and patches by reference.
* **`List<T>` + row fn became `virtual_list(&scroll, ..., visible fn, row fn pointer)`**: rows are keyed by index, the row function is a non-capturing `fn(&V, usize, &mut Mounter)` (rows
  are mounted long after `view`'s borrow ended, so they borrow the owner at mount time instead of capturing it).
* **`bars` is an addition** (a series *run* record, 4.3). `quad`/`text` replace `Column/Button`.
* **Mount does not memcpy const templates**: records and instances are written element by element (the const-template/`NODES` lowering is not built).
* **No display link**: ticks come from a wall-clock NSTimer (60 Hz, absent in idle) and the frame is committed straight from the turn; the link is paused
  after the first tick and never runs again (so no per-tick link cost and no wakeups at all when idle; bough-metal measured ~60 K instr per link tick).

### 4.2 Backend (bough-metal re-implemented at runtime size)
bough-metal's presenter has 1920x1440 and 4,096 instances compiled in and is welded to its own demo `Scene`; heavy is 2560x1600 with ~3,300 instances plus
runs. Its winning design is re-implemented in `backend/metal.rs` over the façade's pool (IOSurface ring + completion-handler publish, memoryless-stencil
repair from the rect table, one instanced draw, AOT metallib, `IsInUse` skip, 4th surface) with the same Metal call sequence. The shader is bough-metal's with the
48-byte instance changed to bough-text's `atlas::Instance` layout so glyph instances are written by the text crate straight into the pool (quads use the
`aux` words). Reused as is: `bough_metal::app` (AppKit host), `bough_metal::perf` (rusage counters), `instance::Hdr`.

### 4.3 The `bars` series run
Zgx draws the 26 sparkline bars of a card as 26 elements, each with its own `live` wire. Done literally (and it was, first: 780 wires/tick, one `sinf`-heavy closure plus
a `set_ink_handle` each) they cost ~430 K of the 860 K instructions per stream frame. `bars` is one record owning a run of N solid quads, one wire,
one ink write (old and new union), and it memoises its samples: bar `i` at tick `t` is bar `i-1` at tick `t+1`, so when `key` advances by one the window shifts
and only the new rightmost sample is computed (26x fewer `sin` calls); any other jump recomputes all. Pixel output is identical (the full-redraw and fresh-mount tests
cover it). This is the "text mechanism applied to quads": a record may own a run of instances and a single operand patches the run.

## 5. Changes to the frozen crates (all of them)
1. `bough-core/src/damage.rs` + `Cargo.toml`: `TILE_SHIFT` is `5` (32 px tiles) under the new cargo feature `tile32`, `4` otherwise (default behaviour and
   tests unchanged). Reason: the plane is `u128` x 96 rows = 2048 x 1536 px at 16 px; heavy at 2x is 2560 x 1600. Cost: 32 px repair granularity in this slice (over-repair).
   The façade enables the feature for itself only.
2. `bough-metal/src/app.rs`: `run_sized(title, w, h, f)`; `run` now calls it with 960x720. Nothing else in bough-metal or bough-text was touched.

## 6. Known gaps
* No input, hit testing, focus, accessibility; no resize; no p99 frame-interval, GPU-time or latency measurements (only CPU/instructions/footprint).
* Memory: +19 MB over zgx at the default ring of 4 (tie at 3); the surfaces are not lazily trimmed in active modes. A 3-ring is not safe against the compositor holding two
  surfaces + one in flight (bough-metal's measurement: ~3 % of frames at 120 Hz); here commits are at 60 Hz but that was not verified visually.
* Damage granularity is 32 px (tile32). Rows' unclipped overscan ink makes a scroll tick also repaint ~1-2 tile rows above the table (the tiles' lower edge) and below it;
  `translate_run` damages old-union-new per 64-record page chunk, so a scroll frame repaints ~ the whole table viewport (1.33 M px of 4.1 M).
* `update` is half `format!` (the workload's functions); text is re-shaped for every changed label although digit-slot patching (bough-text RESULTS, idea 1) would turn most of it
  into a handful of `uv` writes; each text/quad geometry change still pays core's `set_ink_at` (two plane adds + summary maintenance, ~350 instructions).
* Text: ASCII fast path; Latin-1/other text and `ff/fi/fl` lines take the rustybuzz fallback (which keeps glyph ids and x advances only, not y offsets; it lives in the
  façade because rustybuzz is a dev-dependency of bough-text; 2.5 % of heavy's log lines); no wrapping/bidi/line breaking in the façade; baseline = CSS half-leading from hhea metrics,
  which matches zgx's glyph rows on DejaVu.
* The row clip is a rectangle (vertex-stage clamp); no rounded clip; at most 32 clips, 512 spaces (about 500 mounted rows), 16,384 instances.
* `ZGX_PARITY` is honoured (`Heavy::turn()`); `micro_bench` scenes (state, vscroll-fixed/variable) have no bough adapter yet (the façade has what they need, nothing was written).
* Slope noise: a single slope sample has +-10 K of startup variance (visible on the idle rows); stream/both medians are stable to 1 %.
* The measurement of zgx's window (CPU, memory) is zgx's own `heavy_workload` `dist` binary started the same way; GPU Mcycles/s and WindowServer CPU are not measured for either.
