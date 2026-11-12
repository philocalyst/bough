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
