# Data-structure findings, Round 2 — the tree, chained techniques, and the external frontier

Status: **runtime contract extension** — extends [datastructures.md](datastructures.md) (Round 1)
under [../07-the-ladder.md](../07-the-ladder.md)'s laws. Round 1 fixed the leaf structures
(Signal cell, Slot, List, tape, subscribers, read-sets, patch queues, spatial, routing,
SharedString). Round 2 attacks the structure Round 1 deliberately deferred: **the editable
retained tree itself**, plus the node-record economics at 1M-node scale, order-without-position,
and the sharing primitives the bleeding-edge crates ship (triomphe, elsa, and what beats elsa).
Four new runnable, zero-dependency experiments join the lab: `tree2`, `order`, `nodes2`, `share`
(`cd ds-lab && CARGO_TARGET_DIR=$PWD/target cargo run --release --bin <name>`; canonical captured
output appended to [ds-lab/runlog.txt](ds-lab/runlog.txt) under "ROUND 2 CAPTURE").

Method and machine are Round 1's (Apple M3 Pro, macOS, system rustc, `opt-level=3`,
single-thread, `Instant` + `black_box`, medians). Two additional caveats surfaced this round:

- **Allocator/page state moves absolute numbers.** The same binary measured elsa-style per-item
  boxing at 255 ns/push cold (fresh pages from the OS) and 7.8 ns/push warm (allocator magazines
  primed by earlier runs). Ratios within one run are stable; absolute ns across runs are not.
  All numbers quoted below are from the single canonical back-to-back capture unless flagged.
- **P/E-core scheduling** adds run-to-run scale factors of up to ~2× on this machine. Again:
  trust ratios, re-run before porting numbers.

Literature and crate facts below were researched against primary sources (papers, crate source,
engine source) in three companion sweeps; load-bearing external numbers carry their citation
inline. Where our measurement and a paper's disagree, both are shown.

---

## TL;DR — Round 2 adopted recommendations

| # | Structure | Recommendation | Headline |
|---|---|---|---|
| T1 | **Editable display store** | **chunked 256-node pages + Fenwick over counts** | edits **36–99 ns at 10k→1M** (dense memmove: 178 ns→20.6 µs; rebuild: 114 µs→12.7 ms/frame); traversal 1.2–2× dense scan; 10.7 B/node |
| T2 | Rebuild-vs-retain law | retain + diff; rebuild only per-owner pockets | at 1M nodes a 64-edit frame is **~200 µs retained vs 12.7 ms rebuilt (~60×)**; at 10k the rebuild is 114 µs — fine, which is *why* the owner-grained tier works |
| T3 | Packed-memory array | REJECT as the hot store | edits are fine (182–341 ns) but the occupancy-masked scan costs **4.8–6.1× dense at *every* density** — the per-bit ctz chain, not bandwidth, is the tax |
| T4 | Gap buffer (SlotTable-style) | REJECT as the global store; the *per-chunk* shift IS a bounded gap | uniform edits 8.4–41.9 µs at 100k–1M ≈ dense memmove; matches Compose's own LinkBuffer retreat |
| T5 | Succinct BP topology | cold/serialized form only | **3.0 bits/node vs 64**, but findclose 12–293 ns vs 0.6 ns subtree-lookup; no production dynamic-succinct library exists |
| N1 | Hot-record width @ 1M | keep 8 B; split per-pass 1 B lanes | scan µs/1M: 4 B **73**, 8 B **113**, 16 B **205**, 32 B **432**; flags-only lane **6×** the 8 B record scan |
| N2 | **Chained dirty-skip** | subtree_size + epoch dirty-lane + parent[]; **adaptive cutover ~1 % dirty** | 0.01 % dirty: **20× with work attached** (4.8× even bare); >1 %: streamed full scan wins — scheduler picks by dirty count |
| O1 | Order maintenance | u64 labels + windowed relabel on the linked slab | insert-after **6–41 ns amortized** (all patterns) vs 7.3 µs physical front-insert (**~180×**); order query 0.125 ns; f64 midpoints exhaust after **53** inserts |
| S1 | ThinArc / HeaderSlice | ADOPT for header+slice payloads | **8 B handle (Option free)** vs 16 B `Arc<[T]>`; ONE allocation; construct 34.2 vs 36.0 ns; clone parity (3.91 = 3.91 ns) |
| S2 | Borrow-to-read discipline | kernel read paths take borrows, never clone Arcs | Arc-clone-per-call 3.91 ns vs borrow 0.84 ns (**4.6×**) — ~30–80 µs/frame of pure refcount traffic at 10k styled nodes |
| S3 | Frozen arenas | **chunked stable arena**, not elsa-style boxing | stable `&T` under append at **3.0 ns/push ≈ plain Vec (3.1)**; elsa-style 7.8–255 ns, 1.7× slower reads, 40 vs 16 B/item |

---

## T · The editable tree (`bin/tree2.rs`) — the centerpiece

Round 1's flat DFS (paint-order) SoA store won traversal by 26–44× but priced structural edits
at O(N) memmove, leaning on "describe rebuilds paint order every turn". Round 2 asks: can the
retained store absorb edits *without* a rebuild, keeping the linear scan? Five candidates, all
holding the same 8 B hot record `{subtree:u32, kind:u16, flags:u16}` in DFS order, all replaying
**one identical logical edit script** (insert-leaf-as-last-child / remove-leaf, generated against
a dense reference; UNIFORM = fresh random descent per edit, CLUSTERED = one parent per 32-edit
burst, sanity-asserted to converge to identical trees):

| N | script | (R) rebuild/frame | (D) dense memmove /edit | (G) gap buffer /edit | (K) **chunked** /edit | (P) PMA /edit |
|---|---|---:|---:|---:|---:|---:|
| 10k | uniform | 114 µs | 178 ns | 188 ns | **44 ns** | 186 ns |
| 10k | clustered | 141 µs | 28 ns | 41 ns | **51 ns** | 237 ns |
| 100k | uniform | 1.03 ms | 3.41 µs | 8.44 µs | **99 ns** | 341 ns |
| 100k | clustered | 0.97 ms | 230 ns | 277 ns | **36 ns** | 255 ns |
| 1M | uniform | 12.7 ms | 20.6 µs | 41.9 µs | **79 ns** | 281 ns |
| 1M | clustered | 12.3 ms | 8.76 µs | 6.63 µs | **48 ns** | 182 ns |

Traversal per frame (same capture): dense 12.7 µs @100k / 133 µs @1M; chunked 18–52 µs @100k /
197–265 µs @1M (1.4–4× dense, fill/fragmentation-dependent — see below); PMA 622–633 µs @1M
(~4.8×); gap ≈ dense. Memory (topology only): dense 8.0, gap 8.0–11.3, **chunked 10.7**, PMA
17.2–26.8 B/node.

**(K) Chunked pages + Fenwick is the adopted editable store.**
`Vec<Box<[Node; 256]>>` in paint order + a `counts` array + a Fenwick tree over counts for
O(log chunks) position lookup (binary descent gives (chunk, offset) directly):

- an edit is a ≤2 KB in-chunk `copy_within` + a Fenwick update + O(depth) ancestor
  `subtree` bumps — **bounded at every scale**, 36–99 ns measured from 10k to 1M, which is
  **260× cheaper than dense memmove and 5 orders below the rebuild** at 1M uniform;
- traversal iterates chunk-by-chunk — linear runs of 256 nodes with one pointer hop between,
  measured 1.2–2× a pure dense scan when healthy; heavy uniform churn fragments fill toward
  ~60 % and cost toward 4×, and because chunks are **position-independent** (order lives in the
  chunk list, not in addresses), an incremental compaction (memcpy neighbors together, splice
  the list) restores density chunk-locally — the maintenance op is O(page), never O(N);
- any subtree is still a contiguous *logical* range `[p, p+subtree)`; the Fenwick descent turns
  a logical position into (chunk, offset) in ~5–20 ns (Round 1 measured Fenwick query at 3.3 ns).

This is Zed's SumTree philosophy (B+-tree of chunks with summaries; `TREE_BASE=6`, rope leaves
≤128 B — [zed.dev/blog/zed-decoded-rope-sumtree](https://zed.dev/blog/zed-decoded-rope-sumtree))
flattened to one level over 256-node pages, and it is what Compose's experimental **LinkBuffer**
(linked chunks replacing the SlotTable gap buffer; reorders "over 2× faster") is groping toward
from the other side.

**(D)/(G) Dense memmove and the gap buffer lose the same way.** Both are O(distance): the gap
buffer only wins when consecutive edits cluster (41–277 ns clustered — real, and exactly why
Compose shipped it for cursor-shaped recomposition), and degrades to memmove-class on uniform
edits (8.4–41.9 µs at scale). A movable gap is the right trick at *chunk* granularity — (K)'s
in-chunk shift IS a bounded gap — not at tree granularity. This matches the text-editing data
(gap buffers win pure-local, lose scattered —
[coredumped.dev "gap buffers vs ropes"](https://coredumped.dev/2023/08/09/text-showdown-gap-buffers-vs-ropes/))
and Compose's own retreat from it.

**(P) The PMA is a literature-honest loser here — for a reason worth recording.** Our
implementation uses the canonical density thresholds (τ_leaf 0.92 → τ_root 0.70; Bender & Hu,
[TODS 2007](https://www3.cs.stonybrook.edu/~bender/newpub/BenderHu07-TODS.pdf), the paper that
also measured their adaptive variant at 4× fewer moves / 7× faster on sequential patterns), a
64-slot word as the leaf segment, and a Fenwick-backed select. Edits behave as promised
(182–341 ns, O(log²N) amortized, 6–206 rebalances per 8k edits). The killer is the **read side**:
the occupancy-masked scan (`ctz`-loop over a bitmask) costs 0.61–0.72 ns/node **at every density
from 50 % to 90 %** — 4.8–6.1× the dense scan — because the per-bit extract is a serial
dependency chain, not a bandwidth problem (`tree2`'s density sweep decouples this from edit
machinery). A UI store is scanned every frame and edited rarely; paying 5× on every scan to make
rare edits polylog is the wrong side of the trade. (PMAs stay interesting for *batched* edits —
CPMA's parallel batch inserts hit 43× self-speedup and 2.81 B/elt,
[PPoPP 2024](https://arxiv.org/pdf/2305.05055) — but chunked pages get bounded edits without the
scan tax.)

**(S) Succinct balanced-parentheses topology: 21× smaller, 20–470× slower to navigate.**
Full BP bitvector + per-word excess/min-excess aux = **3.0 bits/node** (literature-tight
implementations reach 2.37–2.38 bits/node with ~0.5 µs/op on 2010 hardware — Arroyuelo et al.,
[ALENEX 2010](https://users.dcc.uchile.cl/~gnavarro/algoritmos/ps/alenex10.pdf)); our
`findclose` measures 12.4 ns @10k and 293 ns @1M (word-linear block-skip; a production range-min
tree makes it O(log), still 1–2 orders above the 0.6 ns explicit `subtree_size` load). No
maintained dynamic-succinct-tree library exists in any language (checked: sdsl-lite, DYNAMIC,
all Rust crates — static only), and no shipping engine uses one. **Verdict: BP is the
*serialized/snapshot* form** (undo checkpoints, cross-process protocol, `UiSnapshot` wire
format: 250 KB of topology for a 1M-node tree) — never the live tree.

### The composite verdict — display store v2

> **ADOPT**: paint-order chunked pages (256 nodes, 8 B hot records, Fenwick over counts) as the
> *editable* display store; per-kind cold payload slabs unchanged from Round 1; incremental
> chunk compaction as the only maintenance op; BP bitvector as the cold serialized form.
> The Round-1 flat store survives as what a *fresh* lowering emits (a 1-chunk-run store);
> promotion to paged form is a policy knob, not a representation change — both are DFS order.

This does **not** reopen Round 1's Flag A: `List<T>`'s public `Deref<[T]>` contract keeps its
contiguous item storage (Flag A option 1). The display store is kernel-internal, has no slice
contract, and is precisely where chunking belongs.

---

## N · Node records at 1M (`bin/nodes2.rs`)

**N1 — width is the frame bill.** Same loop, same visited field, only record width varies
(1M nodes, past LLC): **4 B = 73 µs, 8 B = 113 µs, 16 B = 205 µs, 32 B = 432 µs** per full scan.
Past cache, a scan is `bytes/node × N` — every "harmless" inline u32 is another 25 µs/frame at
1M. This is the measured backbone for Round 1's "8 B hot record, cold data side-tabled" rule,
now with the scaling law attached. A pass that reads only flags should read a **1 B flags lane**:
19 µs vs 113 µs (6×) — kind-segregated SoA taken to per-pass lanes, the Vello/Unity column
discipline ([vello_encoding](https://github.com/linebender/vello/blob/main/vello_encoding/src/encoding.rs)
is exactly flat per-column streams with no node objects at all).

For calibration against what shipping engines carry per node: Godot 4's base `Node` is
**968–1032 B** before any gameplay/UI data ([official forum measurement](https://forum.godotengine.org/t/memory-usage-of-godots-various-base-classes/115136)),
a `Control` 2.4–2.5 KB; WebKit hand-packs bitfields into pointer spare bits
(`CompactUniquePtrTuple`) to fight exactly this. An 8 B hot record + side-tabled cold data is
**two orders of magnitude** below the conventional-OOP node before any algorithmic win.

**N2 — the chained skip, and its honest crossover.** Chain four Round-1/2 pieces:
paint-order SoA (linear scan) + `subtree_size` (O(1) subtree skip) + a per-node **epoch-stamped
"subtree contains dirty" lane** (publish walks `parent[]` marking ancestors, epoch-deduped so a
second dirty leaf under the same parent stops at the first stamped ancestor; clearing is
`epoch += 1`, never an O(N) reset — readset.rs' trick reapplied) + `parent[]`:

```
while i < N {
    if dirty_epoch[i] == epoch { visit(i); i += 1 }      // in the dirty closure
    else                       { i += subtree[i] }        // skip the whole clean subtree
}
```

Measured at 1M nodes (realistic uniform-attachment tree, max depth 28 — an earlier
window-attachment generator degenerated to depth 30,597 and buried the win; generator realism
is load-bearing):

| dirty | visited + skips | bare skip-visit | with ~pipelined work | full scan (same work) |
|---|---|---:|---:|---:|
| 0.01 % | 968 + 4.4k | 23.6 µs (**4.8×**) | 22.4 µs (**19.9×**) | 444 µs |
| 0.1 % | 7.0k + 24.3k | 131 µs (0.9×) | 124 µs (**3.6×**) | 444 µs |
| 1 % | 46k + 107k | 520 µs (0.2×) | 522 µs (0.9×) | 444 µs |

The skip walk is branchy dependent loads (~3.5 ns/step); the full scan streams (0.11–0.44
ns/node). So the chain is the order-of-magnitude lever precisely on the common frame (≤0.1 %
dirty) and a *loss* past ~1 %. **LAW: the scheduler picks the traversal per frame by dirty
count — below ~1 % of N, skip-walk the closure; above, one streamed full pass.** Both endpoints
are measured constants, so the cutover is hardcodable per record width. (Unreal's `Volatile`
flag — "this changes every frame, skip the caching machinery" — is the same law shipped as a
manual annotation; Unity DOTS ships the streamed endpoint as chunk change-filters and
recomputes hierarchies fully. We get both, chosen by count.) Publish-side marking is
memory-latency bound: ~1 µs per *cold* dirty path at 1M (random 4 B-lane + parent[] misses ×
depth); a 1-bit dirty lane (128 KB, L2-resident) would mark cheaper but pays an O(N/64) word
clear per frame — epoch-vs-bit is a real trade to re-measure on the final record layout.

---

## O · Order without position (`bin/order.rs`)

Durable order queries ("does A paint/focus before B?") over `Slot`-identified items must not
depend on physical position — positions shift on every edit (that scatter is Round 1's measured
splice identity-tax). The literature answer is **order maintenance** (Dietz–Sleator
[STOC 1987](https://www.cs.cmu.edu/~sleator/papers/maintaining-order.pdf); Bender–Cole–Demaine–
Farach-Colton–Zito [ESA 2002](https://erikdemaine.org/papers/DietzSleator_ESA2002/paper.pdf):
u64 tags need ~2.4·log₂n bits ≈ 48 bits at n=10⁶, so a u64 is comfortable; O(1) amortized
insert with local relabeling). Implemented in deployment shape — items in a doubly-linked slab
(2×u32 links, which `List` records carry anyway) + a u64 label; insert-after = O(1) splice + one
midpoint; on gap exhaustion, redistribute a window grown until it has quadratic headroom:

| pattern | ns/insert (amortized, 100k inserts) | relabels | relabeled items/insert |
|---|---:|---:|---:|
| random | **6** | 0 | 0.00 |
| hotspot chain (after previous) | **41** | 19.2k | 13.8 |
| front (adversarial, same gap) | **38** | 30.4k | 14.5 |
| physical `Vec` front-insert | 7,251 | — | — |

Order query = one u64 compare (0.125 ns measured). The naive alternative — f64 fractional
midpoints, the CRDT-toy scheme — **exhausts after 53 inserts** at one hotspot (measured; Figma's
production system mitigates with variable-length strings that grow without bound under this
adversary). 16 B/item, ~180× cheaper than physical order at the front, and it composes with T1:
chunk edits stay O(page) while labels answer global order with no scatter-fixups.

**ADOPT** u64 windowed-relabel order labels wherever durable cross-container order is queried
(z-order, focus order, arrangement projections over `List`). Reject fractional keys.

---

## S · Sharing primitives (`bin/share.rs`) — triomphe, elsa, and past them

**S1 — ThinArc/HeaderSlice (triomphe / servo_arc).** `triomphe::Arc` drops std's weak count
(header = 1 word, no weak-safety RMW dance —
[docs.rs/triomphe](https://docs.rs/triomphe/latest/triomphe/)); its `ThinArc<H, T>` puts
`{count, len, header H, [T]}` in **one allocation behind a one-word pointer** (std `Arc<[T]>` is
a two-word fat pointer; `Arc<Vec<T>>` is two allocations and an extra hop). Servo's Stylo hangs
its rule tree and style-sharing on exactly this (lock-free racy insertion of rule nodes, style
structs Arc-shared parent→child —
[Mozilla Hacks, Quantum CSS](https://hacks.mozilla.org/2017/08/inside-a-super-fast-css-engine-quantum-css-aka-stylo/)).
Reimplemented dependency-free (the header→payload offset is a **compile-time constant**, so
deref does zero layout math):

- sizes: `ThinArc` **8 B**, `Option<ThinArc>` **8 B** (NonNull niche), `Arc<[u32]>` 16 B;
- construct+drop (64-u32 payload): ThinArc **34.2 ns**, `Arc<[u32]>` 36.0, `Arc<Vec>` 31.5
  (+ a permanent extra hop and second allocation);
- clone+drop: **3.91 = 3.91 ns** — clone parity with std on M3's cheap LSE atomics; the win is
  the *word*, the *single allocation*, and the final-drop path, not the clone.

**ADOPT** for every shared header+slice payload: children arrays, glyph runs, rule/style chains,
`Arc<[T]>` promotions from the commit diff (07 §2.1) — the promoted form should be thin.

**S2 — Borrow-to-read (ArcBorrow's lesson).** A call that clones an `Arc` to *look* at the
payload costs 3.91 ns vs 0.84 ns for the same call taking `&[T]` (**4.6×** — two RMWs of pure
refcount traffic). At 10k styled nodes/frame that is ~30–80 µs of nothing. triomphe's
`ArcBorrow` (a `NonNull` + lifetime, bit-identical to `&T`) is the type-level spelling.
**LAW: no kernel read path takes an owned handle; ownership transfers take the Arc, reads take
the borrow.** (This is also Round 1's `!Send` graph logic extended to the shared-payload layer.)

**S3 — Frozen arenas: beat elsa by chunking.** `elsa::FrozenVec` hands out stable `&T` under
`&self` appends by requiring `StableDeref` payloads — i.e. **a Box per item**: the outer Vec
reallocation moves boxes, not pointees ([Manishearth/elsa](https://github.com/Manishearth/elsa)).
Correct, and the wrong constant factor. A **chunked stable arena** (`Vec<Box<[MaybeUninit<T>;
4096]>>`; chunks never move, only the pointer-vec reallocates — the boxcar/append-only-vec
shape) gives the same stability guarantee with no per-item allocation:

| | push (1M × 16 B) | random read | full iterate | bytes/item |
|---|---:|---:|---:|---:|
| elsa-style (`Vec<Box<T>>`) | 7.8 ns warm / **255 ns cold** | 1.18 ns | 27.8 ns | ~40 |
| **chunked stable arena** | **3.0 ns (state-independent)** | **0.70 ns** | 14.0 ns | ~16 |
| plain `Vec` (unstable!) | 3.1 ns | 0.61 ns | 19.5 ns | 16 |

Push matches plain `Vec` while addresses stay valid forever — and unlike the boxed form its
cost does not swing 30× with allocator/page state. **ADOPT** as the representation for every
interner and registry that hands out `&T` under append (style intern, wire table, kind
registries); elsa's *pattern* is right, its per-item indirection is not. (Full iteration is even
*faster* than `Vec` here — 4096-item chunks prefetch as well as one slab and the accumulate loop
vectorizes per chunk.)

---

## Cross-checks against the frontier (external evidence, verified sources)

- **Compose SlotTable** stores a group as **5 packed ints (~20 B)** including a transitive
  descendant count — the same subtree_size idea, 2.5× our record — over a *single global gap
  buffer*, whose reorder pathology their LinkBuffer prototype exists to fix
  ([how-compose-works](https://android.googlesource.com/platform/frameworks/support/+/androidx-main/compose/docs/compose-component-api-guidelines.md),
  gap-buffer internals write-ups). T1's page-local gaps are the fixed version of their design.
- **Slint** compiles static topology to flat index arrays (zero runtime tree construction; runs
  in 264 KB RAM by line-by-line rendering —
  [slint.dev blog](https://slint.dev/blog/porting-slint-to-microcontrollers)); our chunked store
  is the dynamic generalization: static regions lower to full chunks.
- **Stylo/Blink** both bolt structural sharing onto styles only (rule tree, style structs,
  MatchedPropertiesCache). Round 1's tape dedup + this round's ThinArc give the same sharing as
  a *default* of the lowered form, not a specialized cache.
- **Hashers**: foldhash (now hashbrown's default) at 0.79–2.63 ns/hash vs SipHash's 9–12
  ([orlp/foldhash](https://github.com/orlp/foldhash)) — independent confirmation of Round 1's
  Flag B ("std SipHash on a hot path is an anti-pattern, make it a lint").
- **SmallVec inline-N is a stack liability**, proven in this repo: commit `b172d695cd` replaced
  `SmallVec<[HistoryEntry; 1000]>` undo/redo stacks (~78 KB inline *each*, ~156 KB per
  `EditableTextHistory` value) with `Vec` after real stack overflows. Corollary for the kernel:
  inline capacity only for small N × small T in non-movable owners (Round 1's inline-8 read-set
  is fine; nothing history-shaped ever is).
- **Incremental-computation literature** (Acar's self-adjusting computation: overhead shrinks as
  per-node work grows, up to 3 orders end-to-end; Adapton PLDI'14: demand-driven 7×–2000×,
  [paper](http://matthewhammer.org/adapton/adapton-pldi2014.pdf)) supports N2's shape: an
  order-maintenance clock + dirty closure + demand-driven skip is exactly their machinery
  specialized to a paint-ordered array, with the epoch lane as the constant-time clock.
- **Tree edit distance is categorically out**: RTED/APTED-class algorithms run seconds-to-minutes
  on 1k-node trees ([RTED, VLDB 2012](https://arxiv.org/pdf/1201.0230)); React's O(n) keyed
  reconciliation exists *because* of this. The tape diff (Round 1 §5) + Slot identity already
  encode the O(n) keyed answer; nothing in this design should ever "diff trees" structurally.

## Order-of-magnitude scorecard (measured, this lab)

| Commonly-exercised op | Conventional cost | Adopted cost | Factor |
|---|---:|---:|---:|
| structural edit @1M (uniform) | 20.6 µs memmove / 12.7 ms rebuild-frame | 79 ns (chunked) | **~260× / ~10⁵** |
| 64-edit frame @1M, structure+traverse | 12.7 ms (rebuild) | ~200–270 µs (chunked) | **~50–60×** |
| paint traversal @10k (Round 1) | 3.70 ns/node pointer-chase | 0.08–0.11 ns/node flat | **26–44×** |
| dirty-frame visit @1M, 0.01 % dirty | 444 µs full pass (with work) | 22.4 µs skip-walk | **20×** |
| order insert at front, 100k items | 7.3 µs memmove | 38 ns label | **~190×** |
| read of a shared payload per call | 3.91 ns (Arc clone habit) | 0.84 ns (borrow) | **4.6×** |
| interner push with stable `&T` | 255 ns cold / 40 B (boxed) | 3.0 ns / 16 B (chunked) | **up to 85× / 2.5×** |
| per-node hot memory vs OOP node | 968 B (Godot `Node`) | 8 B + cold slabs | **~120×** |
| topology bytes, serialized | 64 bits/node | 3.0 bits/node (BP) | **21×** |

Every factor above is reproducible from the lab (`tree2`, `order`, `nodes2`, `share`, plus
Round 1's bins) on this machine; the composite claims (frame cost) compose measured parts, not
projections. All new `unsafe` (ThinArc, chunk arena `assume_init`) carries stated invariants and
remains **Miri-pending** (no rustup in this environment) — same status and same obligation as
Round 1's frontier items.
