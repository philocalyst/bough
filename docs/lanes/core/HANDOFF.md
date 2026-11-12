# Core round 6 handoff — 2026-11-11

Stopped at the user's request, before an isolated measurement slot became available. **State:
CANDIDATE for the diagnostic harness; LEAD for the structural proposal.** No core representation
change, optimization acceptance, or independent replay. The next session must measure before settling
the structural card. The shared measurement lock was released and the waiting runner stopped.

## 1. What is built, and where

Branch `lane/core/round6`, base `aa9943e7ae50595f6af026773ee82b46a4b85ae0`.

- `f5d7986d53`: [frozen measurement card](cards/2026-11-11-cost-model.md).
- `8031481254`: compiled diagnostic extension to the existing `core_bench` and `trace_bench` examples.
  Shared implementation: `crates/bough-core/examples/support/cost_r6.rs`. Pass `--cost-r6` to either
  example; original modes remain. There are no production-source changes.
- [DESIGN-R6.md](DESIGN-R6.md): substantial proposal, verification/correction of all six lead premises,
  proposed struct layouts and scan shapes, motion proof, publication approach, oracle/mutant/unused
  cost obligations and settlement order. It is **unmeasured**, not a settled implementation card.
- [COST-R6.md](COST-R6.md): protocol, explicit analytical cache-line bounds, dependency chains,
  assembly observations, toolchain/executable provenance and validation limits.
- [witness-r6.rs](witness-r6.rs): three minimal baseline correctness witnesses, actually executed.
  [replay-witness-r6.py](replay-witness-r6.py) compiles/runs them from repository root after building
  the core example; it checks the common measurement lock first.
- [summarize-r6.py](summarize-r6.py): future CSV-to-table renderer, with completeness checks for
  204 cells and five samples each. Syntax checked; not run on nonexistent data. The expected CSVs
  and `TABLE-R6.md` have deliberately **not** been fabricated.
- [lockfile-normalization.patch](lockfile-normalization.patch): reviewable shared change for the
  coordinator. Cargo added three missing git `source` lines for already-pinned zgx packages. The
  root lockfile has been restored; no shared/root file changes are committed by this lane.
  The patch uses zero context: replay with `git apply --unidiff-zero` after coordinator review.

The intended matrix is 140 structural/query/snapshot cells plus64 spatial cells, at N=1,000 /
10,000 /100,000 /1,000,000. It includes fresh front/middle/end edits, streaming and bulk build,
translation, four snapshot states, shallow/depth32 topology, ordered/interleaved geometry, contiguous/
dispersed1/10/100% tile damage, and present/gap hits. Setup and oracle work are outside instruction
intervals. Structural cells compare records and exact damage with `oracle.rs`; queries/culls compare
results; snapshot cells compare all records. The deliberately omitted move must be rejected by the
sequence comparison. **These harness assertions and its mutant have not yet run.**

Private continuation/logs live in `runtime/lanes/core/`. `STATE.md` is canonical. Build/check/test
logs, both versions of linked assembly, witness execution and the measurement contention log remain
there. The waiting script `run-matrix.py` is stopped; do not restart it blindly (see traps).

## 2. Every measurement / observation produced here

**Performance measurements: none.** Neither R6 benchmark executable was run. There are no instruction,
wall-time, cache-miss, GPU-time or STATUS ratios to hand over. I held the shared measurement directory
while waiting, but other sessions continued to compile/test/benchmark and began new jobs. The guard
never reached its `starting isolated matrix` state. The user then requested this clean stop. There
are no partial performance CSVs to salvage.

What was actually observed:

| observation | result | method / scope |
|---|---|---|
| core correctness suite | 25 integration tests passed, zero failures; no doctests | `cargo test --release -p bough-core`, existing suite on this source |
| workspace all-target check | passed, existing warnings | `cargo check --workspace --all-targets --locked`, with the baseline lockfile normalization described above |
| machine cache line |128 bytes | `sysctl hw.cachelinesize`; this is a hardware property, not a miss counter |
| memory / machine |38,654,705,664 bytes RAM (36GiB), Mac15,6 / M3 Pro | `sysctl`; no footprint measurement |
| OS |macOS15.7.4, build24G517 | `sw_vers` |
| stale hit witness | before `Some(1)`, after `None`, saved cell `[100,100..120,120]`, validity **true** | move one record down64 pixels via `scroll_run_copy`, window320², point(110,110), query actual core methods |
| affine cull witness | ink `[109,9..121,21]`, hit `Some(0)`, cull **[]** | translate `[10,10..20,20]` by+100x using `set_run_xform`; hit(115,15), cull `[110,10..120,20]` |
| page reuse witness | new ordinary record still has ink `[109,9..121,21]`; transform delta matrix zeros, translation `[100,0]` | remove the transformed record, insert into the empty tree, query the reused page |

Cargo's elapsed command times, preserved as operational observations rather than performance claims:
first example build23.60s, corrected example build24.04s, core test compile46.33s, workspace check
1m26s. They ran amid other work and must not be compared as compile benchmarks. Individual integration
suite elapsed times were9.90 /0.95 /0.31 /0.14 /1.88 /0.01s (7/1/4/8/2/3 tests respectively).

Pinned toolchain: nightly-2026-10-17; rustc1.101.0-nightly `5d8fec0669a64607db9ccbf663e7c23125bf4748`,
LLVM23.1.4; Cargo1.101.0-nightly `3960fa69b`. Release, default features, thin LTO, debug1. Executables:

- core_bench SHA256 `6f8d03246abae78d0fa9edcbab69913160d09b9536dd50f1b7f9af42f56065b7`;
- trace_bench SHA256 `7bbb95185fc4b87713c2ab01a4b0c8af8ba7f4b70713da2fef380e58469bfacb`.

Reference bbtree was read directly at `23aeb9dca642efa829a96736e065157307bdbe72`. Historical numbers
quoted in DESIGN-R6 (such as the old Metal copy/redraw comparison) are explicitly prior repository
evidence, **not measurements from this session**. All cache-line counts/layout costs in COST-R6 are
analytical bounds. Proposed branch sizes are design arithmetic, not compiled layout measurements.

## 3. Judgement, newly recorded insights and open suspicions

The full proposal is in DESIGN-R6; the following captures the reasoning and remaining leads that
would otherwise be lost. Proven observations and hypotheses are distinguished.

### The most valuable structural change is to stop reconstructing meaning from repaint work

The stale-hit witness is not just a missing invalidation flag. It shows that the current core uses
one derived representation to answer two different questions: which pixels must be recomputed, and
whether the semantic scene at a point may have changed. Transport deliberately makes the first set
smaller while moving semantic support through its interior. Any future general motion optimization
will amplify this defect unless the distinction is architectural.

I would first introduce an explicit semantic-change generation (conservative O(1) invalidation) and
capture the witness as a failing regression. It can be precise later. A second full tile ring is not
required for the first fix. Then make mutation methods produce three kinds of evidence: changed
semantic support, changed stored resources, and required repaint after valid retention. This also
prevents the inspector and accessibility from accidentally subscribing to a repaint certificate.

An unchanged image is not necessarily an unchanged semantic tree: reorder identical-looking records,
change an interaction flag, or replace an owner at the same pixel bounds. Conversely, a changed stored
page may produce identical pixels. Hashes, page write generations, paint logs and hit validity should
not collapse these distinctions merely because some old scenes make them coincide.

### The physical tree and the semantic tree must remain distinguishable

A B-tree path is a path through storage partitions, not a chain of UI ancestors. It cannot directly
supply event bubbling, inherited clips or accessibility parents. One earlier suggestion in the core
results about collecting semantic parents along a hit descent would be wrong for the present spatial
page traversal. A branch's minimum depth supports *search for* an ancestor; the branch itself is not
that ancestor.

Likewise, one page is not one scroll/compositing context. A packed leaf may straddle contexts, while
one context may occupy thousands of leaves. Give context identity and local coordinate frames a
semantic representation, then let storage cuts be independent. Otherwise moving a panel will require
repartitioning all its pages just to create a place to attach a transform.

The useful unification is **one sequence operation maintaining several measures**, not one scalar
flag pretending to answer every reader. Count, minimum depth, bounds, extent and change summary can
compose at the same boundary. Their correctness predicates and consumers remain different.

### The chosen flat-array costs are subtler than “pointer tree versus SIMD tree”

Verified: default prefix structural invalidation rebuilds from page zero, not from the edit. A split
at the tail therefore still has a global component. Even a fitting front insert shifts a suffix of
block starts. There are five directory columns plus derived arrays; reindexing follows pointers to
all later page headers. Those loads can overlap—they are not a serial linked-list chain—but they
still touch unrelated memory.

At1M records the minimum-depth column is only about31KB (~245 hardware lines). That makes the linear
parent scan less absurd than a million-pointer walk, and sets a serious constant-factor target for
replacement. Yet depth-one wide trees expose it, so RT8's depth gate alone misses the relevant case.
Measure scan distance and query count per topology epoch, not only semantic depth.

An ordered summary tree should be the default candidate, with a leaf root and no empty upper levels.
Its rank/min-depth scans and split/join paths should replace the flat maintenance. I do **not** know
that fanout32 beats16 or64, nor that the proposed u64 prefixes beat current two-level binary search
at1K. Those are costs to measure; “bbtree uses SIMD” is not evidence. A genuine small-tree regression
rejects the candidate. Do not paper over it with a scene-selected second tree.

### Movement needs a stable run, not merely a list of records

The main initial mount paths already batch contiguous rows. The more fundamental lost information is
that retained runs become `RecordInit` arrays again during `move_run`/`rewrite_run`. This destroys the
opportunity to relink interior subtrees and keep their local frames/resource identities intact.

I would make cut/join of a stable run the structural operation, with boundary materialization only.
Uniform translation/depth changes can later be inherited tags on that run's decomposition. But all
consumers must understand them: the GPU currently expects flat directory indices and leaf-local
state. Rebuilding its flat directory each frame would hide the same O(P) cost behind a faster CPU
method. Include GPU descriptor maintenance in the experiment's accounting from day one.

Removal is different from movement. Deleting K records with K live resources cannot honestly claim
constant work by deferring their destruction off-camera. Deferred retirement can improve latency,
but total instructions, retained memory and reclamation stalls must remain in the ledger.

### Spatial locality has a structural lower bound that no branch factor fixes

Paint order can interleave content from the entire window. Then page and branch AABBs become broad,
and a tiny dirty fraction still admits most leaves. An ordered tree fixes edit/rank complexity; it
does not magically become an R-tree. The interleaved fixture deliberately breaks the locality the
old scenes tend to provide. A miss inside broad bounds is especially useful because output size is
zero and cannot explain a long scan.

Before adding another spatial index, inspect why paint order is interleaved and whether explicit
compositing contexts provide useful contiguous boundaries. Separate spatial indexes were already
rejected in settled work; merely renaming one as an acceleration layer does not discharge upkeep,
ordering and fallback costs. For opaque overlap, an opaque-cover certificate may prune more work
than a tighter union bound; union ink alone does not prove coverage. For transparency, many
contributors really are required. Count admitted pages/groups and emitted records separately.

### Numeric compression is not the numeric contract

Verified: `NRect` narrows public absolute input, directory summaries and clips, not only page-local
ink. Normalizing a page falls back to that same narrow absolute representation. The i32 `off` field
does not grant unbounded world coordinates; its cached i16 lanes have another bound. Rank prefixes
are also u32, depth is u16 with a sentinel, and clip IDs have lifetime/cardinality constraints.
These should become explicit checked contracts rather than silent narrowing or debug-only assumptions.

5K/6K windows fit today's rectangle coordinate range but exceed the damage plane. Solve that domain
separately before proposing to double every ink lane. A blocked or strided growable tile plane must
also reach text's direct u128 mask sink, application copy planning, frame-slot row storage and shaders.
Changing only `MAX_TILES_X/Y` cannot be a complete result.

For much larger world coordinates, compact local frames are plausible, but a wide escape format has
a dispatch cost. Hoisting it once per page may be acceptable; it is not literally free. Compare a
simple all-wide baseline and charge compact-scene regressions. The design's zero-tax demand is a
verification obligation, not a property obtained by naming a format “compressed.”

### Snapshot work is already incremental in one dimension

Verified: `freeze_into` copies changed page bodies only, but scans every page and copies the directory,
prefix, clips and damage metadata. Start by removing discovery work through changed-subtree summaries
or bounded publication replay. A COW rewrite before measuring that option could add per-write branches,
atomics and delayed reclamation to solve a problem that a bounded log solves more cheaply.

A snapshot is a transitive resource graph. Pages referring to mutable/reused glyphs, instances, spaces,
clip IDs or image slots are not an immutable scene. The memory lane's growable regions and snapshot
leases must agree on generation/fence ownership. A writer-side handle table pointing at “the current
page” cannot automatically resolve an old snapshot's version of that handle. Reader projections may
need epoch-specific resolution; count its memory and lookup chains.

Open rollover concern: page write/structural generations are u32. Equality comparisons eventually
repeat. This is not a demonstrated near-term failure, but any long-lived snapshot or generational
lease design needs an explicit rollover/reset strategy and a small forced-wrap test. Do not assume
“versioned” means sound across reuse, overflow or reader lag.

bbtree's own concurrency document explicitly acknowledges plain-read data races. Borrow its cost
model, bulk builder and local invariants, not its optimistic-reader protocol for a safe Rust scene.
It also has per-entry semantics, not atomic multi-resource publication.

### Additional concrete suspicions to test, not findings to claim

- `scroll_run_copy` recognizes an equal `(clip, dx, dy)` record and early-returns after translating
  the supplied run. This appears intended for disjoint pieces of one motion. Repeating it on the
  **same or overlapping run** could move content twice while retaining one-vector metadata. The
  precise public contract needs a test; I did not execute this witness. Also test edits between
  equal-vector calls and nested clips. Do not infer unsoundness from the source branch alone.
- `set_kind_flags` changes interaction state without adding repaint damage. It is another likely
  hit-cache invalidation gap, distinct from the confirmed scroll case. Exercise a cached hover
  chain while flags change at fixed geometry; callers may invalidate elsewhere.
- The confirmed affine cull/reuse failures suggest mixed transformed/untransformed structural
  edits deserve a systematic audit: split, merge, insert into a transformed page, move across
  frames, normalize, free/reallocate. Current `insert_split` copies the old page's affine to new
  pages. Check how newly inserted absolute records and moved records are meant to inherit it.
  I did not prove further failures; do not fix them by guessing at the frame contract.
- `set_clip_rect` and external resource updates may change reader-visible state without changing
  a leaf body. A changed-page-only snapshot journal would miss them unless resource changes have
  their own entries. Clip reuse also needs generations; widening a permanent u16 allocator only
  postpones exhaustion.
- Current façade list placement can iterate to converge (bounded passes), and per-row spaces still
  move even when core pages can shift lazily. Benchmark one complete keyed-list turn as well as
  the core operation before claiming foundational improvement.

### What I would do next, in order

1. Obtain an actual isolated slot and run the frozen204-cell matrix. Inspect every sample, oracle
   failure and occupancy effect. Do not settle the structural implementation from this unmeasured note.
2. Freeze a small correctness card for the three proven witnesses. Add exact regression tests and
   mutants, settle semantic invalidation and affine-frame/reuse contracts with lead. Keep this
   verdict separate from the tree performance experiment.
3. Prototype rank/min-depth/bounds summaries over existing leaves, with split/join and bulk build,
   paired against current code. Keep leaf ABI unchanged initially to isolate the cost. Measure the
   required GPU-directory consumer rather than deferring its cost indefinitely.
4. Coordinate wide damage/resource domains with memory/GPU. Then admit multi-region motion through
   explicit proof types and a cost policy; start with opaque clipped contexts and an immutable source.
5. Decide publication and parent projections from real reader workloads. COW and parent caches are
   options with measurable costs, not prerequisites to having a sound foundational tree.

## 4. Traps for the next session

- **No measurements exist.** The original request's measured table is unfinished by explicit user
  stop. Reports now say so. Do not quote the planned matrix or analytical line bounds as results.
- The first harness build did operation-name string matching inside each query iteration. I found
  it in assembly and removed it before any timing. Use commit8031481254, not the rejected assembly.
- Short mutation intervals still include a small enum/slice check and counter noise. Examine ranges;
  do not interpret tens of instructions as a new mechanism. Query loops batch; their black-box/call
  overhead remains. The full harness is compiled but **not smoke-tested**; a first oracle mismatch
  may be a fixture/harness issue. Preserve it and diagnose before producing any table.
- Repeated insert/remove can create reusable slack and erase split costs. These fixtures rebuild
  fresh. Append1 naturally fits at the first three sizes and splits at1M; insert64-end is the
  forced-overflow companion. Compare both, not just one favorable tail state.
- The fixture intentionally overlaps geometry at large N to stay within the current domain. It is
  not a million-row tall document or a footprint test. Handles are unbound; façade/resource costs
  are not included. Exact tile percentages do not mean exact record percentages.
- `oracle.rs` shares the narrow rectangle construction, and some damage expectations use real
  page boundaries. It is useful for existing contracts, not independent proof of a wide-domain
  representation. Add a wide scalar geometry oracle and final pixel oracle for that work.
- The snapshot replica omits the live handle table. Comparing copied records does not prove an
  accessibility/inspector identity API or safe concurrent access.
- All three source witnesses remain unfixed. Passing25 existing tests does not invalidate them.
  The witness program prints observed failures rather than making an intentionally red workspace test.
- The root Cargo.lock needs the supplied normalization for the exact `--locked` check replay. Do
  not silently broaden dependency upgrades or commit root changes from the lane. The normalized
  lock was used for validation, then restored.
- The pinned Cargo uses per-package `target/release/build/.../out` artifacts and separate `.rmeta`
  metadata. Direct rustc linking needs both `.rlib` and `.rmeta`, dependency search directories and
  thin LTO (Apple's older linker cannot consume the newer raw bitcode). The replay script handles
  that case. Earlier failed direct-link attempts were tooling setup failures, not witness outcomes.
- Public code/docs have no private absolute paths; logs and disassemblies do. Keep them in ignored
  runtime. The public lockfile patch is reviewable, not applied.
- The runtime matrix guard was waiting for all cargo/rustc/other-lane executables. It is now stopped,
  and its former lock is released. Reacquire atomically and recheck ownership before any continuation;
  never reuse an old “core” lock assumption or kill other sessions' work.
- Do not run `cargo fmt --all`; format only intended files. Do not clean other worktrees' targets.
  There is no remaining owned compiler/test/benchmark to reap. The lane target is retained for replay.
- No comparison pixel gates, STATUS performance floor, GPU timing, latency, remote host work or
  independent replay were run. This handoff does not authorize bypassing the lead's settlement step.
