# R6 core cost model

**Handoff state: CANDIDATE. No R6 instruction matrix was measured before the user stopped this session.**

Baseline investigation, no production optimization. See [the frozen card](cards/2026-11-11-cost-model.md)
and [the proposal](DESIGN-R6.md). The planned outputs `TABLE-R6.md`, `core-r6.csv` and
`spatial-r6.csv` do not exist: other lanes continued heavy work, and the user requested handoff
before an isolated slot became available. The cache model below is analytical, not measured.

## Method and scope

The existing `core_bench` and `trace_bench` examples gain `--cost-r6`; their original modes remain.
The shared diagnostic uses only ordinary core/oracle interfaces. Every structural measurement starts
with a fresh bulk-filled tree; setup, scalar oracle, invariant checks and destruction are excluded.
Build includes tree construction/allocation but excludes prepared input and destruction. Snapshot
cells distinguish initial allocation, unchanged, one changed page and all changed pages. Query loops
have no name/operation dispatch inside the repeated interval. Counter-pair overhead is calibrated and
subtracted; loop/call/black-box overhead remains, making tiny queries slightly conservative.
Single-edit measurements include a small enum selection and slice-length check in the harness.
These fixed instructions do not explain the large-N slopes; they limit comparisons of tiny cells.

The frozen protocol calls for five samples, median as headline; none has been collected. Queries repeat 1,024 times; culls repeat
`clamp(1,000,000/N, 4, 1024)` times. Mutations/build/snapshot use one operation per interval and five
fresh setups. There is no CPU-frequency inference from these counts and no wall/GPU timing claim.
Warm repeated queries are not a cold-cache benchmark; the analytical line counts below do not pretend
to be measured misses. Allocation and occupancy differences are part of the fresh structural costs.

Fixtures have exactly N records, an empty root, and unique payloads. Coordinates lie on a 100x100
tile grid inside a 3,200² window; each record covers a 12² interior square. At high N, repeated
spatial positions deliberately overlap. Ordered records follow tile order; interleaved records
visit `(i*9973)%10000`. Depth is either one or repeated 1..32 chains under the root. Damage covers
exactly 100/1,000/10,000 of the 10,000 tiles (1/10/100%), in a band or a dispersed permutation.
Actual draw counts are recorded, because percentage damage alone does not define work.

Front edits start at rank 1 to retain the root. Middle is N/2. End insertion appends; end removal
starts N-K. A front move goes to N-K after removal; middle/end moves go to rank 1. K is 8 or 64.
Natural bulk tail occupancy varies with exact N: append1 is not a forced-split test; insert64-end
is its forced-overflow companion. Repeated build-append8 includes all splits instead of recycling
an insertion hole. Handles/resources are absent in these fixtures; core handle tests still run,
but these counts do not include façade scope destruction or many live handle repairs.

The omitted-move mutant must disagree with the sequence oracle before measurement. All real
edit cells compare complete records and exact damage rows to `oracle.rs`; queries/culls compare
results; snapshots compare every record. These are baseline diagnostics, not evidence that the
three separately reported correctness witnesses are fixed. The public oracle shares narrow
coordinate constructors and is unsuitable as the sole oracle for a wider-domain change.

## Analytical cache model (distinct touched lines, not cache misses)

This machine reports **128-byte hardware cache lines**. bough uses 64-byte leaf alignment and 16-byte
NEON vectors. Report both units rather than silently calling a 64-byte layout unit a machine line.
A 1,472-byte page occupies 23 aligned 64-byte units and **12 hardware lines** for either allowed
128-byte alignment phase. Adjacent arena pages can share a hardware boundary line; independently
boxed pages need not. No page-boundary-sized allocation is promised.

Define `L(b)=ceil(b/128)` for an aligned byte interval. An unaligned interval costs at most `L(b)+1`;
a sum over separate columns is an upper bound and may double-count shared boundary lines. Let
P=ceil(N/64), S=number of later directory entries shifted, Q=translated whole pages, V=pages admitted
by spatial summaries, G=admitted groups, M=emitted records, and D=ancestor depth. Exclude allocator
metadata, code/stack and oracle buffers from these data-structure counts. They remain in instructions.

| N | P | chunk count ceil(P/64) | 128B lines for directory min-depth | lines for all leaf bodies, if independent | directory/prefix/chunk live bytes, approximately |
|---:|---:|---:|---:|---:|---:|
| 1,000 | 16 | 1 | 1 | 192 | 484 |
| 10,000 | 157 | 3 | 3 | 1,884 | 4,749 |
| 100,000 | 1,563 | 25 | 25 | 18,756 | 47,281 |
| 1,000,000 | 15,625 | 245 | 245 | 187,500 | 472,656 |

Directory columns consume 26P bytes (pointer8/id4/ink8/min-depth2/length4), local prefixes about4P,
block prefixes P/8, chunk ink P/8: about30.25P before capacity rounding, sentinels and allocation.
This excludes pool pointer/id maps, handles, four damage planes and clips. Small table entries use
that approximate formula, not a heap measurement.

| operation | distinct-line count / bound | dependent reads and ideal |
|---|---|---|
| locate / leaf query | <=ceil(log2(ceil(P/32))) block-prefix probe lines + at most 2 local-prefix lines + pointer line + needed page lines | each binary probe depends on the previous; then pointer→page. General rank ideal O(log P), stable-cursor access O(1) |
| fitting insert/remove K | boundary page <=12 lines, input `L(24K)` for insert, local prefix <=2 lines, later block-prefix `L(4 ceil(S/32))`, affected chunk's inks <=5 lines, handle entries if bound | no global page chain, but unrelated block-prefix suffix remains. Ideal O(K+log P) |
| structural insert/remove | shifted columns <=sum over b=(8,4,8,2,4) of `(L(bS)+1)`; S later page-header lines; full prefix rebuild reads lengths and writes prefixes <=`2(L(4(P+1))+1)+L(4(ceil(P/32)+1))+1`; boundary/new pages and input separately | the directory stream exposes S independent page pointers, then writes their headers. They can overlap in flight; not an S-deep pointer chain. Prefix accumulation is serial arithmetic. Ideal insert O(K+log P), remove additionally O(K) lifetime work |
| move K | union of remove+insert line sets plus input scratch `L(24K)` and up to12ceil(K/64) source page lines | two rank searches and full record staging today. Same directory lines may be touched twice; do not sum them as distinct. Ideal aligned interior move O(log P) plus boundary/summary work |
| append / bulk | fitting append uses boundary set; split append still reads/writes the entire prefix arrays. Bulk touches every source record `L(24N)`, every destination page (<=12P), directory arrays and allocation metadata | bulk O(N) is appropriate; repeated append rebuilds increasingly large prefixes, O(P²) total component. Existing end-biased splits do not remove it |
| translate K | Q header lines + `L(8Q)+1` directory ink lines + affected chunk-summary lines + <=24 boundary-page lines, plus locate and damage row lines | pointers→headers are independent across pages. Ideal whole aligned subtree translation can use lazy tags in O(log P); current O(Q) is a genuine target, not O(K) record rewriting |
| parent / subtree end | local depth column 1–2 lines each boundary; up to`L(2P)+1` contiguous min-depth lines; rank and candidate page reads | SIMD min scan streams; parent query is not P chained heap pointers. Ideal logarithmic nearest qualifying summary, ancestors additionally O(D) output |
| cull | all chunk bounds `L(8ceil(P/64))+1`; admitted directory ink blocks <=`5ceil(P/64)` lines; admitted pages <=12V; output `L(4M)+1`; visited plane rows <=17 lines | chunk→page mask→page pointer→group mask→record/clip is dependent by level, independent within each scan. Up to~187,500 leaf lines at1M when summaries admit all. A broad overlapping hierarchy has worst O(N) even for few dirty tiles |
| hit | same summary dependency as cull, reverse order and early exit; <=12V page lines; no output stream | hit-present can stop; hit-gap can inspect all admitted groups. AABB overlap determines V, not damage fraction |
| freeze idle / one | source and replica header comparisons about2P independent lines + pointer/id directory streams and two copies' directory/prefix line sets; one changed body adds <=24 page lines, mostly new relative to headers | source pointer→source header→id→replica pointer→replica header. All pages scanned even with zero copied bodies. Ideal idle O(1), delta O(changed paths/resources) |
| freeze all / cold | up to24P source+destination body lines, two directory line sets and allocations on cold | O(N) required for materialized copies; acquisition may be O(1) only if construction/reclamation costs are charged elsewhere |

These are explicit **analytical upper bounds** for the named arrays, with overlap noted; they are
not exact dynamic address traces. Reindexing also gives a useful lower bound: a front structural
edit at1M touches roughly15,625 distinct page-header lines irrespective of its 8-record input.
The small page-depth array explains how a linear parent scan can still be fast in absolute time:
at1M it is only~245 contiguous hardware lines. A replacement must beat that actual stream, not a
fictional million random pointer reads.

For culling, the upper bounds intentionally include unused columns through the <=12V bound. An
exact admitted page without per-record clips often needs header1 + gink1 + ink(4–5) + payload(2–3),
with overlaps between column boundary lines. A partial tile mask adds plane lookups; clipped groups
add clip IDs and dependent clip table entries. The proposed hierarchy must retain this projection
of hot columns rather than loading an AoS node wholesale.

## Assembly review

Disassembled both linked release executables with the system LLVM objdump. The first harness version
matched operation names inside every timed query iteration (string loads/comparisons). This was
removed **before measuring**. The final timed query specializations use direct `bl parent`,
`bl subtree_end`, `bl ancestors` with simple decrement branches; there is no indirect call/vtable.
Black-box barriers produce deliberate stack stores/loads; these are harness costs, not claimed core
spills. Query selection, fixture generation, scalar oracle, sorting/reporting and output are outside
the measured query loop.

The bulk loop walks prepared slice chunks, directly calls `insert_impl` and moves the final Bough
into an output slot. Tree destruction is outside the successful timed interval (the generated
conditional replacement-drop arm is inactive because the slot starts empty). Input creation is a
scalar map with integer tile arithmetic; it is deliberately outside the interval. Edit dispatch is
one small enum branch, not an indirect call. Freeze calls the existing routine directly.

The inlined cull retains NEON comparisons/narrowing and `tbl` compaction; it is a large specialization
with spills and panic/bounds-check branches inherited from existing code. The depth loops contain
8-lane compare/narrow/mask scans; prefix binary search retains dependent loads. No production loop
was changed to hide these costs. Private disassemblies retain the full code and first rejected
harness assembly; public evidence below pins the executables. A future optimization must inspect
its new leaf/branch loops again rather than treating this inspection as transferable.

## Validation and limitations

- `cargo test --release -p bough-core`: passed (existing tests; no production changes).
- `cargo check --workspace --all-targets --locked`: passed, with existing warnings.
- Diagnostic sequence/damage/query/snapshot oracles and omitted-move negative control: implemented
  in the compiled harness but not executed. Three separate correctness witnesses were executed
  and still fail the desired contracts; their exact observations are reported in DESIGN-R6.
- No STATUS, comparison GPU pixel, GPU time, display latency, parallel-reader, large-domain or
  independent-review qualification was run. No 1.02 performance claim is made for the proposal.
- Cargo initially added three missing git-source entries to the root lockfile for the already
  pinned zgx crates; no versions changed. The [patch](lockfile-normalization.patch) is preserved for the coordinator; the root
  lockfile is restored, so reproducing `--locked` from the untouched baseline requires that known
  normalization (three `source` lines for zgx-cosmic-text, zgx-harfrust and zgx-workload). This is a baseline lockfile issue, not a committed lane dependency change.

Reproduction (after the baseline lockfile is resolved once by Cargo):

```sh
export PATH=$HOME/.local/share/cargo/bin:$PATH RUSTC_WRAPPER= CARGO_BUILD_JOBS=4
export CARGO_TARGET_DIR=$PWD/target
cargo build --release -p bough-core --example core_bench --example trace_bench
# Hold the round measurement lock and wait for all other builds/tests/benchmarks to finish.
target/release/examples/core_bench --cost-r6 > docs/lanes/core/core-r6.csv
target/release/examples/trace_bench --cost-r6 > docs/lanes/core/spatial-r6.csv
python3 docs/lanes/core/summarize-r6.py
```

`R6_FILTER` and `R6_MAX_N` are diagnostic subsets, not a substitute for the complete 204-cell run.

## Provenance

- Harness source commit: `8031481254`.
- Source base: `aa9943e7ae50595f6af026773ee82b46a4b85ae0`; card commit `f5d7986d53`.
- Reference bbtree: `23aeb9dca642efa829a96736e065157307bdbe72`, read-only.
- macOS15.7.4 (24G517), Mac15,6 / M3 Pro, 36GiB, hardware line128B.
- rustc1.101.0-nightly `5d8fec0669a64607db9ccbf663e7c23125bf4748` (2026-10-16),
  pinned nightly-2026-10-17; LLVM23.1.4; Cargo1.101.0-nightly `3960fa69b`.
- Release profile: opt-level3, thin LTO, debug1, default features; repository rustflags unchanged.
- `core_bench` SHA256: `6f8d03246abae78d0fa9edcbab69913160d09b9536dd50f1b7f9af42f56065b7`.
- `trace_bench` SHA256: `7bbb95185fc4b87713c2ab01a4b0c8af8ba7f4b70713da2fef380e58469bfacb`.
- Core tests: 25 integration tests passed, zero failures; no doctests present.
- The handoff commit pins the reports and witness source; there are no performance samples. Private build/check/
  test logs, rejected harness assembly and source witness execution remain in lane runtime storage.

Witness reproduction after the core example build:

```sh
python3 docs/lanes/core/replay-witness-r6.py
```

The probe prints the observed incorrect certificates/transform state; it does not change core code
or turn those failures into passing correctness assertions.
