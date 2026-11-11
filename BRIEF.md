# bough: the complete brief

Last updated 2026-11-01 by the lead session. This brief replaces chat history as the source of truth.

The brief covers four things:

* where every piece of work is;
* what is measured, and by whom;
* what a true reflective interface needs (measured against current zgx);
* every outstanding item and API change.

Sections marked **verified** were re-run by the lead. Sections marked **agent-reported** come from an agent's own
measurements. Every agent ran on a machine at load average 15–140, so instruction counts are the reliable column
and times are indicative.

---

## 0. One-paragraph state

bough is a from-scratch Rust GUI framework. Its design:

* **Tree:** a bbtree-style paint-preorder record tree in 1 KiB SoA pages, with page summaries and an NRect min/max
  algebra.
* **Damage:** exact damage in 32-px GPU tiles.
* **GPU:** the GPU culls bough's pages itself and repairs only damaged tile rows, through a pre-armed command stream.
* **Text:** a table shaper with digit patches; it is now grafted into vendored parley.
* **Layout:** cephal.
* **Reactivity:** inline `Signal<T>` with compiler-proven wires (`sig!` → `offset_of!`).

It beats zgx on every desktop scene measured, by:

* 6–28× fewer instructions per frame;
* 1.5–4.5× lower CPU;
* 26–65 % less footprint.

On zgui's whole perf suite it uses 86–1,461× fewer instructions per interaction, and 7.9× on a like-for-like cold
first frame. On zgx's suite.py scenes it is 2.4–66× lower on every scene ported.

The one losing column is **GPU time on lab and vlist**, about 1.5–2.4×.

Round 4 (the user's direction, `research/ROUND4.md`) is mid-flight on four branches:

* **built-in primitives:** virtualization as damage, transforms, typed no-opt-in reflection, transitions, and the
  inspector;
* **vendored parley** with grafts;
* **vendored wgpu-hal, winit and wgsl-rs** shaders.

All four agents were stopped. Every branch is committed and pushed to the GPUI rework archive as `bough/*` (§10). The merge state is in §9.

---

## 1. Where everything is

### 1.1 Repositories, branches, worktrees

| what | where |
|---|---|
| main repo | `/Users/mileswirht/projects/bough`; main branch **`canonical`**; pushed as `bough/*` branches of `philocalyst/gpui-rework-archive` (§10) |
| worktree `input` | `/Users/mileswirht/projects/bough-input` (branch `input`): hit testing, focus, text fields, zgui ports, transforms, transitions, inspector |
| worktree `api` | `/Users/mileswirht/projects/bough-api` (branch `api`): List/Reader/Entity/re-description, zgx suite ports, reflection, virtualization, API cleanup, app ports |
| worktree `floor` | `/Users/mileswirht/projects/bough-floor` (branch `floor`): presenter, per-frame floor, GPU time, scroll by copy, vendored wgpu/winit/wgsl-rs, `bough-gpu` |
| worktree `text3` | `/Users/mileswirht/projects/bough-text3` (branch `text3`): text rounds 3–4, vendored parley with grafts |
| historical branches | `fusion` (the fused system, = canonical @ 42bc651), `integrate`, `layout` (merged; worktrees removed) |
| build dirs | `/Users/mileswirht/projects/bough/target/<branch>` (one per worktree; **never share a target dir between worktrees**, since cargo reuses artifacts across paths and comparisons go stale); `target/fusion` = canonical's; `target/zgx-headless` = zgx's headless drivers |
| agent scratch | `/private/tmp/claude-501/-Users-mileswirht-Downloads-designs-reactivity/16fd4c0d-…/scratchpad/` (zgx-input, zgui-input, zgx-copy, zgui-copy clones; the reflection prototype `reflect/main.rs`) |

### 1.2 Crates (workspace members after all merges)

| crate | role | key files / docs |
|---|---|---|
| `crates/bough-core` | the Bough: paint-preorder records in 64-slot SoA pages, page directory with summaries, NRect (negated max: intersect = max, union = min), clips, page affines (transforms), tile planes (one bit per 32-px tile, ring of 4), exact damage incl. `damage_edges` bands, `scroll_run_copy` + `ScrollRec`, hit testing (`bough/hit.rs`) | `RESULTS.md`, `tests/{differential,hit,edges}.rs` |
| `crates/bough-mem` | one GPU-visible region carved into fixed `Col<T>` columns (no mirror pass) | `src/lib.rs` |
| `crates/bough-text` | text engine: table shaper (dense step table, digit patches, NumFmt), byte slots + NEON classifier, ring of lines, line-local `Para` (hot/cold), delta text model, variable-font instance tables, table caches; **now built on vendored parley** (the table/slots/shape modules live in `vendor/parley/src/bough/` and are re-exported) | `RESULTS.md` Parts I–III (+ the parley commits on `text3`), `tests/{exact,slots,lines,para,variation,parley_twin,breaks,…}.rs` |
| `crates/bough-metal` | direct-Metal presenter `fused` (objc2): binned compute repair (`fs_bin`, `fs_tile`, `fs_copy`), bindless bind table `FBind`, pre-armed stream released by shared events, publisher thread, `frame()` / `frame_paced()`, residency set, content-addressed ring, resize, GPU-prep thread; plus the research presenters (`v2`, `gpu`, `present`, `scene`) kept as its benchmark harness | `RESULTS.md` Parts I–IV (§14.9, §15), `INTERFACE.md` (region layout, bind table, kinds, transforms, transitions) |
| `crates/bough-gpu` | the same presenter on **vendored wgpu-hal**, kernels in **wgsl-rs**, build-time wgsl-rs → WGSL → naga → MSL → `xcrun metal` → one metallib; `Fused::adopt` / `RegionMem` | `RESULTS.md`, `build.rs`, `src/{hal,fused}.rs`, `src/shaders/` |
| `crates/bough-damage-lab` | the damage-model lab (stamps rejected, tiles adopted) | `RESULTS.md` |
| `crates/bough` | the façade: `Signal`, `sig!`, elements/operands, `App`, runtime `rt.rs`, layout (cephal "deep-kids"), text glue, input (`input.rs`), templates, decoration kinds (`deco.rs`), transforms, transitions (`transition.rs`), inspect (`inspect.rs`), and on `api`: lists (`list.rs`, `listview.rs`), `reader.rs`, `entity.rs`, `describe.rs`, `changed.rs`, `select.rs`, `lanes.rs`, `boxops.rs`, `reflect.rs` | `API.md` (root), `RESULTS.md`, `LAYOUT.md`, `INPUT.md`, `INSPECTOR.md`, `API-RESULTS.md` |
| `comparisons/bough-heavy` | zgx heavy (stream/scroll/both/idle), headless + windowed + micro | `tests/{correctness,gpu_bounds}.rs` |
| `comparisons/bough-scenes` | zgx lab, animate, blink, micro state | `tests/{correctness,present}.rs` |
| `comparisons/bough-vlist` | zgx vlist (measured variable list), micro vscroll | `tests/correctness.rs` |
| `comparisons/bough-zgui` | zgui's whole perf suite (gallery, shell, table, label churn, resize, cold start), drag probe, startup_bench, **the inspector app** (`src/inspector.rs`) | `tests/{input,drawing,transforms,transitions,inspector,first_pixel}.rs` |
| `comparisons/bough-text-bench` | zgx text_bench (stock SF NS), editor_streaming (delta and whole-string), i18n, code editor (engine), label churn | `tests/correctness.rs` |
| `comparisons/bough-suite` (api) | zgx suite.py ports: mount, teardown, update, list-state, script-layout, anime, style, i18n, variable-list, core, startup, stream-memory | `tests/{lists,virtual,reflect,frame,layout_screen}.rs` |
| `comparisons/bough-widgets` (api) | zgx widget_bench port | |
| `comparisons/bough-app-mail-dash`, `bough-app-sketch-code` (api) | zgx `apps_bench` ports (in progress) | `bench/`, `tests/` |
| `vendor/` | wgpu, wgpu-core, wgpu-hal, wgpu-types, naga, wgpu-naga-bridge, wgpu-core-deps-apple 29.0.4; winit 0.30.13; wgsl-rs b2de16a; parley 0.11.1, fontique 0.11.1, harfrust 0.12.0, skrifa 0.44.0 | `vendor/README.md` (provenance and every local change) |

### 1.3 Documents

| doc | content |
|---|---|
| `DESIGN.md` | architecture (§0 win conditions … §10 measurement) |
| `API.md` | public API and how it lowers; "As built in round 3"; "Round 4" (reflection, virtualization, cleanup) |
| `STATUS.md` | the fused system's numbers vs zgx (round 2/3 era), soundness audit (§4), windowed-suite commands (§6) |
| `research/00-bbtree-synthesis.md` … `04-reformulation-method.md` | bbtree, damage, trees/SIMD, reactivity/memory surveys; the Joffe reformulation method |
| `research/ROUND3.md`, `research/ROUND4.md` | agent briefs; ROUND4 = the user's direction (built-in primitives, vendoring, wgsl-rs) |
| `crates/*/RESULTS.md`, `INTERFACE.md`, `INPUT.md`, `INSPECTOR.md`, `API-RESULTS.md`, `LAYOUT.md` | per-area results, designs, rejected attempts |
| `bench/baselines/{TARGETS.md,macos/SUMMARY.md,linux/SUMMARY.md}` | zgx and zgui baselines (zgx 06debe9d, zgui 951a0ea) |
| `bench/windowed/README.md` | zgx's macOS windowed suite runner with a `bough` entry per scene |
| memory | `~/.claude/projects/-Users-mileswirht-Downloads-designs-reactivity/memory/` (mission, references, routing, method, direction) |

### 1.4 Measurement tools

| tool | measures |
|---|---|
| `bench/integrate/slope.py <fw> <render> <scene>` | per-frame instructions (process `ri_instructions`, TICKS 300 vs 600), bough or zgx, heavy/lab/animate |
| `bench/layout/slope.py <fw> <render>` | the same for vlist |
| `bench/floor/slopes_ab.py` | two bough builds interleaved per cell (`BOUGH_A`, `BOUGH_B`), e.g. direct Metal vs wgpu |
| `bench/floor/gputime.c`, `gpu_slope.py`, `gpu_pairs.py`, `windowed_gpu.py` | GPU time per process from the I/O Registry plus the GPU clock (no root, no Screen Recording); ABAB pairs |
| `bench/integrate/windowed.py`, `bench/results/fusion/run_windowed.sh` | 10 s windows: CPU %, instr/s, footprint, ring stats |
| `bench/integrate/micro.py` | zgx `micro_bench` semantics |
| `bench/input/run_all.sh` | bough's side of zgui's suite; zgui's side: `bench/results/input/zgui/` (+ the instruction-count patch) |
| `bench/api/bough/run.sh`, `bench/api/zgx/` | zgx suite.py scenes (both sides, `/usr/bin/time -l`) |
| `bench/zgx/build.sh`, `build_text.sh` | zgx headless drivers / text drivers into `target/zgx-headless`, `target/text3-zgxbin` |
| `bench/windowed/run_macos_comparison.py` | zgx's own windowed suite with bough entries (not run: §8) |

Environment, always:

```
export RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH          # rustup nightly-2026-10-17 (rust-toolchain); the nix cargo on PATH is wrong
export CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/<branch>
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo check --workspace --all-targets      # examples and benches too: agents twice left stale examples
cargo test --workspace --release
```

### 1.5 References

| ref | where |
|---|---|
| zgx (zeronsh/zgx) | `~/projects/_ref/zgx`, checkout 06debe9d (benchmarks); `origin/main` fetched 2026-11-01 = ced61a79 (reflective interface, §5) |
| zgui (zortax/zgui) | `~/projects/_ref/zgui` (951a0ea) |
| bbtree | `~/projects/_ref/bbtree` (design docs 00–16) |
| ripple (trueadm) | `~/projects/_ref/ripple` |
| GPUI rework archive | `~/projects/_ref/archive` (blobless; `archive-index`, `local/architecture-performance` 0eb9b0caae; inspector-*-sol, wgsl-buildtime-sol, codex/wgpu30, local/gpu-scroll-copy) |
| gpui-ce | `~/projects/gpui-ce3`: `origin/inspector`, `origin/reflection`, `parley_stack_01…06`, main's `transition.rs` / `style_transitions.rs`, `gpui_wgpu`, `gpui_render` (wgsl-rs pin) |
| Loupe inspector | `~/projects/gpui-ce-inspector` (`codex/loupe-production`, `crates/gpui_inspector`) |
| cephal | `~/Downloads/ilo-prod-work/cephal` |
| local design packet | `~/Downloads/designs/reactivity` (the GPUI-rework API contract) |
| hosts | Mac M3 Pro (the only GPU/display box); ilo-prod `root@87.99.136.215` (Ryzen 3700X, NixOS, `/root/work/env.sh`); mothership `miles@` (Xeon, nushell login, disk nearly full) |

---

## 2. Numbers (what is won, what is not)

### 2.1 zgx desktop scenes, instructions per frame (slope)

**Verified** on canonical ca6412f (direct Metal, before scroll by copy). R0 = through the GPU's input; R1 = plus the GPU
frame.

| scene | zgx R0 | bough R0 | zgx R1 | bough R1 | R1 × |
|---|---:|---:|---:|---:|---:|
| heavy stream | 2,982,643 | 160,037 (text3 3b: ~148 K) | 6,190,927 | 263,047 | 23.5 |
| heavy scroll | 161,932 | 26,708 | 1,443,768 | 129,243 | 11.2 |
| heavy both | 3,153,118 | 193,515 | 6,896,602 | 299,690 | 23.0 |
| lab both | 813,004 | 27,842 | 1,408,099 | 133,402 | 10.6 |
| animate | 226,715 | 13,753 | 816,343 | 134,555 | 6.1 |
| vlist scroll / fling / jump | 108,843 / 178,601 / 335,715 | 13,154 / 20,112 / 36,730 | 1,140,929 / 1,308,185 / 1,333,209 | 126,254 / 131,798 / 145,407 | 9.0 / 9.9 / 9.2 |

Idle is about 0 for bough, against zgx's 554 K (heavy) and 114 K (lab) at R1. The R1 floor is Metal's per-command
cost: the bindless pre-armed stream with the publisher thread brought it from ~200 K to ~100 K.

**wgpu-hal vs direct Metal (agent-reported, `bench/results/floor4/slopes_r1.txt`, R1):** wgpu/direct per scene was
0.81–1.33:

| scene | direct | wgpu |
|---|---:|---:|
| heavy stream | 290 K | 335 K |
| heavy both | 333 K | 443 K |
| lab both | 150 K | 154 K |
| vlist | 129–152 K | 139–160 K |

The pre-armed per-frame floor itself is at parity (106.6 K vs 105.6–116.3 K). The rest of the gap is the unfinished
raw-encoder hot path (§7.2).

### 2.2 Windowed 10 s (agent-reported, fused system)

| measure | result |
|---|---|
| CPU % | zgx → bough: heavy both 7.2 → 1.6, lab both 2.8 → 1.4, animate 3.6 → 1.8, vlist 2.2–2.5 → 1.3 |
| footprint | 26–37 % below zgx on active scenes, 65 % below on blink |
| skipped frames | 0 in every window |
| interrupt wakeups | 60/s against zgx's 180–317 |

After floor 3b, instr/s fell a further ~10 %.

### 2.3 zgui `cargo xtask perf` (agent-reported, zgui re-run the same session; CPU-only, both null-rendered)

| measurement | zgui instr | bough instr | × |
|---|---:|---:|---:|
| hover.crossing | 1.83 M | 4,277 (**verified**) | 428 |
| kitchen.keystroke | 4.45 M | 3,424 (verified 3,419) | 1,299 |
| kitchen.tab_traversal | 4.53 M | 3,103 (verified 3,098) | 1,461 |
| kitchen.glide_tick | 10.5 M | 21,113 | 497 |
| shell.tooltip_crossing | 11.3 M | 17,262 | 655 |
| shell.drag_step (laid out) | 3.44 M | 101.6 K | 34 |
| label-churn (30 labels) | 10.9 M | 53,259 | 205 |
| kitchen.resize / shell.resize_step / thread.resize | 29.6 M / 16.4 M / 130 M | 2.97 M / 1.51 M / 21.7 M | 10 / 10.9 / 6 |
| cold.first_frame (like for like: real layout, style, wrap, SVG) | 1,147 M | 145.5 M | 7.9 |

zgx `startup_bench` 185–220 M vs bough 109–117 M. zgx `drag_probe`: bough 117–126 K per move vs zgx 182–220 K.

### 2.4 zgx suite.py and subsystem scenes (agent-reported, `crates/bough/API-RESULTS.md`)

Whole-scene million instructions, zgx → bough:

| scene | zgx | bough |
|---|---:|---:|
| script-update | 3,931 | 653 |
| anime | 3,003 | 658 |
| script-layout | 2,896 | 174 |
| list-state | 844 | 187 |
| style | 1,247 | 697 |
| i18n | 1,252 | 384 |
| core | 87,724 | 1,328 |
| text | 4,541 | 228 |
| stream-memory | 1,391 | 497 |

Per unit, zgx → bough:

| unit | zgx | bough |
|---|---:|---:|
| list-state, per tick | 3.33 M | 3.9 K |
| style, per frame | 2.0 M | 0.12 M (lanes) |
| anime, per frame | 2.37 M | 0.096 M (lanes) |
| script-layout reverse of 1,000 rows | 5.4 ms | 0.17 ms |
| teardown removal, per record | — | 118 instructions |

Losses then were startup, mount vs zgx's Rust-only run, and whole-process teardown. All three were the 120 M font
table; the table cache and pairfast fix it, but they have **not been re-measured on the merged tree** (§7.6).

### 2.5 Text (agent-reported)

| benchmark | zgx | bough | × |
|---|---:|---:|---:|
| zgx text_bench on stock SF NS | 4,464–4,511 M | 113–115 M | 39 |
| editor_streaming, delta form (time per update) | 10.7–12.6 ms | 0.08–0.15 ms | ~100 |
| editor_streaming, delta form (whole run) | 230 M | 41.7 M | 5.5 |
| editor_streaming, whole-string form | | 179 M | |
| i18n | | | 5–9 |
| code editor, engine only | | | 20–64 |

The code face is DejaVu Sans Mono, not Menlo. zgui label churn: 3.1 µs/frame vs 1,348 µs.

Parley grafts (`text3`, grafted vs unmodified parley, `parley_bench`):

| workload | upstream | grafted | × |
|---|---:|---:|---:|
| label churn, per frame | 759 K | 157 K | 4.8 |
| heavy text, per tick | 2.63 M | 419 K | 6.3 |
| log line | 199 K | 13 K | ~15 |
| text_bench paragraph | 913 K | 120 K | 7.6 |
| i18n switch | | | 5.2 |

**The façade does not lay out through parley `Layout` yet** (§7.3).

### 2.6 GPU time (agent-reported, the losing column)

Headless Kcycles per frame (I/O Registry GPU time × mean GPU clock; the clock swung 338–1,143 MHz under load):

| scene | zgx | bough | verdict |
|---|---:|---:|---|
| heavy stream / scroll / both | 446 / 268 / 627 | 313 / 260 / 451 | win |
| animate | 97 | 75 | win |
| lab stream / scroll / both | 63 / 123 / 134 | 141 / 147 / 206 | **loss ~2.2×** |
| vlist scroll | 203 | 495–522; with scroll by copy, median 0.58× of that (322–359) | **loss ~1.6×** |

Windowed animate: 26.3 vs 14.5 Mcycles/s (loss). A full-window copy costs 0.215 ms, against a dense redraw's 0.249 ms,
so copying cannot win vlist. Moving zero pixels can (§7.2).

---

## 3. Architecture as built (one paragraph each)

* **Records and damage.** A record is a paint-preorder slot: depth, ink NRect, kind, payload, handle, clip. Pages carry
  an offset plus an affine. Damage is written at update time into one tile plane per epoch (ring of 4):
  * exact symmetric differences for paint writes;
  * edge bands for geometry;
  * the transformed ink's tile cover for transforms;
  * exposed strips plus show-through bands for scroll copies.

  Repair for a surface of age `a` is the OR of the last `a` planes.
* **The frame.** The CPU writes only the damaged tile rows into the frame slot: row, span, prefix count, 128 bits. The
  GPU's bin pass culls the core pages directly per damaged row, and the tile pass shades 32×32 tiles from the bins.
  The pre-armed stream encodes 16 frames ahead, each waiting on a shared event. A frame is a slot write plus an event
  signal.
  * Publication follows the turn source: `frame()` is for input and publishes the same refresh through the publisher
    thread; `frame_paced()` is for clocks and publishes one refresh later with no wake.
  * Surfaces are content-addressed. Held surfaces serve repeated states with no GPU work (blink, caret, hover).
* **Reactivity.** `Signal<T>` is `repr(C)` with the head first: `Cell<u32>` plus the value, and the QUEUED bit is
  bit 31. `sig!(self.path)` expands to `offset_of!` plus a never-called typed accessor, which proves a struct field.
  A wire is a monomorphic patch fn, with recipes of 16 B or less. Creation order is topological order.
  * Routes are typed: `on_click(|m: &mut Self| …)`.
  * Re-description (`FIXED = false`) is a lockstep walk over a preorder tape.
  * `Reader` is fetch-high / block-low: pending is a HELD bit, and a cohort commits in one epoch.
* **Layout.** cephal over the Bough (deep-kids). Lists are stacked groups placed by a prefix sum. Sized labels skip
  measuring.
* **Text.** One-line labels are digit or window patches. Multi-line text is the ring of lines, then line-local
  paragraphs. Numeric operands render without a `String`. The glyph pool lives in the GPU region.
* **Input.** Hit testing is a range query over the paint preorder plus hit cells, a move inside the cell costing one
  compare. Focus and tab order is a flag-column scan. Hover is colour writes.

---

## 4. The API

### 4.1 As built (canonical + branches)

* **State:** `Signal<T>`, `List<T>` with durable `Slot`s and a per-turn delta log, `Entities<T>` / `Entity<T>`,
  `Reader<T>` with producers, tickets, cohorts, `latest` / `pending`, faults and `boundary`, and `View::changed` on
  any owner.
* **Operands:**
  * values;
  * `sig!(..)`, `.map(f)` (16 B or less), `.num(f, NumFmt)`, `.transition(curve)`;
  * `select` (one keyed fan-out wire);
  * box operands: opacity, translate, width, height;
  * a text colour operand.
* **Elements:**
  * typed builders, `lanes`, templates (`ltemplate.rs`);
  * decoration kinds: image, gradient, shadow;
  * scroll views with `scroll_run_copy`;
  * `transform_subtree`;
  * text fields;
  * tooltips;
  * list regions, virtualized by exposure (api).
* **Reflection:** `reflect::schema::<T>()`, `fields_of`, `set_in(&mut v, "a.b[3].c", "text")`, and on `App`:
  `reflect_fields`, `reflect_set`, `reflect_records`, `reflect_style`, `reflect_set_style`, `reflect_set_color`,
  `reflect_stubs`. Inspection (input): `App::pick_record`, `record_view`, `record_ancestors`, `last_repair`.

### 4.2 Interface changes already made (api, round 4; `API.md` "Round 4")

* `Wired::proven` is removed; arrays use `sig!(self.arr).at(i)`.
* Routes inside list rows and entity views are refused at mount: they would have been dispatched with the wrong
  owner.
* `Lists::regs` is private.
* `List` / `Entities` chunks are held through `Box::into_raw` provenance.
* Moved-text damage reads the Bough's current ink. This was a real stale-glyph bug, found by the reflection gate.
* Earlier: wire heads are validated by address, closing the case of a `Signal` moved between `App`s.

### 4.3 Interface cruft still to strip or decide

1. **Three frame entry points:** `App::frame`, `frame_paced`, `frame_as`. The turn source should pick
   publication; users should never choose. Routes and clocks already know their source.
2. **`App::rt` is public.** A caller mutating it between `frame()` and the next `turn()` in stream mode races the
   GPU's reads (torn pixels, not CPU UB). Make it crate-private; inspection goes through reflection.
3. **Inspection accessors on `App`** (`pick_record`, `record_view`, `record_ancestors`, `last_repair`, `reflect_*`):
   fold them into one `App::inspect()` handle, or the devtools session (§5), instead of widening `App`.
4. **`sig!`'s two forms** (`self.path`, `v: Type => path`). Keep only one if the value form can be inferred.
   `prove` is `#[doc(hidden)] pub unsafe` only because of the macro's expansion.
5. **`NumFmt` re-exported at the root** for `.num(f, fmt)`. Consider `.num(f)` with `fmt` from the operand's type.
6. **`Mounter`'s fields are crate-private**, so custom elements outside the crate are impossible. Decide the public
   element-authoring API: the zgx-style component libraries need it.
7. **Leftover research knobs:** the `tile32` no-op feature on bough-core, env vars (`BOUGH_STATS`, `BOUGH_CHECK`,
   `BOUGH_NOFRAME`, `BOUGH_STREAM`), and `sabotage_copy`. Move them behind one `devtools` / `diagnostics` feature,
   or remove them.
8. **Warnings:** keep `cargo check --workspace --all-targets` at zero, including bough-gpu's two dead-field warnings
   on `floor`'s WIP.

### 4.4 Friction from writing the inspector on the public API (`crates/bough/INSPECTOR.md`)

| # | friction | status |
|---|---|---|
| 1 | no "record under this point" | fixed: `pick_record` |
| 2 | layout box unreachable | fixed: `record_view` |
| 3 | last frame's repaired tiles unexposed (stale after a full repaint) | fixed: `last_repair` |
| 4 | kind byte had no names | fixed: `kind_name` |
| 5 | quads have dynamic width and span but no dynamic x | **open**: a rect operand `.rect(sig)` / `.at(sig)` (api's box operands cover layout boxes, not quads) |
| 6, 9 | a state-driven variable list had to be a fixed pool of `TREE_ROWS` texts over one `Signal<Vec<String>>` | **largely addressed** by api's virtualized lists; the inspector must be ported onto them |
| 7 | transitions are identified by lowering order | **open**: reach a transition through the property it drives (reflection) |
| 8 | one `App` per run loop; no second window | **open**: a multi-window host (winit) |
| 10 | editing target state needs `&mut App<T>` + `Edit` | **available on api** (`reflect_set`), not yet wired into the inspector |

### 4.5 The API in one example: the design's counter

`crates/bough/examples/counter.rs` is `designs/reactivity/final/examples/progressive.rs` rung 1 in bough. Its test
is `crates/bough/tests/counter.rs`, **passing**: it finds the button by its interactive record's layout box, clicks
it three times through hit testing, and checks count == 3 and presented == full redraw.

* `bough::prelude` is the design surface (`crates/bough/src/ui.rs`): flow text, buttons and boxes laid out by
  cephal, `const` `Flex` recipes, routes as methods, and `Application::run`.
* `View::FIXED` is gone. The runtime never read it, so no measured path changed. After the removal and the
  prelude split, `cargo check --workspace --all-targets` has zero warnings, and the `bough` and `bough-zgui`
  suites pass (35 tests, 0 failures).
* The benchmark ports moved to `bough::raw`, which is the old prelude unchanged.

```rust
use bough::prelude::*;

const BAR: Flex = Flex::row().align(Align::Center).gap(12.0);

pub struct Counter {
    pub count: Signal<u32>,
}

impl Counter {
    pub fn inc(&mut self) {
        *self.count += 1; // a publish: the label's port patches, `view` does not rerun
    }
}

impl View for Counter {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .layout(BAR)
            .p(24.0)
            .child(text(sig!(self.count).map(|n| format!("Count: {n}"))))
            .child(button("+").on_click(Counter::inc))
    }
}

fn main() {
    Application::run(Window::document(Counter { count: Signal::new(0) }).title("Counter"));
}
```

Left against the design:

* `&self.count` / `self.count.map(f)` as ports without `sig!` (offsets proven against the `type_info` schema).
* `El<'_>` as the return type.
* Placement (`.align`, `.at(handle)`).
* `Handoff`, `Target`, `.with(..)`, `#[command]`.
* Rung-0 reruns: plain-field handlers re-describe; this is built on `api`, and decided from the owner's type by
  `type_info`, with no annotation.

---

## 5. A true reflective interface

The goal: anything running on bough can be seen, queried, measured and driven, by a person (the inspector), an agent
(CLI/MCP) or a test (`TestApp`). This has to work with **zero opt-in from component libraries** and **zero cost when
nobody is looking**.

### 5.1 The reference: current zgx (`origin/main` ced61a79, 2026-11-01; `docs/agents/inspect.md`, `docs/design/plugin-hooks.md`)

zgx's devtools are one protocol over a Unix socket (mode 0600, `$TMPDIR/zgx-inspect-<uid>`), a CLI
(`zgx inspect …`, text or `--json`) and an MCP stdio server. All three serve the same commands.

* **Targets:** one selector language, shared by inspect, `TestApp` tests and plugin hooks:
  * `#id`, `"name or text"`, `role:button`, `@n` (a stable node number), `.class` and `.a.b`;
  * `a >> b` (descendant), `#list row 3` (a list's row, mounted or not), `"X"[2]`, `hook:name`.

  Several matches are an error that lists them. Hidden nodes match, with a rule for the one a person can reach.
* **Reading:**
  * `tree [target] [--depth]`: `@n #id role "name" ="value" x,y wxh` plus flags;
  * `node`: kind, **the source line that made it**, role, name, value, state, box, text;
  * `find`;
  * `state [name] [--full]`: named states and their values;
  * `errors`, `lint`.
* **Frames:** `frames N [--since mark] [--runs] [--keep]` gives each frame's time, the states written, the runs they
  caused with their total, phases (input, script, update, style, layout, text, damage, paint, encode, submit, present,
  wait) with instructions, the costliest runs with **the cause that queued them**, and **`why:` notes for fast paths
  missed**. Examples: a scroll repainted because a node draws over it; something shows under it; a cadence hold. It
  also gives the GPU's own time. `mark` sets a point to read from.
* **Seeing:** `snapshot target --out png` (as the window last drew it); `search` (find in page, including virtual
  lists' unmounted rows).
* **Memory:** `memory [--human]` reports every holder: process footprint, heap, GPU (atlas, textures, layers, …),
  text caches, scene nodes, devtools' own, VM heaps, images.
* **Acting:** click, hover, right-click, drag (stepped, crossing what lies between), type, key, scroll, command (a
  named command as its menu or shortcut runs it), `set <state> <json>`, `wait target [--text|--gone]`, sleep.
  Popovers, menus and tooltips are in the tree under their opener.
* **Windows:** `windows` lists them, and every command takes `--window`.
* **Gestures:** `record` / `record stop --out tape`, `replay target tape [--repeat N] [--every ms]`, through the
  window's own input path.
* **Switches:** runtime toggles of fast paths (`scroll_copy`, `refresh_grid`, `partial_repaint`), so a replay can be
  measured each way in one process.
* **Plugins:** `ui.contribute { target, before | after | replace }` and `ui.filter`, using the same selectors.
  `private = true` opts a subtree out; `hook = "name"` declares a stable point.
* **Safety and cost:** devtools are behind a build feature. Nothing listens or keeps books until a host serves them.

### 5.2 What bough has toward it

| piece | where | state |
|---|---|---|
| typed value reflection, no opt-in (type_info) | `api`: `reflect.rs`, `App::reflect_*` | built; gates `tests/reflect.rs` |
| edits as ordinary writes (a reflection edit queues the cell's head → wires, damage, effects) | `api` | built |
| record reflection (element type name, layout box, cephal `Style` reflected and editable, paint, text) | `api` | built |
| virtual content as typed placeholders (`Rt::stubs`, `reflect_stubs`) | `api` | built |
| hit testing / pick through transforms | canonical / `input` | built |
| synthetic input entry points (pointer, wheel, keys, text) | `crates/bough/src/input.rs` | built (headless + window host) |
| inspector app (pick, tree, overlay of ink/layout/damage) | `input`: `bough-zgui/src/inspector.rs` | first cut; state panel not wired to reflection |
| per-frame counters | benches only (`proc_pid_rusage`, `BOUGH_STATS`) | no in-process frame log |
| GPU time | `bench/floor/gputime.c` (external), command-buffer timestamps not collected | not exposed |

### 5.3 What has to be built (in dependency order)

1. **A semantic side column.** For every record: `role`, `id`, `class` set, accessible `name`, `value`/state flags,
   `hook`.
   * It comes from the typed builders: a `button` is `role:button`, `.id("send")`, `.class("tool")`, the label's
     text is its name.
   * It must be sparse: only records that have a semantic pay for it.
   * Accessibility (AX) reads the same column, so the inspector and VoiceOver agree.
   * No opt-in: every builder fills what its type implies.
2. **Provenance.** Put `#[track_caller]` on every builder and operand method, and intern `Location::caller()` into a
   static table. Each record carries a `u32` location index, which answers "the line that made it". The column
   compiles out without the `devtools` feature.
3. **One selector language** (parser plus resolver over the Bough preorder and the semantic column):
   * `#id`, `"text"`, `role:`, `@n`, `.class`, `>>`, `row n`, `[n]`, `hook:`;
   * `row n` resolves through the virtualization placeholders, materializing on demand for actions;
   * `search` reads unmounted rows' source data through reflection on the list's items.

   The same parser serves inspect, `TestApp` and (later) plugin targets.
4. **Named state everywhere.** Reflection must cover every live owner, not just the root: list rows, entity views,
   readers. That needs a registry of live owners (owner class plus address, maintained by mount and release), so
   `state` and `set` reach all of them.
   * Names are field paths from the schema (`Inbox.selected`, `rows[3].unread`).
   * Enums: open them in value walks. This needs the active variant's index. `type_info` has no per-variant
     discriminant (`VariantId` exposes `name` / `non_exhaustive` only), and `discriminant_value` returns the
     declared discriminant. **Options:**
     * (a) a small rustc extension (`VariantId::discriminant()`), carried in our toolchain until upstream;
     * (b) treat enums with only default discriminants as index == discriminant, if `type_info` can say a type has
       no explicit discriminants;
     * (c) keep `Debug` only.

     Decide.
5. **The frame log.** A ring of N frames, zero-cost when not serving (a monomorphized `App<D: Devtools>` or one
   per-frame flag). Per frame:
   * time, and phases with thread instruction counts (input, settle/wires, describe, layout, text, damage, slot,
     encode, present, wait);
   * signals written (named through reflection);
   * wires run with **their cause** (signal path → wire → record);
   * damaged tiles, and GPU time (command-buffer `GPUStartTime` / `GPUEndTime` from the publisher's completion);
   * **`why:` notes** at every fast-path refusal: scroll copy refused (what draws over the clip), content addressing
     missed, a bin overflow redraw, a stream drain and re-arm, a cadence hold, a layout fallback.

   Most of these sites already exist as counters in `BOUGH_STATS`.
6. **Memory by holder:** region columns (len/cap per `Col`), pages, glyph pool, instance pool, text caches (strikes,
   atlas, para cold halves), GPU surfaces / residency / bins, heap (malloc zone stats), `phys_footprint`.
7. **Snapshot:** the presented surface's region (the held surface, or `reference_pixels`), written as PNG.
8. **Acting and waiting:** click, hover, drag in steps, type, key, scroll, command, set, plus `wait` evaluated after
   each turn. These go through `input.rs`'s entry points, so they exercise the real paths.
9. **Switches:** runtime bits (devtools builds only) for `scroll_copy`, content addressing, partial repaint (force
   full damage), pre-armed stream vs one command buffer, edge-band damage. Release builds keep these compile-time.
10. **Record/replay tapes:** input events with timestamps and phases, replayed through the host's input path, with
    `--repeat` statistics.
11. **Transport:**
    * a Unix socket (0600) speaking JSON lines;
    * `bough inspect` as a CLI;
    * an MCP stdio server with the same tools;
    * all behind `devtools`, with nothing running until served.
12. **Windows:** a multi-window host (vendored winit, §7.2). Every command takes a window.
13. **`TestApp`:** a headless `App` plus the same targets and actions, for tests.
14. **The inspector UI.** An ordinary bough app on this protocol, in its own window, with panels:
    * tree;
    * node, with provenance;
    * state (edit through `reflect_set`);
    * styles (edit through `reflect_set_style`, relayout live);
    * frames, with the why-notes;
    * memory;
    * a damage-tile overlay;
    * switches;
    * transitions by property.

    Its own friction log (§4.4) is the API's test.
15. **Plugin hooks (later):** contribute/filter by target, `private` and `hook` points, over the same selector
    resolver.

The inspector is the place to judge the API ("does it improve life?"). Keep `INSPECTOR.md`'s log going, and fix the
API rather than working around it.

---

## 6. Round-4 direction, item by item (`research/ROUND4.md`)

| item | state | where |
|---|---|---|
| virtualization as damage, not lists | **built (api)** as exposure: list regions mount only rows meeting what is visible of them, follow scroll / container moves / visible-rect changes, and release two-phase; placeholders are typed (`Rt::stubs`). It differs from the stub-record design: placeholders are not Bough records, exposure is per-region (O(regions)), and the extent index is per region. **Not yet:** the template `virtual_list` (zgui's long list) is still separate; containers other than list regions (a long plain column, a grid, a document) are not virtual. | `api` listview.rs, rt.rs, tests/list_window.rs, bough-suite/tests/virtual.rs |
| transforms as a core primitive | **built (input)**: page affine generalizing offsets, exact transformed tile covers, inverse hit testing, per-instance affine in kernels, `transform_subtree` | `input` b0a65e7, tests/transforms.rs |
| typed reflection, no opt-in | **built (api)** on `type_info` (§5.2); enum walks open (§5.3 item 4) | `api` reflect.rs |
| transitions built in | **built (input)**: `.transition(curve)` on any operand, layout transitions relayout per frame, `smooth` is velocity-continuous; ~320 instructions per running colour transition per frame (CPU form). **Not yet:** the GPU-evaluated paint transitions (lead's steer: CPU per frame = OR of tile masks). | `input` 9f23a52, tests/transitions.rs |
| inspector, dogfooding the API | **first cut (input)** + friction log | `input` 110c440 |
| interface cleanup | **partly (api)** (§4.2); the rest in §4.3 | API.md "Round 4" |
| text = vendored parley + grafts | **vendored + 6 graft steps (text3)**, each twin-gated against upstream parley; bough-text's engine moved into `vendor/parley/src/bough/`. **Not yet:** the façade on parley `Layout`; PlainEditor; Menlo; multi-core; SIMD ligatures; retiring bough-text's own path | `text3`, vendor/README.md |
| wgpu + winit vendored, wgsl-rs shaders | **wgpu-hal presenter with all kernels in wgsl-rs (build-time metallib)**, façade selects it with `--features bough/wgpu`, every pixel gate green on it (floor 3379273). **Not yet:** parity on whole scenes (0.81–1.33×), the raw-encoder hot path (WIP), startup overlap, the winit host, wgpu as default, removing bough-metal | `floor`, crates/bough-gpu/RESULTS.md |

---

## 7. Outstanding work, by area

Each item has its design pointer. "Agent design" means a concrete design is written in that area's RESULTS file.

### 7.1 Core, damage, virtualization

1. Generalize virtualization to every container: a long `column`, a grid, a document, a tree view, not just list
   regions. Unify the template `virtual_list`. Decide between per-region exposure (api's) and stub records in the
   Bough's page summaries (lead's), by measuring a 1M-row grid and nested scrolls. Memory must be O(visible).
2. Anchoring when a materialized size differs from its estimate: keep the first visible record fixed, through the
   scroll record, so scroll by copy covers it.
3. `parent` / `ancestors` queries still lose to pointer chasing. `freeze_into` is O(pages) (81 K at 100 K nodes); give
   it a change counter.
4. Per-bar strip damage was rejected at 32–65 K a tick. Find a representation where precise strips are free (heavy
   repaints ~1,058 of 4,000 tiles).
5. Transforms combined with scroll by copy: a rotated subtree inside a copied scroll. Add a gate for exactness, since
   f32 rounding under rotation may not be translation-invariant.

### 7.2 GPU / presenter (vendored wgpu-hal, wgsl-rs)

1. **Finish the raw-encoder hot path** (floor WIP db078e0: `raw_compute_encoder` / `raw_pipeline` in vendored
   wgpu-hal). Reach parity with direct Metal on every scene (`bench/floor/slopes_ab.py`), then make wgpu the default
   and delete bough-metal's frame path. Its research presenters stay only as harnesses.
2. **The winit host**: Accessory activation, no focus stealing, **multiple windows on one event loop** (shared
   device, queue, metallib and pipelines; per-window surfaces), resize on its path. Strip winit's startup.
3. **Startup below the device floor**: `MTLCreateSystemDefaultDevice` is ~22 ms. Overlap it with mount, layout and
   shaping (input built `prepare_in_background` for direct Metal; port it to `Fused::adopt` on wgpu).
4. **GPU time on lab and vlist (the losing column):**
   * **Move zero pixels on scroll.** Either a compositor-scrolled tall layer (CALayer position / contentsRect) or a
     wraparound strip texture with an offset in the final composite. Repaint only exposed rows. Weigh footprint.
   * **A GPU-driven raster path for text-dense rows.** A compute cull writes an indirect draw; one render pass draws
     with load/store only where geometry lands. Start from round 2's `vs_vclip`. Choose per row by representation.
   * **Remove the bin-scan / staging cost** (47–64 % of GPU time). Options: glyph runs pre-binned by row at write
     time, bins patched by delta, 2D binning, 16-px quadrant masks.
   * Measure with ABAB pairs (`bench/floor/gpu_pairs.py`): median ratio per scene, both frameworks. The full table
     has never been run.
5. GPU-evaluated paint transitions (§6).
6. Half-precision accumulation on the wgsl-rs path, if the GPU pairs show a gap (f32 today; wgsl-rs f16 is
   unverified).

### 7.3 Text (vendored parley)

1. **Put the façade's text on parley `Layout`** (labels, lines, paragraphs, wrapping, inline runs of mixed faces and
   sizes, bidi) with the grafts as its fast paths. Keep every win in §2.5 and the vlist R0 cells. Then retire
   bough-text's own layout path.
2. **The editor on `PlainEditor`**, fed by the recorded edits (`splice` / `push_str`): caret, selection, scroll
   (picks up scroll by copy). Measure code_editor_bench whole-frame at R0 and R1, with pixel gates.
3. Menlo through harfrust AAT (stock text_bench with no caveat). Kerning across segment boundaries beyond Latin-1.
4. Multi-core shaping of large documents: `ChunkedDoc`, exact placement by a prefix sum of byte counts, output
   identical for any thread count (agent design, RESULTS III.0b).
5. The SIMD ligature tokenizer: closed-form f-run parse (agent design, III.0b).
6. Port `bough/script.rs` from ttf-parser to read-fonts (one font stack).
7. A process-wide shaped-text cache keyed by (string, size). This is the whole-process teardown loss (§2.4).

### 7.4 Input / interaction / UI

1. A rect operand for quads (§4.4 #5). Transitions addressable by property (#7).
2. Port the inspector onto virtualized lists and reflection (§5.3).
3. Accessibility: an AX tree from the semantic column (§5.3 item 1). IME (marked text), clipboard, cursor shapes,
   OS drag and drop, context menus, popovers/menus as windows, DPI changes.
4. Still not drawn: gradient-filled text, non-uniform borders. Inline formatting comes through parley (§7.3).
5. Real inline layout (currently blocks), through parley.

### 7.5 API

1. Strip the cruft in §4.3. Publish a stable element-authoring API (custom elements outside the crate).
2. Re-describable producers. Entity views as re-describable keyed items are already built (api).
3. One frame entry point (§4.3 #1).
4. Reflection: enums (§5.3 item 4); a registry of live owners; a schema larger than 256 nodes (currently
   `truncated`); `TypeId::info` ICEs on tuple types in this toolchain (worked around by a trait).

### 7.6 Benchmarks still to cover or re-measure

1. **Re-measure on the merged tree:**
   * every STATUS §1 cell;
   * zgx suite whole-process cells (startup, mount, teardown);
   * zgui suite;
   * text;
   * GPU pairs.

   Every agent measured its own branch only.
2. **zgx ports in progress (api WIP):** `apps_bench` mail, dashboard, sketch, code. Not started: zgx-gpu `layout`
   (rich text, a dragged resize) and `frame`; `callback_mount`; script-layout with `animate`; `state` (Luau-only:
   no counterpart). The remaining zgx desktop examples are interactive or need a second display: `input_latency`,
   `key_workload`, `field_workload`, `display_transition`.
3. **zgx's own windowed suite** (§8).
4. Visual parity re-checks after the wgpu port (heavy, lab, animate vs zgx `render_parity`).
5. zgui: all scenarios ported; keep its instruction-count patch with the zgui copy (`bench/results/input/zgui/`).

### 7.7 Soundness and robustness

* `App::rt` public (§4.3 #2).
* GPU index masks exist on direct Metal. Confirm on the wgsl-rs kernels (`gpu_bounds` passes on wgpu per floor
  3379273).
* Every `unsafe` keeps its `// SAFETY:`. Re-audit `reflect.rs` (vtables from `trait_info_of`, offsets from
  `type_info`) and `list.rs` chunk provenance.

---

## 8. Measurements that need the user

zgx's windowed suite (`bench/windowed/run_macos_comparison.py`, bough entries ready, dry run passes) needs:

* **Screen Recording** permission for the terminal running Claude Code (System Settings → Privacy & Security → Screen
  Recording), for its pacing and blank-row captures;
* **Automation for System Events** (its preflight fails with −1743);
* windows open 23–39 s, longer than the 12 s limit used so far;
* an idle desktop, about 1 h per scene.

GPU Mcycles/s, WindowServer CPU and the p99 frame interval are unmeasured for both frameworks until then. GPU time
headless and windowed can be measured without permissions (`bench/floor/`).

---

## 9. Merge state and procedure

| branch | tip | contents | merged into canonical? |
|---|---|---|---|
| `canonical` | see `git log` | fusion + input (round 3/3b) + text3 (3b) + floor (3b, scroll by copy, vendored GPU stack, bough-gpu) | — |
| `input` | 83f5519 (merged) | 3b (like-for-like cold start, drawing, resize scenarios, first pixel across cores), transforms, transitions, inspector | see §9.1 |
| `text3` | 9c176bd | vendored parley + grafts 1–6, twin gates | see §9.1 |
| `floor` | db078e0 | bough-gpu complete on wgpu-hal (`bough/wgpu` feature, all pixel gates green at 3379273) + raw-encoder WIP | see §9.1 |
| `api` | ae360b7 | round 3 (List/Reader/Entity/re-description/suite ports) + round 4 (reflection, virtualization as exposure, cleanup) + app ports WIP | see §9.1 |

### 9.1 Merge log (this session)

* **`input` → canonical** (ed88c97, 507b19c). It conflicted with floor's scroll by copy in five files:
  * `FBind` keeps `r[4]` first and appends `img` (224 B; the GPU interface is append-only);
  * the background GPU-prep thread (`GpuPrep`) also builds `pso_copy`;
  * `App::draw` syncs images, then draws with the copy plan;
  * `rt.rs` keeps both `scroll_band` and `edge_band_of`;
  * `Cargo.lock` was regenerated.

  132 tests green, zero warnings.
* **`text3` → canonical** (the vendored parley stack plus grafts 1–6). The conflicts were the workspace manifest
  (canonical's members plus text3's comment), `vendor/README.md` (one file with a GPU-stack section and a text-stack
  section) and `Cargo.lock`. `cargo check --workspace --all-targets` was clean, and the full suite passed: 143 tests, 0 failures. That run
  finished just after the first push, on the merge commit before the counter was added.
* **`api` → canonical** (ec1d34aae0, 2026-11-11), without `FIXED`.
  * `View::FIXED` was removed from the trait and from all 84 declarations. Application views re-describe after their
    handlers. `Config::mount_once` (`Config::once()`) is a harness-only option, which the scene benchmarks set to
    measure the wire path alone.
  * The commit message lists every hand resolution:
    * prelude gains List/Slot/Changed/Entity/Entities/Reader/boundary/list;
    * `quad_geom` keeps edge-band damage and gains box translation;
    * `Arc<Shaper>` from the process font table for every face;
    * listview replaces measured_list, and its scroll path uses scroll by copy;
    * canonical's elements step over a re-description as opaque parts;
    * the text3 measured-list gate was ported onto `index_list`;
    * the zgx dev-dependencies are pinned git revs.
* **`floor` → canonical** (81e393a919). It brings bough_gpu::fused on wgpu-hal behind `--features wgpu`, and
  fs_bin/fs_tile in wgsl-rs. `App::new` keeps both start paths: wgpu's threaded device boot with region adoption,
  and direct Metal's `prepare`/`Fused::new_on`. **Caution, unchanged:** the wgsl-rs kernels lack the transform and
  decoration kinds (AUDIT F10).
* **Verification of the two merges:** `cargo check --workspace --all-targets` passed before the last edits. The test
  suite was **not run** (stopped on request). Run `cargo test --workspace --release` before anything else.
* **canonical → `main`** (the archive's default line, 2026-11-11). The merge allows unrelated histories, with bough
  at the repository root beside `reactivity/` and `designs/`. bough's `design/` copy was dropped: it was a byte-identical
  subset of `designs/reactivity/`, and references now point there. The archive index was copied into `archive/`.

### 9.2 Procedure for any later merge

1. Merge into the main checkout (`--no-ff --no-commit`).
2. Resolve conflicts, keeping both mechanisms where they are additive. `FBind` and other GPU interfaces are
   **append-only** (`r[4]` then `img`). Regenerate `Cargo.lock` from the base.
3. Run `cargo check --workspace --all-targets` (zero warnings), then `cargo test --workspace --release`.
4. For anything touching damage or the presenter, run a sabotage check (`--features bough/sabotage_copy` must fail
   the vlist gates).
5. Commit with the trailer. Re-measure the touched STATUS cells with the slope scripts.

---

## 10. Where it is pushed

This work continues the GPUI rework, so it lives in that repository: **`philocalyst/gpui-rework-archive`**.
**`main`** has bough at the root next to the preserved `reactivity/` checkpoint, `designs/reactivity/` and `archive/`,
and keeps its own history (merged, not rewritten). The `bough/*` branches keep bough's history:

| archive branch | local branch |
|---|---|
| `bough/canonical` | `canonical` (the integrated system, with this brief) |
| `bough/input`, `bough/api`, `bough/floor`, `bough/text3` | the round-3/4 agent branches, at their last state (WIPs labelled) |
| `bough/fusion`, `bough/integrate`, `bough/layout` | historical integration branches |

Vendored crates are plain source in `vendor/` with no history of their own. Provenance (upstream commit, version)
and every local change are listed in `vendor/README.md`; the upstream copy for a diff is the published crate of
that version.

```sh
git -C /Users/mileswirht/projects/bough remote add archive https://github.com/philocalyst/gpui-rework-archive.git
git -C /Users/mileswirht/projects/bough push archive canonical:bough/canonical input:bough/input api:bough/api \
    floor:bough/floor text3:bough/text3 fusion:bough/fusion integrate:bough/integrate layout:bough/layout
```

---

## 11. Risks and lessons

* **Disk:** about 18 GB free. Builds of the vendored stacks plus four target dirs filled it twice. Prune
  `target/<branch>/{debug,release}` and the scratch zgx/zgui clones first.
* **Load:** 15–140 from other sessions. Never trust times; interleave A/B in one process session.
* **Agents' habits:**
  * they skip `--all-targets` (stale examples);
  * they forget untracked files;
  * they measure only their own branch.

  The lead must re-verify after every merge.
* **Nightly features in use:** `type_info` (with `rustc_attrs` for `#[rustc_comptime]`, `ptr_metadata`). The
  toolchain is pinned (`nightly-2026-10-17`). Upgrading may move `type_info`'s API.
* **wgsl-rs subgroup operations** are our addition. naga 29 accepts them without `enable`; keep the fork's patch
  small.
