# Working in this repository

**lorlei** (until now called bough) is a from-scratch Rust GUI framework, the workspace at the repository root:
`crates/`, `comparisons/`, `bench/`, `vendor/`. The repository also keeps **`reactivity/`**, the preserved
clean-room GPUI rework, with its own workspace and the workflow in the last section of this file. The design both
answer to is `designs/reactivity/` (start with `final/00-model.md`); the audit list is `research/AUDIT.md`.

> **Naming.** The project is lorlei. Crate names (`bough`, `bough-x`, `bough-expr`, ...), the `bough::prelude`
> path, the repository URL, environment variables (`BOUGH_*`) and most of the older documents still say bough.
> Treat the two names as one project. Do not rename identifiers piecemeal: a rename is one reviewed change, made
> by the owner, across manifests, paths, scripts and docs together.

## Read first, in this order

1. `docs/r8/final/REPORT.md`: the closing report for rounds 6 to 8. What stands, how it compares, every open
   avenue (its section 4, summarised below), the refuted list, and the questions that are the owner's.
2. `docs/TALLY.md`: every experiment tried and what it showed. `docs/SETTLED.md`: exclusions you do not reopen.
3. `docs/r8/ROUND8.md` with its seams (`SEAM.md`, `LAYOUT-SEAM.md`, `TEXT-API.md`, `TEXT-SEAM.md`) and the lane
   finals in `docs/r8/final/`.
4. `CONTRIBUTING.md` (what a PR carries), `docs/CARD.md` (cards), `docs/LANES.md` and `docs/AVENUES.md` (older
   lanes).
5. The independent review of the round-8 work (`independent-review` lane, report of 2026-11-12): about ninety
   findings, sixteen failing probes, five completion gates.

## What exists: three bodies of code

They are not yet one executable. Say which one every claim belongs to.

| | what it is | where |
|---|---|---|
| **A. the façade** | what an author writes today: `View`, elements, `sig!`, handlers, on the round-6/7 runtime (write log, gapped directory, exposure-driven mounting, record-space damage, integer raster kernel, Wayland host, lazy GPU device). The scoreboard's `bough` row is this. | `crates/bough*`, `main` |
| **B. the new core** | UI as typed dataflow: staged expressions, one state block compared once per turn, one column per view call site, nothing unexposed exists. Crates: `bough-expr` (expressions), `bough-x` (the core, events, reflection, the **element layer** `el.rs`), `bough-paint` (per-archetype kernels), `bough-lay` (layout as data, extents), `bough-face` (staged text), `bough-ax` (accessibility projection), `bough-host` (Wayland host, no `dlopen`), `bough-span` (CPU span executor), `bough-glyph`, `bough-guibench-x` (the new-core benchmark app, registered in the runner as **`bough-x`**). | `main` (partly), `r8/*` |
| **C. lane experiments** | arena and scene image (`r8/threads`, not merged), GPU-resident raster, the text API proposal and stand-in shaper, the round-7 async engine (`r7/*`, not merged). | branches |

**`bough-x` is the new core's benchmark binary and its admitted contract is narrow**: one prepared Latin-1 face
(DejaVu Sans 14/28 px), opaque colours, rows placed by arithmetic, remount on resize, group-1 keymap, CPU spans. Its
numbers (start-up 4.3 ms, hover 0.08 ms) show what the representation costs, not what an author's program costs.
State this contract wherever `bough-x` appears in a chart or a claim. The work that makes B the framework is the
**element layer** (façade elements driving the new core through `Element::drive<S: Sink<V>>`); until the façade's
scenes run on it, B is a prototype.

## Contribution policy

Anyone, human or agent, may take any open avenue below. A contribution is a construction, a gate, a measurement, an
independent replay, a tool, or a documented boundary (something that provably does not work, and why). Read
`CONTRIBUTING.md` for the PR's contents; these are the rules that cost the most to learn.

**Evidence**
- Freeze a card (`docs/CARD.md`) or an experiment entry (a prediction and a kill line, as in the register below)
  *before* building or measuring. Do not fit a prediction to a result; replace it before running or report the miss.
- Report every cell, losses included, with the binary, host, load and timing mode. A number is measured only for
  the binary and host named with it. Estimates are marked, kept apart, and never rank.
- Measure alone: nothing else heavy during a measurement, no builds on the timed cores, a lock file for the batch.
  Under a fixed-rate compositor most interactive cells measure the compositor: say which timing mode a cell used.
- Keep a negative control (a mutant that must fail the gate). A gate whose inputs are all ASCII, all opaque, all
  uniform or all on whole pixels proves that subset only.
- A fast path is a contract: state what it admits, keep the adversarial case that forces the fallback, and test
  every "flat" claim past its admission limit.
- No benchmark-specific code paths, no cached or faked results, no work removed from a measured cell. A cache in the
  benchmark app that the framework does not provide is a defect (the app-level filter cache was deleted for this).
- Refuted ideas stay refuted until you have a new idea: the list is in REPORT section 5 and `docs/TALLY.md`.
- A closed review finding lands with its probe as a test.

**Code**
- Application code and new benchmarks use `bough::prelude` only; `bough::raw` serves the existing scene ports.
  The author's API must not change for performance reasons: the work happens below it.
- Every `unsafe` carries a `// SAFETY:` argument and its own test; an invariant held only by a comment is a defect.
  Safe interfaces must be sound: wrong-view, wrong-backend and substituted-row misuse is a compile error or a checked
  refusal. Nothing has yet been run under Miri, Loom or a sanitizer (X28).
- Retained equals a full redraw at every ring age; a fresh mount equals the retained tree. An optimisation lands with
  its oracle and a mutant that fails it.
- Toolchain: `rust-toolchain.toml` pins nightly-2026-10-17. Put rustup's proxies first on `PATH`, clear
  `RUSTC_WRAPPER`. Format only intended files (`rustfmt --edition 2024 --config skip_children=true`); `cargo fmt
  --all` reaches vendored crates. Vendored edits are listed in `vendor/README.md`.
- No absolute paths, credentials, transcripts or private logs in anything committed: the repository is
  public. Private material stays in the ignored `runtime/`.

**Working together**
- One lane or topic per branch (`lane/<lane>/<topic>`, `r8/<topic>`); integrations go to `main` through an
  integration branch that has run the whole-workspace suite (209 targets, 401 passed, 0 failed at the close).
  Never force-push `main`. Do not overwrite another lane's `docs/lanes/<lane>/`, the shared documents or root
  manifests; send a reviewable patch.
- No session accepts its own result: the `independent-review` lane replays it.
- Each concurrent session has its own `CARGO_TARGET_DIR` and worktree. Never share a target directory; never bare
  `git stash` (the stack is shared); stop only the processes you started, never by pattern (`pkill -f`).
- Commit messages say what was verified and what was not. Agent commits end with a `Co-Authored-By` trailer naming
  the agent.
- Delegated helper agents run on Sonnet or Haiku; the lead alone uses the larger model.

**Hosts and machine**
- Remote hosts need an explicit, live allocation from the owner. Reach them through the terminal multiplexer
  (`rex`), not a direct shell login. Keep a remote's disk use small and clean up what you started.
- On the owner's Mac: no synthetic global input (no `CGEventPost`, AppleScript or System Events), no permission
  prompts, and windows must not take focus or appear on the active Space (headless only). Heavy work at
  `nice -n 15` with two or three cargo jobs, one cargo at a time.
- On the shared Linux benchmark host: builds on the non-timing CPUs at lowest priority; CPUs used for timing are SMT
  siblings of the build CPUs, so no builds during a timed run.
- Do not message outside people or other sessions without the owner's direction.

## Roadmap (everything still open)

REPORT section 4 has size estimates, notes and what is already known for each line; section 9 gives the order.
Sizes: S days, M a week or two, L more. Do the integration items first.

**Gates before any new optimisation** (REPORT section 3): (1) safe interfaces sound: open are the round-7 async
items A01-A06, K03 (unvalidated GPU programs), P01-P04 (fixed only on `r8/threads`), part of C07, C10's purity
half, and the list memo's 64-bit hash with no equality check; (2) the sixteen probes pass on the integration branch
(re-run the review's own files unchanged); (3) one end-to-end binary: every scenario through one branch with
layout and behaviour checks and a fallback for each capability the fast paths refuse; (4) whole-operation
measurement under control; (5) every "flat" claim tested past its limit with a full retained-state ledger.

1. **Integration (first).**
   - **Element layer** (L): every façade element and modifier, handlers as methods, text through `bough-face`, runs
     inside a row, nested lists, then driving the benchmark app and the five heavy scenes (sheet, chat, ide,
     dashboard, canvas). Measure it against the hand-written sheet (hover 6,826, theme 31,010, mount ~230 K) and
     measure compile time and code size per component on a deep real view, early.
   - Merge `r8/threads` (M): 19 conflicting hunks; or go straight to frame versions (X5) and take only the arena.
   - Re-measure the exposed-edit regression (1,433 to 2,612) on a quiet host (S).
   - A machine-generated feature x commit x gate matrix (S); merge the round-7 async engine (M); one prepared-face
     format for `bough-glyph` and `bough-face`, one paint contract for the three rasters (M); the heavy scenes and
     all runner scenarios on the new core (M); finish the **`bough-x`** runner registration with its contract stated
     on every chart.
2. **Typed core and incremental computation.** `World<V, B>` with a validated erased schema (M); partial evaluation
   by dependency (M); pointer influence box (S); sweeps as rects (S); live-record scan under endless motion (S);
   cold edits that change geometry (M); **collection operators with maintained indexes** (filter, sort, group, join;
   substring index) (L); order lane so an insert costs one row (M); nested lists as one child store (M); column
   grouping revalidated on geometry change (S-M); CPS with higher-ranked lifetimes, const-generic stage sets,
   `min_specialization` (S each); places and descriptors with generations (S).
3. **Identity and publication.** Item key and generation in relations, one publication transaction remapping focus,
   hover, press, accessibility and reflection (M; decision owed); immutable frame versions with reclamation (M-L);
   a frame generation binding sample, pick and projection (S); lexical ancestry as a parent column, key-up, repeat
   and function keys, Tab that can be declined (M).
4. **Layout.** Wire the kept form (layout as flat words, one function, a memo, `Extent`) into `bough-x` and the
   benchmarked binary (L); parametric answers (M); hierarchical extents with sparse exceptions, 100 M rows (M); a
   sound shape check without hashing each row (S); straight-line solver for fixed and flex rows (M); `guess(w)` and a
   (row, offset) anchor (M); coverage of percentages, min/max-content, baseline, absolute children, DPI, a real
   measure callback (M); shape memo eviction (S); the sheet's gutter-breakpoint resize (S).
5. **Text** (the widest gap to gpui-ce and the web; `docs/r8/TEXT-API.md`). Shaper worker with stand-ins (M);
   `text`, `para`, `text_input` elements (M); spans, inheritance, fallback families (L); wrap, truncation, clamp,
   alignment (M); selection, copy, hit-testing, measure without mounting (M); `TextState`, IME, undo, a multi-line
   editor (L); large documents and highlighting by ranges (M); inline boxes, links (M); bidi, complex scripts, colour
   emoji (L); generational glyph runs (M); streaming cliffs (S-M); build-time shaping of constant strings (M);
   rendering quality: gamma, hinting, SDF for large text (M); SIMD classification of breaks and newlines (S-M, X20).
6. **Raster and GPU.** Tile prefix caching for canvas hover (M, X14); per-damage executor choice (S-M); the new
   raster as the Linux benchmarked path (M); GPU start-up from a build-time shader plan (M); per-machine work
   estimation (M); incremental binning (M); occlusion mask cost (S); core-owned bands (M, X6); a cross-platform GPU
   executor from one shader source, wgpu or direct Vulkan (L); author-written `wgsl-rs` shaders as paints (M);
   batched draw lists and shader variant caches (M); late latching and speculation (M); animation endpoints (S);
   16-bit coordinate, 33rd archetype, empty atlas limits (S each); independent visual fixtures (M); images and YUV
   (L); canvas on the GPU under 9 ms (M, X15).
7. **Start-up.** `fallocate` + `MAP_POPULATE` (X7), huge pages (X8), page pre-touch on other threads (X9), model after
   first write (X10), static linking (X11), pixel section from the page cache (X12): all S, all unrun; the façade's
   own 56-65 ms; start-up on macOS for the new core.
8. **Latency after the write.** What an 80 µs hover is made of (S); cold interaction locality (X1-X4); commit on
   input with a frame pending (S); **animation paced by the output** (S, X22: the spinner and ticker cells sit at 62.5
   fps because the benchmark app's clock drives them; fix it in the framework); real macOS presentation latency
   (decision owed); queue release policy and deadline pacing on a quiet machine (S).
9. **Memory and allocation.** A full retained-state ledger (S); one reservation for the store's vectors (M); epoch
   bump arenas (M); first-touch control (S); huge pages for columns (S); `MADV_FREE` after peaks (S); capacities from
   use (S); atlas residency by exposure (M); resource pages as pinned immutable inputs (M); a global-allocator swap
   on the façade (X23). A faster allocator on the new core is refuted (at most 6 % of mount).
10. **Threads.** Cache-owned workers over ranges of the published half (M); a scoped pool that survives a panic (S);
    a bounded prioritised queue (S-M); cache and branch-miss counters as standing columns (S). Nothing inside a turn
    is worth a thread.
11. **Async.** Holds and transitions on the new core (M); generational ids and owned guards (S-M); wake tokens and
    typed pinning (S); cost under many questions, bursts and cancellation storms (S).
12. **API, composition, state.** Composition as a cold schema over hot typed data: bundles, required components,
    queries, plugins, resources (M-L); state as plain fields, derived values as expressions (M); behaviour overridden
    at author-declared points (M); a dependency contract for owner-grain re-description (S); dormant-run rules (S-M);
    demand that is not exposure (M); native views and multiple windows (L).
13. **Accessibility.** The NSAccessibility adapter with lazy children, then AT-SPI (L: without it there is a
    projection, not accessibility); typed semantic components (M); ids by item key (M); accessibility in motion (M);
    text and virtual-focus semantics (M); a static lint over reflection (S-M).
14. **Reflection, inspector, developer loop.** The inspector port against source locations, no style editors (L);
    per-element locations and a stable schema (S-M); validated mutation (M); the agent/test socket: query the tree,
    inject turns, record and replay, diff pixels (M); three-line typed-selector tests (M); hot reload (L).
15. **Platforms.** Fractional scale as a rational (M); keyboard groups, AltGr, compose, repeat (M); IME (M); resize
    without remounting (S-M); translucent surfaces (S); Windows, X11, web (L each); an AppKit host over `bough-x` (M).
16. **Animation.** An animation benchmark at Freya's level: many springs, interruption, staggered lists,
    layout-affecting motion (M); springs as expressions of `Time` with `until` (M); layout-affecting motion without
    re-solving (M).
17. **Measurement.** The new core through the runner for every scenario, with its contract stated (M); a matched-CPU
    and a hardware-GPU configuration (M, X27); randomised paired blocks on a quiet host (S, X26); validity that fails
    closed and a propagated exit status (S); paired per-frame phase samples (S); estimates validated or refused (S);
    counter scope stated per number (S); missing workload families (editable document, emoji/bidi/IME, variable-height
    lists, nested scrollers, image churn, translucent layers, DPI changes, several windows, async streams) (L); the
    heavy scenes in every framework (M); binary size and memory on equal terms (S).
18. **Compile time and size.** Compile time and size per component as standing gate columns (S); shared code for
    equal-layout archetypes (M); a capability manifest per profile (S); reflection and accessibility kept out of
    binaries that do not call them (S).
19. **Experiment register X1-X28** (REPORT section 4.19): each with a prediction and a kill line. X1-X12, X16, X18
    and X19 are the lanes' own; X13-X15, X17, X20-X28 are first bids by the lead. Run as written, or replace the
    prediction before running.

**Decisions that are the owner's** (REPORT section 7): item key and generation in `Rel`; how macOS presentation
latency may be measured; accessibility order and the screen-reader window policy; the text questions (who owns
editing state, where fonts come from, how text should look, selectable by default, rope-backed `text()`); the
inspector's form; how far Bevy's vocabulary enters the API; live-edit pins and pointer transparency; GPU portability
(vendored wgpu or direct Vulkan and Direct3D); and the rename from bough to lorlei across identifiers.

**Latest measurements.** The full zortax gui-bench matrix was re-run on 2026-11-13 from `cc65ee215f`: 658 cells in each
of fixed 60 Hz and unthrottled timing, all frameworks plus `bough` (façade) and `bough-x` (new core). `bough` had
cell errors at 10,000 rows (search at 60 Hz in both runs; rows at 144 Hz in the fixed run; coarse table at 60 Hz);
`bough-x` had none; floem fails the layout check in 54 of 56 cells, vizia in 8. The heavy scenes were not re-run.

---

## reactivity/ workflow (the clean-room GPUI checkpoint; unchanged)

The clean-room library is the standalone `reactivity/` workspace. Original
`crates/gpui*` code and feature branches are references; qualify changes against
the actual clean-room composition before adoption. Designs live in
`/root/designs/reactivity`, mirrored by `rework/`.

- Keep the root model/window exclusively borrowed. Immutable resource leases
  have explicit scene lifetimes. Do not introduce Rc, Cell or RefCell into the
  production window engine. Prefer safe Rust; justify any new unsafe invariant
  and test it independently.
- Create unique worktrees and targets with fail-fast shell commands. Verify a
  destination does not exist before creating it. Prefer `worktree add --no-checkout`
  and sparse checkout of `reactivity` and `.cargo` in the newly created worktree.
  Never run sparse-checkout setup on an existing worktree after creation fails.
- Freeze a clean commit before a build/test gate. Do not edit that worktree until
  its compilers, tests and before/after source checks finish. Build inputs include
  `.cargo`, `clippy.toml`, workspace manifests/lock/toolchain and all first-party
  and vendored source used by the build. Keep failures and corrected attempts.
- Format only intended files with `rustfmt --edition 2024 --config skip_children=true`.
  `cargo fmt --all` traverses path dependencies and has modified vendored source
  accidentally; do not use it for this workspace.
- Give each concurrent lane its own target. Cross-worktree target reuse has
  returned an identical stale Rust executable for changed source whose mtime
  preceded the baseline build. Clean changed first-party packages before such
  reuse, or use a fresh target. Profile selection matters: `cargo clean -p gpui`
  does not clean release artifacts; use `cargo clean --release -p gpui` for a
  release comparison. Verify the changed library was rebuilt, not merely the
  final executable. Preserve rejected stale captures. Pinned Cargo's
  `-Z checksum-freshness --config
  'build.fingerprint="content"'` passed an actual old-mtime 17→19→17 control.
  [Cargo documents](https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#checksum-freshness)
  that build-script `rerun-if-changed` still uses mtimes. Clean the GPU package
  after overlays, compare generated WGSL against the typed shader, and verify
  generated WGSL/SPIR-V bytes inside the executable that ran. Inspect Cargo JSON
  freshness and actual executable identities; source guards alone are insufficient.
- Treat execution and measurement separately. Preserve full source/toolchain,
  features, input guards, executable hashes and exact workload gates. Report
  failed gates and fallbacks. Software Vulkan and private X11 establish neither
  hardware GPU performance nor display scanout latency.
- Inventory shipping code with `reactivity/tools/core_lines.py`: seven first-party
  libraries and library build scripts; exclude tests, comments, blanks, examples,
  tools, evidence and vendors. Report dependency/vendor, generated shader, whole
  machine-code, stack and retained-capacity costs separately. Do not forecast
  final line counts or count helper code twice.
- Mothership is available as `ssh mothership` (user miles); lorlei work above reaches it through `rex` instead. Build/copy work takes
  a shared lock and runtime/profile measurements an exclusive lock at
  `/home/miles/gpui-ce3-compute-20261024/host.lock`. Keep at least 8 GiB free and
  release/reap every owned process. Its AST adapter has no render node; current
  Vulkan measurements use a software driver. Preserve source/evidence archives
  before removing regenerable caches.
- Keep public tests on ordinary library interfaces. Test exact semantic authority,
  failed newer candidates and older eligible acknowledgements, resource lifetime,
  numerical boundaries and actual native accessibility. A bounded mutant should
  falsify any new optimization certificate. Never substitute tolerance or reduced
  work for an established exact/workload gate.
- Standalone application ports have independent lockfiles. Resolve necessary
  dependency edges without unrelated upgrades, then run their locked offline
  checks against the composed core before claiming those ports still build.
