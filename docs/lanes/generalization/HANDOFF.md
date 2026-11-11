# Generalization handoff — stopped at the lead's request

This is a partial map, not a completed or accepted round. The lead requested a clean checkpoint on 2026-11-11,
superseding the instruction to continue through every scenario. No framework code was changed. No remote host,
synthetic global input, deployment or outside messaging was used. No independent replay has accepted this work.

## What exists

Branch `lane/generalization/round6`. Earlier commits: `0691ce0e7b` freezes the original card; `eb6a6df95d`
contains the first rows/table port; `e6d4108a6f` applies the approved three-line root Cargo.lock source repair
and freezes the completion amendment; `1654288212` adds calling-thread measurement and plain containers;
`9c00a6b7f3` records the first instruction tables and stale route capture. This checkpoint adds the six remaining
scenario ports, corrects their ticker oracle to read the configured update count, and preserves all obtained data.
The comparison remains a **standalone workspace**, not a root workspace member. Its own Cargo.lock is committed.

`comparisons/bough-guibench/` contains:

- `src/spec.rs`, `src/cli.rs`: byte-for-byte reference files, reference commit and hashes in OVERFIT.md.
- `src/model.rs`: rows, coarse Vec table, fine List/signals table, ticker mutations; `--plain` bypasses ScrollBox.
- `src/scenarios.rs`: spinner, typing/search, virtual rows. Resize adapts the rows scenario.
- `src/driver.rs`: real AppKit window and headless host, ordinary pointer/key/resize/clock entry points, model
  oracle, exact pixel comparisons, fixed layout samples, per-thread instruction counters, resource snapshots.
- `src/main.rs`: `bough-bench`; `src/bin/headless.rs`: isolated scenario/operation execution.
- `matrix.py`: table batch, 120-second process isolation, four ring gates, three pairs of 8/32 repeat measurements.
  The `rest` and `adversarial` batch names are reserved but **not implemented**. It is not a resume-capable runner.
- `report_matrix.py`: public summary from private cell JSON. `run.py` is the older process-wide/lock runner;
  use `matrix.py` for the lead-authorized calling-thread measurements under load.

All eight scenario names now dispatch, but that does not establish full conformance. Search and virtual already
fail the fresh-mount gate. Spinner uses a 16ms timer rotation fallback. Resize only alternates 1280×800 and
640×480; the complete requested storm and scale changes are not implemented. Native timer lifetime is checked
separately from headless delivery. All raw escapes and intended prelude expressions are in FRICTION.md.

## Every obtained number and its meaning

The public evidence is deliberately split by interpretation:

1. [TABLE-CELLS.md](TABLE-CELLS.md): **161 completed cells**, through coarse/plain N=5,000 remove. Every completed
   cell's median/min/max, exponent and gate result is present. Counts at N=10,100,1,000 are complete for the
   scheduled table variants; N=5,000 is partial. N=10,000, raised-cap follow-ups and the runtime mutant were not reached.
2. [TABLE-CELLS.json](TABLE-CELLS.json): all individual repeat totals, all three slopes, gate exits and references,
   fitted exponents, and frozen executable provenance. This preserves precision rounded in the Markdown tables.
3. [MEASUREMENTS.csv](MEASUREMENTS.csv): 5,049 numeric output records from the preserved JSON executions,
   including first-stage gates, mount diagnostics, the six new smoke cells, and any samples completed after the
   last aggregate cell save. Arguments, exit status and local relative record identifiers are included.
   **Gate-window instruction totals include oracle mounts; they are not operation costs.** Mount ns, RSS and
   footprint were taken under load without a resource lock and are diagnostic levels, not qualified time/memory
   slopes. This CSV intentionally preserves them rather than silently discarding inconvenient measurements.
4. OVERFIT.md retains the first 19-cell report and differing-byte counts. Historical NM statements refer only
   to that stage; the checkpoint and current tables supersede its continuation instructions.

Measurement method: on the local M3 Pro/macOS/direct Metal, release/thin LTO, pinned nightly-2026-10-17,
`bough_metal::perf::tinstr` reads calling-thread `thread_selfcounts`. One repeat is operation plus undo; each
replicate is `(I(32)-I(8))/48`, with the middle pair reversed. Median and min/max use three replicates.
The timed non-gate window excludes model oracle creation, semantic checks, fresh mounts and resource sampling.
R0 runs the ordinary input and frame path through GPU input without rendering. R1 additionally renders and
waits for presentation; it counts CPU submission/wait instructions, not GPU instructions or elapsed GPU time.
Counts were explicitly authorized under concurrent load. Different operations and their undo can have different
costs: these are pair averages, not separate forward-only costs. Fits use log/log regression and flag exponent
above ideal+0.20. Until the actual linear-select mutant runs, these remain **unqualified scaling diagnostics**.

Frozen G6.2 executable SHA-256: `41c689d99c8e9ff2d9b7d05ece084e3e8a1ce48ede0e69731c8adc1129eabbec`.
Six-scenario smoke executable: `8548e9dfaa61670035c0019994c05c465adeff74b5929e9454d0eea4f3eb8f7a`.
They are preserved privately as `headless-g62` and `headless-rest-1`; neither should be overwritten by a later build.
The latter predates the final K=100 oracle correction. Original spec/CLI tests cover shared data parsing, not
all the newly added driver branches. Full final-tree workspace validation and independent replay remain open.

Useful anchors from the table (instructions per operation, R0):

| cell | count | observation |
|---|---:|---|
| coarse plain select, N=1,000 | 6,567,603 | actual re-description; correct ordinary selection pixels |
| coarse plain select, N=5,000 | 32,874,375 | approximately proportional to N |
| coarse plain update10, N=1,000 | 4,662,665 | full owner traversal around sparse application changes |
| coarse plain update10, N=5,000 | 23,336,384 | approximately proportional to N |
| coarse plain swap, N=1,000 | 3,311,480 | positional traversal despite two-row semantic edit |
| coarse plain swap, N=5,000 | 16,465,534 | broad traversal persists |
| coarse plain remove, N=5,000 | 116,538,216 | structural churn much more expensive than value traversal |
| fine scroll select, N=1,000 and 5,000 | 12,330 | exposure-limited mounted set; see correction below |
| coarse scroll select, N=5,000 | 9,639 | stale pixels; omitted work is not an optimization |

Six new smoke cells all used N=100, ring=4, four operation/undo repeats. Ticker K=1, spinner, typing and resize
passed both pixel comparisons and their available semantic checks. Search failed retained/fresh with **154,509
bytes**, first differing byte **251,152**. Virtual scroll failed retained/fresh with **198,048 bytes**, first
**1,884,161**. Neither failure is a measured performance result. Their full instruction/resource diagnostics are
in the CSV; successful gate totals were respectively 449,555,090; 504,088,435; 476,707,206; and 452,090,856
instructions including oracles. Those large totals must never be quoted as operation costs.

## Insights, mechanisms, and hypotheses to carry forward

### The crucial late correction: a List is implicitly exposure-virtualized

The original card and first report assumed `list()` without `.viewport()` mounted every row. The source's
introductory prose encouraged this reading. Executed N=100 fine table mounted only **138 records**, versus
approximately five records per ordinary row. Default-cap fine N=5,000 **did not fail**. Investigation found
`listview.rs::lr_place_region` computes a visible rectangle even when `virt` is false, and `lr_window` chooses
rows intersecting it plus overscan in that branch too. The ordinary list is full-height in layout but still
mounts by exposure. `LList::update` sums `regs[reg].rows`, which are mounted rows, not all logical List slots.

Consequences: the source-derived space-cap hypothesis is not disproved for truly mounted rows, but these cells
never tested it. Neither plain nor scroll fine tables in the current data are a 5,000-mounted-row comparison.
Do not tell core/memory that the requested capacity failure has been reproduced. Do not call the flat fine
select curve proof of locality in a large mounted owner. The first next experiment should explicitly retain all
rows, probably by `.overscan(self.rows.len())`, while keeping the same 1280×800 viewport. Verify record counts
before counting instructions. Freeze that change as a distinct workload, preserve the native-exposure results,
then execute default 5,000/10,000 failures and only afterward raised-spaces=16,384 follow-ups. This change was
**not made** before the checkpoint. Owner: core/memory; exposure policy/documentation also surface-api.

The structural design should make logical count, mounted/exposed count, and allocated storage independently
observable. A benchmark should report all three. Implicit exposure is useful, but it cannot silently stand in
for the mounted workload a caller thinks they requested.

### Correct retained pixels are not necessarily correct application pixels

`template.rs::ScrollBox::mount` makes the whole subtree opaque; `ScrollBox::update` only calls
`describe::update_opaque`. Coarse data changes, header changes, descendants do not. Both renderer paths consume
the same stale tree, so presented==full is insufficient. The fresh-state mount is the independent authority.
Search uses the same plain iterator/filter below a scroll box and reproduces a fresh failure. This strongly
suggests the same omission, though search's text-input/header state should be isolated before assigning every
pixel to that mechanism. Owner surface-api/core. Preserve scroll/clip identity while reconciling descendants;
turning every application field into a signal is not a general repair of rung 0.

Append/truncate sometimes pass at large N because their differences lie below the viewport. This is a limit of
visible-image equality, not proof the offscreen tree is right. Add a later scroll revealing changed rows, then
compare fresh. A cheap semantic model check alone cannot detect that the retained tree missed a mutation.

### Route lifetime is separate from description lifetime

Plain coarse post-replace selection fails after apparently correct re-description: `input.rs::On::update`
updates its child but does not reinstall/update the captured handler. The closure captured the original row ID,
while the visible row now has another ID. This is reproduced, not merely conjecture. `root_owner_only` separately
rejects List/entity-owned routes, causing this port to delegate fine selection at the root with fixed-height
coordinate arithmetic. That workaround is not evidence that reusable interactive rows work. Owner surface-api.

The structural fix is retained receiver identity plus refreshable route operands, with dispatch resolving the
actual owner/generation. Avoid assuming a closure mounted once stays semantically valid through positional
re-description. Test remove/reinsert and slot reuse, not just replacing labels. Hit testing and dispatch must
agree about which owner survived reconciliation. This is one of my top three real-application priorities.

### Distinguish application cost, reconciliation cost, and drawing cost

Coarse Vec insert inherently shifts its suffix; the stronger structural ideal is not achievable for that model
without changing its data structure. Fine `list.rs::swap` via `permute_with` copies a span, insertion shifts Vec
order, and list replay rebuilds a height suffix via `assign_from`. These are source mechanisms to test; do not
attribute the entire coarse cost to the renderer. `LList::update` performs a sum of mounted-row record lengths
to advance the description cursor. That should be a cached aggregate maintained with structural deltas, but
its scaling has not yet been isolated because exposure kept the sum small.

Plain coarse select grows approximately fivefold from 1,000 to 5,000. That establishes actual owner traversal
cost where the ScrollBox path omitted it. A normal rung-0 traversal can be linear by design; the important API
question is how a developer introduces a stable child ownership boundary and fine dependency without rewriting
the whole application. Reusable owners need a cheap skip backed by dependencies, not an opaque subtree that
never updates. Owner surface-api/core. This is the second of my top three priorities, together with scroll
re-description correctness as one coherent ownership/invalidation problem.

### Virtual scroll failure needs a reduced oracle before a framework accusation

The raw index_list adapter estimates 32px rows, builds rows from prelude elements, binds its scroll signal and
adds `.scrolls`. Retained/fresh fails after scroll. Possible causes include rebasing spaces, exposed-row range,
content origin, or fresh-mount initialization from a nonzero scroll signal. A mismatch can arise on either side
of the oracle. Save/inspect the first BGRA pair, compare row identities at the top edge, and probe offset 0,
32, and a nonintegral offset before changing engine code. Confirm whether index_list expects an external
ScrollBox or owns its own viewport. Do not blindly add another scroll container: that could double translation.
Owner core/gpu if reduced engine failure; adapter ownership remains this lane until then.

### Timer, search, text and numeric domains remain open

The prelude lacks timer registration, native repeating rotation and count-only virtual sources. These are
expressiveness findings independent of speed. The timer adapter uses AppKit main-loop ownership and calls the
same `App::turn`/`frame_paced` function in native and headless paths. A 16ms fixed angle step is not display-linked
animation or elapsed-time animation; frame pacing claims would be false. Search currently scans the entire Vec
for its filter; even a fixed empty result can cost O(N), which the planned filter-nochange cell should expose
outside the opaque container. Typing and filtering need separate curves.

The adversarial matrix is **not implemented or run**. Predictions remain in the frozen card. In particular:
128×128 damage tiles of32px imply a 4,096px boundary; packed/short coordinates and f32 positioning suggest
32,767 and 2^24 boundaries; bounded spaces and IDs may reject large mounted owners; 200-level nesting may stress
recursive description/layout; 10K signals and 10K dependents can expose queue/fanout costs; non-Latin text needs
an independent shaping/font oracle, not same-renderer equality. These are hypotheses, not confirmed failures.
My third priority is making domain limits explicit and growable where appropriate, with typed capacity failure
and boundary tests, rather than leaving scene-size assumptions hidden behind fast-path success.

For the future adversarial implementation: use ordinary host keys/pointers for churn and signal turns; assert
actual mounted counts; record predictions before each run. For wide damage, add a known expected pixel near
the far edge because presented/full can agree on the same clipped error. For nested scrolling, deliver both
container wheel inputs before one frame and label that distinction from a single input turn. For scale, the
current App has no live scale-change entry point: record unsupported, do not simulate by fresh mounting.

## Traps and exact continuation priorities

- Preserve all failures and benchmark executable hashes. Current target binaries can differ from frozen captures.
  The final oracle-only edit changes K=100 correctness checks; K=1 smoke provenance remains the older executable.
- Matrix output directory must be new. Interrupted cells can have sample JSON but no aggregate row; CSV preserves
  those numbers. Do not infer completion from an existing directory or rerun into it.
- R0 still executes frame processing. R1 includes CPU wait work and can show spread under load. Neither is a
  GPU instruction counter. Do not divide gate totals by operation counts.
- Current gates stop on their first failing assertion. A cell's reported first failure does not certify all
  later checks. Some coarse/plain truncate samples fail although four-repeat gates pass: inspect longer-run
  sample errors for exhaustion/churn; do not turn missing medians into zero or assume they are timing noise.
- The negative-control mutation and synthetic fitter checks exist, but the measured mutant was scheduled at
  the end of the unfinished batch and did not execute. Run it early in the next batch, including a large size.
- Native AppKit startup focuses the root/text field via a raw harness escape. No global keyboard/mouse events
  were synthesized. F1–F8 are AppKit private-use Key::Char values; actual physical F-key delivery is not qualified.
- Native font is Arial unless BOUGH_FONT/ZGX_FONT_FILE selects another. Save the face choice for replay.
- Frame repair, fresh reconstruction, exact layout points, semantics, text fidelity and workload cardinality
  are different gates. Passing one cannot substitute for another.
- No accepted resident/footprint per-row slopes, locked mount times, or model-construction measurements exist.
  The current mount bracket starts after model construction and includes configuration/font loading and frame.
- The cap escape changes only spaces to16,384. It does not certify any other capacity. Default must be run first.
- Finish matrix.py's remaining batches, then adversarial module, after validating mounted-list semantics and
  the runtime mutant. The current six N=100 smoke cells are seeds, not a completed scenario sweep.
- Keep framework fixes with surface-api, core, memory or gpu. This lane's job is evidence and reproducible
  workloads. Final ranking should be revisited after the unrun domains, not frozen around today's easy-to-find bugs.

Private logs, frozen binaries, raw BGRA failures and full process records remain under
`runtime/lanes/generalization/`. Public files contain no workstation-specific absolute paths. The continuation
STATE.md records the stopped state. No benchmark process from this lane remains running at handoff.

## Final checkpoint validation

After the K=100 oracle correction, locked/offline release build of both binaries passed, and release tests
passed all11 inherited spec/CLI tests. A bounded two-second real ticker window at N=100 presented122 frames
and delivered121 timer callbacks with138 records; no synthetic global input was used. K=100 headless ticker,
N=100, ring4, four repeats passed both pixel gates and semantic state checks. Its diagnostics were:
mount89,229,814 instructions,95,852,834ns, RSS21,348,352 bytes, footprint119,850,496 bytes,138 records;
gate-window453,938,785 instructions (includes oracle work), RSS109,035,520 bytes, footprint365,889,920 bytes,
9frames and surfaces[3,2,2,2]. These resource/time values remain unqualified under-load diagnostics.
`git diff --check` passed. No final framework-wide test rerun was performed; the framework source is unchanged.

Final headless SHA-256: `facf00f6304abc25a3137a15d11d8a7db778553d9e175ae4cc5112661affd72c`.
Final window binary SHA-256: `cda2273d65363858b04ff1286cc5868f7a4c9b241b48935c6b45abbb6bcae540`.
