# Propagation: where bough's wires beat Ripple's model

Lane `surface-api`, round 6. Ripple read at `ref/ripple` fa847e6 (`packages/ripple/src/runtime/internal/client/runtime.js`).

## What Ripple pays, per operation

* **Read inside a block run**: `get_tracked` → `register_dependency`: a `Dependency` node per (block, value), found by a
  linked-list walk, recycled across runs (`create_dependency`), linked into the value's doubly linked subscriber list.
* **Write**: `set` → `clock++`, `mark_subscribers` walks the value's subscriber list, `schedule_update` pushes each
  block once (`SCHEDULED`), the queue is sorted by creation id at the flush (`flush_queue`).
* **Flush**: per scheduled block `is_block_dirty` walks its dependency list comparing clocks (a derived is recomputed
  lazily on the way), then `run_block` reruns the block's whole function, which re-tracks every read.

Every edge is rediscovered at run time, every value carries a version, and a block's dirtiness is a walk.

## What Rust lets bough know instead

| Ripple, at run time | bough, at compile time or once |
|---|---|
| an edge is a `Dependency` node rebuilt per run | an edge is a wire made at mount from a `sig!` operand: `offset_of!` proves the field, the patch is monomorphised for (property, value type, mapping); a re-description never re-links it |
| a clock per value, a clock per edge | no versions: a write queues the head (the lead's per-thread write log names it); the settle runs exactly those heads' wires; the destination's compare is the cut-off |
| `is_block_dirty`: walk the block's edges | an owner's dirtiness is a byte compare decided by a **plan** built at compile time from rustc's type reflection (`grain::plan::<V>()`, a `const`): the plain bytes it was described from against the bytes now. No list, no clock |
| the block reruns and patches what it reaches | the owner reruns `view` and compares its operands with the retained **owner seal** in one streaming compare; equal: the walk is skipped |
| subscribers of every tracked read | only a *plain* read of a cell (`*self.n`, `&self.s`, `self.items.len()`) while an owner describes itself creates an edge (a wake wire, the same mechanism, made once and kept); `sig!` reads create none at run time |
| sort the queue by creation id | owners re-describe rows first, then the root (tree order is the mount order) |

Built this round (commits on this branch, `grain.rs`, `describe::OwnerSeal`, `tests/owner_grain.rs`):
`signal_cost`, one `sig!` write per `App::turn` in a re-describing app: 16 rows 32,244 → 1,197 instructions, 256 rows
510,970 → 11,234; the mount-once path is 1,113 / 11,150, so the re-describing app now costs what the wire path costs
plus ~84 (the plan compare). gui-bench fine, a turn that changes nothing: 4.1k → 0.4k.

## Next: derived values and effects as wires in a compile-time order

* **A derived value is a cell** (`Derived<T>` field, value inline, a wire head like a `Signal`). Its recompute is a
  wire on each source it names through `sig!`; the recompute runs at the front of each source's chain (as
  `latest` wires do now) and queues the derived's own head only when the value changed.
* **Topological order is declaration order.** A derived may read only fields declared before it (checked by
  `offset_of!`-free field indices from `type_info`, a `const` assertion); recompute wires are installed in that order,
  so a source's chain recomputes `D1` before `D2 = f(A, D1)`. No height sort, no runtime graph.
* **Glitch-freedom without a sort**: every patch reads exactly its own source, and a derived's readers are wires on
  the derived's head, which is queued only after the recompute: no reader can observe a stale derived next to a fresh
  source. A second recompute of `D2` through `D1`'s head is cut by the equality compare; a const closure over the
  dependency sets can drop it entirely.
* **Effects** stay `View::changed`, one site per owner, run in waves after the wires.

## What still costs at run time, and why

* The plain-read recorder: one load of a global and a predicted branch per plain cell read outside a description; a
  thread-local push per plain read during one. An owner whose plan has no cells does not record at all (`Plan::cells`).
* The plan compare: proportional to the owner's plain bytes, only after a handler ran on it (a wire-only turn: none).
* Owners whose plain state the plan cannot see (`Vec`, `String`, `Box`) re-describe after their handlers as before;
  the owner seal and the per-item seals make that rerun a compare.
