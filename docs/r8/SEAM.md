# The expr / raster seam (round 8)

Status: **v1, 2026-11-12.** Sections 1 to 4 are the expr lane's v0 proposal as sent. The raster lane answered in
`wt/r8-raster/docs/r8/SEAM.md` section 5 (commit `58b3c09c5f`); the lead decided its eight requests. **Section 6
is the settled text**: where it differs from sections 1 to 4, section 6 wins. Two points are still open and are
marked so.

The seam is three things: the expression trait, the column layout, and the uniform block. Each item says what is
fixed, what is open, and where the proposal departs from `ROUND8.md`.

## 1. The uniform block

* A block is at most 64 words of 32 bits: `#[repr(C)] struct U { w: [u32; 64] }`, four cache lines, one `u64` mask
  names any subset of its words. An `f32` is stored as its bits. Word 0.. are assigned by the application's
  `uniforms!` declaration in field order; a lens type (`trait Lens { const SLOT: usize; type T: Word; }`) is the only
  way an expression names a word.
* The expr lane keeps two copies: the application's own struct (plain fields, written by plain `&mut` code with no
  write barrier) and the copy last given to the raster. A turn ends with one compare of the two (the changed-word
  mask); that compare is the whole of "noticing a write".
* The raster receives `&U` (the new block) with every frame and, for the frame after a change, the changed mask.
  On the GPU the block is one uniform buffer of 256 bytes, uploaded when the mask is non-zero.
* Open: more than 64 words (an app with many independent animations). Proposed answer: a second tier of per-record
  animation words in a column, not a bigger block. Not built.

## 2. The expression trait

```rust
pub trait Expr: Copy + PartialEq + 'static {
    type Out: Copy + PartialEq;
    type Stage: Stage;          // Const | Mount | Turn | Frame, joined from the inputs' stages by the type system
    const READS: u64;           // the uniform words it reads
    fn eval(&self, u: &U) -> Self::Out;       // pure, #[inline(always)]
}
```

* **The expression value is its own per-record constants.** `U<L>` and `K<const V>` are zero-sized, `Rec<T>` holds
  a `T`, and every combinator is a `#[repr(C)]` struct of its operands, so the bytes of a value of type
  `Select<Eq<U<Hover>, Rec<u32>>, K<A>, K<B>>` are exactly the one `u32` the record owns. There is no separate
  `Consts` type and no `Cols<..>` argument (departure from ROUND8 section 1).
* **`delta` is not a method.** For a pure expression the exact derivative over a page is
  `eval(old) != eval(new)` for each of its 64 records, which the compiler turns into two vector evaluations and a
  compare. It is exact for an opaque `Map<F>` too, so ROUND8's "an opaque closure has the weakest derivative" is
  wrong whenever the closure is pure, which a Frame expression must be. Structural rules remain only as page
  filters (`READS`, and later a per-page summary for `Eq`), never as the definition of damage.
* What the raster needs from an expression: `eval` monomorphised into its kernel, and a cold walk of the structure
  for WGSL (`fn wgsl(w: &mut dyn Emit)`, one line per node). A user function is a named zero-sized type
  implementing `Func<Args>`; the raster lane decides how its body reaches WGSL (the natural form is a
  `#[wgsl]` module function the type forwards to). A capturing closure cannot be a Frame expression; that is a
  compile error (`size_of::<F>() == 0` asserted in a `const` block).
* Stage rules the raster can rely on: a paint operand may be any stage; a geometry operand must be `Cpu`
  (`Const`, `Mount` or `Turn`); a transform or clip operand may be `Frame`, and then the record's damage is
  `bounds(old) ∪ bounds(new)`, computed by the expr lane.

## 3. Archetypes and the column layout

* An archetype is a paint kind applied to expression types: `Fill<E>`, `Num<D, E>` (a number drawn with tabular
  digits: the value is a record constant, formatted while shading), `Label<E>`, `Outline<E, T>`. The set of kinds
  is closed and is this trait, which the raster lane implements once per backend:

```rust
pub trait Backend {
    fn fill<A: FillArch>(&mut self, col: ColRef<'_, A>, space: &Space, u: &U);
    fn num<A: NumArch>(&mut self, col: ColRef<'_, A>, space: &Space, u: &U);
    // one generic method per paint kind; a kind added later gets a default that calls the fallback kernel
}
```

  Each method is monomorphised per archetype and reached through the column's `&'static Descriptor` once per
  column per frame. Nothing is dispatched per record.
* A column is pages of 64 records, `#[repr(C, align(64))]`:

```rust
struct Page<A> { x0: [i32; 64], y0: [i32; 64], x1: [i32; 64], y1: [i32; 64], rec: [A; 64] }
```

  with `live: u64` per page in a side array (one line covers 512 records). Rects are device pixels in the
  column's *space* (below), half-open. `A` is `#[repr(C)]`, alignment at most 4, no padding (an `unsafe trait Pod`).
  Pages of one column are one contiguous allocation: that allocation is the GPU buffer on unified memory.
* A column is created by the first record emitted at its *position* in a store (a view call site), not looked up
  by type: a store is a non-generic struct holding type-erased columns, and the typed `emit::<A>` at a call site
  writes through the position's base pointer after one `TypeId` compare. The compiler never sees a type that
  lists a component's archetypes, so types stay flat and a component costs one instantiation of its body per sink.
* A **store** is the records of one static shape: the root's fixed part, or the rows of one list. A list's store
  materialises a contiguous window of rows; row `r` lives at ring slot `r mod cap`, its `k`-th record of a column
  at `slot * K + k`. No directory, no handle table, no order lane.
* A **space** is `{ origin: (i32, i32) evaluated from the block (scroll), clip: Rect }`. Screen rect = record
  rect + origin, cut by clip.
* **Paint order** is not stored: it is (store order, row, position in the row's shape). For a store whose rows do
  not overlap each other (flow layout: every list in the sheet) the raster may paint column by column. A store
  whose rows overlap (a canvas of layered nodes) must be painted in (row, position) order or with depth; the
  store says which (`Store::overlap`). Open, and the first thing the layered canvas scene will test.
* What the raster receives per frame: the block and mask, the damage (a bitmap of 32-pixel device tiles,
  `stride` words per tile row, the same geometry as `bough-core`'s), and per column a bitmask of pages whose bytes
  changed since the last frame (the upload ranges).

## 4. Questions for the raster lane

1. Is a 16-byte `i32` rect per record acceptable in the kernel's inner loop, or does the GPU side want `i16`
   relative to a per-page origin (half the bytes, one more add)?
2. `Num` and `Label`: is formatting digits while shading acceptable on the GPU (an atlas of ten digit glyphs and
   arithmetic placement), or should the expr lane hand over glyph instances as a third lane?
3. Painting column by column needs "rows of a store do not overlap". Is a per-store flag enough, or does the
   plan of ROUND8 section 6 need it as a type (`Flow` / `Layered` marker on the list call)?
4. The changed mask plus page bitmasks as the upload protocol: is a page (about 1.3 KiB for `Fill`) the right
   upload unit?

## 5. The raster lane's answers

In `wt/r8-raster/docs/r8/SEAM.md` section 5, not copied here. Its measured constraint: wgsl-rs transpiles tokens,
so a generic impl is silently dropped from the WGSL; an expression reaches the GPU as (leaf name, operands), the
leaves being concrete integer functions in a `#[wgsl]` module, and one macro emits `eval` and the code node from
the same identifier.

## 6. v1: what both lanes build on

### 6.1 Accepted as the raster lane asked

1. **Record words.** A record is whole 32-bit words: every `Rec` operand has `align == 4` and `size % 4 == 0`,
   and `repr(C)` order is the depth-first order of the type, so word `k` of a record is the `k`-th `Rec` word met
   walking it. `unsafe trait Words` marks it (zero bytes are valid, no padding).
2. **Code as data.** `const CODE: &'static Code` on every expression. Consequence, accepted: a Rust closure has no
   code tree, so `Map<F, _>` is **not** a Frame expression. It is `Turn` stage: evaluated on the CPU for exposed
   records when a word it reads changes, and its value written into a record word. The finite difference is how
   the CPU knows which records to re-evaluate, so the derivative costs nothing extra there. A Frame-stage user
   function is a named zero-sized type forwarding to a `#[wgsl]` leaf.
3. **Frame-stage arithmetic is integer** (Q8 geometry, Q15 colour and progress). The block may hold any bits, but
   no Frame expression does float arithmetic. For the sheet this means the scroll offset is an `i32` word and a
   number drawn while shading is an integer scaled by `10^D` (a total in hundredths), not an `f32`.
4. **Paints are open and composed.** A record is a value of a `Paint` type; the backend has one generic method
   (`column<P: Paint>`); `Over<Shadow, Over<RRect, Border>>` is one record. **Damage is
   `window(old) ∪ window(new)`**, evaluated by the expr lane's delta kernel through the paint's own `window`,
   which replaces the expr lane's `MOVES` / `offset` pair and makes "a shadow inks outside its rect" the same
   rule as "a transform moves it".
5. **Paint-only geometry may be Frame stage.** The rect lanes are the layout's answer and are CPU stage; any
   paint operand over them (a paint rect, a radius, a clip) may be Frame.
7. **Raster-owned words.** Words 60 to 63 are `PtrY`, `PtrX`, `Pick`, `Time`. The application reads them and never
   writes them; the turn-end compare covers only the application's words. Hover is then `Pick` read back, and
   the CPU hit test in `bough-x` (`store::hit_col`) stays only as the fallback for a CPU-only frame and as the
   oracle for the pick.
8. **A space is an index.** `space: u16` per page into a table of `{ origin value, clip }` beside the block.
   Rows of one clip group are contiguous, so a page has one space. Cost on the expr side: one load per page in
   the delta kernel.
* **Order.** No order lane: `z = store.z0 + row * shape_len + col.pos`. `Flow` or `Layered` is a marker type on
  the list call; `Flow` is checked at compile time against the row's paints (`OUTSET == 0`, `GEOM_READS == 0`).
* **Memory.** A column is VM-page aligned (16 KiB), a whole number of VM pages, reserved once and committed on
  demand, and never moves (today `RawCol::grow` reallocates while a shape is being discovered: to be replaced).
  A turn that writes a page the GPU is still shading: copy-on-write per dirty page, with the per-column page
  bitmask as the set; owned by the threads lane when it exists.

### 6.2 Accepted in part: request 6, the plan as const data

A list's rows are a `Row` type (a tuple of records), so `Row::PAINTS: &'static [&'static PaintDesc]` is const
data and the expr lane provides it. To make that true of every store, a fixed shape is described as one row
value too (`s.row((a, b, [c; 10]))` in place of separate `emit` calls); that also removes the one remaining
per-record check (a `TypeId` compare per `emit`), leaving one compare per store per pass.

`children: &'static [&'static Descriptor]` for the whole application is **not** available as const data without
the view having a type that mirrors its tree, which is exactly what description by continuation does not build
(and what kept types nested in round 7). Two ways to get it, in order of preference:

* the plan's whole-application half is assembled once at start-up from the tree of stores the first mount
  discovers (each node carrying its const `PAINTS`); the per-archetype half stays const. An archetype first seen
  later is drawn by the CPU kernel while its GPU function compiles, as already decided;
* or a component declares `const CHILDREN` (a macro can write it). Not built; say if the start-up form is not
  enough.

### 6.3 Open: where the traits live

`column<P: Paint>` means the sink's `emit<P: Paint>` must name `Paint`, and `Paint` names `Expr`. The proposal:

```
bough-expr   (no dependencies; expr lane)   U, lenses, Words, Code, Stage, Expr
bough-paint  (raster lane)                  ops, op!, Paint, prim!, the kernels, the backends   -> bough-expr
bough-x      (expr lane)                    pages, stores, sinks, relations, the world          -> bough-paint
```

`bough-paint/src/expr.rs` already says it is a mirror to be swapped; `bough-expr` is that swap. Until the raster
lane agrees, `bough-x` keeps a minimal trait of its own (`store::Arch`: `READS`, `changed`, the window) and its
oracle paint kinds, so nothing in the stores depends on which way this goes.

### 6.4 Open: the expression trait's exact text

The two lanes' traits differ in three places; proposed resolution in each:

| | expr lane (`bough-x/src/expr.rs`) | raster lane (`bough-paint/src/expr.rs`) | v1 |
|---|---|---|---|
| stage | `type Stage: Stage`, joined by `Join` impls | `const STAGE: Stage` | both: the type lets a bound refuse a stage (`T::Stage: Cpu`), the const is what `Code` and the plan read |
| inputs | `eval(&self, u: &U)` | `eval(&self, cx: &Cx)` with `Cx { u, rect }` | the raster's: `Lane` (the record's own rect) is an operand |
| block words | `Un<L: Lens>` (a named, typed word) | `Uw<const S: usize>` | `Un<L>`, with `Uw<S>` as the untyped form underneath |
