# Column plan: side columns that move with the records

State: `LEAD` (the core half of `STORE-R6.md`'s contract: the trait shape, fixed here so the store work can be
written against it; not implemented yet, see "When").

A core page holds 64 records in paint order. Every operation that moves records moves whole slot ranges of a
page's columns: `Page::shift` (inside a page), `Page::copy_from` (between two pages: split, merge, overflow),
`Page::write` (a new record), `Page::set_sentinel` (slots become padding). A **column plan** is the set of
*other* per-record columns that must make exactly those moves. The core calls the plan in the same four places
and nowhere else, so nothing above the core ever repairs an index after a splice (bbtree design 07, layer 3:
"`KeySlots` moves keys and calls the columns; it no longer knows which columns exist").

```rust
/// Per-record side columns, one `Side` page per core page (same page id; slot i belongs to record slot i).
pub trait ColumnPlan: 'static {
    /// The side page: plain data. Columns are `[T; 64]`, grouped by the pass that reads them.
    type Side: Copy;
    /// The all-padding side page (what a fresh or freed page holds; padding must be inert for every scan
    /// the plan's owner runs at full capacity, as EMPTY ink and depth 0xFFFF are for the core's).
    const BLANK: Self::Side;
    /// What an insert supplies per record beside its `RecordInit`, and what a read returns: the whole of a
    /// record's side state, so `read` then `write` elsewhere is a move (`move_run`, `rewrite_run`).
    type Init: Copy;
    /// `false` only for `()`: no side pages are allocated and every call below is dead code.
    const HAS_SIDE: bool = true;

    /// Slot `slot` takes `v` (the record was just written there).
    fn write(side: &mut Self::Side, slot: usize, v: &Self::Init);
    fn read(side: &Self::Side, slot: usize) -> Self::Init;
    /// `n` slots move from `src` to `dst` inside one page (memmove semantics), in every column.
    fn shift(side: &mut Self::Side, src: usize, dst: usize, n: usize);
    /// `n` slots are copied from `src[from..]` to `dst[at..]` (two distinct pages).
    fn copy(dst: &mut Self::Side, at: usize, src: &Self::Side, from: usize, n: usize);
    /// Slots `[lo, hi)` become padding.
    fn clear(side: &mut Self::Side, lo: usize, hi: usize);
}

impl ColumnPlan for () { type Side = (); const BLANK: () = (); type Init = (); const HAS_SIDE: bool = false; /* no-ops */ }

pub struct Bough<P: ColumnPlan = ()> { /* ... */ side: SidePool<P> }
```

Rules (bbtree 07's, which are what keep it free):

* **Derive, do not pass.** The plan is a type parameter of `Bough` and of the page-moving helpers; no plan value
  is stored or handed around. `Bough` (no parameter) is `Bough<()>`: today's code, bit for bit.
* **Every plan method and every helper that calls one is `#[inline(always)]`**, so the per-column loops fuse with
  the core's own (`shift` is six `copy_within`s today; a plan adds its columns to the same straight line).
* **One source of truth for the moves.** `Page::shift / copy_from / write / set_sentinel` become
  `PageOps::<P>::shift(page, side, ..)` etc.; `insert_in_page`, `insert_split` (`copy_virtual`), `remove_impl`,
  `merge_next`, `split_at` and `PagePool::free` call only those. A new mutation path cannot forget the side.
* **A plan is declared, not hand-written**: `column_plan! { pub struct RowSide { geom: [i32; 4] = [0; 4], color: u32 = 0, ... } }`
  generates `Side`, `BLANK`, an `Init` with one field per column, and the five methods as one line per column
  (`copy_within`, `copy_from_slice`, `fill`). A hand-written impl is allowed (a plan whose `Init` packs fields).

What the core adds for the plan's owner:

| call | meaning |
|---|---|
| `insert_run_with(pos, recs, inits: &[P::Init])` | the records and their side state in one pass; `insert_run` requires `P::Init: Default` |
| `side(page_id) -> &P::Side`, `side_mut(page_id)` | the side page of a core page (`Page::id`); type-stable storage, the reference is valid until the next structural edit only by borrow, the *address* for the Bough's life |
| `locate_side(h: Handle) -> Option<(&mut P::Side, usize)>` | one hop from a handle to its record's side slot |
| `for_pages(|page: &Page, side: &P::Side, len|)` and a `_mut` twin | paint-order stream of (core page, side page) for write-back, re-description, projections |
| `read_run_with(start, len, recs, inits)` / `rewrite_run_with` | a permutation of a run carries its side state |

Storage: `SidePool<P>` is chunked and type-stable (chunks of 64 side pages, a chunk table that only grows, no
reallocation of a chunk), indexed by page id like the page arena, so page `id`'s side is two loads from the id and
none of them depends on the directory. `freeze_into` does not replicate side pages (they are CPU-only writer
state); a reader that needs a column asks for it explicitly.

What a plan may not assume: a record's slot is stable only between structural edits (that is what the handle is
for); side columns are not translated or re-framed by the core (geometry in a side column is in the plan owner's
coordinates: space-local fixed point in `STORE-R6.md`, which a scroll does not touch); pages split at 64 records
and merge below 8, whatever the side page's size, so a side page is sized for 64 slots.

Cost when unused: `P = ()` folds to the current code (gate: every cell of `TABLE-R6.md` within 1.00x, and the
disassembly of `insert_in_page::<()>` equal to today's). Cost when used: per moved record, one copy per column,
in the pass that already touches the record's page.

When: the page-moving paths are being rewritten for directory nodes (round 6, core lane); the plan parameter
lands with that rewrite so the paths are rewritten once. Until then the shape above is the contract.

---

# Record state planes: any per-record boolean, as bit columns the core moves

State: `LEAD` contract (the core implementation follows it; this section is what `store` and `surface-api` build on).

**Mechanism.** A *state plane* is one bit per record: plane `f`, record at (page, slot) -> bit `slot` of a `u64`
that belongs to the page. A Bough has `STATE_PLANES = 16` of them. Nothing in the core knows what a plane means.
The situations it must serve, all the same four operations: hovered, pressed, focused, focus-within, disabled,
selected, checked, expanded, dragged-over, invalid, search-hit, current, visited, loading, read-only, and the three
the core itself reads: no-hit, interact, tab-stop (today's `KF_*` bits of the kind byte), plus *dormant* (a record
standing for a run of unrealised items).

```rust
pub const STATE_PLANES: usize = 16;
/// Planes the core reads. The rest (4..16) are the caller's to name.
pub const S_NOHIT: u8 = 0;     // KF_NOHIT   (hit_top skips it)
pub const S_INTERACT: u8 = 1;  // KF_INTERACT (flagged_chain)
pub const S_FOCUS: u8 = 2;     // KF_FOCUS   (tab order)
pub const S_DORMANT: u8 = 3;   // a dormant run (the cull reports it instead of drawing it)

#[repr(C, align(64))]
pub struct StatePage {            // one per core page that has any state; same page id; 272 B
    pub bits: [u64; 16],          // now
    pub seen: [u64; 16],          // as of the last `commit_state` of each plane
    pub any: u16,                 // bit f: bits[f] != 0          (the page summary)
    pub changed: u16,             // bit f: bits[f] != seen[f]    (the page is on plane f's flip list)
}
```

| operation | cost | meaning |
|---|---|---|
| `set_state(pos | handle, f, on)` | one locate + one mask write | one record |
| `set_state_run(start, len, f, on)` | one mask write per page of the run (a whole page: one store) | a range: select all, disable a subtree, clear search hits |
| `state(pos | handle, f) -> bool` | one load | |
| `any_state(start, len, f) -> bool` | one summary test per page of the run; one test per directory node once the directory has nodes | "is anything in this subtree selected / focused / invalid": focus-within, a parent's indeterminate checkbox |
| `count_state(start, len, f)` | a popcount per page | selection count |
| `next_state(pos, f, back) -> Option<pos>` | summary scan, then one `ctz` / `clz` | tab order, next search hit, next invalid field |
| `flips(f, |pos, handle, now|)` | pages on plane f's flip list only; per page `bits ^ seen`, one `ctz` per flipped record | **the read side**: exactly the records whose bit differs from what the consumer last saw |
| `commit_state(f)` | the same pages: `seen = bits` | the consumer has applied the flips |

**Moves with the record.** Bits are a column: every page operation that moves records (`shift`, `copy_from`,
`write`, `set_sentinel`, split, merge, free) moves the same slot ranges of all 32 words, as shifts and masks, in
the same call sites as the column plan's hooks (one hook per site serves both). A moved record keeps `bits` and
`seen`, so a move is not a flip. A new record's bits are 0 in both.

**Free when unused.** A page with no bit set in `bits` or `seen` has no state page; one bit of the core page's flag
byte (`F_STATE`) says whether it has one, so a page operation on a stateless page tests one bit of a byte it has
already loaded. Gate: every `TABLE-R6.md` cell within 1.02x with no plane used.

**Read by variant tables (not the core's part, stated so the two halves meet).** A description says how a record
looks or lays out under a set of flags (`.when(Flag::Selected, ..)`; `.hover(..)`, `.active(..)` are instances).
The store resolves that once into a table indexed by the record's flag bits (or by the subset its description
names). A turn then is: handlers write masks (`set_state_run`); the frame calls `flips(f, ..)` for each plane with
a consumer and applies the table row to exactly those records (a paint write, or a style write that dirties
layout); `commit_state(f)`. No signal per row, no walk of the unflipped.

**Summaries upward.** `any` is per page. With directory nodes (next core card) each node carries the OR of its
pages' `any` words, so `any_state` over a subtree of a million records is a descent, and `next_state` is the same
descent the depth scans use. Until then both scan the run's pages (one line each).

**The kind byte's flags.** `KF_NOHIT / KF_INTERACT / KF_FOCUS` stay readable from the kind byte until the hit
paths are measured against the plane (the plane costs a load of the state page per candidate page and saves the
byte-gather multiply per candidate group); the setter writes both, and the planes are the contract.

**Dormant.** A dormant run is a record with `S_DORMANT` set. The frame cull's variant
(`cull_tiles_dormant`) tests `bits[S_DORMANT]` of each admitted page that has `F_STATE`, so a page without dormant
records pays one bit test, and reports each dormant record that touches a damaged tile with the tile span it is
needed for instead of emitting its payload.
