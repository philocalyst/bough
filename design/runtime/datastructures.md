# Data-structure findings for the GPUI-fork reactivity runtime

Status: **runtime contract — the adopted representations for the fundamental structures**,
kernel-internal, bound by [../07-the-ladder.md](../07-the-ladder.md)'s public laws (Signal
keeps `T` inline for `Deref`; `Slot` stays a typed Copy generational id; the sealed delta
ops). Both design-level flags below have been resolved upstream: Flag A (List storage law) in
07 §1; Flag B (hot-map hashers) in [fastpath.md](fastpath.md). The experiment lab (16
zero-dependency runnable binaries + captured run log) is preserved at
[ds-lab/](ds-lab/) — `cargo run --release --bin <name>` — re-run it when porting these
numbers to x86/older-ARM targets, and extend it before adopting any new representation.

**Round 2 exists**: [datastructures-2.md](datastructures-2.md) attacks the *editable* retained
tree (chunked 256-node pages bound structural edits at 36–99 ns from 10k to 1M nodes — ~260×
under dense memmove at 1M), the chained dirty-skip traversal law (~1 % dirty cutover),
order-maintenance labels, and ThinArc/chunked-arena sharing primitives. Where §4 below prices
display-store structural edits at "O(N) memmoves — acceptable", Round 2 supersedes that with
the paged store; everything else here stands.

Scientific evaluation of every fundamental data structure the design commits to, with runnable
Rust experiments. Baselines and creative/frontier replacements were implemented and measured, not
just reasoned about.

## Machine & method caveat

- **Machine:** Apple M3 Pro, macOS (Darwin 24.6), `rustc 1.97.1`, `opt-level=3`, `lto=false`,
  `codegen-units=16`, single-thread, warm-cache.
- **Timing:** custom `std::time::Instant` harness (`src/lib.rs`), min / median / p99 over many
  samples with warmup; `std::hint::black_box` on inputs/outputs. Apple Silicon has cheap LSE
  atomics and an aggressive prefetcher, so **absolute ns are indicative of relative cost on this
  microarch only** — x86 / older ARM will differ, especially for pointer-chase vs linear-scan gaps
  (M3's prefetcher flatters linear scans) and Arc/Rc (M3 flatters atomics). Sub-nanosecond figures
  are at/again­st the harness floor and several autovectorize; treat those as "free / bandwidth-
  bound," not literal.
- **No `rustup`/Miri available** in this environment (system rustc only). Every `unsafe` block
  carries a stated, checkable invariant and is marked **Miri-pending** below.
- Experiments live in `ds-lab/src/bin/*.rs`; full captured output in `ds-lab-runlog.txt`.
- Contracts read first: `07-the-ladder.md` §1/§4/§14, `runtime/scheduler.md`,
  `runtime/collections.md`, `runtime/fastpath.md`, `runtime/memory.md`, `05-one-register.md` §3–4.

---

## TL;DR — adopted recommendations (headline numbers)

| # | Structure | Recommendation | Headline |
|---|---|---|---|
| 1 | `Signal<T>` cell | `{ value: T, slot: Cell<u32> }`; bookkeeping in side WireTable | overhead **4 B** (vs 16–24 naive); `Signal<bool>` **8 B** not 20; read 0.29 ns |
| 2 | `Slot<T>` | `NonZeroU64` packed `[index:32 \| gen:32]` + `PhantomData<fn()->T>` | 8 B, `Option<Slot>` **free (8 B)**, hand-written Copy (derive adds bogus `T:Copy`) |
| 3 | `List<T>` store | contiguous `Vec<T>` (Deref target) + side slot directory | Deref iter == plain Vec; `get(slot)` **3–8 ns** O(1); splice identity-tax 1.8–2.8× |
| 4 | **Display store** | **flattened DFS (paint-order) SoA**, 8 B hot record | full traversal **0.08 ns/node vs 3.70 (AoS) — 44×**; subtree = contiguous range |
| 5 | Tape span | 32 B record; **wide 128-bit hash**; `std ==` confirm | hash **12–21×** the scalar loop (17.9 GB/s); dedup 10k→1 lowering |
| 6 | Subscriber lists | **intrusive** slab (default) + **CSR cache** for high-fanout | unsubscribe **4–11 ns** O(1); publish CSR 0.33 vs intrusive 2.33 ns/sub (**7×**) |
| 7 | Read-sets | inline-8 sorted `SmallVec<u32>` + merge-diff; epoch-stamp dedup | diff **11 ns**, zero-alloc; bitset loses (scans all slots) |
| 8 | SoA patch queue | **dense-scatter dedup** (no full sort) + coalesce | **3.3 ns/patch** vs 9.9 (sort) / 9.4 (radix) |
| 9 | Spatial/order | tile-bucket hit index; 64 px damage bitset; **Fenwick** extent | hit-test **170×**; extent query **3.3 ns** O(log n) vs 3657 ns naive |
| 10 | Handler routing | **FxHash/identity** TypeId map; 3-word inline closure | dispatch **6.3 ns** vs 9.3 (SipHash); capture-free closure **0 B** |
| 11 | `SharedString` etc. | **SSO 24 B** (inline ≤22 B); text hole; `Memo`==`Signal` | text hole **3×** cheaper than `format!`, zero-alloc; same size as the enum |
| F | Frontier/unsafe | unchecked-under-invariant get **1.49×**; niche-stacking; reject bit-pack | see §Frontier |

**Two design-level flags need an upstream decision — see the flagged section at the end.**

---

## 1 · `Signal<T>` cell layout  (`bin/signal.rs`)

**Current design (memory.md §0):** value inline (Deref returns `&T`), an interned `u32` slot id,
intrusive links; no `Rc`, no heap cell, no side value arena. Interning is lazy ("on first bind").

**Candidates & measurements (overhead = `size_of::<Signal<T>>` − `size_of::<T>`):**

| T | Naive (value+slot+subs+dirty+rev+flags) | **Lean (value + `Cell<u32>`)** |
|---|---|---|
| `bool` | 20 B (overhead 19) | **8 B (overhead 7 → 4 B slot + pad)** |
| `f64` | 32 B (overhead 24) | **16 B (overhead 8 → 4 B slot + 4 pad)** |
| `Color=[f32;4]` | 36 B | **20 B** |
| `String` | 48 B | **32 B (overhead 8)** |

- Steady starred read (`*sig`) = **0.29 ns** — a plain field load; in describe it additionally does
  one read-set push (no side-table touch).
- Interning is idempotent and lazy: an unwired `Signal` never allocates a slot or a `WireRecord`.

**Why the slot must be *in* the cell (not address-keyed):** a starred read in `view` records the
cell in the describe read-set, and a publish must find its recorded readers — both need a stable
identity. Addresses move; a `HashMap<*const,…>` is slow. One interned `u32` is the minimal identity.
`Cell<u32>` (interior mutability, sound because the graph is `!Send`) lets interning happen under the
`&self` of describe. All subscriber/dirty/revision state lives in a dense per-window `WireTable`
keyed by slot, allocated on first *reactive* use — so unwired `Signal`s are pure inline fields.

> **VERDICT — ADOPT.** `struct Signal<T> { value: T, slot: Cell<u32> }`. Overhead is a flat **4 B**,
> absorbed into alignment padding for any `T` with align ≥ 4 (f64/Color/String/u32…). `Signal<bool>`
> is **8 B**, not the naive 20. The equality cutoff lives at the consumer (07 §1), so the cell needs
> **no** `PartialEq`, no dirty bit, no clone-before-write. `!Clone` and Deref/DerefMut are unaffected.
> Recommended `WireRecord` (side table, per wired cell): `{ subs_head:u32, dirty_next:u32,
> revision:u32, owner:u32, flags:u8 }`.

---

## 2 · `Slot<T>` / generational ids  (`bin/slots.rs`)

**Current design:** typed, `Copy`, generational, item-not-position, transferable across containers of
the same `T`; `Option<Slot>` should be free.

| Repr | `Slot` | `Option<Slot>` | `[Option<Slot>;8]` |
|---|---|---|---|
| A `{index:u32, gen:u32}` | 8 B | 12 B | — |
| B `u64` packed | 8 B | **16 B** | 128 B |
| C `{index:u32, gen:NonZeroU32}` | 8 B | **8 B** | — |
| **D `NonZeroU64` packed `[index:32\|gen:32]`** | **8 B** | **8 B** | **64 B** |

- **Finding (compiler footgun):** `#[derive(Copy, Clone, PartialEq)]` on a generic `Slot<T>` adds a
  spurious `T: Copy`/`T: Clone` bound even though the only field is `PhantomData<fn()->T>` (always
  Copy). `Slot<T>` must be Copy **regardless of T** (07 §1) → hand-write the impls with no `T` bound.
- `PhantomData<fn()->T>` gives typed branding at **zero size**, is variance-correct, and keeps
  `Slot` `Send`-neutral. `SlotD<String>` is still 8 B.
- **ABA safety:** after `remove`+reinsert at the same index, gen is bumped, so the stale `Slot`
  fails validation. 32-bit gen = 4.29×10⁹ recycles/index before wrap; scheduler.md mandates a wrap
  diagnostic, so ABA is unreachable in practice. 32/32 is the honest split (40-bit index is overkill
  for a per-window slab < 2³² items).
- Generational `get()` ≈ **1.75 ns** vs raw index 0.46 ns (bounds check + `gens[]` second cache
  stream). See Frontier §1 for an invariant-backed unchecked variant that closes ~1.5× of this.

> **VERDICT — ADOPT SlotD.** `struct Slot<T>{ bits: NonZeroU64, _t: PhantomData<fn()->T> }`,
> `bits = (index<<32) | gen`, `gen ≥ 1`. 8 B, `Copy` (hand-impl, no `T` bound), **free `Option`**,
> typed, ABA-safe.

---

## 3 · `List<T>` slab + **the `Deref<[T]>` question**  (`bin/list.rs`)

**Current design (07 §1, collections.md):** `Deref<Target=[T]>` (read-only, never DerefMut);
stable generational `Slot`s that survive moves & transfer between containers; ops
`push/insert_at/take/move_before/edit`; vector tier (O(N) moves documented) below `N_vec`, sum-tree
above.

**Is `Deref<[T]>` even implementable over a tombstoned slab? — YES, in the vector tier, but only
because item storage and identity storage are *separated*:**

```rust
struct List<T> {
    items:       Vec<T>,        // CONTIGUOUS, in list order  => Deref::deref returns &items  ✓
    pos_to_slot: Vec<u32>,      // parallel to items: which slot owns each position
    slots:       Vec<SlotEntry>,// generational directory, TOMBSTONED & reused (this is memory.md's
    free:        Vec<u32>,      //   "tombstoned slab", NOT the item array)
}
struct SlotEntry { gen: u32, pos: u32 }   // pos == NIL when taken/detached; 8 B
```

The "one slab with tombstones" (memory.md §3) is the **slot directory**, and the "contiguous
`&[T]`" (07 §1) is the **item store** — they are different arrays. Reconciling that is the whole
trick, and it works:

| | plain `Vec<T>` (no identity) | **List (contiguous + slot directory)** |
|---|---|---|
| Deref full iter (10k) | 5.9 µs | **2.7 µs (identical slice path)** |
| `get(slot)` O(1) | — | **3.16 ns/lookup @10k, 8.6 ns @100k** |
| splice @ random mid (1k) | 792 ns | 2125 ns (**identity tax 2.7×**) |
| splice @ random mid (10k) | 4.1 µs | 11.5 µs (**2.8×**) |
| splice @ random mid (100k) | 79 µs | 139 µs (**1.8×**) |

Per-item overhead of stable identity over a contiguous store = **12 B** (4 B `pos_to_slot` + 8 B
`SlotEntry`), *separate* from the `Vec<T>`. The splice "identity tax" is the O(N) **scatter-fixup**
of shifted slots' `pos` (random writes into the `slots` slab) on top of the raw memmove.

> **VERDICT — ADOPT** the contiguous-store + side-directory repr for the vector tier. `Deref<[T]>`
> returns `&items` honestly and for free; `get(slot)` is O(1); splice is O(N) = memmove + scatter-
> fixup, which collections.md already documents as the vector-tier cost. **Refinement:** make
> `items` a **gap buffer** so cursor-local insert/remove/move (the common case) costs O(edit
> distance), not O(N) — both the memmove and the scatter-fixup then scale with locality, not size.

---

## 4 · The display store — **adventurous** (`bin/store.rs`)

**Current design (memory.md §0/§3):** "no retained tree of element *objects* … kind-segregated SoA
slabs; a node's hot header is a few machine words; cold data side-tabled." The store is traversed in
**paint order every presentation build** and queried by region and by z/extent.

Three layouts, full paint-order traversal of 10k nodes (accumulate `flags^kind`):

| Layout | hot record | traversal | ns/node |
|---|---|---|---|
| (A) AoS fat node, `first_child`/`next_sibling` DFS | 32 B | pointer-chase across ID-ordered memory | 3.70 |
| (B) SoA header, DFS over dense `u32` arrays | 16 B | pointer-chase, dense arrays | 2.20 |
| **(C) flattened DFS (paint-order) SoA, LINEAR SCAN** | **8 B** | **linear scan** | **0.08** |

**(C) is 44× faster than (A) and 26× faster than (B).** The idea: store nodes **in paint (DFS)
order**; the hot record is `{ subtree_size:u32, kind:u16, flags:u16 }` (8 B), cold/per-kind payload
side-tabled via a `payload_idx`, and a dense `u32 parent[]` (by paint position) for hit/focus.

Two structural superpowers fall out:
- **A full paint traversal is a linear memory scan** — perfect prefetch, no stack, no branch
  misprediction. (The literal 0.08 ns/node is autovectorized; a real per-node paint does more work,
  but the *locality/branch* advantage over pointer-chase DFS is the durable ~10–40× effect.)
- **Any subtree is a contiguous range `[p, p+subtree_size)`** → "relayout the dependency closure,
  not the window" (fastpath §7) becomes a linear scan of one slice, no tree walk. (Probed: 1000
  random subtrees in 25.6 µs.)

Cost: structural edits (insert/move a subtree) are **O(N) memmoves** of the flat arrays. This is
acceptable *and on-model*: describe already rebuilds paint order into reused bump pages every turn
(memory.md §3). The flat scene is the **lowered, diff-stable form** produced by lowering, not an
incrementally pointer-mutated retained tree — exactly what the contract says exists at rest.

> **VERDICT — ADOPT a kind-segregated SoA store with a flattened DFS (paint-order) hot layer.**
> Hot record 8 B in paint order; cold payload in per-kind slabs behind a `payload_idx`; dense
> `parent[]` for the (unavoidably pointer-chase) hit/focus walk. This is the single biggest
> structural lever found and it aligns with "no retained tree of element objects."

---

## 5 · The tape  (`bin/tape.rs`)

**Current design (05 §4, fastpath §6):** compact opcodes in reused bump pages; per-span 128-bit
rolling content hash keyed by `#[track_caller]` + `.key()`; dedup intern (hash → LoweredSpan) with a
**mandatory memcmp confirm**; SIMD the hash and confirm.

`SpanRecord` = **32 B** `{ call_site:u32, key:u32, hash:u128, off:u32, len:u32 }`.

| span size | 128-bit hash scalar byte loop | **wide (8 B/iter)** | speedup |
|---|---|---|---|
| 64 B | 1.01 GB/s | **17.86 GB/s** | **17.7×** |
| 256 B | 0.53 GB/s | **11.1 GB/s** | **21.0×** |
| 1024 B | 0.46 GB/s | **5.83 GB/s** | **12.7×** |

Confirm memcmp — **`std ==` beats a hand-chunked u64 compare** (it lowers to platform SIMD memcmp):
64 B → 25.4 vs 8.0 GB/s; 1024 B → 50.0 vs 20.9 GB/s. Dedup collapses 10k identical rows → **1
lowering**. A forced 1-bit difference is caught by the confirm regardless of hash.

- **Caveat found:** the 10k-span intern loop measured 116 ns/span — dominated by **std `HashMap`
  SipHash**, not the content hash. The intern table must key by the already-computed 128-bit hash
  with an **identity/Fx hasher** (see §10), or a custom open-addressing table, or the hashing you
  just paid for is thrown away and re-done with SipHash.

> **VERDICT — ADOPT.** 32 B span record; **wide 128-bit hash** (bandwidth-bound, multi-GB/s);
> `a == b` for the mandatory confirm (simplest *and* fastest). Intern table keyed by the 128-bit
> hash via an identity hasher, **not** std SipHash.

---

## 6 · Subscriber lists  (`bin/subscribers.rs`)

**Current design (memory.md §3, scheduler.md):** intrusive links on retained records — subscribe/
unsubscribe is pointer surgery, zero alloc; O(1) unmount via removal metadata.

1000-subscriber publish (the scatter a publish does):

| | fresh | after churn |
|---|---|---|
| Vec-per-cell | 0.33 ns/sub | — |
| **intrusive doubly-linked slab** | 2.33 ns/sub (median) | 2.33 ns/sub |
| **CSR contiguous** | 0.33 ns/sub | 0.29 ns/sub (rebuilt) |

- **Intrusive publish is ~7× slower** than contiguous iteration even fresh — the `cur = n.next`
  walk is a serial data-dependent load chain the prefetcher can't hide. Churn scatters nodes and
  keeps it there.
- **But intrusive unsubscribe+resubscribe = 4.2–10.9 ns** (a handful of array writes, no scan, no
  realloc) — the property that matters for mount/unmount churn. Memory: 12 B/sub in a shared slab
  vs 24 B cell header + 4 B/sub for Vec-per-cell.

> **VERDICT — KEEP intrusive as the default** (its O(1) zero-alloc churn is the right invariant, and
> memory.md is correct). **Refinement — hybrid:** for *high-fanout, stable* cells (theme, a palette
> read by 10k rows — the fastpath §3 workload-17 case), the publish walk dominates and intrusive
> locality is 7× worse. Give such cells a **CSR cache**: a contiguous `Vec<u32>` of targets,
> rebuilt lazily when membership changes (dirtied on subscribe/unsubscribe), scanned linearly on
> publish. Choose per-cell by measured fanout×churn — intrusive is the source of truth, CSR is the
> publish-time materialization.

---

## 7 · Describe-frame read-sets  (`bin/readset.rs`)

**Current design (07 §1):** per-owner set of read cells, diffed each describe to update reverse
edges; small; must be zero-alloc steady-state.

8-cell read-set, one cell in / one out per re-describe:

| Candidate | cost | note |
|---|---|---|
| **(A) sorted inline-8 `SmallVec<u32>` + merge-diff** | **11.2 ns** | exact added/removed, zero-alloc |
| (B) bitset (2048 slots) XOR | 21.8 ns | **scans all slots**, not O(reads) — loses when slots≫reads |
| (C) epoch-stamped mark | 4.4 ns | O(reads) *recording* primitive, not a diff by itself |

Inline-8 covers the common view with zero heap (`Small` enum = 40 B).

> **VERDICT — ADOPT** an inline-8 sorted `SmallVec<u32>` of interned slots + sorted merge-diff (exact
> added/removed, ~ns, zero-alloc). Use an **epoch-stamp array as the intra-frame dedup** (a cell read
> twice records once). **Reject the bitset** except for pathologically dense read-sets — it pays
> O(total slots), not O(reads).

---

## 8 · SoA patch queues  (`bin/soa.rs`)

**Current design (fastpath §3):** per-kind `PatchQueue{ slots:Vec<u32>, values:Vec<V> }`; drain =
sort+dedup by slot (keep-last), scatter, coalesce touched runs into upload ranges; zero-alloc.

10k patches → distinct slots, per-patch cost:

| distinct | (A) std stable sort + keep-last | (B) **dense-scatter dedup** | (C) LSD radix + keep-last |
|---|---|---|---|
| 3.7k | 9.9 ns | **3.3 ns** | 9.4 ns |
| 6.3k | 10.4 ns | **5.0 ns** | 9.4 ns |
| 628 (of 1k) | 8.7 ns | **4.2 ns** | 9.1 ns |

`winner[slot] = last-index` in O(patches) gives keep-last for free with no sort; enumerate via a
first-touch list; sort only the *distinct* touched slots (radix when that set is large) and coalesce
ascending runs (10k→276 upload runs in 1.2 µs). All allocation-free with reused scratch.

> **VERDICT — ADOPT dense-scatter dedup** as the default drain (beats sorting all patches whenever
> there's write amplification — exactly what SoA queues target); radix only for sorting the distinct
> set when it's large. Value storage: keep small POD in the SoA lane, indirect the rest (see
> Frontier §2 on a tagged `Value`).

---

## 9 · Order sum-tree, damage tiles, hit index  (`bin/spatial.rs`)

**Current design (fastpath §1/§4, collections.md):** tile-bucketed hit index; 64 px damage bitset
with density promotion; prefix-sum extent index for virtualization.

- **Hit test** (1000 queries, 5000 boxes): linear scan 9108 ns/query → **128 px tile buckets 53.6
  ns/query = 170×**. Topmost hit = max slot index in the point's tile bucket. A uniform grid beats a
  BVH to build/query for axis-aligned, low-overlap UI.
- **Damage** (200 rects → 64 px tile bitset, 2040 tiles, 256 B): union in **542 ns**; idempotent bit
  set gives O(1) dedup independent of patch count; `count_ones` gives the density-promotion total.
- **Extent index** (100k variable-height rows): **Fenwick tree in a flat `Vec<u64>` of n+1 words**
  → row→pixel query **3.3 ns O(log n)** vs naive rescan 3657 ns (**~1100×**); single-row height
  update **8.8 ns O(log n)**; pointer-free.

> **VERDICT — ADOPT** uniform tile-bucket hit index, 64 px damage tile bitset, and a **Fenwick tree**
> (flat `Vec<u64>`) for order/extent — the lean, pointer-free alternative to a node-based sum-tree
> for virtualization, with O(log n) query *and* update.

---

## 10 · Handler routing + closure SSO  (`bin/routing.rs`)

**Current design (07 §3, 05 §2):** route by owner/target `TypeId`; region closures in 3-word inline
storage, oversized captures spill to a counted pooled box.

- 1000 dispatches over a 32-type table: std `HashMap` (SipHash) **9.29 ns** → **FxHash 6.29 ns**;
  sorted array + binary search **16.4 ns** (branch mispredicts on 16 B `TypeId` compares). `TypeId`
  is already a 128-bit avalanched value — SipHash re-hashing it is pure waste.
- Closure SSO: capture-free closure / fn-item = **0 B**; fn pointer 8 B; 3× `u64` capture = 24 B.
  A 3-word (24 B) inline union holds `fn ptr + ≤2 words` of captures; the common region producer
  (reads a field, returns an element) is capture-free → inline, zero-alloc.

> **VERDICT — ADOPT** an FxHash/identity-hash `TypeId` map (not std SipHash). Sorted+bsearch is
> *not* the small-table win I hypothesized (mispredicts) — only a tiny linear-scan table (≤~8) would
> beat the hash map; a compile-time perfect hash is viable when the type set is closed. Closure
> storage = 3-word inline union; capture-free producers are 0 B; oversized captures spill to a
> counted pooled box.

---

## 11 · `SharedString` / text hole / `Memo` / `TargetRef`  (`bin/text.rs`)

- **`SharedString` (SSO):** `[u8;24]` with a tag byte at index 23 (`0..=22` inline length, `0xFF`
  heap). **24 B — the *same* size as the plain `{Static|Arc<str>}` enum** (two fat pointers + tag),
  but inlines labels ≤22 B (`"Add"`, `"3 selected"`, `"file.rs"` …) with **no heap**. Clone: SSO
  inline copy **1.88 ns** vs Arc 4.96 / Rc 6.21 ns. (On M3, Arc≈Rc — LSE atomics are cheap — so the
  Rc arm is *not* worth losing `Send` at the worker snapshot boundary; keep `Arc<str>` as the heap
  arm.) `unsafe` union with a stated tag-offset invariant; **Miri-pending**.
- **Text hole:** `write!` into a retained buffer + byte-compare = **14 ns/publish, zero heap** vs
  `format!` **41.7 ns + 1 alloc** each = **3.0× cheaper** and allocation-free (memory.md §4).
- **`Memo<T>`** is storage-identical to `Signal<T>` (16 B for f64) — differs only in API (no
  DerefMut, built from declared sources). Zero extra footprint.
- **`TargetRef`** = 4 B interned name (`NonZeroU32` → free `Option`); the manifest (name→node) is a
  per-describe map resolved *after* the pass, so forward refs are legal (07 §5, BSN `#Name`).

> **VERDICT — ADOPT** SSO 24 B `SharedString` (inline ≤22 B, `Arc<str>` heap arm); the `write!`-into-
> retained-buffer text hole; `Memo` == `Signal` shape; `TargetRef` = `NonZeroU32` + per-pass manifest.

---

## Frontier / adventurous / unsafe scorecard  (`bin/frontier.rs`)

Techniques with a lower prior of winning, tested anyway (per the request). Every `unsafe` has a
stated invariant; **all Miri-pending** (no Miri in this env).

1. **Unchecked-under-invariant `get`** — ADOPT (small, free). If `gens.len()==items.len()`
   (Invariant A) and slots are only minted with index<len, a *successful* `gens.get(i)` proves
   `i<items.len()`, so `items.get_unchecked(i)` is sound. Measured **1.49×** (1.26 → 0.85 ns) on the
   hot slab get. *Invariant:* `gens.len()==items.len()` maintained by every op; never shrink below a
   live index.
2. **Tagged `u64` `Value`** for mixed patch kinds — REJECT-for-now. The `enum{Color|Opacity|Len|
   TextHole}` is already **8 B** and niche-clean; the packed `u64` is also 8 B. Adopt packing only if
   a future kind widens the enum past 8 B while all live values stay ≤32-bit.
3. **Bit-packed flat node (`u64`)** — **REJECT** (a "low-chance" idea, confirmed to lose). Vs the 8 B
   struct it was *slightly slower* (19.3 vs 17.2 µs/100k) — both autovectorize, packing buys nothing
   when the struct is already 8 B and aligned. Packing only earns its keep crossing a size class
   (12→8 B) or into a niche.
4. **Unchecked SoA scatter write** — ADOPT (hottest loop). Interned slots are minted < capacity, so
   `get_unchecked_mut` is invariant-backed; **1.13×** by removing the branch+panic path.
5. **NonZero niche-stacking** — ADOPT everywhere optional handles cluster. 4× `Option<NonZeroU32>`
   packs to **16 B** (not 32); e.g. the virtualizer's per-row retention reasons, with all-None =
   all-zero.

Techniques considered and *not* worth a separate experiment: MMU/page-protection write-watching
(05 §3 already rejects it — platform-specific, false-sharing, "plain Rust + a segfault handler");
full generative/branded lifetimes à la `GhostCell` (the maintained-invariant unchecked get in §1
captures ~all the benefit with a one-line invariant and no HRTB ceremony on the public surface).

---

## ⚑ DESIGN-LEVEL FLAGS (need an upstream decision — do not silently absorb)

### Flag A — `List<T>: Deref<Target=[T]>` is unimplementable over the promoted (sum-tree) tier

`Deref` is a trait on the single type `List<T>`, so `Deref<Target=[T]>` must return one contiguous
`&[T]` **at every size**. collections.md promotes to a **sum-tree above `N_vec`**, and a sum-tree
stores items in scattered leaf chunks — you *cannot* return a whole-sequence `&[T]` from it without
materializing an O(N) copy (which defeats promotion and can't yield a borrow anyway).

**These two contracts collide.** Options:

1. **(Recommended) Item storage stays contiguous at every size** (a gap buffer); the "sum-tree" of
   the promoted tier indexes **extents / order-statistics only** (pixel↔row for virtualization —
   the Fenwick tree of §9), layered *over* the contiguous store, not replacing it as item storage.
   Then `Deref<[T]>` is honest and O(1) at all sizes; large moves are O(N) memmoves (measured cheap
   to ~10⁵; 05 §14 already walls 100k-datum scale). **This makes the vector-tier repr the only item
   storage; the sum-tree is a geometry index, not a container.**
2. Make `Deref<[T]>` a vector-tier-only capability — impossible, since promotion is internal to one
   type and `Deref` can't conditionally exist.
3. Change `List`'s read surface from `Deref<[T]>` to a cursor/`.get()/.iter()` API that never
   promises a contiguous slice — a public-surface change (07 §1 explicitly sells `Deref<[T]>` slice
   patterns/`.iter()`/`.len()` "for free").

The fundamental impossibility: you cannot simultaneously have {contiguous-list-order `&[T]`, O(1)
`get(slot)`, **O(1)** move} — `Deref<[T]>` forces contiguous-list-order, which forces O(N) moves.
The design should **explicitly adopt Option 1** (contiguous storage forever, sum-tree = extent index
only) and state that "promotion" changes the *geometry index*, not the item storage — otherwise
`List<T>: Deref<[T]>` is a latent contradiction the first time someone builds the sum-tree as the
item container.

### Flag B — the tape dedup intern must not use std `HashMap`/SipHash

fastpath §6 / memory.md §3 spec the intern table as "span content-hash → LoweredSpan." If that table
is a std `HashMap`, SipHash **re-hashes the 128-bit content hash you already computed** — measured
**116 ns/span**, dominating the ~ns hashing it's supposed to leverage. Mandate an **identity/Fx
hasher over the precomputed 128-bit hash** (or a custom open-addressing table). Same point applies to
the `TypeId` handler router (§10) and any slot-keyed map on a hot path: **std SipHash is a hot-path
anti-pattern in this codebase** and should be a lint, not a footnote.

---

## Coverage map (all inventory items + adds)

| Inventory item | experiment | status |
|---|---|---|
| 1 Signal cell | `signal.rs` | done |
| 2 Slot/gen ids | `slots.rs` | done |
| 3 List slab + Deref question | `list.rs` | done + **Flag A** |
| 4 Node/display store | `store.rs` | done (adventurous flat-DFS) |
| 5 Tape | `tape.rs` | done + **Flag B** |
| 6 Subscriber lists | `subscribers.rs` | done (intrusive + CSR hybrid) |
| 7 Read-sets | `readset.rs` | done |
| 8 SoA patch queues | `soa.rs` | done |
| 9 Order sum-tree / damage / hit index | `spatial.rs` | done |
| 10 Handler routing + closure SSO | `routing.rs` | done |
| 11 SharedString / Memo / TargetRef | `text.rs` | done |
| Frontier (unsafe/mix-match/low-odds) | `frontier.rs` | done |
| R2: editable tree (dense/gap/chunked/PMA/BP) | `tree2.rs` | done → [datastructures-2.md](datastructures-2.md) |
| R2: order-maintenance labels | `order.rs` | done → round 2 |
| R2: record width / lanes / dirty-skip @1M | `nodes2.rs` | done → round 2 |
| R2: ThinArc / ArcBorrow / frozen arenas | `share.rs` | done → round 2 |

Re-run everything: `cd ds-lab && CARGO_TARGET_DIR=$PWD/target cargo run --release --bin <name>`.
Full captured output: `ds-lab-runlog.txt`.
