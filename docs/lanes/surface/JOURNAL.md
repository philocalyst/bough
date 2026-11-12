# Surface lane, round 8: journal

Branch `r8/surface` (worktree `wt/r8-surface`), from `r8/expr` `9469bf18f1`, fast-forwarded to `0e0ec02e9e` (the
expr lane's green gates). Nothing is pushed. Brief: `bough/runtime/lead/ROUND8.md` (Amendments 1 and 2), the rules
of `ROUND7.md`. Newest entries last. Every experiment has its prediction and kill criterion written before it runs.

Provenance unless a line says otherwise: the shared M3 Pro (aarch64, macOS 15.6), `nice -n 15`,
`CARGO_BUILD_JOBS=2`, one cargo, toolchain nightly-2026-10-17. No window is opened; everything is headless.

The lane owns three surfaces, each to be a projection of the one `view` and the one store tree, not a mechanism
beside them:

1. **Events**: the dataflow run backwards. A pick names a record; the record's column names its handlers;
   a handler is a monomorphised function taking `&mut V`. Authors write the façade's `.on_click(..)`.
2. **Accessibility**: a pull-only projection of the stores (and of rows described on demand), with deltas by
   comparison, settled geometry in motion, reduced motion as a block word.
3. **The inspector** (first-class, by the user's direction): picking, the element tree with source locations,
   live readout and editing of operands and theme words, state readout, per-element cost, on queries over the
   archetype columns.

## 0. What was read, and what it means for this lane

* `bough-x` at `0e0ec02e9e`: a turn is a compare of the block; damage is a finite difference over exposed pages;
  a store is non-generic, its columns created per site at discovery from a `&'static Desc`; rows are values; a
  list is a window on a relation (`Rel`, an 8-entry journal). Events today: `Hover<A, F>` (one kind), a
  per-column `Option<Hov>` (trampoline + hit kernel), `World::pointer` walks the store tree with the CPU hit
  kernel (`Store::hit`), the handler reads its operand by tuple path (`a.0.0.2.0`). No focus, keys, text,
  drag or wheel; no ids; no semantics; no reflection.
* `bough-paint` (`r8/raster`): `Hit<P, I>` marks a paint as answering the pick with an id expression the author
  chooses (`ROW_ID`, `DRAG_ID` constants in the canvas); the pick returns the top *inked* record and gives
  `NO_HIT` when that record is not `Hit` (so a label over a button blocks it). The per-record order word is
  `column << 20 | index`. Raster owns block words 60..63 (`PtrY`, `PtrX`, `Pick`, `Time`).
* The façade's input (`crates/bough/src/input.rs`): `Interact` gives `on_click(Fn(&mut V))`, `on_press`,
  `on_hover(Fn(&mut V, &Ev))`, `hover_bg`, `focusable`, `on_key`, `on_drag`, `scrolls(sig, line, max)`; a
  route is a trampoline plus at most 16 bytes of closure captures; the dispatch chain is the hit record's
  ancestors flagged interactive; Tab is preorder. Amendment 2 keeps exactly this surface.
* The façade's `reflect.rs`: compile-time reflection of any state struct with `#![feature(type_info)]`
  (field names, offsets, `Debug` and an `Edit` from text), no derive. Usable for state readout as it is.
* Round 6 accessibility: `bough/runtime/lead/prompts/a11y.md` (the card), and the clean-room sparse AccessKit
  work (`wt/a11y/reactivity/crates/gpui/SPARSE_ACCESSIBILITY.md`: a 100K-node full projection 22.7 ms, a leaf
  delta 69 µs; AccessKit holds a complete tree, so "unmaterialised" has to be decided here).
* The reflection design session (`wt/composition/docs/lanes/composition/REFLECTION-SESSION.md`, sketches
  01-03, read only): identity must survive re-description and reload (sketch 01); provenance per property
  (02); behaviour is overridden in code at declared points, an action is a type (03, user decision
  2026-11-11).
* gpui-ce's inspector (`~/projects/gpui-ce`, read only): see section 1.

## 1. The inspector: gap table and plan (sent to the lead before any code, 2026-11-12)

What the fork has (survey of `~/projects/gpui-ce`, read only). The fork carries the inspector's *core* and no UI:
Zed's `inspector_ui` was removed with the non-Apache crates (`478d1652f6`), and nothing in the fork calls
`toggle_inspector` or `set_inspector_renderer`. The core is: an id per element (`GlobalElementId` stack +
`#[track_caller]` `Location` + a per-frame instance counter, built every frame in debug builds); pick mode
(hover makes active, click selects and ends picking, the wheel walks the hit stack 36 px per layer, a 30 % blue
fill over the hitbox while picking, view caching off while picking); one state type, `DivInspectorState
{ base_style, bounds, content_size }`, whose `base_style` replaces the element's builder style while that
element stays selected; `#[derive_inspector_reflection]` on `Styled` (every `fn(self) -> Self` style method by
name, with docs, invocable: Zed's completion of style methods); a renderer registry per state type and a panel
docked right at 30 rem; besides: `.debug()` outlines, an accessibility JSON dump with per-node provenance
(element id, view type, location), a frame-time overlay. Only `Div`, `Img`, `Svg`, `Stateful` have locations:
text, lists (`uniform_list` returns `None`), canvas and editable text cannot be picked. No layout, paint, timing,
focus, key-context or entity readout. Bevy (`bevy-inspector-egui`, the remote protocol `bevy_remote`,
`bevy_dev_tools`) for comparison: a world inspector of entities by hierarchy, every component editable through
`Reflect`, resources, filter-by-query inspectors, click-to-select through picking, JSON-RPC `query` / `get` /
`insert` / `mutate_component` / `*_resource` / `registry/schema` for agents, UI outlines, diagnostics per
system (not per entity). Bevy's `+watch` methods, observers and hooks are excluded here (Amendment 2).

| capability | fork | Bevy | new core today | query-based plan |
|---|---|---|---|---|
| pick an element under the pointer | div/img/svg only; full redraw while picking | picking backends, observer events | `Store::hit` (handler columns only) | any record: rect lanes are untyped, so one cold walk picks every record of every archetype, text and list rows included; the pick id is the raster's |
| pick depth | wheel over the hit stack | no | no | the paint-order stack under the point, same walk |
| highlight | blue fill while picking only | gizmos/outlines | no | the inspector's own overlay view: four words of its own block per highlight, none in the app's; selected stays highlighted; *all* matches of a query highlighted at once |
| identity, source location | element-id path + `Location` + instance counter, rebuilt per frame | `Entity` + `Name` | (store, row, position) by arithmetic, no ids | the same address; `Location` per *site* recorded at discovery (cold) from `#[track_caller]` on the façade's constructors: no per-record bytes, nothing per frame |
| element tree | none (no dump) | `ChildOf` hierarchy | store tree; row types | store tree + row shape + list counts; unmaterialised rows described on demand from the row function (pure), searchable without scrolling |
| style readout | builder `StyleRefinement` only (not hover/active, not computed) | reflected components | nothing | each operand: name, expression code (`map(sheet::fill)(Theme, Hover, const 3)`), stage, words read, value now, record words; rect (layout answer), space, clip, scroll |
| layout readout | bounds, content size | `ComputedNode` | rect lanes | rect lanes, list extent (n x row height), the layout lane's per-row answer when it exists |
| edit a style property live | any `StyleRefinement` field of one element, only while selected | any reflected field | no | record operands (`Rec`) written in the column and re-applied by the inspector after each settle (a pin, printed as a source edit); literals compiled into types or Rust functions are **not** editable (gap) |
| edit theme values live | no (per element only) | resources | words are plain data | a theme word or palette entry is one word: write it, every dependent repaints by the finite difference, no per-record CPU work beyond the exposed delta |
| who depends on a value | no | no | `READS` masks | query: columns whose reads include the word, live counts plus unexposed counts; a dry-run delta reports records evaluated and tiles damaged before the edit |
| state readout and edit | no | reflected resources/components | `View::block` only | block words by name; the app's state struct through compile-time reflection (`reflect.rs`, `type_info`, no derive) with `Edit` from text |
| handlers / behaviour | no (key context exists, unconnected) | observers listed | one hover handler per column | each column's handler kinds, closure type, declared actions (sketch 03); focus chain and Tab order as queries |
| per-element cost | frame-time overlay only | per system | measured by hand | per column on demand: delta and paint kernels run on that column's pages under the instruction counter, divided per record; damage of the last turn by column |
| reflected style methods | `styled_reflection`: names, docs, invoke | `TypeRegistry` | none | not a method list: the element's style is a struct, reflected by `type_info` (fields, types, `Edit`); completion from that (gap until the element layer exists) |
| custom per-type panels | renderer registry by `TypeId` | per-type UI via `Reflect` | none | a plugin implements a trait for a component type (static); generic rendering from code otherwise |
| accessibility debug | JSON dump with provenance | AccessKit node per entity | none | the accessibility projection is the same queries; its dump carries site, location and archetype |
| agents / remote | none | JSON-RPC | none | the query algebra serialised (later; same model) |
| cost when unused | release ~0; debug builds pay per element per frame | plugin not added: 0 | 0 | 0 by construction: the inspector is a separate crate that reads the world it is handed; measured (instructions of the sheet cells, `__text`, RSS with and without it linked) |

What a query-based inspector does that the fork cannot: select by predicate across the whole app and
highlight every match (all readers of `Theme`, every click handler, every text containing a string); say who
depends on a value and what changing it will cost before changing it; edit a shared value instead of one
element's copy; show where every operand comes from as code; inspect and search rows that were never
materialised; be the same mechanism as the tests and the accessibility tree, so what the inspector shows is
asserted headless; snapshot, diff and restore the entire visual state (64 words) for A/B of themes.

What the new core cannot express yet: source locations (needs the façade element layer over `bough-x` with
`#[track_caller]`); live edits of literal values (constants in types and in Rust function bodies; pins cover
record operands only); the inspector drawn in-window (needs text and layout in `bough-x`); layout readout
beyond rects (layout lane); style completion (needs the element layer's style struct).

Plan, in order, each headless with tests before anything platform-side:
1. Queries as the one walk (with the expr lane's design note): typed iteration over columns by component, the
   entity address with every result, component tables in descriptors.
2. The inspector model on queries: pick (any record, depth), readout, theme words with dependents and dry
   run, pins, state through `type_info`, tree dump, damage and cost by column. Tests on the sheet; zero-cost
   measurement.
3. Events on the façade's surface (`.on_click(V::method)`, closures over `&mut V`), pick dispatch underneath.
4. Accessibility on the same queries.

**Correction (same day): the inspector UI exists.** It is gpui-component's `crates/component/src/inspector.rs`
(689 lines; the same in `gpui-component-release`), on gpui-ce's core; Zed's original `inspector_ui` is in
gpui-ce's history at `478d1652f6^`; `gpui-ce-0.3` adds a bench of the closed inspector's cost (upstream
#64309). What it adds to the core: a toggle action and key binding (cmd-alt-i / ctrl-shift-i); a panel with a
pick-mode button and close; the element's source location as a `file://` link with copy, its global id (Zed:
instance number, open in the workspace); Origin / Size / Content Size; a **Rust style editor**: the element's
style printed as builder calls (`div().p_1().mx_2()`, found by matching the reflected `Styled` methods as
subsets of the style), edited live, unknown methods as inline diagnostics, `.` completes method names with their
docs, the part no method expresses kept aside; a **JSON style editor** of the whole `StyleRefinement`, edited
live, parse errors shown; Reset to the style at pick time. Edits last while the element stays selected (its
README lists persistent and multi-element edits, a highlight after picking, a hierarchy view and pausing the UI
as future work). Consequence for the plan: the model gets a code view of a picked element (its operands printed
back as the author's calls, from the expression code), edits through it (operand pins, printable as source
edits, and theme words) with a diagnostic when a call cannot be applied live, and completion from the element
layer's style struct when it exists. The key binding, the panel, links and copy belong to the in-window panel,
which waits for text and layout on the new core.

Provisional answers from the lead (2026-11-12), each kept reversible: the in-window panel written in bough is the
product but waits for text and layout, so the model and a query protocol come first with a textual viewer as the
first client; dev-only pins that print as source edits are accepted; pointer transparency as in S1; screen
readers get the exposed window plus a page each side plus counts, other rows described on demand, scrolling only
on an explicit action. Events wait for raster and expr to answer S1-S3.

## 2. Amendment 3: reflection in the core first (2026-11-12)

The user narrowed the inspector to a thin proof and made reflection in the core the first deliverable: "if more
reflection was moved into the core it would become trivial". No style editors; source locations welcome. The
plan of section 1 is superseded where it differs: no code view, no edit-through-code, no pins for now.

### 2.1 What was built (`bough-x/src/reflect.rs`, commit `ecb4185a8e`; merged with r8/expr `8af3f228d0` at `feb3214293`)

* **Reflection is a sink.** `Learn` is driven once over the same `view` (the counters the writing sink keeps)
  and, at each record, is handed the record's type. It keeps per column what only the type knows: the paint
  kind, the record type's name, stage, words read, whether it moves, whether it hears the pointer, the operands
  (`Arch::OPS`: name, expression code, the expression *type* as the compiler writes it, stage, words read,
  record word range), and three monomorphised readers (value of an operand now, the core's own derivative,
  the offset). Per site it keeps where in the view the row, scope or list was described: `Sink::row`,
  `scope`, `list` are `#[track_caller]`, the other sinks never read the location.
* **The walk.** `Reflect::items` / `each`: every live record in paint order as an `Item`: entity (store path,
  row, position in the row's shape), place (column, record: what a pick id names), rect (layout answer),
  screen rect, clip, operands with values now, location, record words. `stack(x, y)`: every record under a
  point, top first, handler or not. `find(ent)`. `lists()`.
* **Rows on demand.** `Reflect::rows(view, block, list path, range)`: rows of a list described from the
  view's own row function, whether materialised or not, writing nothing and scrolling nothing. The test holds
  a stored row's description equal to its on-demand description.
* **Names come from the compiler.** An operand's `ty` is `core::any::type_name` of its expression type:
  `Map<sheet::fill, (Un<sheet::Theme>, Un<sheet::Hover>, Rec<u32>)>`. The functions and lenses it was built
  from are named with no table and no macro.

Tests (`crates/bough-x/tests/reflect.rs`, 6): the walk is exactly the live records; pick a cell and read its
entity, value, colour stage and type, location, record word, rect; locations are the view's own lines (header,
footer, the outline inside the body's scope, the list); 10,000-row list rows 5,000-5,001 described with nothing
materialised; an entity survives scrolling past the ring (row 75 lives at slot 11); the thin inspector: pick the
header, write `Theme`, every record whose colour changed was damaged and agrees with the core's derivative,
every record that does not read the word kept its values. Negative control: `mutant_reflect` (a list record's
ring slot taken for its row) fails `an_entity_survives_scrolling`. The pixel gates (`tests/sheet.rs`) pass.

### 2.2 Cost when unused (measured, prediction written before: identical cells, `__text` within 64 bytes)

`sheet_x`, 10,000 rows, `dist`, M3 Pro, load 13.8, two runs each, base `0e0ec02e9e` against `ecb4185a8e`:
every steady cell identical to the instruction (idle 55, hover 7,104, pointer between headers 8,315, theme
31,203, exposed edit 1,611, unexposed edit 1,083, select 773, frame in motion 631, at rest 6, scroll three rows
5,408, a page 30,062, re-describing idle 15,975, re-describing edit 16,035); mount (183 K to 208 K) and
insert/remove (85.1 K to 85.7 K) vary run to run in both binaries alike (page faults; the app's `Vec`). Binary:
the same 621 text symbols, the same `__const` and `__cstring` bytes, `__text` 128 bytes *smaller* (394,164
against 394,292: code layout, no function added or removed). Resident bytes: no field was added to any core
structure. Prediction held.

## 3. What the user's inspector (Loupe) needs from a core, and what the new core answers (2026-11-12)

Read for facts only (`~/projects/Private-Forks/gpui-ce-inspector`, `eaa93a2`, read only; no UI ported): the engine
side (`crates/gpui/src/inspector/`, hooks in `window/inspector_*.rs`, `element.rs`, `view.rs`, `div.rs`), the
four lenses and the analysis modules of `crates/gpui_inspector`, and `crates/gpui_lightbox`. Loupe records per
frame (`FrameRecord`: phase timings, causes with `#[track_caller]` notify sites, view spans, scene stats, an
element tree at the Tree level, `ElementDetails` at Full: box model and `LayoutFacts` for "why this size",
style facts, a11y role/label), per input event (`InputRecord`: kind, hit path from the tree, key contexts,
actions, handled, the frame it caused), per entity (observers, subscribers, notify counts and last site), and
on demand (key binding resolution with verdicts); it holds the app still by replaying the previous frame's
layers. Its "closed costs one Option check" is not literally true in four places (id-stack copy per located
element, entity type-name map, notify origin, refresh scope).

Answers in the new core (F: free, data that exists or a reader derives; N: a new fact):

| Loupe fact | new core |
|---|---|
| element tree, kind, identity | F: store nesting, row, position, `type_name` of the element; N: parent position within a row, a const of the façade element type (0 when unused) |
| source location | F per row/scope/list call (built); N per element: `#[track_caller]` constructors, the value carries the location to `Leaf::at` (no-op elsewhere; 0, same mechanism as measured) |
| bounds, visible, clipped, paint order, primitives | F: rect lanes, origin, clip, walk order, sites |
| interactivity flags | F: the column's handler kinds; scrollable: the space's word; stateful style: operand reads Pick/Focus/Press |
| colours, radius, border, opacity, text | F: operand values now |
| box model, `LayoutFacts`, why this size | N (layout lane): a reflection method on the element's layout type giving its spec and per-axis typestate; why-this-size is then static per axis from the type, not reconstructed |
| phase timings | F: the host times the separate world calls |
| causes of a frame | F: changed words (reader's previous block), relation journals, sweeps, the dispatch that ran; a write outside any handler has words, not a line (a write barrier is excluded) |
| views rendered / self time | F equivalents: rows a list pass wrote, columns differentiated, records changed, damage per column |
| hold, replay | F: a frame is a pure function of (block, columns) |
| input record, hit path, handled, frame caused | N: `Dispatch` returned by every entry point and the changed-word mask returned by settle (values in a register: 0 when dropped) |
| key tester | gap: no keymap in the new core yet |
| entities, observers, notifies | excluded concepts; equivalents F: readers of a word, how often it changes, the handler of the turn it changed |
| Lightbox: pixels, painted text and quads, clickables, appearance, time | F (raster; reflection; `Env`; `Time` is a word, sweeps give a motion's extent and end) |
| Lightbox lint (13 rules) | all but gradient/image contrast static over reflection, for every row on demand and every theme or hover state by evaluating under alternate blocks, with no render |

A "last turn" record is not needed: a turn's facts persist until the next turn (block, tiles, sweeps, journals,
the lists' last plans, the pick and focus words), or are derived by a reader holding its previous block, or
are return values. History is the reader's ring, kept only while it is attached.
