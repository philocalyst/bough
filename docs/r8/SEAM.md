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

## 5. raster: answers and counter-proposals (2026-11-12, raster lane; goes back through the lead)

Accepted as written: the block (64 words, one mask), the expression value as its own record constants, damage
as `eval(old) != eval(new)` over exposed pages, no write barrier, pages of 64 with rect lanes, the tile
geometry of the damage bitmap. What follows is what the raster needs added or changed, then the four answers.

### 5.1 Measured first: what reaches the GPU and how

wgsl-rs transpiles tokens, not types. Probe (`crates/bough-paint/src/probe.rs`): a trait with a static
method, implemented for a concrete type and called through a generic function with a turbofish, is
monomorphised into WGSL (`shade_Hover` calling `Hover_eval`). A generic impl
(`impl<C: Ex> Ex for Twice<C>`) compiles as Rust and is silently dropped from the WGSL (`Twice_eval` is
called and never defined). So nested combinators cannot go through wgsl-rs's own generics, and a proc macro
never will see trait resolution. The route that works:

* **Leaves are concrete functions** in a `#[wgsl]` module (`ops`), pure, integer, in the wgsl-rs subset. They
  import the round-7 kernel's arithmetic (`sd`, `cov`, `scale`, `blend`, `shadow`, ...), which stays the one
  definition of a pixel.
* **A combinator is (leaf name, operands)** and nothing else. One macro emits both faces from the same
  identifier: `eval` is `ops::NAME(a.eval(u), b.eval(u), ..)` and the code node is `Call("NAME", [A, B, ..])`.
  CPU and GPU agree by construction because neither face has a body of its own.
* The per-archetype GPU function is a pure function of (record words, uniform words read, x, y), rendered from
  the code tree. Rendered expressions are valid WGSL and MSL unchanged (calls, `123u` literals, parameter
  names; constructors and casts are leaves), so the leaves go through naga once, at the raster crate's build,
  and the per-application part needs no naga at run time on Metal. (Vulkan needs SPIR-V: naga at start-up or an
  application build step. Open.)

**Requests that follow from this:**

1. Record layout: every `Rec<T>` is a whole number of 32-bit words (`align == 4`, `size % 4 == 0`), and the
   `repr(C)` order is the depth-first order of the type. Then word `k` of a record is the `k`-th `Rec` word met
   walking the type, which is how the GPU function names it. A `Rec<u8>` needs a shift leaf, not a byte lane.
2. Code as data, not only a walk: `const CODE: &'static Code` on each expression (a static tree mirroring the
   type; `&C::CODE` in a generic impl is a promoted constant). The `dyn Emit` walk can be derived from it; the
   reverse is not true, and the compile-time plan (5.4) needs to read reads, stages and cost without running
   code.
3. **Frame-stage arithmetic is integer** (Q8 geometry, Q15 colour and progress, plain `u32`). A float in a
   Frame expression ends bit-exactness between CPU, Metal and x86 (contraction, `sqrt`, conversions), and with
   it the right to choose the executor per tile with no seam. Floats belong to `Turn` and `Mount` and are
   converted when the column is written. An `f32` stored as bits in the block is fine as long as no Frame
   expression does arithmetic on it.
4. A user function is a named zero-sized type forwarding to a leaf in a `#[wgsl]` module of the user's crate;
   its module's source is appended to the leaves. A capturing or non-wgsl closure is `Turn` stage: evaluated on
   the CPU into a column word. (Agrees with your `size_of::<F>() == 0` rule; adds "and its body is a leaf".)

### 5.2 Paint kinds: open and composed, not a closed list of methods

`Backend { fill, num, label, outline }` is the interpreter's kind list moved into a trait. The place the type
system pays most is composition: a card that is always "shadow under rounded body under border" should be one
record of one archetype `Over<Over<Shadow<..>, RRect<..>>, Border<..>>`, because then (a) its layers never
interleave with another node's, so it is one entry in a tile's order, not three; (b) the cut between its own
layers is static (inside the body's core the shadow is not evaluated: known from the types, no test at bin
time); (c) rect, clip and theme operands are shared. Proposed:

```rust
pub trait Paint: Pod {                    // the paint value is the record, as an expression value is
    const READS: u64;                     // block words read by any operand
    const GEOM_READS: u64;                // ... by operands that move its window (transform, clip, paint rect)
    const OPAQUE_CORE: bool;              // its core replaces what is beneath (alpha known ONE from the types)
    const OUTSET: Outset;                 // how far ink reaches beyond the rect (a shadow: 1.5 blur widths + offset)
    const COST: Cost;                     // Flat | Edge | PerPixel | Neighbourhood
    const CODE: &'static PaintCode;
    fn window(&self, rect: Rect, u: &U) -> Win;         // device pixels it may touch, after transform and clip
    fn core(&self, rect: Rect, u: &U) -> Win;
    fn shade(&self, rect: Rect, u: &U, x: i32, y: i32) -> Q15x4;   // the definition; spans and rows are derived
}
pub trait Backend { fn column<P: Paint>(&mut self, col: ColRef<'_, P>, u: &U); }   // one method
```

`Fill<E>`, `Outline<E, T>`, `Num<D, E>` are `Paint` impls in the raster crate; the expr lane's sink emits
`P: Paint` values. Damage must use `window(old) ∪ window(new)`, not the rect: a shadow inks outside its
rect, and "a blur damages more than it covers" is this one constant.

### 5.3 Geometry that only feeds paint may be `Frame`

"A geometry operand must be Cpu" is right for what feeds layout. But a box that grows, moves and rounds while
nothing reflows (every Freya animation demo) must not take a turn per frame. The layout rect stays in the
rect lanes (Cpu stage); the paint may carry a `Frame` rect expression over it (`Lane` is a leaf: the page's
own rect), e.g. `MixRect<Lane, Rec<[i32; 4]>, Ease<Time, Rec<Phase>>>`, a `Frame` radius, a `Frame` clip.
Sub-pixel motion comes from that expression being Q8. The record's window under the block is `Paint::window`,
so damage and binning need no second rule. This replaces "a transform or clip operand may be Frame" with
"any paint operand may be Frame; the rect lanes may not".

### 5.4 The plan needs the archetype list as const data (tension with "no type lists a component's archetypes")

Section 6 of ROUND8 is a `const fn` over the application's archetypes. Flat types and that are compatible if
the list is *data*: each component's `&'static Descriptor` carries
`paints: &'static [&'static PaintDesc]` (one per emit call site, built from the associated constants above)
and `children: &'static [&'static Descriptor]`. A `const fn plan(root)` walks it. Whatever writes the
component's column writer can write these two slices (a macro, or the user's `impl`). Without them the
per-archetype half of the plan is still const (each `PaintDesc` is), but the whole-application half (is a GPU
pipeline needed at all; the block's layout; the GPU source) moves to start-up, and a new archetype appearing
mid-run costs a Metal compile (about 200 ms for the round-7 kernel; the CPU kernel is exact, so it can draw
those frames with no seam, but the plan is then not the compiler's). Please say which it will be.

### 5.5 Block words the raster writes

`Time`, `Pointer` (x, y) and `Pick` (the record under the pointer) are presenter state. On the GPU-resident
path a pointer move is: write `Pointer`; a pick pass writes `Pick` into the block *on the GPU*; damage,
binning and shading read it in the same command buffer; the CPU reads the one word back afterwards to dispatch
events. So the block needs reserved words (say 60..63) with lenses, which the application reads and never
writes, and the turn-end compare must treat them as raster-owned. `Time` advancing is not a turn.

### 5.6 Spaces

`Space { origin, clip }` per column does not fit the canvas: 20 frames x (1 + 4 groups + 1 top layer) = 120
clips, each following the pan. Per column that is 120 x (archetypes) columns of a few records each (a 64-record
page minimum per column: 5,000 nodes in roughly a thousand pages) and as many descriptor calls per frame. A
space should be an index: a `space: u16` per page (rows of one clip group are contiguous), into a table of
`{ origin expression value, clip }` beside the block. Until that exists the raster treats origin and clip as
paint operands (`Clip<Offset<P, ..>, ..>`), which is the same thing spelled per record.

### 5.7 Overlap and paint order (the lead's question)

* **A composed paint removes most interleaving before any key exists** (5.2): a node is one record.
* **The order key is arithmetic, not a lane.** With (store, row, position) order and a static shape,
  `z(col, i) = store.z0 + row(i) * shape_len + col.pos`: three constants per column and the record index.
  A store whose rows hold child stores of varying size needs one `u32` per row (a prefix sum), nothing per
  record.
* **Per-tile order without sorting.** The raster's first pass per column (one descriptor call) evaluates
  `Paint::window` into a window lane it owns (`i16 x 4`, device pixels, after transform and clip; this is also
  where `Frame`-stage geometry is evaluated, once per record). Binning then walks a layered store row-major
  over those lanes: non-generic code over plain data, appending to tile lists that are therefore in order.
  No merge, no sort, no dispatch. A tile's list holds (column, index) words; the tile kernel runs maximal runs
  of one column through that column's kernel: one indirect call per (tile, run), not per record. (So the
  budget "one indirect call per column per frame" holds for the window pass and for flow stores; for layered
  stores it is per tile run. On the canvas that is about 70 K calls for a full 4,000-tile frame against
  roughly 2 G instructions of shading.)
* **Where the compiler proves a store needs none of this:** a store is `Flow` when its rows come from flow
  layout *and* every column's paint has `OUTSET == 0` *and* no column has `GEOM_READS != 0` (nothing spills
  out of or moves off its row). All three are constants of types. Then rows cannot overlap, column-by-column
  painting in position order is exact, tile lists are per column with no key, and a tile's runs are as long
  as they can be. A single-column store is ordered by construction whatever its paints do (row order is z
  order). Everything else is `Layered` and gets row-major binning. A list row with a drop shadow as a separate
  column is `Layered`; as one composed paint it is single-column and free.
* The occlusion cut generalises unchanged: a tile's list starts at the last entry whose opaque core covers the
  tile (`OPAQUE_CORE` is a constant; no alpha is inspected at bin time).

### 5.8 Answers to the four questions

1. **`i32` rect lanes are fine.** No inner loop reads them: the kernel reads a record's rect once per (tile,
   record), and binning scans the raster's own 8-byte window lane, not the page. `i16` with a page origin
   would save bytes only in a loop that does not exist. Keep device pixels, half-open; sub-pixel positions are
   a paint operand (5.3).
2. **`Num`: yes**, digits formatted while shading are exact (integer division by a power of ten per digit
   cell, an atlas of ten glyphs) and make a number write a word write on every executor. I build it after
   the canvas and animation scenes. **`Label`: no**: shaped text is a column of glyph instances (position,
   atlas texel, with the colour an expression), produced at `Turn` stage; that is a third column, not a third
   lane, and needs nothing new in the seam.
3. **A type** (5.7): `Flow` / `Layered` on the list call, with `Flow` checked against the columns' constants at
   compile time. A run-time flag would put the ordered path's code and tests in every list.
4. **On unified memory nothing is uploaded**: the column allocation is the buffer
   (`newBufferWithBytesNoCopy`), which requires it to be VM-page aligned (16 KiB), a whole number of VM pages
   long, and never moved while a frame is in flight. So: reserve address space per column up front and commit
   on demand (the mapped scene image wants the same). The page bitmask is still needed, for two other things:
   discrete GPUs (a 64-record page is a good upload unit), and **the race**: a turn that writes a page while
   the GPU is still shading the previous frame from it tears that frame. Either turns wait for the GPU, or a
   dirty page is copied on its first write in an epoch (the bitmask is exactly that set). I assume
   copy-on-write by page unless the threads lane has a better rule.
