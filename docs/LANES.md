# Research lanes: where to direct effort

These are tasks, not claims that a result exists. Read `docs/SETTLED.md` (what not to reopen) and the matching sections
of `research/AUDIT.md` before starting. Each lane has one primary output, and IDs in brackets refer to AUDIT items.
Every lane ends in the same evidence: a frozen card (`docs/CARD.md`), gates that pass or a named obstruction, and a
measurement taken alone against a pinned baseline. **`docs/AVENUES.md` is the exhaustive backlog behind every lane**:
each open item with its AUDIT ID and state, the per-platform realisation table (macOS, Windows, Linux, web, mobile),
and the design's 31 reference workloads mapped to lanes.

The order is the order of leverage. bough is fast on one platform, and most of the distance to a framework people can
use is platform reach, capabilities, and the design's API surface, not more speed on macOS.

| # | lane (`docs/lanes/<slug>/`) | question | evidence required |
|---|---|---|---|
| 1 | **Portable host and presenter** (`platform-host`) | Can bough's region-in-a-buffer, binned-compute presenter run through wgpu on Windows, Linux and the web without losing its macOS wins? | The same scene snapshots and reference-raster images on every backend; idle at zero wakeups per platform; STATUS cells on Metal within 5 % of direct Metal |
| 2 | **Platform capabilities** (`platform-capabilities`) | IME, accessibility, clipboard, drag and drop, menus, dialogs, cursors, multi-window, DPI and appearance, specified once and realised per backend | The design's platform bar (T1–T6) as CI jobs on every supported backend; a scripted-IME test; a VoiceOver/NVDA/Orca smoke test |
| 3 | **The design's surface** (`surface-api`) | Can the public API become the design's (re-description everywhere, `&self.x` wires, owner-free `El<'f>`, `ChildSeq`, `Handoff`, `Target`, `Delta`, commands) at no cost to the wins? | API tests compiled from `designs/reactivity/final/examples`; compile-fail message corpus; STATUS cells and zgx suite within 5 % |
| 4 | **Text** (`text`) | Parley `Layout` under the façade (fallback, bidi, rich spans, emoji), an editor on it, hinting and snapping, keeping the grafts' speed | Twin gates against unmodified parley; CJK/Arabic/emoji pixel gates; zgx text_bench cells; editor p99 |
| 5 | **GPU and presentation** (`gpu`) | Close the GPU-time loss (lab 1.7–2.2×, vlist 1.5×) and move the frame path entirely to wgsl-rs | ABAB GPU pairs on every scene (median ratio ≤ 1.0 against zgx); identical pixels; cold start ≤ zgx |
| 6 | **Pacing and input latency** (`pacing`) | Present at most once per refresh, never drop input, and bound input-to-present at p99 | AWAGY: presents ≤ refresh, dashboard worst gap ≤ 2 frames over 10 runs, p95 ≤ 8.4 ms; a measured input-to-photon p99 |
| 7 | **Virtualization and memory** (`memory`) | Mount and memory proportional to what is visible, in any container; a per-window memory ledger with budgets | 1 M-row column and grid: O(visible) mount and memory with scroll by copy kept; footprint ≤ the best measured framework |
| 8 | **Verification and tooling** (`verification`) | Can a contributor on any platform run the gates, see the scene, and trust a green CI? | CI on macOS (then each new backend); scene-description snapshots; `Win::demo`-style tests; a stable-toolchain build; the inspector on the prelude |
| 9 | **Independent review** (`independent-review`) | Do submitted results hold when someone other than their author replays them? | A replay receipt per accepted claim: same commit, fresh target, its own run of the gates and the measured cells, discrepancies recorded |
| 10 | **Async and entities** (`async-entities`) | The design's latency shapes: questions (`map_async`/`Reader`), actions (`.action`), streams (`Remote`), the entity kernel, atomic publication and priority lanes | Stale-completion and drop-cancel race tests; the entanglement property test; the adversarial-graph and fault-matrix workloads |
| 11 | **Style, motion and media** (`style-motion-media`) | Tokens and cascade, variant tables, analytic springs and motion lanes, the `IntoMedia` image/video pipeline | A palette swap touching only consumers; animation tiers entering only their phases; the image-pipeline workloads |
| 12 | **Components** (`components`) | Can a full component set be written on the prelude alone? | The crate builds on `bough::prelude` only; a friction log; the kitchen-sink workload |
| 13 | **Developer loop** (`developer-loop`) | Do compile time, incremental rebuilds and binary size stay plain-Rust sized? | Instantiation counts, incremental rebuild and link times, artifact size tracked in CI |

---

## 1. Portable host and presenter (`platform-host`)

**Why it is open.** Everything above the presenter is plain Rust and portable: bough-core's record tree and damage
planes, bough-text, cephal layout and the façade. Three pieces are macOS-only:

* the presenter: `crates/bough-metal`, an MSL file compiled at load, IOSurface surfaces, a `storageModeShared`
  region, a shared-event pre-armed stream;
* the host: `crates/bough/src/backend/host.rs`, an AppKit local event monitor on an Accessory-policy app;
* the vendored winit 0.30.13, which was never wired in.

The wgpu-hal presenter (`crates/bough-gpu`, `--features wgpu`) exists but runs only on Metal (build-time MSL
metallib) and lacks the transform and decoration kinds [F10].

**First bounded task.** Make `bough-gpu` build its kernels through wgsl-rs → naga for SPIR-V and HLSL as well as MSL.
Run the existing headless pixel gates on Linux Vulkan (lavapipe is acceptable for correctness, not for performance).
Freeze the region and bind-table contract it relies on: storage buffer, bindless or bind-table emulation, subgroup
ops, and fallbacks where a backend lacks them.

**Required first output.** The headless gate suite green on a second backend, or a named obstruction: for example, a
kernel feature with no portable equivalent, with the cost of the fallback measured on Metal.

**Then.**

* A winit host [6.1, X10] with event parity: key up, modifiers, horizontal and momentum wheel, middle button, scale
  changes, occlusion.
* Windows (DX12 through wgpu), Linux (Wayland and X11, Vulkan), the web (WebGPU).
* Each platform gets its own idle, startup and frame measurements, on its own hardware class.

**Success criterion.** One codebase passes the same scene snapshots on three backends, and Metal through wgpu keeps
STATUS within 5 % of direct Metal. Only after that, delete bough-metal's frame path.

**Do not spend the session on:** another Metal-only presentation trick before a second backend runs, or porting
the AppKit monitor host to more platforms. Its design (a local event monitor, no responder chain) cannot carry IME.

**Start from:** `crates/bough-gpu`, `crates/bough-gpu/RESULTS.md`, `vendor/README.md` (wgpu stack and wgsl-rs
grafts), `designs/reactivity/final/07-platform.md` §1–2, AUDIT §6.

## 2. Platform capabilities (`platform-capabilities`)

**Why it is open.** None exist [6.2–6.4, PL1–PL7]. The design specifies them as typed capabilities passed to
handlers (`fn copy(&mut self, clipboard: Clipboard)`), with `Prefer<T>` or `Require<T>` for window features, so
nothing is silently omitted on a platform. The gap register's note on menus, tray and dialogs reads "THIS NEEDS TO BE
CLEAN AND ELEGANT AND CROSS PLATFORM". The archive has deep prior work on accessibility and IME:

* `production/accesskit`;
* `production/accessibility-sparse`, a 100 K-leaf projection where updating one field went from 77.7 ms to 73.6 µs;
* `ime-*`;
* `luna/x11-ime-floor`.

**First bounded task.** Define the IME wire protocol of design 07 §4 as Rust types:

* `Enable/Update/Disable(InputMethodState)` with revisions;
* UTF-8 and UTF-16 ranges;
* `Preedit/Commit/DeleteSurrounding/SetSelection`.

Drive it from a scripted fake IME in a headless test of `text_input`, including commit-on-blur. No platform code
yet.

**Required first output.** The protocol types, the fake IME, and `text_input` passing the design's sub-transaction
rules (06 §3), or the rule that cannot be met with today's text-field model.

**Then.**

* Realise the protocol on macOS (an `NSView` host with `NSTextInputClient`), then on winit's IME events.
* An AccessKit projection from the record tree's semantic column, starting from the archive's sparse projection.
* Clipboard, drag and drop, menus and dialogs as capabilities, each with one cross-platform API. Wrap the ecosystem
  where it is good (`muda`, `rfd`, `arboard`) behind bough's capability types; don't expose those crates.

**Success criterion.** Gates T1–T6 green on two platforms.

**Start from:** `designs/reactivity/final/07-platform.md` §4–7, `06-text.md` §3, `designs/reactivity/gap-register.md`
S1, S4 and S6, `archive/RESEARCH_BRANCHES.md` (accessibility and IME families).

## 3. The design's surface (`surface-api`)

**Why it is open.** Re-description now runs after every handler (the api merge), but four things remain:

* its cost is unmeasured [F8];
* its spelling is not the design's: `sig!`/`Wired` are on the design's rejection list [D2], and elements are typed by
  their owner [D4];
* there is no `El<'f>` [D3];
* most of the ladder is missing: `Handoff`, `Target`, `#[derive(Delta)]`, `.with()`, commands, focus claims, motion
  and tokens [D5–D23].

**First bounded task.** Experiment X1: accept `&self.count` as a wire when `reflect::schema::<V>()` proves a
struct-only path to a `Signal<T>` at that offset. Re-express the `sig!` compile-fail corpus as rejection tests. The
mount cost per operand must stay ≤ 20 ns.

**Required first output.** X1 passing or failing its gate, plus F8: the measured cost of re-description per click on
the dashboard and the zgx suite.

**Success criterion.** The design's `final/examples/*.rs` compile against bough with only documented deviations, and
every STATUS and zgx-suite cell stays within 5 %.

**Do not spend the session on:** reintroducing a per-view static switch (`FIXED` was removed on purpose), or a
general `#[component]` macro (design approach D).

**Start from:** `designs/reactivity/final/00–04`, AUDIT §2 and §12 (X1–X3), `crates/bough/src/{elem,signal,describe,reflect}.rs`.

## 4. Text (`text`)

**Why it is open.** One font file per face, no fallback, no bidi, no rich spans, no emoji [T1–T4]. Parley is
vendored, but the runtime uses only the grafted kernels, never `Layout` or fontique. The text field has a caret only
[T5].

**First bounded task.** Experiment X6: put labels and paragraphs through parley `Layout`, behind a feature, with the
grafts as the covered fast path. The gates are the existing text cells, the twin tests, and new CJK, Arabic and emoji
pixel tests.

**Then.**

* An editor on `PlainEditor` [T5].
* Hinting and bi-level glyphs through skrifa's `HintingInstance` [T11].
* Paint-time pixel snapping [T12].
* Selection across elements [T16].
* The inline formatting context with atoms [T13].

**Success criterion.** zgx text_bench with rich paragraphs in all faces at ≥ parity, and the vlist R0 cells within
5 %.

**Do not spend the session on:** shaped-string caches on the update path (shaping is cheaper than lookup), or GPU
shaping (exact but not cheaper). See `docs/SETTLED.md`.

**Start from:** `crates/bough-text/RESULTS.md`, `vendor/parley/src/bough/`, `designs/reactivity/final/06-text.md`.

## 5. GPU and presentation (`gpu`)

**Why it is open.**

* GPU time loses on glyph-dense and list scenes: bin scanning is 47–64 % of GPU time, and a tile in a log row scans
  ~230 bin entries to find ~5 glyphs.
* The frame path is a 1,596-line MSL file compiled at load [F3, F5].

**First bounded task.** RT2, 2D binning with a parallel scatter: count, prefix and scatter per (row, column chunk).
This is the representation change the failed v7 tile-list analysis named.

**Then.**

* RT3: copy or redraw chosen per frame by a cost model.
* RT1: tile-flip presentation, which zgx ships, with a tearing detector.
* RT4: one tall compositor layer scrolled without repainting.
* Every kernel in wgsl-rs (shared with lane 1), and a build-time metallib or binary archive for startup.

**Success criterion.** ABAB GPU pairs (`bench/floor/gpu_pairs.py`) with the median ratio ≤ 1.0 on every scene,
identical pixels, and cold start ≤ zgx's.

**Start from:** `crates/bough-metal/RESULTS.md` §12–15, `BRIEF.md` §2.6 and §7.2, AUDIT §15.1.

## 6. Pacing and input latency (`pacing`)

**Why it is open.** The host draws a frame per input event: ~240 presents/s on a 120 Hz panel. The 75.7 ms dashboard
stall in one of three runs is unexplained [F1, F2]. Input-to-present p99, the design's headline latency claim, has
never been measured [Q13].

**First bounded task.** Trace the stalled run first (F2), with `os_signpost` and `Prof` around shaping, image decode
and layout, before changing anything.

**Then.** X4, input coalesced into display-link turns.

**Success criterion.** Over 10 AWAGY runs: presents ≤ refresh, worst dashboard gap ≤ 2 frames, p95 ≤ 8.4 ms at every
row count, and input-to-photon p99 reported.

**Start from:** `crates/bough/src/backend/host.rs`, `bench/awagy/`, AUDIT F1, F2 and X4.

## 7. Virtualization and memory (`memory`)

**Why it is open.**

* The prelude's `.scroll()` mounts every child.
* The api branch's listview virtualises `List`/`index_list` rows only.
* Footprint is 175 MiB against an RSS of 68 MiB.
* Capacities are fixed at construction and panic on overflow [F6, F7, R4, S4].

**First bounded task.** X7: stub records for offscreen subtrees of any container, measured on a 1 M-row column, a
grid and nested scrolls, against listview's per-region exposure.

**Then.** A per-window memory ledger with a debug allocator hook, so that steady-state allocation is asserted to be
zero; growing columns instead of panicking.

**Success criterion.** O(visible) mount and memory with scroll by copy kept, and footprint ≤ the best framework
measured in AWAGY.

**Start from:** `crates/bough/src/listview.rs`, `crates/bough-core/RESULTS.md`, `designs/reactivity/final/08-runtime.md` §5.

## 8. Verification and tooling (`verification`)

**Why it is open.**

* There is no CI.
* The merged tree's test suite has not been run.
* Every pixel gate compares the GPU path with itself, which proves damage exactness, not raster fidelity [Q10].
* There is no design-level test API [Q2].
* The build is nightly-only because reflection is unconditional [S8].

**First bounded task.** Run `cargo test --workspace --release` on `main` and fix or record every failure. Then add
macOS CI: check, clippy, tests, and the counter example.

**Then.**

* Serialised scene snapshots as the primary gate: tronical's "low-level scene description" point (gap-register S2).
* A CPU reference rasteriser with an NV-FLIP tolerance.
* `Win::demo`-style turn-barrier tests.
* Reflection behind a feature, so the default build is stable.
* The inspector on the prelude.

**Success criterion.** A contributor on Linux or Windows can run the headless gates and get the same verdicts.

**Start from:** `designs/reactivity/final/10-quality.md`, AUDIT §8 and §9.

## 9. Independent review (`independent-review`)

This lane replays others' accepted claims from a clean checkout and a fresh target. It reruns their gates and their
preregistered cells on the stated hardware, and records the receipt and any discrepancy in the claim's PR. It never
reviews its own lane's work.

## 10–13. Async and entities, style/motion/media, components, developer loop

Their backlogs are complete in `docs/AVENUES.md`. The first cards:

* **`async-entities`**: `signal.map_async` with an injected executor handle and the `StampedCompletion` wake law
  [A1]. Then the `A → B → A` stamp test [A6] and `Entity::new_cyclic` [B1].
* **`style-motion-media`**:
  * the token cascade with reverse-use lists, measured by a 10 k-consumer palette swap [D16];
  * then `IntoMedia` with off-thread decode [PL5, F9].
* **`components`**: checkbox, switch, slider, select, popover and context menu on `bough::prelude` only, with a
  friction log [X11].
* **`developer-loop`**: put the compile and size gates in CI before the API grows [B4].
