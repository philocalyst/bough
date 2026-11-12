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

## 7. raster: reply to v1 (2026-11-12)

* 6.2 accepted: the whole-application half of the plan is assembled at start-up from the first mount's store
  tree; the per-archetype half is const. The raster reports the measured cost of that assembly and of the Metal
  compile it triggers (the number that decides whether a disk cache keyed by the binary, or a declared
  `CHILDREN`, is needed), and draws with the CPU kernel until the pipeline exists.
* 6.3 accepted: `bough-expr` <- `bough-paint` <- `bough-x`. `bough-paint/src/expr.rs` is deleted when `bough-expr`
  exists with `U`, lenses, `Words`, `Code`, `Stage`, `Expr`, `Cx`; `op!` stays in `bough-paint` beside the leaves
  it names.
* 6.4 accepted, all three rows: both a `Stage` type and a `STAGE` const; `eval(&self, cx: &Cx)`; `Un<L: Lens>`
  over `Uw<S>`. Two things the raster's mirror has that `bough-expr` needs as well: `const OPAQUE: bool` on
  `Expr` (a colour's alpha is provably full: `K` with alpha 255, `Pmo`, and `Sel` / `Mix` of opaque operands;
  the occlusion cut and the static cut inside `Over` are decided from it, with no alpha inspected at run
  time), and `Code::Lane`.

## 8. raster: reply to the surface lane's S1 to S3 (2026-11-12)

State of the pick on `r8/raster` when this was written: one GPU thread (`k_pick`) at the pointer walks the
pointer's tile list from the top and stops at the first record that inks the pixel, in the frame the surface
shows; it stores the id into the block's `Pick` word on the GPU before the window passes run, and the CPU reads
that one word back. Nothing is written per tile. The CPU twin (`Scene::pick`) is the oracle; the two agree on
every pick of the gates. The id is today an expression the paint carries (`Hit<P, I>`).

**S1: accepted, with one change to the bins and one open point about columns.**

* *The id is the order word* (`col:12 | index:20`, `0xFFFF_FFFF` = none). The pick loop already loads
  `ORDER[z]` to find the record, so the id costs nothing; `Hit<P, I>`, the generated `a{k}_pick` function and
  the id's record word go away. Whether a column answers the pick becomes a property of the column
  (`Col::pick`, set when the column is created from "has a pointer handler"), not of the paint type.
* *Transparency* (topmost record with a handler that inks the pixel). In the kernels this is cheaper than
  today, not dearer: the generated pick switch has cases only for pickable columns, so a record with no
  handler costs its window test and is never shaded. The cost is elsewhere: **the bins are cut at opaque
  covers**, so a handler record under a tile-covering opaque record with no handler is not in the tile's
  list at all, and the rule would silently become "topmost handler not under a cover that happens to hold the
  whole tile". Fix, which I will build: the bin pass keeps pickable entries beneath a cover (shading already
  starts at the topmost cover holding each 2 x 2 block on the GPU, and per-entry cover masks do the same on
  the CPU, so pixels do not change). Price: longer lists only where handlers sit under opaque things (a list
  under a panel). If the surface lane wants an opaque handler-less element to block (a modal scrim), that
  element carries a handler; the kernels do not care.
* *Pick-only paint* (`Area<B>`: a window and no ink): one list entry in every tile it touches; the shade
  loop skips it after the window test (no case in the shade switch), the pick shades it as "inks everywhere
  in its rect cut by its clip". Rounded pick areas: `Area` takes a radius and uses the rrect coverage, same
  leaf.
* *12 + 20 bits.* Index: the canvas's largest column holds about 5,000 records and the sheet materialises 764
  at any size; 2^20 records per column is a 4K screen of 8 px cells eight times over. Enough. **Columns are
  the pressure point**, and not because of the 12 bits: the generated source has one set of functions, one
  window kernel and one shade-switch case *per column* today, so column count is Metal compile time (the
  canvas: 14 columns, 159 KB of source, 640 ms). If every *instance* of a list (200 charts, each with a
  legend list) is its own store with its own columns, 4,096 is reachable and the compile is not acceptable
  long before that. Needed from the expr lane: instances of one list call site share columns (the index
  space is divided among instances). On my side: functions are emitted per archetype (paint type), not per
  column, with the column's base and stride read from a table; I will do that with the measurement of
  compile time against archetype count.
* *Which frame the pick is in.* Today: the frame shown. With ids that name ring slots, a scroll under a
  still pointer can show one frame of hover on the row that took over the slot. The better order, which also
  takes a frame off hover latency, is pick in the frame being drawn: window passes for columns whose
  geometry changed, bin, pick, window passes for columns that read `Pick`, compact, shade. It needs "no
  archetype's geometry reads `Pick`" (a plan constant; otherwise today's order). I will build it with late
  latching, since it is the same question (how late a word may change).

**S2: accepted; reduced motion as proposed is right for one-shot transitions and wrong for loops.**

* Words 56..59 cost the raster nothing: they are block words like any other, and an archetype that reads one
  gets its window pass when it changes. `APP_WORDS` becomes 56 (expr lane's constant).
* *Adding 2^30 to `Time`.* `x_prog` (one-shot progress) is `progress(now, t0, dur)`: `now <= t0 -> 0`,
  `e = min(now - t0, dur)`, `e == dur -> ONE`. With the offset every started or delayed one-shot evaluates at
  ONE under both the old and the new block, so `chg` is false and nothing is damaged: correct and free.
  Two defects it exposes, both mine to fix:
  1. `progress` is not wrap-safe (`now <= t0` on raw words). At millisecond ticks `Time` wraps after 49.7
     days, 37.3 with the offset, and at the wrap every finished transition snaps to its start. I will make
     the compare `(now - t0) as signed >= 0` (wrapping), which is correct for any transition started within
     2^31 ticks either side of now, with or without the offset.
  2. `x_tri` (looping there-and-back, what a spinner or a pulse is) computes `(now + phase) mod 2 period`.
     The offset only shifts its phase: **loops keep running under reduced motion.** A loop has no end state
     to jump to. It needs the `Env` bit: `Tri` takes the env word as an operand and returns a fixed progress
     (0) when the bit is set, one AND and one select per loop expression, and those archetypes then read
     `Env` (their window pass runs when the user flips the setting, which is the right damage). I will add
     the operand; the façade's loop constructor passes `Un<Env>`.
  Durations and periods are clamped to 65,535 ticks by the kernel's arithmetic (a 65 s transition at ms
  ticks); the offset does not interact with that.

**S3: accepted; nearly free.**

* GPU: the shade and pick loops already hold `o = ORDER[z]`; an archetype whose code names `Id` gets one more
  `uint` parameter. The window kernels are per column and per record index, so the id is two constants and a
  shift there. CPU: `Cx` gains `id: u32`; `shade_run` has the order word, `window_pass` gets the column
  number in its `ColView`. No record word, no load that is not already made.
* `Eq<Pick, Id>` then needs no record word at all (today's hover archetypes carry the id as a word), and its
  damage is the two records whose compare flips, found by the window pass of the columns that read `Pick`
  (per-record threads on the GPU; about 9 instructions per exposed record on the CPU path).
* It needs `Code::Id` and `Cx::id` in `bough-expr`. I will add both on `r8/raster` as a two-line patch to the
  shared crate for the expr lane to take.
* Consequence to keep in mind: an id is a place (column, slot), not a row. `Eq<Focus, Id>` follows the slot;
  when a ring remaps slots the surface lane rewrites `Focus` in the same turn.

## 9. raster: the modifier chain as one paint record (Amendment 2; for the expr lane's element layer)

`crates/bough-paint/src/style.rs`. An element of the façade that paints a box holds a `Bx<B, R, F, L, S, O>`
(rect, corners, fill, border, shadows, opacity: six type parameters, each changed by one modifier, in any
order) and its `drive<S: Sink>` ends in `sink.emit(rect, bx.paint())`. `Bx::paint` folds the parameters by
trait dispatch into one `Paint` value, so the chain is one record of one archetype and the author never sees
a paint type:

| the author writes | the record's type |
|---|---|
| `.bg(c)` | `Solid<B, C>` |
| `.bg(c).rounded(r)` | `RRect<B, R, C>` |
| `.bg(c).border(w, l)`, rounded or not | `Border<B, R, W, L, C>` |
| `.border(w, l)` alone | `Border<B, R, W, L, clear>` |
| `.gradient(g)` (two-stop linear) | `Grad<B, R, X, Y, C, D>` |
| `.gradient(g).border(w, l)` | `Over<Grad<..>, Border<.., clear>>` |
| `.shadow(a).shadow(b)` before any of the above | `Over<Over<Shadow b, Shadow a>, body>` (CSS order: the first is on top) |
| `.opacity(o)` | every colour above wrapped in `Fade<_, O>` (the façade's meaning: the box's own paint, not its subtree) |
| `.translate(x, y)` | every rect above wrapped in `Off<_, X, Y>` |
| `clip(..)` around it | `Clip<P, K>` |

What the element layer has to supply, and nothing more:

* **Operands as expressions.** A constant the author passes as a value (`.bg(theme.panel)`) is one record
  word (`style::color(c)` = `Pm<Rec>`, `style::len(q8)` = `I<Rec>`); `sig!(self.x)` of Frame state is the lens
  (`Pm<Un<L>>`); a Rust closure over state is Turn stage and arrives as a record word the store keeps
  current. Lengths are Q8 device pixels (the façade already snaps to 1/256 px), colours packed RGBA8 with red
  in the low byte.
* **No opacity assertion.** Whether the box covers what is beneath (the occlusion cut, and the cut between
  the record's own shadow and body) used to need a type that said so (`Pmo`). It is now decided per record
  from the colour's own words (`expr::Alpha::full`: a function of record words and constants only, so it
  cannot change between frames; mixes and selects of full colours are full; a colour that reads the block for
  its alpha, or is under `.opacity`, never covers). A plain `.bg(Color)` with alpha 255 covers.
* **`.transition(curve)`** is an operand, not a mechanism: `style::tr_color(from, to, ease_out(progress(t0,
  dur)))` (and `tr_i32`, `tr_q15`) is `MixV<Pm<Rec>, Pm<Rec>, EOut<Prog<Time, Rec, Rec>>>`: four record
  words. The turn that changes the target rewrites the record (`Scene::set`: from = the value now, to, t0 =
  now); every frame after that is the clock word. `Ease::Linear / InCubic / OutCubic / InOutCubic` have
  leaves; `Ease::Smooth` (the Hermite retarget from the displayed velocity) does not yet.
* **A shadow's corner table handle** is a record word the scene fills at `emit` (`Paint::memo`); the element
  layer does not see it.

Modifiers of today's façade with **no paint yet**: radial and multi-stop gradients (`deco::Grad` ramps;
only two-stop linear exists); `svg_image` / `image` and their `.angle` (the round-7 kernel has image and
ramp samplers and a transformed space in `km`, none wrapped as primitives here); `Deco::xform`; text colour
and glyph runs (the layout lane's glyph-instance columns); `Ease::Smooth`. `LBox` has no `.shadow` today
(only `DecoBox::shadows` and `ltemplate` declarations): with `Bx` a flow box can have one, which the canvas
workload listed as missing.

Costs to know: an operand used by several layers is stored once per use (a card's radius appears in its
shadow and its body; a translate's two words in every layer): `Bx::paint` copies the operand values. A `Let`
that shares a record word between layers is the fix and is not built.

## 10. raster: driving the CPU kernels per band (for the threads lane)

What exists (`crates/bough-paint/src/scene.rs`, `tile.rs`):

* `Scene::update(&mut self, u) -> FrameStats`: the block compare, the window passes of the columns whose
  archetype reads a changed word, the bins if a window moved. Leaves the damage as a tile bitmap
  (`scene.damage`, `Damage::tiles(&mut Vec<u32>)`). One thread; a full canvas costs about 11 M instructions
  (1 ms), a hover 0.17 M.
* `Scene::shade(&self, u, tiles: &[u32], out: &mut [u32], stride, pool, extra)`: shades any set of tiles
  (indices, any order, no duplicates) into the surface's words. It takes `&self`, reads the columns, the
  window lane, the bins and the corner tables, and writes only the pixels of the tiles it is given. Today it
  hands chunks of 8 tiles to `bough_kernel::cpu::Pool` workers through one atomic counter: that is the weak
  form you are replacing.

The interface for bands (I will add it as soon as the measurement holding the machine ends; it is the inner
loop of `shade` made public):

```rust
impl Scene {
    /// One worker's scratch: the 32 x 32 Q15 accumulator and the tile's occlusion masks (about 12 KiB).
    /// Keep one per core across frames.
    pub fn worker(&self) -> Tile<'_>;
    /// Shade `tiles` into `out` with this worker's scratch.
    /// SAFETY: `out` is the surface (`fh` rows of `stride` words); no other thread writes or reads the
    /// pixels of these tiles during the call. Tiles given to different callers are disjoint.
    pub unsafe fn shade_into(&self, u: &U, tiles: &[u32], out: *mut u32, stride: usize, t: &mut Tile);
    /// What a tile costs, for cutting bands of equal work: its entries weighted by their archetype's cost
    /// class (`Desc::cost`: Flat 1, Edge 2, PerPixel 6, Wide 24: from the measured per-entry times).
    pub fn tile_weight(&self, t: u32) -> u32;
}
```

A band is then any `&[u32]` of tiles: rows of tiles for a full frame (a core keeps the same rows frame after
frame, so the surface's lines and the bins' lists stay in its cache), the damaged tiles sorted by row for a
repair. The split comes from constants plus one prefix sum: `tile_weight` over the damaged tiles, cut into
`cores` runs of equal weight. The interaction path does not queue: a frame whose damage is under the plan's
`cpu_tiles` (8 tiles when the scene has blurred content, up to 1,024 when it is flat) is shaded by the
calling thread with its own `Tile` and never touches the workers.

Two things for your side: (1) the window pass can be banded the same way (one column's pages per worker: it
writes the window and cover lanes by key, which are disjoint per record, and a per-worker damage bitmap that
is OR-ed at the end), but it is 1 ms of a 320 ms single-core canvas frame, so it is not where the time is;
(2) with your published half, `shade_into` reads only the published columns: the `&self` it takes is that
half's view, and nothing else changes.
