# Surface API round 6 handoff

Stopped at the lead's request on 2026-11-11, before the first build or measurement. The production
implementation is unchanged. Research state: **CARD**, with an unbuilt harness/oracle candidate.
Nothing is GATED, MEASURED, independently accepted, or ready to integrate as a correctness fix.

## 1. What exists

Base: `aa9943e7ae50595f6af026773ee82b46a4b85ae0`, branch `lane/surface-api/round6`.

| Commit | Artifact | Status |
|---|---|---|
| `7052f930a3` | [Frozen propagation card](cards/2026-11-11-propagation.md) | Committed before any build |
| `539765177c` | [F8 harness](../../../crates/bough/examples/propagation.rs), `propagation_support/{dashboard,scroll}.rs`; card clarification | Not compiled or run |
| `4789413bb3` | [Scroll oracle](../../../crates/bough/tests/propagation_scroll.rs) | Formatted; not compiled or run |

The harness copies AWAGY dashboard/scroll descriptions, adds constructors and step functions, and adjusts
relative asset paths. Application descriptions use `bough::prelude`; harness infrastructure imports
`App`, `Config`, `Caps`, and the existing instruction counter. No root manifest, shared report, existing
gate, vendor, or other lane was edited. No production repair, route oracle, mutant execution, assembly
capture, or `DESIGN-R6.md` exists yet. This handoff preserves the design reasoning that would have gone there.

The scroll test describes ten plain rows below an outer scroll port and a nested scroll port. Its twelve
operations change paint/width, scroll both ports, insert/remove/swap rows, move/resize the port, truncate,
reset offset, clear, and repopulate. For each ring size 1..4 it compares presented pixels with a full
redraw, then retained pixels with a fresh mount of the same state. **Expected**, not observed: the
baseline should fail the first changed plain color. It does not yet test wheel glides, raw `scroll_y`,
window resizing, or actual scroll-by-copy eligibility.

## 2. Measurements and verification

**There are no measured performance numbers from this lane.** No Cargo build/check/test command ran:
the shared measurement lock was held by `core` on every check. Its continuation said it was waiting for
other builds to drain. This lane neither acquired nor removed that lock. The stop instruction arrived
before it became available. No baseline executable or executable hash exists.

| Preregistered cell | Runs/results |
|---|---|
| Counter | 0; not measured |
| AWAGY dashboard | 0; not measured |
| AWAGY scroll, 1,000 rows | 0; not measured |
| Plain column, 10 / 100 / 1,000 / 10,000 children | 0 in each cell; not measured |
| List, same four sizes, forced mounted | 0 in each cell; not measured |
| Deep tree, 32 nested columns | 0; not measured |
| Fixed template-shaped tree, 64 wired leaves | 0; not measured |
| STATUS and zgx/zgui mount-once regression floor | Not measured |
| Dispatch owner-chain overhead | Not measured |

Planned measurement is `proc_pid_rusage` `ri_instructions`, through `bough_metal::perf::snap`, subtracting
`snap_overhead`. Each cell constructs mount-once and ordinary Apps, warms 20 turns with frames, then
brackets 1,000 `App::turn` calls without presentation (`F8_REPS` overrides). Run the complete executable
three times. CSV reports repeats, instructions, instructions/turn, and retained record count. These
counts and sizes are **workload settings**, not measurements.

The bracket includes handler, settle, effects, re-description, and layout; it is not an isolated timer
around `Element::update`. The once/ordinary difference is useful but also includes their different
retained representations. For plain fields, the once arm intentionally leaves stale output and is only
a cost control. It cannot qualify as a faster correct implementation. The all-wires tree still has
constant *by-value layout operands*; it is not a proof that the present builder represents a fully static
template. List overscan forces mounting and spaces are raised from APP_CAPS' 4,096 to 16,384 for this
capacity-controlled experiment. Do not claim success at shipping capacities.

When resuming, use the pinned nightly, own target, cleared `RUSTC_WRAPPER`, four jobs, and the shared
measurement protocol. `BOUGH_FONT` must name a readable font. First build:

```sh
cargo build --release -p bough --example propagation
```

After acquiring the measurement lock, run `target/release/examples/propagation` three times, preserving
stdout, stderr, revision, toolchain/features, font identity and executable hash. Do not substitute another
lane's build. Keep the baseline binary before production edits. Preserve compile failures and partial
runs; the harness has not yet passed even a smoke check.

The only completed validation is source inspection, intended-file formatting of the new scroll test,
and `git diff --check`. Required workspace all-target checking and release tests remain **NOT RUN**.
Stopping now follows the lead's explicit handover instruction, not completion of the frozen card.

## 3. Mechanisms, deductions, and proposed direction

### What the current propagation path actually does

`App::turn`, `flush`, and input settlement settle root wires, run `changed` waves, then re-describe the
root unless the harness chose `Config::once()`. `describe::redescribe` runs `view()` and walks its result
against a tape, with `pos` for paint preorder and `kid` for layout children. Dynamic items own scopes,
tapes, and record/child counts. The root walk does not have dependency knowledge about ordinary Rust reads.

`LBox::update` calls `restyle` on each visited box before updating children. That path constructs/interns
layout styles; a stable tree is not merely reading one equality word per node. Hypothesis: this, description
construction, and dynamic-part handling will dominate a plain-column F8 slope more than tape loads.
Measure before designing SIMD around the tape.

`LList::update` advances `pos` by `1 + sum(row.nrec)` over all mounted rows, even for an unchanged List.
The List F8 cells isolate this predicted linear overhead. Each actual nested list structural edit already
propagates record-count changes through `lr_count` to enclosing rows. A cached aggregate belongs to that
same count invariant; it should not become a second independently maintained truth. A future implementation
must cover mount/unmount, virtualization, permutation, nested dynamic growth, and release.

### Scroll repair: retain the boundary, reconcile its content

`ScrollBox::mount` uses `mount_opaque`; `update` calls `update_opaque`. The latter advances the cursors
without visiting the child descriptions. `ScrollY` has the same explicit opacity. The recorded child tape
is therefore not enough: update must restore the content's mounting context and use it.

The bounded repair I would try first is a tape entry identifying the existing ScrollView, followed by
normal port/content tapes. Update the port in the parent flow, then update content with the retained
content space/clip, zero local origin, child depth, `mounting_view`, and its independent layout group.
Restore parent `lay.cur`, `kid`, origin/space/clip/depth and mounting-view context afterwards. Preserve the
existing offset, scroller, clip registration, layout placement hook, and view identity. Do not remount the
port or allocate a new space/clip on every description. Keep mount-once resource layout/cost unchanged
where possible; even an extra tape operation should compile away when no tape is retained.

Important details: `LBox::update` does not set `lay.last_box`; do not use that mount-only scratch field to
find an updated port. Obtain its retained lid from the tape or ScrollView's retained hook. New dynamic
children must inherit `mounting_view`, otherwise nested clips/tooltips will not follow scrolling. The
content has `lay.cur = NONE` at mount because it is a separate group; treating it as a regular layout child
silently changes ownership. Raw `ScrollY` also has explicit rect, follows-window, and inset-width behavior.
Do not blindly share the two paths without accounting for those differences.

Further **unverified suspicions**: dynamic removal may leave stale nested-view/clip/tip registrations,
because `Scope::cut_scope` cuts wires/layout/list/producers but has no visible general ScrollView removal
step. Port-width changes may need explicit content available-width invalidation; check the placement hook,
not just paint equality. These need focused lifecycle and resize witnesses before being called defects.
The current scroll fixture is a starting point, not coverage of those invariants.

### Owner routing is also an owner re-description problem

Removing `input::root_owner_only` alone is unsound: route trampolines currently receive the root pointer.
Changing that pointer alone is still incomplete: `mount_row` builds with a null tape, `lr_replay` settles
edited row signals but never re-describes plain row fields, and entity mounts also use null tapes. Same-key
entity updates skip an opaque item. This independently explains why a superficially working interactive
row could still leave its plain UI stale.

A class is a **schema of signal offsets**, not an owner identity. One List region shares its class across
many rows. Do not put a single receiver pointer or dirty flag in `classes[cls]`. Entities can also have
multiple displayed views, each with its own retained tree. One entity edit must refresh every mounted view
of that generation; a row route must settle the proper class and invalidate the right descriptions.

The structural direction I favor is: **owner as invalidation block; retained operand sites as patch
locations; storage logs/generations as mutation authority**. Plain values force evaluation at their owner's
grain. Promotion to Signal/List/Entity gives a finer grain without a component macro, hooks, or a view switch.
Contained owners still need a real mounting path (D8); List/entity support alone does not complete that rung.

For L4, store the receiver type in the route builder independently of the mounting `Element<V>` owner type.
The current Click/Press/Hover/Key/Drag `ActOp<V>` bounds tie the closure receiver to V; allowing ancestor
receivers requires changing those builder types/bounds, not just runtime dispatch. At mount, walk the owner
chain and bind the nearest matching type. A missing match is a located mount error. The normal route call
should not repeat a TypeId search.

I considered two receiver-lifetime designs; **neither is implemented or selected by measurements**:

* Validated owner tokens with parent, generation, receiver pointer, class, and mounted descriptions. Dispatch
  is short when tokens can prove liveness, but every removal/reuse/reparent path must revoke them before
  dereferencing either the receiver or a validator stored inside an ancestor. A validator pointing into a
  dead parent is not safe merely because the leaf has a generation.
* A typed access path rooted in the current App owner. Each erased step uses a proven field offset plus
  `List::edit_slot` or `Entities::edit`, checking generation and obtaining the child through its live parent.
  This naturally marks existing mutation logs and avoids trusting a cached pointer into a removed owner.
  It costs O(owner depth) at dispatch and may conservatively dirty ancestors. Bind the path at mount, benchmark
  depth 0/1/4/16, and only replace it with token shortcuts if that cost matters and lifecycle proof is sound.

Preserve direct root dispatch cost. Adding an owner id to each Route may enlarge all five optional routes in
an Act due to alignment; inspect `size_of` and code generation before claiming unused cost is zero. Route
closure storage already permits 16 capture bytes, so stealing some bytes for an owner id would change the API.
An extra runtime table or per-Act id has its own costs and is not automatically better.

A route should validate both receiver lifetime and the still-live mounted interaction. A drag capture can
survive removal of its element; a removed entity can keep last pixels by current contract while its handle
is dead. Tests must cover drag release after removal, slot reuse in a later turn, ancestor removal, nearest
same-type ancestor, multiple entity views, and a root-targeted route nested in a row. Scrollers/text fields
also currently assume root-relative offsets. Do not globally remove their guard without binding those
controls to their owner too.

For re-description, give ordinary View rows their own tape and typed update function (the raw `list_with`
callback is mount-only and cannot magically supply one). Refresh dirty descriptions after effects have
settled, not half-way through an event. Keep enclosing dynamic/list record counts correct when a child
changes structure. Root traversal plus a separate child queue can otherwise double-update a child or skip
using stale counts. Entities with no records, multiple top-level roots, and multiple displayed views make a
single guessed first-record anchor inadequate. Settle this representation explicitly before building it.

### Skipping by proof: builder type is insufficient today

A literal `Color` and one computed from `self.plain` have the same type. The same is true for `f32` sizes,
conditional static strings, and most layout fields. An associated const folded through current element
types can conservatively identify *possible* by-value operands, but cannot prove those operands constant.
A tree containing only wired paint still has ordinary layout specs. Nor does a Wired type prove that its
source offset or captured mapping recipe remains identical on the next description.

Thus accept compile-time elimination only where the representation actually carries a proof (typed static
structure/operands or a complete runtime operand identity check). Reject a const that simply declares
existing plain operands static. No ambient tracking of ordinary `&self` reads is available in Rust without
changing the programming model. Start with owner selection and honest retained comparisons; let F8 decide
whether a new operand representation is needed to delete `once()`.

### Packed operands: promising filter, not a correctness shortcut

The existing tape stores node ids, not a contiguous canonical stream of all render-affecting operands.
Raw-byte comparison of Rust structs is unsuitable: padding, pointer-bearing strings, float NaNs/signed zero,
and pointer identity versus content equality must be defined. A useful packet would explicitly encode only
semantic operands, with no uninitialized bytes, and associate changed lanes with known patch sites. Exact
block equality can then skip entry into element updates; a hash alone cannot certify equality without a
collision check. Count packet construction/storage and equality work alongside savings. A changed paint
operand must not force layout reinterning. Benchmark sparse and dense changes and small trees; a summary
filter that taxes every small case fails the floor. Mutants should omit one operand class from the summary.

### Keyed structure: preserve the design's identity law

`update_dyn` is positional; a differing `dyn_key` replaces the item in place. The entity wrapper supplies a
key, so shifting an array of entity views can trigger many replacements. But Vec/each are positional by L3,
and List is the durable keyed form. Applying an LCS to all Vec items changes public identity semantics.
Do not silently do it in this repair. Improve keyed List delta/range handling and preserve row-owned state,
then connect movement to core's motion/damage work. If explicit keyed description reconciliation is wanted,
it needs a settled API/identity law and duplicate-key behavior first. Ripple's prefix/suffix/key-map/LIS
approach is a reference for that separate problem, not permission to change ordinary Vec semantics.

### Ordering and batching: take the mechanism, not the entire JavaScript runtime

Local sources examined: Ripple `fa847e61d8da0e4eda4ad7384ab7ac7645d46253`; Solid `next`
`ce5b288e33e3ffb924fc558fa5b09b1beff14e1b`.

Ripple's compiled hydration reactivity fixture combines static `template_el` construction, retained node
locations, and render blocks comparing prior operands. Tracked reads attach dependencies to the active
block/derived computation. Writes use equality/version changes to schedule subscribers; scheduled flags
deduplicate; the flush queue orders blocks by creation id when necessary and separates phases. The useful
lesson for bough is a stable patch program per invalidation block. It is not proof that each source signal
should create one dynamically dispatched Rust effect or that every expression needs its own block.

Solid next's `heap.ts` uses height buckets, dependency marking, and deduplication. The core scheduler has
additional handling for writes during recomputation/missed wakeups; height sorting alone is not the full
correctness argument. `store/projection.ts` uses a derived producer plus leaf-level store subscriptions:
refresh producer state before serving leaves, then notify changed observed paths. `store/reconcile.ts`
compares the incoming immutable backing data and notifies observed nodes. A Rust analogue would be explicit
typed projection outputs with per-output equality and generation validation, rather than Proxy machinery.

Current bough leaf wires already patch known destinations directly. `settle_cls` scans class offsets for
queued heads; a head runs its installed wire chain, and async rescan can repeat settlement. Producer/wire
installation order matters. There is no general height-ordered derived DAG to optimize yet. Do not add a
heap to every leaf paint write. Distinguish owner containment order, computation dependency order, and
publication order: they are different graphs. For future projections use explicit dependency scheduling
only where needed, with a transaction boundary before rendering/effects can observe the committed state.
Test diamonds, write-during-recompute, cross-owner effects, removal during a wave, and async supersession;
a reverse-order or dedup-disabled mutant must expose a torn state or duplicate dependent execution.

One additional **unverified** lifetime concern is `App::effects`: it invokes root `changed` before iterating
published child raw pointers. Inspect whether root `changed` can remove a published child before that loop.
Do not claim this is safe merely because it originated from a previously live wire; write a bounded witness.

## 4. Continuation traps and next work

* The harness and test are uncompiled. Fix compiler errors before interpreting any results. A deep generic
  type or the wired-tree macro may need a straightforward correction; preserve the failed attempt.
* The F8 measured loop does not call `frame`; examine pending damage/text allocation behavior over the batch.
  If it accumulates unrelated work, revise the frozen measurement clarification and preserve the original.
* A low scroll baseline can reflect skipped child work. Always pair costs with retained/fresh correctness.
* `On::update` currently only updates the underlying element; it does not refresh captured route recipes.
  A positional row closure capturing a new index/value can retain an old capture. This is source-observed
  behavior, not yet reproduced by a test. Fixing generic On update must not reinstall wires/scrollers each turn.
* Scope release is two-phase: revoke reachability now, reclaim storage later. Receiver revocation cannot wait
  for deferred reclaim. Nested list changes also need enclosing record counts updated immediately.
* Existing entity removal deliberately retains last pixels. Fresh-mount tests must distinguish removing a
  view from removing its entity; blindly fresh-mounting a dead entity currently panics.
* Keep exact pixel comparisons and existing gates. The new oracle is not a replacement for generalization's
  table scenarios, the scene suites, native controls, or cold layout comparison.
* Do not run broad formatters. Do not modify other worktrees or shared docs/root manifests. If Cargo resolves
  lockfile source entries, keep a reviewable patch and report the exact inputs rather than silently committing it.
* No performance floor is certified. Preserve the 1.02x regression floor and 1.05x no-once target. All are
  UNKNOWN here. No higher-level propagation optimization was authorized before proposal/card settlement.

Next bounded sequence: (1) compile and measure F8 three times, preserving the baseline; (2) run the scroll
oracle on baseline and keep its failure; (3) repair ScrollBox with lifecycle/offset/layout checks and a
skip-child mutant; (4) freeze the precise owner-path/tape design, repair typed routes and child plain-field
re-description together, and add generation/removal/ancestor/multiple-view mutants; (5) run touched-crate
release tests, all-target checking, assembly inspection and all floor cells; (6) write DESIGN-R6 with actual
numbers and settle the larger owner/operand program before implementing it. Independent review must replay
any proposed acceptance.
