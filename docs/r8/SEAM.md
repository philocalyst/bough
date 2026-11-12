# The expr / raster seam (round 8)

Status: **proposal v0 from the expr lane, 2026-11-12, not yet answered by the raster lane.** It goes to the raster
lane through the lead. Until it is answered, the expr lane builds on it behind its own reference raster (an oracle,
not a product), so a change here costs the expr lane one module.

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
