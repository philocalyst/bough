# bough round 3, branch `api`: the rest of the API, and zgx's suite scenes

`List<T>`, `Entities<T>`/`Entity<T>`, `Reader<T>` (async, cohorts), re-description (`FIXED = false`) and
`changed` are built; the contract changes are recorded in `API.md` ("As built in round 3"). Sixteen of
zgx's suite scenes and examples are ported natively onto bough (`comparisons/bough-suite`), each with a
pixel gate. Apple M3 Pro, macOS 15.7, `nightly-2026-10-17`; bough's `dist` profile, zgx 06debe9d's
`release` profile (the profile `scripts/perf/suite.py` builds; zgx's `dist` is ~1 % fewer instructions).
The machine was shared with other agents the whole time (load average 50–110), so **instruction counts
are the columns to read**; times are indicative.

```
cd /Users/mileswirht/projects/bough-api
export RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/api
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
cargo test --workspace --release                               # every gate (bough: list, describe, reader, entity, layout; bough-suite: 6 test files)
cargo build --profile dist -p bough-suite && bench/api/bough/run.sh   # bough's whole-scene counts -> bench/api/bough/{results.jsonl,raw/}
bench/api/zgx/run_part1.sh; bench/api/zgx/run_part2.sh          # zgx's, the same method (prebuilt release examples; part 2 built out of tree)
BOUGH_DIST=$CARGO_TARGET_DIR/dist python3 bench/integrate/slope.py bough 0 heavy   # STATUS §1 cells (A/B against target/fusion/dist)
```

Method, both sides: `/usr/bin/time -l <binary>` (zgx's `suite.py` `measure`), three runs, the least
"instructions retired" (whole process, every thread). Per-unit numbers are each binary's own report
(zgx's own counters where it prints them: `thread_selfcounts` / `proc_pid_rusage`; bough's binaries use
the same counters for the same spans).

## 1. Tables

### 1.1 Whole scene, million instructions (least of 3)

The zgx-script scenes run Luau scripts; bough has no scripting layer, so each is ported natively and the
zgx number is given twice where zgx has a Rust-native variant of the same workload: its suite scene (what
`suite.py` runs) and its Rust run alone (`ONLY=rust` or the per-scenario `ONLY=`, which still runs both
languages of that scenario). **Every bough process includes the font's shaping tables, built once per
process: 120 M instructions** (`SimpleScript::build`, bough-text). zgx's headless script and core scenes
load no font: their text is laid out by a fallback measurer, with no shaping.

| scene | zgx: what its count includes | zgx | zgx Rust only | **bough** | × vs suite scene |
|---|---|---:|---:|---:|---:|
| mount | 2,000 rows, Luau + Rust runs, each after a 100-row warm-up | 390.9 | 173.1 | **220.9** | 1.8 |
| mount (N = 10,000) | Rust run | – | 676.4 | **428.0** | 1.6 vs Rust |
| script-update | 5 scenarios × (Luau + Rust) × 600 updates | 3,931.2 | – | **653.0** (5 × 1 run) | 6.0 |
| anime | Luau + Rust + direct, 600 frames each | 3,002.5 | 1,221.9 (Rust), 1,044.7 (direct) | **658.0** (wires + lanes) | 4.6 |
| anime, one engine, 600 / 1,200 frames | | | 1,221.9 / 2,644.1 (Rust); 1,044.7 / 2,221.1 (direct) | wires **593.0 / 1,110.1**; lanes **203.3 / 260.1** | |
| script-layout | Luau, without and with `animate` | 2,896.0 | – | **173.7** (without) | 16.7 |
| list-state | Luau, 4,000 messages, 210 ticks | 843.9 | – | **187.3** | 4.5 |
| list-state, 400 ticks | | 1,525.7 | – | **187.6** | 8.1 |
| style (boxes) | Rust, 600 frames | 1,247.4 | = | **696.7** (3 variants in one process) | 1.8 |
| style, 1,200 frames | | 2,450.6 | = | **1,221.7** | 2.0 |
| style labels | Rust | 1,536.1 | = | **848.1** | 1.8 |
| i18n / template | Rust | 1,252.0 / 1,226.7 | = | **384.1 / 365.4** | 3.3 / 3.4 |
| variable-list | Rust (index microbench) | 191.3 | = | **74.0** | 2.6 |
| core | Rust (scene microbench) | 87,723.9 | = | **1,328.2** | 66 |
| text | Rust + GPU text system | 4,540.8 | = | **228.4** | 19.9 |
| stream-memory | Rust + GPU, 1,200 ticks | 1,390.7 | = | **496.9** | 2.8 |
| startup | GPU device + font db + 4 shaped lines | 188.6 | = | 203.6 | **0.93 (loss)** |
| teardown, N = 2,000 / 10,000 / 30,000, one run each | Rust | 148.8 / 682.7 / 1,998.9 | = | 204.1 / **429.1** / **983.3** | **0.73 (loss)** / 1.6 / 2.0 |
| teardown, default (the three sizes × 5 repeats) | Rust | 2,759.5 | = | 5,986.3 | **0.46 (loss)** |

The losses are whole-process cells dominated by mounting work that zgx does not do: the 120 M font
tables (startup, mount against the Rust run, teardown N = 2,000), and zgx's repeated runs reusing its
process-global text cache (teardown's five repeats remount the same strings; zgx's later repeats cost a
fraction of its first, bough shapes every mount again). See §3.

### 1.2 Per unit (each binary's own report)

| scene | measure | zgx Luau | zgx Rust | **bough** | × vs Rust |
|---|---|---:|---:|---:|---:|
| mount (2,000 rows) | mount + layout, M instr | 122.3 | 95.3 | **57.3** | 1.7 |
| | teardown (drop), M instr | 29.6 | 17.7 | **6.4** | 2.8 |
| teardown removal frame | 2,000 / 10,000 / 30,000 rows, M instr | – | 1.1 / 5.4 / 13.6 | **0.71 / 3.52 / 10.38** | 1.5 / 1.5 / 1.3 |
| | per node | – | 184 / 180 / 152 | **118 / 117 / 115** | |
| | frames after it until freed | – | 3 / 17 / 87 | 3 / 15 / 44 | |
| script-update, per update | template | 19.8 k | 11.2 k | **1.77 k** | 6.3 |
| | function-prop | 743.2 k | 267.9 k | **1.08 k** | 248 |
| | rows | 1,904.2 k | 570.1 k | **182.0 k** | 3.1 |
| | rows-in-place | 753.2 k | 569.4 k | **182.3 k** | 3.1 |
| | handler (bough: no hit test, see §4) | 90.3 k | 48.2 k | **3.53 k** | 13.7 |
| anime, per frame (slope 600→1,200) | per-element reactive style | – | 2.370 M (Rust signals) | **0.863 M** (wires) | 2.7 |
| | the floor | – | 1.961 M (direct scene) | **0.096 M** (lanes) | 20.4 |
| | time per frame | 1,142 µs | 2,483 µs / 1,272 µs | **54.5 µs / 4.9 µs** | |
| style, per frame (slope) | boxes | – | 2.005 M | **0.121 M** (lanes), 0.216 M (quads), 0.540 M (wires) | 16.6 / 9.3 / 3.7 |
| | time | – | 1,058 µs | **6.7 / 12.3 / 33.4 µs** | |
| | labels (bough: opacity only) | – | 1,340 µs | **50.4 µs** (1.15 M instr) | |
| list-state, per tick | | 3.33 M, 4.37 ms | – | **3.9 k**, 0.3 µs | 850 |
| script-layout, 1,000 rows reversed | change + its frame | 3.08 + 2.28 ms | – | **0.17 ms** (1.07 M instr) + 0.001 ms | |
| i18n | per frame | – | 179.7 µs | **16.9 µs** (342 k) | 10.6 |
| | locale switch | – | 677 µs | **409 µs** (7.7 M) | 1.7 |
| | template, per frame | – | 213.3 µs | **15.6 µs** (340 k) | 13.7 |
| variable-list, 1 M rows | construction / variable scan / update | – | 3.62 / 6.08 / 1.00 ms | **0.65 / 3.75 / 0.36 ms** | 5.6 / 1.6 / 2.8 |
| | fixed scan / append+resize | – | 0.287 ms / 0.200 ms | **0.244 ms** / not built | |
| core | idle flush over 1,002 nodes | – | 3.4 ns | **2.8 ns** (71 instr) | 1.2 |
| | fixed text update + flush | – | 128.8 ns | **90–154 ns** (2.2 k instr) | ≈ |
| | leaf translation + flush | – | 160.8 ns | **113 ns** | 1.4 |
| | 1,000 cards translating + flush | – | 1,142 µs | **1.27 µs** (lanes), 40 µs (1,000 wires) | 900 / 29 |
| | hit test among 1,000 cards | – | 177,017 ns | **12.7 ns** | 13,900 |
| | million-row virtual range | – | 3.2 ns | **2.5 ns** (fixed: closed form); 42.5 ns (variable: Fenwick) | 1.3 |
| | rotation (2 lines) | – | 2,350 µs / 489 µs | not built | |
| text (1,000 paragraphs) | first measure / same width / new width | 353 / 857 / 693 ms | – | from scratch **0.6–1.5 ms** (12 M instr each); retained 1.6 / **0.00** / **0.04 ms** | ~300+ |
| | stream 3,000 chars, 6 per step | 2,184 ms | – | **1.3 ms** (11.8 M) | |
| stream-memory | footprint / max RSS | peak 229 MB / 49 MB | – | **126 MB / 20 MB** | |
| startup | renderer + shaping | 28.5 ms | – | 42–59 ms | **loss** |

zgx's `text_bench` measures rich paragraphs (regular, bold and monospace runs); bough-text has one face
here, so every run is measured in it (a text3 gap, reported, not fixed here).

### 1.3 STATUS §1 cells did not move (A/B interleaved, slope method, RENDER = 0, instr/frame, two rounds)

| cell | canonical (`target/fusion`) | **api** |
|---|---:|---:|
| heavy stream | 179,608 / 179,090 | 184,093 / 180,136 |
| heavy scroll | 27,067 / 26,757 | 27,853 / 26,663 |
| heavy both | 214,286 / 214,973 | 215,777 / 216,329 |
| lab stream | 28,361 / 26,841 | 27,441 / 27,169 |
| lab scroll | 9,622 / 9,599 | 9,349 / 10,205 |
| lab both | 35,903 / 35,841 | 35,964 / 36,491 |
| animate | 13,378 / 13,295 | 14,824 / 13,176 |

Also measured (api only): vlist scroll / fling / jump 12,495 / 17,601 / 29,127 (STATUS: 12,989 / 19,573 /
35,662, measured on another day: the mounting modes most likely gain from the sized labels), heavy stream RENDER = 1 381,539 (383,397),
animate RENDER = 1 140,415 (141,530). All within noise or better.

## 2. What was built, and what each structural idea bought

* **Wire heads validated by address** (`rt.rs`). A wire records the head it was linked into; `settle` and
  `add_wire` cut any chain whose wire does not match. Closes a hole the `sig!` audit missed: a `Signal`
  swapped between two `App`s carried a wire index into the other arena (safe code could run a wire of
  another type). Owner classes (`Rt::classes`) give list rows and entities their own wired-offset lists.
* **`List<T>` = stable slab + order vector + delta log** (`list.rs`). Rows never move in memory, so a row
  is an owner for life; order ops edit a `Vec<u32>`; the log is what regions replay. Gate: 40 seeds × 400
  random ops against a `Vec` oracle (slots, positions, stale handles); the log replayed on the previous
  order gives the current one.
* **Lists are stacked groups placed by a prefix sum** (`listview.rs`). The first version made rows cephal
  children of a flex column: correct, but a row's text edit re-ran the column over all its siblings
  (cephal reports `Change::Full` to a parent that asked speculative questions): 633 k instructions per
  `rows` update, losing to zgx's 570 k. Rethink: a list's column has no layout of its own to do. The
  container became a sized leaf, each row a layout group in its own space, `y_k` a prefix sum. An edit
  lays out one row; if its height holds, nothing else moves (182 k per update, 3.1× under zgx); a splice or
  a height change moves the rows after it by space writes plus `translate_run`s (bough-core page offsets);
  a permutation rewrites the window's records once (`read_run` + `rewrite_run`, handles follow): 1,000
  rows reversed in 1.07 M instructions. list-state's toggle is 3.9 k instructions a tick.
* **A one-line label is a sized leaf** (`layout.rs`, `lay_deep.rs`). Its size is its shaping (width of the
  shaped line, line height): no measure function, so cephal never asks it questions and never stores a
  cache for it, and a same-width edit (a digit) re-places its glyphs with no layout at all. Mount's cephal
  pass fell from 37 M to 17 M for 2,000 rows; every `rows` edit stops at its row.
* **Font tables built once per process** (`text.rs`). `SimpleScript` + `Shaper` are immutable and shared by
  every `App` (`Arc`): a second window costs 5.9 M instead of 126 M.
* **`select`: the domain changed from N predicates to a keyed table** (`select.rs`). All selects of one
  signal share one wire whose table maps key → destinations; a change patches the old key's and the new
  key's: function-prop 268 k → 1.08 k per update.
* **`lanes`: style as one pass over an instance column** (`lanes.rs`). One record owning a run of up to
  4,096 instances, one wire whose patch is the monomorphic loop `inst[i] = f(t, i)` plus one union for
  damage: anime 0.096 M a frame against zgx's direct-scene floor 1.96 M; style 0.121 M against 2.0 M.
* **Two-phase release; teardown is page release** (`rt.rs`, `bough.rs`, `lay_deep.rs`). Immediate release
  cost 1,180 instructions a record (zgx: 184 a node): the buddy allocator scanned its free list (O(n²),
  now a bitmap), `remove_run` removed whole pages one at a time (each a directory shift and re-index:
  O(pages²), now one `dir_remove_range`), and dropping text states freed five buffers each. Now the
  removal frame only *cuts*: wires die, layout groups leave the engine (one pass over kids lists, text
  states moved by index into an orphan list, no deallocation), list regions stop; nodes, texts, instance
  runs and glyph blocks return to their free lists in idle frames, a budget at a time (zgx's
  `Ui::reclaiming` model), and text states are reused with their capacity. 118 instructions a record in
  the removal frame at every size.
* **Re-description as a lockstep walk** (`describe.rs`). A tape of node ids recorded in preorder at mount;
  `Element::update` walks the new description and the retained records together, comparing operands
  and patching through the same monomorphic `Apply` paths the wires use; `Option`/`Vec`/`each` realign
  positionally and splice preorder runs. Gate: 8 seeds × 80 random re-descriptions (texts, colours,
  positions, specs, gaps, `Option` and `Vec` parts, clips, a wired counter alongside) equal a fresh mount
  record for record; a sabotaged walk fails it.
* **Reader: pending is a bit in the wire head** (`reader.rs`). A producer bumps the ticket and puts its
  source on `HELD`; the settle scan skips a held head's non-`latest` wires (producers and `latest(..)`
  sinks run first in the chain, whatever the declaration order); completions are admitted at the next
  turn by ticket; the cohort commits when its last member is staged, queuing every member's and held
  source's head: one settle, one epoch. Gate: 29 seeds × 120 random steps (writes, unrelated writes, jobs
  completed in random order, superseded tickets): both readers and the held sink always show one
  committed selection, `latest` shows current intent, unrelated signals are never held. Committing as soon
  as any member is staged fails it on the first seed.
* **Entities**: a carved slab of generational handles; entity views are owners like list rows. **changed**:
  the turn's list ops, entity edits and reader commits, with bounded waves.
* **A bug the gates found**: laid-out text (`label`, `para`) drew no pixels unless some `text()` label of
  the same size existed (its strike's tokens were prepared only by labels); the vlist scene drew its text
  by that accident. Strikes now prepare their ASCII tokens when created.

## 3. Correctness gates (all green: `cargo test --workspace --release`)

| gate | where |
|---|---|
| `List` vs a `Vec` oracle; log replay == final order | `crates/bough/tests/list.rs` |
| list regions: retained == fresh mount after random splices, moves, permutations, edits, take-and-reinsert, retain, truncate (12 seeds × 60 turns) | `tests/list.rs` |
| re-description == fresh mount (8 seeds × 80 turns), sabotage-checked | `tests/describe.rs` |
| reader cohorts: no tearing under random schedules, sabotage-checked | `tests/reader.rs` |
| entities + `changed` | `tests/entity.rs` |
| pixels, ring ages 1–4 and the windowed ring, + retained == fresh mount: update rows, function-prop, list-state, script-layout (reverse / sort / move), list splices + moves + edits between a header and a footer, teardown/remount, anime (wires, lanes), style (wires, quads, lanes, labels), i18n | `comparisons/bough-suite/tests/{lists,anime,style,i18n}.rs` |
| every earlier gate (heavy, lab, animate, blink, micro, vlist, layout, core, text, metal) | unchanged, green |

## 4. Gaps, honestly

* **Startup and the font tables.** `SimpleScript::build` costs 120 M instructions per process (36 K
  glyph-pair lookups); it is bough-text's (text3's area) and makes startup lose (203.6 M vs 188.6 M) and
  the small scenes' whole-process cells carry it. Cached per process now; caching it on disk or building
  the pair classes directly would remove it.
* **Teardown's whole process loses** (N = 2,000: 204 M vs 149 M; the default 5 repeats: 5,986 M vs
  2,760 M): the removal frame wins at every size, but every repeat remounts and reshapes, and zgx's later
  repeats reuse its global text cache. A process-wide shaped-text cache keyed by (string, size) is the
  missing piece.
* **Not ported**: `state` (Luau table operations only; no UI, no scripting layer), zgx-gpu `layout`
  (rich text with bold spans, a dragged window resize: no styles in bough-text, no resize in the façade),
  zgx-gpu `frame` (reactive opacity and size on layout boxes: bough's layout styles change only through
  re-description), `widgets` and `callback_mount` (buttons, text inputs, hover: the `input` branch), the
  four `app-*` scenes (whole Luau applications: mail, dashboard, sketch, code editor), `editor_streaming`
  (no text editor), script-layout `animate = true` (no list-move animation), anime's Luau engine.
* **API**: lists are vertical stacks only; no list virtualization yet (list-state mounts all 1,000 inbox
  rows where zgx mounts the visible ones; its per-tick cost is unaffected, its mount is larger); entity
  views and producers cannot be re-described; no `boundary`/fallbacks/faults for readers; `changed` is the
  root's only; the update scenes' `handler` invokes the handler directly (no hit test in this branch); no
  translate / opacity operands on layout boxes (style and core use `lanes` or colour-only quads); no
  rotation; `HeightIndex` has no resize/append.
* **Measurement**: load average 50–110 throughout; times are indicative. zgx's `stream_memory` prints
  "rss 0 MB" (its own reader); its footprint and max RSS above come from `/usr/bin/time -l`.

Raw data: `bench/api/zgx/{results.jsonl,raw/}` (zgx, 51 labels), `bench/api/bough/{results.jsonl,raw/}`
(bough), the A/B slopes above in this file.
