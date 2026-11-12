# Composition lane: handoff (2026-11-12 checkpoint)

## State

* Branch `lane/composition/round6`, head: the commit that adds this file (parent `2f085b1316`). Nothing of this
  lane is merged into `round6` yet. This branch has `round6` (at `c9492b826b`) and `lead/core-r6` (at
  `4f4c469ef5`) merged into it.
* Commits this session, in order:
  * `56f72e961f` DESIGN.md edits the stopped session left (accurate for the built code). Its untracked `sem.rs`
    sketch did not compile (it used APIs that did not exist) and stored per instance; it was **not** committed.
    It is kept as `runtime/lanes/composition/sem-wip.rs`.
  * `d5bfacc45c` merges `round6`, plus surface-api's `signal_cost` fix.
  * `85a96a1754` freezes the card `cards/2026-11-11-reflection-native-views.md`.
  * `8632b5ed36` A1: reflection as a walk of the description.
    * `Element::probe`, `probe.rs`, `tree.rs`, `find.rs`.
    * Deletes `Rt::reflect` (per-mount type names, the part HashMap, the owner registry) and `foreign::Slot::at`.
  * `6ea68e61ae` merges `lead/core-r6`. Reflection writes go through `signal::queue`, as the lead asked.
  * `2f085b1316` A2: the devtools socket server, the accessibility projection with deltas, and the `bound(sig!)` selector.
* Uncommitted: `REFLECTION-SESSION.md` and `sketches/` in this directory. They belong to the user's design
  session, so I did not touch them and they keep `git status` from being clean.
* Local branch `tmp/composition-base2` (`3569e18fff`) is HEAD minus A1. It exists only as the A/B base for
  `runtime/lanes/composition/ab_dist.sh`. Never merge it; delete it when done.
* Part B (interleaved native views) was not started this session. What exists is from the previous session:
  * foreign records in three modes: Under (a hole), Over, Sampled;
  * `LayerHost`;
  * the hole kernel in Metal.

## What exists now (for a reader new to the lane)

* **Reflection is a walk, not stored data.**
  * `App::tree()` runs `View::view` again, and `Element::probe` shows each element to a `Probe` instead of a mounter.
  * A preorder cursor plus the depth column joins each element to the record it emitted. Each join is checked
    against the record's kind family and depth; on a mismatch the walk leaves that container unattributed
    (`Tree::lost`) rather than guess.
  * Static data only: `type_name` of builders and of route closures, part keys, and `type_info` schemas of owners.
    The schemas give signal paths and values by offset (`reflect::signal_path`).
  * Every builder in the crate implements `probe`: boxes, text, quads, routes, parts, `With`, `embed!`, list rows
    (`list()`), entity views, scroll boxes, anchors, offsets, tooltips, fields, inputs, images, foreign elements,
    readers.
  * The default `probe` is "unknown": it leaves the rest of the container unattributed.
* **Testing API (`find.rs`).**
  * Selectors, typed or as strings: role, name or substring, `#id`, `@part[index]`, owner type, builder name,
    `:state`, `:nth`, descendant/child steps, and `bound(sig!(page: Page => count))`.
  * `get` / `find` / `exists`.
  * Actions go through the real entry points: `click`, `hover`, `type_text`, `press_key`, `scroll`. They check
    actionability: an element that is hidden or covered at its centre is refused.
  * Every action returns the `Turn`s it took, and `App::apply(&Turn)` replays one.
* **Tree (`tree.rs`).** One node per record: role, name, text, value, layout box, visible rect, fill, text style,
  states, routes (with closure names), bindings (path and value), element type, source, part, id, owner path,
  foreign id.
* **Devtools (`devtools.rs`).**
  * The protocol is JSON lines over a unix socket; the op table is in the module docs.
  * `serve()` runs a headless app; `BOUGH_DEVTOOLS=/path` at `Application::run` serves a windowed app through the
    main dispatch queue.
  * Supports record, session and replay, with a tree diff and a pixel diff against a fresh app.
  * `screenshot` writes a PNG of the retained frame and needs no window.
* **Accessibility (`a11y.rs`).**
  * `Tree::semantic()`: roles, names folded from content, values, states, actions.
  * Offsets are relative to the parent, plus a scroll offset, so a move or a scroll changes one node.
  * `delta` / `apply`, and `a11y_act` through the routes.
  * With feature `a11y`, an AccessKit `TreeUpdate` (accesskit 0.25). There is no platform adapter yet.

## Numbers

All are instruction counts (`tinstr` slopes; deterministic per binary, not affected by machine load).

**A1 vs the same tree without A1, profile `dist`** (`runtime/lanes/composition/ab_dist.sh tmp/composition-base2 a1m`):

| cell | before | after | |
|---|---|---|---|
| signal_cost 256 redescribe colour/number/string/idle | 154,927 / 163,069 / 174,670 / 154,215 | 154,541 / 162,428 / 174,318 / 154,187 | -0.2 % to 0 |
| signal_cost once cells (16/64/256) | 556, 2,680, 524, 8,510, 513, 199 | identical | 0 |
| redescribe_cost 1000 rows owned/static/bare per row | 623 / 626 / 216 | 623 / 626 / 218 | 0, 0, +0.8 % |
| redescribe_cost CHURN 1000 per pair | 15,848 | 15,854 | +0.04 % |
| redescribe_cost 100 rows owned (median of 5) | 66,790 | 65,715 | -1.6 % (run spread ±3 % in both) |
| signal_cost binary (dist) | 3,055,840 B | 3,055,744 B | -96 B |
| redescribe_cost binary (dist) | 3,089,328 B | 3,072,720 B | -16.2 KiB |

* The pre-core-r6 pair (`ab_dist.sh d5bfacc45c a1`) showed the same picture: all cells within 0.35 %.
* The release-profile pair moved by up to ±4 % on cells A1 does not touch, and by -24 % on one (`bare`):
  codegen-unit partition noise (see EXPERIMENTS.md). The gate is read at `dist`.
* Runs: three runs of every cell at release, and one run per binary at dist (cells are deterministic). The
  100-row cell is five runs per binary.

**Tests run** (`cargo test --release -p bough ...`):

| tests | result | negative control |
|---|---|---|
| `--test probe` | 15 pass | with `--features mutant_probe`, 10 of 15 fail |
| `--features a11y --test a11y` | 6 pass | data mutants of a delta fail: dropped removals, dropped renamed node, dropped scrolled area |
| `--test devtools` | 2 pass | a replay missing its `set` turn differs in tree and pixels |
| `--test parts --test foreign --test owners --test reflect --test describe` | 25 pass | |

**Accessibility delta sizes** (19-node app, changed + removed):

| change | delta |
|---|---|
| label by signal | 1 |
| show (re-description) | 3 |
| hide | 2+1 |
| insert | 5 |
| remove | 5+2 |
| focus move | 2 |
| type | 1 |
| scroll | 5 (the area plus entries whose visibility flipped) |

## Not verified / known broken

* **Workspace check.** `cargo check --workspace --all-targets` passes on `2f085b1316` (warnings only, all
  pre-existing). `cargo check -p bough --features a11y` passes too.
* **Full suites not run.** The bough suite, bough-core and bough-metal were not run after the merges, nor were
  the GPU suites outside the five binaries listed.
* **Experiments X1–X3.**
  * X1 (reflection reachable but idle) was not measured.
  * X2 (the cost of `tree()`) was not measured. The prediction is 0.8–1.5 M instructions at 256 rows, so an
    inspector running every frame would want a cache keyed by the epoch.
  * X3 (occlusion damage) was not built.
* **Probe coverage.** Elements that use the "unknown" default lose attribution for the rest of their container:
  * templates, `LTemplate`, `VirtualList`, `PortList`, `DecoBox`;
  * rows of `list_with` and `index_list` (only `list()` rows are walked).

  Their records are still in the tree, with implied roles.
* **Probe and runtime state.** The probe re-runs `view()` against current state. Anything a description reads
  from outside the owner (clocks, globals) can put the walk out of step; this is detected, not misattributed.
  A `Config::once()` app whose plain fields changed behaves the same way.
* **Name collision in selectors.** The word `input` matches both the anchor box (builder name) and the inner
  field (role TextInput). Tests use `textinput`. The fix: give builder names their own sigil, or attribute the
  role to the anchor.
* **Windowed devtools path.** `serve_windowed` compiles but was never run against a real window. No windows
  were opened this session.
* **Merge conflicts to expect.**
  * The store lane rewrites `Node`, `LInfo` and `Rt::reflect` as columns. Its `Reflect::types: Col` conflicts
    with the deletion here; resolve by keeping the deletion.
  * `tree.rs` `nodes_by_handle` becomes the identity on store's one index space.
* **Clash with surface-api.** `Route.base` (from `embed!`) and surface-api's `OwnerRef` change the same struct.
  `base` should become part of `OwnerRef`.

## Experiments

See `EXPERIMENTS.md`. X1 is half measured (the unreachable comparison), X2 and X3 are not run, and there is a
release-vs-dist control. A1 itself is the experiment "delete the reflection columns": the gate was at most 1 %,
the result is within 0.35 % at dist, and the binary shrank by up to 16 KiB.

## Underexplored and exotic avenues

1. **The walk's zero-cost proof.**
   * Mechanism: a generic method on `Element` that only reflection calls is monomorphised only where reflection
     is reachable.
   * What is still missing:
     * the reachable-but-idle build (X1);
     * the assembly of `LBox::update` and `Para::update` compared before and after;
     * the same check with `Location` fields added to every builder, so the inspector gets sources for all
       elements, not only parts, `Describe` and foreign elements.
   * Estimate: 0 instructions per turn and 2–8 bytes per builder value, eliminated by SROA where inlined.
   * Cheapest kill: `signal_cost` at dist with `#[track_caller]` `Location` on `div`/`row`/`text` (prediction:
     +0–0.5 %).
2. **Typed selectors.**
   * By bound signal: built.
   * By component type: `owner::<T>()` is built. `elem::<LBox<..>>` needs generics-erased type identity; a
     `const NAME` on builders would do it.
   * By role and text: built.
   * By geometry: `pick_record` exists. A selector `at(x, y)` or `near(sel, dir)` is a filter over `visible`;
     expect 20 lines.
   * By the source line that built the element (`Location`), once avenue 1 lands. This gives the inspector
     "jump to source" and lets an agent say "the button built at view.rs:42".
3. **Turn logs as the record format.**
   * The lead's write log (`4f4c469ef5`) records cell writes. A turn log of input plus writes plus `Tick` is
     already deterministic, since the frame clock only moves by `tick`.
   * Next: store sessions as files, replay a log from one build against a later build (hot reload as replay),
     and bisect a pixel diff to the first diverging turn.
   * Estimate: replay costs one turn per recorded turn. Bisection over 1,000 turns is 10 replays.
4. **Hot reload by state transplant.**
   * `reflect_owner_fields` plus `set_in` already read and write every editable leaf by path.
   * Mechanism: dump `(path, value)` before an exec, rebuild, restore the paths that still exist and still parse.
   * Cost: dynamic fields (`Vec` length changes) need `Items` growth, which is not built.
   * Experiment: transplant `Counter` state across two builds over the socket (`fields`, then `set` per path).
5. **The inspector.**
   * What is still missing: a pick mode that swallows input (one flag checked in `pointer_*` only while
     inspecting), a highlight record at the end of the paint order (as the focus ring), and style pins
     (DESIGN §3.3 gap 5).
   * The gpui-ce fork's panel needs, per selected element, a stable id, a source location, bounds and a style
     refinement applied before layout. All except the pin exist.
   * The fork has no element tree view and no state view; bough has both.
   * Do not copy code out of the fork; its survey summary is in this session's transcript.
6. **Incremental accessibility.**
   * Now: a delta is a diff of two projections, O(nodes).
   * The a11y card wants deltas from the turn's written cells: map the write log's heads to bound nodes (the
     walk knows bindings by offset), plus splice runs for structure, plus focus.
   * Estimate: a label change costs O(1) instead of O(n).
   * Depends on the write log (lead) and state planes (core).
   * A macOS adapter needs an `NSView` that is the accessibility root (`accesskit_macos` 0.27 is in the
     registry).
7. **Native views, z-order and translucency (B).**
   * The Under hole is exact for opaque content.
   * For translucent content, split the paint order at each foreign element into planes (records before F,
     records after F that overlap F transitively). Each plane is a surface layer; the compositor stacks
     S0, F, S1.
   * Cost: the kernel needs a position window per staged record, plus a second target.
   * Experiment: render S1 for one foreign rect with a `pos_lo` header word and compare a CARenderer composite
     against Sampled mode.
8. **Lockstep scrolling.** Commit foreign placements in the same CA transaction as the surface swap. On frames
   where a foreign element moved, take the PACED path: commit on the calling thread after the GPU wait.
   Prediction: one frame of lag becomes zero; extra main-thread wait is 1–2 ms only on those frames.
9. **Occlusion damage (X3).** Generalised: any opaque record covering a whole tile makes earlier records in that
   tile irrelevant, both to the GPU (start the tile loop at the last opaque cover) and to damage. Belongs to the
   gpu and core lanes; it covers video, web views, modals and opaque cards.
10. **Input and focus handoff.**
    * bough's layer-hosting view returns nil from `hitTest:` where `foreign_at` answers Capture, so AppKit
      routes to the view below.
    * `ForeignHost::focus(id)` when bough's tab order reaches a foreign element; `App::foreign_blurred(id)` when
      the native view resigns.
    * Needs an `NSView` subclass for bough's view (bough-metal `app.rs`).
11. **Exposing the real platform handle.** A `Native<T>` field type owning a `Retained<NSView>`, `CALayer` or
    `AVPlayerLayer`. It derefs to `T`, and `native(&self.web)` is the element; its slot keeps the retained
    handle, so teardown is the slot's drop.
    * Linux: Wayland subsurfaces take the place of sibling layers. `wl_subsurface` `place_above`/`place_below`
      is the same plane model, and `set_position` plus a viewporter crop is the clip.
    * Windows: DirectComposition visuals.
    * Each host implements `ForeignHost`. The element API and the hole stay; the platform object stays
      reachable by deref.
12. **Parts folded into the walk.**
    * Part reflection is now static (key in the description, index re-derived). Guards still live in
      `Input::guards`, a side list.
    * Fold: a guard is a route on the act, so store it in the act's route slot as a chain (before / route /
      after) built at mount. Dispatch then runs a chain, `guards.is_empty()` disappears from every click, and a
      remounted item's stale guards go with its act.
13. **Owner-aware routes.** Lists, readers and text fields inside `embed!` are refused (base asserts). Giving
    each embedded view an owner class with a `class_base` removes the refusal.
14. **The selector language as a query algebra for agents.** Batch queries (`find` many), and a structural diff
    between two turns (`tree` at turn k and k+1, keyed by handle). The a11y delta is that diff already.
15. **Stable identity across reloads.** A handle is per run. Across builds, the stable address is (owner type
    path, field offsets, list key, part key and index, `Location`). The walk can emit that path; a replay or an
    inspector selection can survive a rebuild that does not touch the path.

## Where the lead or the plan is wrong

* **Gates at `release` vs `dist`.**
  * Gates read at `release` (thin LTO, 16 codegen units) report ±1–4 % movements on unrelated edits; one cell
    moved -24 %.
  * The 1 % threshold should be read at `dist`, where the same pair is within 0.35 %.
  * The 100-row `redescribe_cost` cell needs medians of 5 even then.
* **Mounting typestate.** "Reflection data is static, const tables derived from type info" is right for owners
  and closures but not for parts' indices, labels and sources: those are values. They are re-derived by
  re-running the description, not stored. A typestate or const-generic mounter (my first plan) would have
  doubled every `update` instantiation for no gain over a separate `probe` method.
* **Lane scope.** B (native views) and the a11y adapter each deserve a lane-sized card. Putting A, B, C and a11y
  in one lane left B unstarted in this session.

## Next

1. **X1 and X2.**
   * X1: a reachable-but-idle `signal_cost`, built at dist (prediction: turn cells within 0.3 %, `__TEXT` +40–90 KiB).
   * X2: `tree()` cost at 16/64/256 rows (prediction: 0.8–1.5 M at 256).
   * Then the workspace check, and merge into `round6`.
2. **B, lockstep and occlusion.**
   * Placements committed in the surface's transaction.
   * The X3 occlusion mask (prediction: 300 tiles to 0 per frame under an animating background, exact).
   * The headless CARenderer composite gate: [hole surface over a CALayer] vs Sampled, tolerance ±1 per channel.
3. **B, the native host.** `Native<T>` plus an `NSView` host with `hitTest:` handoff and focus handoff. Then an
   off-Space WKWebView example, using lane `iteration`'s `bench/tools/space` helper.
