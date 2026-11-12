# surface-api round 6: handoff (2026-11-12)

Written by the Opus session that took the lane over on 2026-11-11 at 21:10 (after the Astra handoff and the Fable
agent). Supersedes the earlier HANDOFF and `STATUS.md`'s "this session" section. Design notes: `PROPAGATION.md`;
card: `cards/2026-11-11-owner-grain.md`.

## State

* Branch `lane/surface-api/round6`, head: the commit that adds this file (code head `68c05f5a36`).
* Merged into this branch: `lead/core-r6` up to `4f4c469ef5` (write-proportional settle, wired seals) and `round6`
  up to `33dfe4738f`. **Nothing of this session is in `round6`.**
* Commits of this session, oldest first:
  * `c2ae86085a` bough-app-sketch-code: `Spec::set_abs` (`cargo check --workspace --all-targets` did not build).
  * `signal_cost` fix (the lead's harness did not compile: `NumFmt::new` arguments).
  * `fd71e725b0` STATUS of the inherited work + card.
  * `51d297ffd0` **owner grain + owner seal** (`grain.rs`, `describe::OwnerSeal`, seals for every element that had
    none, `Signal::peek` for runtime reads, `tests/owner_grain.rs`).
  * `149a3b49f5` **record state flags + variant tables** (`flags.rs`, `tests/flags.rs`, `examples/flags_cost.rs`).
  * `2b5279ad17` merge of `lead/core-r6` (elem.rs conflicts resolved to the lead's `seal_wire`).
  * `68c05f5a36` **direct wires** experiment (`direct.rs`, `tests/direct.rs`, `signal_cost` frame columns).
* Inherited and verified (Fable agent, see `STATUS.md` table): scroll containers reconcile their content, `On::update`
  refreshes route captures, routes resolve receivers along the owner chain, only handled owners re-describe, hover
  follows the tree (finding 8), route seals. Each test passes; each mutant fails its test (run by this session).

## Numbers

All instruction counts on the calling thread (`tinstr` / `proc_pid_rusage`), release, M3 Pro under heavy load from
other lanes (load average 60-130). Instruction counts are load-robust; each was run **once** unless noted.

| what | before | after | command | verified |
|---|---:|---:|---|---|
| signal path, colour write per `App::turn`, re-describing app, 16 rows | 32,244 | 788 (after the lead's write log) / 495 (direct wires) | `examples/signal_cost 16 256` | 1 run each state |
| same, 256 rows | 510,970 | 745 / 502 (direct) | same | 1 run |
| colour write, mount-once app, 256 rows, write above idle | 356 (712 - 356) | 62 (418 - 356) | `signal_cost`, `--features mutant_direct_off` for before | 1 run |
| colour write + frame, 256 rows | 1,087 | 957 (damage moves to the frame) | `signal_cost` column `colour+frame` | 1 run |
| idle turn, mount-once | 302 (lead's log, before flags/direct) | 356 | `signal_cost` | **regression +54** (TLS enter, arm and blur checks per turn) |
| redescribe - once (owner-grain compare per handled turn) | n/a | +85 | `signal_cost` redescribe vs once | 1 run |
| gui-bench fine, no-op turn (`turn(\|_\| {})`), 100 rows | 4,088 | 396 | `comparisons/bough-guibench` `probe 100 fine turn` (tinstr overhead 1,455 removed) | 1 run |
| gui-bench fine, no-op key | 4,732 | 1,015 | `probe 100 fine noop` | 1 run |
| gui-bench fine select (down + up + frame) | 14.86k | 15.0k (down +0.9k, up -0.8k) | `probe 100 fine select` | 1 run; **no gain**: see below |
| gui-bench rows hover (move + frame) | 3.9k | 3.9k | `probe 100 rows hover` | 1 run |
| idle frame (no damage) | 55 | 55 | `probe 100 fine frame` | the 1,500 seen earlier was `tinstr`'s own cost |

Flag flips (`examples/flags_cost 10000`, one run, turn / frame instructions): row hover 2,711 / 1,143; press
(down/up alternating, the click re-describes) 4,034 / 1,143; Tab with focus ring and focus-within 3,814 / 1,398;
disabled subtree by a plain field (the page re-describes) 15,104 / 1,228; the same by `flag_by` (a wire) 2,677 /
1,228; single selection (one row off, one on) 11,035 / 1,262; clear a flag on all 10,000 rows 127,548 / 1,058;
shift-range over 10,000 rows 305,373 / 2,866; checked toggle 7,404 / 1,058; 1,000 search hits 63,854 / 2,089;
drag-over (with the drag route) 1,738 / 1,143. No owner re-described during the flips but the plain-field toggles.

Baselines measured at the start (inherited head `88ed56ff5b`): `redescribe_cost` 1,000 rows owned 648 / static 614 /
bare 233 per row, once 181; gui-bench R0 fine select 14.9k (10) / 15.7k (1,000) / 15.8k (10,000), rows hover 4.0k /
4.5k, coarse select 28.0k (10) / 1.48M (1,000), coarse plain 27.3k / 1.48M. Mount-once floor, dist binaries of the
inherited head, 10x spread, median of 5: heavy stream 157,510; lab both 29,413 (spread 4 %); vlist scroll 12,749
(`runtime/lanes/surface-api/floor.py`). **The floor was not re-measured after my changes.**

## Not verified / known broken

* The whole `cargo test --release -p bough` ran green on `51d297ffd0` (owner grain + seal) only. On the flags and
  direct commits only the lane's test binaries ran (flags, owner_grain, direct, propagation_routes,
  propagation_scroll, seals). Not run at all: `bough-suite`, `bough-awagy`, `bough-zgui` tests, the mount-once floor
  (`bench/integrate/slope.py`, `bench/layout/slope.py`), the gui-bench sweep, the guibench gates.
* `cargo check --workspace --all-targets` passes on the final head (after renaming `List::flag` to `has_flag`: the
  blanket `Flagged::flag(self)` shadowed it, a by-value trait method beating a `&self` inherent one). That is the
  risk of the blanket trait: `Flagged` claims `when`, `hover`, `active`, `focus`, `disabled`, `selected`, `flag`,
  `flag_by` on every type once the prelude is imported. `tests/flags.rs` re-run green after the rename.
* Idle-turn regression +54 instructions (above). The per-turn `root_dirty` compare costs ~85 where a single 8-byte
  range should cost ~15 (`Plan::same` is generic; a fixed-shape fast path is unwritten).
* Fine select did not improve: its root handlers change plain fields (`pressed`, `selected`), so the root re-describes;
  the owner seal makes the walk a compare, but `view()` itself (status `format!`, building the tree) and the moves of
  the description (finding 12) remain. The gui-bench port's selection was not moved to flags (script ready in the
  session scratchpad, not applied).
* Direct wires: entries are armed only after a mount/turn/frame; a box whose node gets variants after mount stays
  correct (arming refuses) but a box that gains an opacity *signal* later is impossible by type. Multi-runtime threads
  fall back (TLS points at the last runtime). The dirty list is drained only by `frame()`: a program that writes and
  never frames keeps a bounded list (one entry per cell).
* Plain reads of `Entities` / `Reader` in a description make the owner re-describe after every handled turn
  (conservative). Owners with a `Vec`/`String`/`Box` field keep the old rule (re-describe after every handler on
  them), though the owner seal makes it a compare where the description seals.
* Findings 7 (list in a port at the window origin) and 15 (f32 offsets) were not worked on; 12 (O(d^2) moves) was
  designed, not built (below).

## Experiments

| experiment | mechanism | result | verdict |
|---|---|---|---|
| owner change detection by a compile-time plan | `type_info` walk of the owner type: scalar leaves as byte ranges, fieldless enums, `Option` payloads; cells skipped | signal path 32k -> 0.8k at 16 rows; no-op turn 4.1k -> 0.4k | kept |
| tracked plain reads as wake wires | cells' read accessors record their head while an owner describes; the owner subscribes with an ordinary wire | needed for exactness (mutant `mutant_grain_track` fails the gate) | kept |
| owner seal | the root's whole description compared before the walk | correct (mutant fails); gain unmeasured in isolation | kept |
| flags with variant tables | flag word indexes a table resolved at mount | 2-4k per input flip, 7-11k per list flip | kept; list flips have a per-mounted-row scan |
| direct wires | `set` stores into the column, the frame makes the damage | write 356 -> 62 above idle; turn+frame 644 -> 514 | kept as experiment; the damage half is the next win |
| `Plan` via `array_len` | | rustc ICE on a named-const array length | replaced by size/element size |

## Underexplored and exotic avenues

1. **Direct damage, not only direct stores.** A direct entry could carry the record's tile span (from its last
   placed ink) and OR it into the paint plane at write time, with a per-page epoch that invalidates the span when the
   page moves; the frame would then do nothing per write. Estimate: colour write + frame 514 -> ~150. Risk: stale
   spans after scroll/layout; cheapest test: store the span at arm, compare against `visible_ink_handle` in a debug
   assert over the suites. Depends on core's page epochs.
2. **Derived cells with compile-time order** (`PROPAGATION.md`): a `Derived<T>` field recomputed by wires installed at
   the front of each source's chain in field declaration order; readers are wires on the derived's head. Glitch-free
   without a height sort; equal recompute values cut by the compare. Estimate: a derived of 2 sources costs one
   recompute + one store per write (~100 instr). Test: a diamond (A -> B, A -> C, B+C -> D) with a counter of D's runs.
3. **Batched writes to one cell**: with direct wires, N writes in a turn are N stores and one damage; for ordinary
   wires the lead's log already dedupes by QUEUED. A `List` analogue (item flags) should log changed *positions as
   runs* instead of rescanning mounted rows (today O(mounted) per flag turn: the 7k of a checked toggle).
4. **Flags as core state planes**: move `Act::{own,sub,inh,eff}` into core's 16 bit columns per page with summaries
   (COLUMN-PLAN.md). Subtree writes become mask writes over the subtree's page range; `any_state` makes focus-within
   and "is anything selected" summary tests. Estimate: range flip over mounted records -> ~2 instr/record.
5. **Per-position flag planes in `List`**: bitsets by position instead of a `u16` per slot; a range select over
   10,000 rows becomes ~160 word writes instead of 10,000 scattered stores (305k -> ~10k). Cost: insert/remove shift
   the planes (O(N/64)), permutations need an inverse map.
6. **Owner seal as streamed columns** (the lead's "seals as columns"): seal words at fixed strides in page-aligned
   buffers so an unchanged owner of 64 rows is a vector compare; with the plan compare in front, most owners never
   reach it. Estimate idle re-description 550 -> <30 instr/row.
7. **Contained views as owners** (rung 2): `part!(self.editor)` proving an offset like `sig!`, a tape and an owner
   seal of its own, routes resolving to it by offset (no `List`/`Entities` hop). A handler on the editor re-describes
   only the editor; the parent's walk steps over it. Needs a mounting element and OwnerRef kind; API addition only.
8. **By-reference update (finding 12)**: `Element::update(&mut self, ..)`, `IntoProp::value(&self)`,
   `ActOp::refresh(&self)`, `BoxOps::refresh(&self)`; `Vec`/`Each`/`Option` hand their items to `update_dyn` by
   `take`/`by_ref`. ~104 sites in 14 files, mechanical. Removes the memmove that was 15 % of a no-op Fine turn's
   samples and the d^2 term (200 levels: 533k -> ~25k predicted).
9. **f64 / integer scroll offsets (finding 15)**: accept `Signal<f64>` wherever `Signal<f32>` scrolls (a sealed
   `Offset` trait, monomorphic read/write fns in the scroller); the API does not get worse (f32 still works).
10. **Variant-driven layout**: `.when(Flag::EXPANDED, |s| s.h(..))` resolving to a spec index per table entry; a flip
    is a restyle of exactly the flipped records. Needs the store lane's spec columns.
11. **Plain-read recording at compile time**: a description type that contains no `&Signal` plain operand and whose
    `view` body is a const fn of `sig!` operands could prove "no plain reads"; Rust cannot see `*self.x` in arbitrary
    code, so this needs a lint or a `#[pure_view]` attribute check. Speculative.
12. **Hover chain as flags only**: once every hover look is a variant, `hover_set` reduces to two flag writes and the
    act table's hover fields can go; the chain diff could be a symmetric difference of two sorted handle lists
    (already) or a summary plane test.
13. **Selection semantics in the API**: `List::select_only(slot)`, `select_range(a..b)` as thin helpers over
    `set_flag` so applications do not each re-implement anchor/shift logic; keyboard extension via a `current` flag.

## API: what is at risk, composition, and what a user-facing review would change

* **Ergonomics bought for benchmarks, at risk**: `sig!(self.x).at(i)` arrays, `.num(f, NumFmt)` and `list_with`
  exist for cost, not clarity; `Config::once()` still splits behaviour; `text(&self.s)` (a plain borrow, a
  constant until the owner re-describes) and `text(sig!(self.s))` look alike and behave differently (this session
  made the first exact through tracked reads, at a cost). Closures capped at 16 bytes of captures (routes,
  mappings) and the `Copy` bound will bite real applications (a `String` capture does not compile).
* **What the flags API costs users**: none of the existing code changed (the gui-bench and counter programs
  compile unchanged); the new surface is `Flag`, `Flagged` (`.when/.hover/.active/.focus/.disabled/.selected/.flag/
  .flag_by`), `Look` (`.bg/.border/.color`) and `List::{set_flag, set_flag_at, set_flag_range, clear_flag, has_flag}`.
  The blanket `Flagged` trait claims common method names on every type in the prelude (see "not verified").
  Before/after of the gui-bench fine row (not applied): `.bg(sig!(self.selected).map(|s| rgb(if *s { SEL } else
  { EVEN })))` + a `selected: Signal<bool>` per row becomes `.bg(rgb(EVEN)).selected(|s| s.bg(rgb(SEL)))` and
  `self.rows.set_flag(slot, Flag::SELECTED, true)`: shorter, and no signal per row.
* **Composing and overriding external components**: variants are the natural override point (a component exposes
  its looks per flag; a host overrides a `Look` without touching the component's code), and `flag_by` lets a host
  drive a component's state from its own signal. Missing: a way to address a component's inner element from outside
  (a part name), and a stable identity for an external view's records so `Flag`s and reflection can target them.
  Owner-scoped re-description makes a contained component cheap to embed only once contained views are owners
  (avenue 7).
* **A user-facing review would change**: one `text()` operand story (plain vs wired visible in the type name or a
  lint); `Signal::set` vs `*sig = ..` (they now differ in cost: direct wires apply only through `set`); the owner-type
  in route closures (`|v: &mut Self|`) is clear, but `on_press` + `on_click` on a root to emulate row selection
  (the gui-bench port) should be a row route plus a flag; selection helpers on `List` (avenue 13); error messages for
  a route whose receiver is not on the owner chain are good (a mount panic naming both types).

## Where I think the lead or the plan is wrong

* "A no-op handler under 300" counted with the frame: the turn alone reaches ~400 on Fine but the per-turn fixed
  cost (settle log, effects, layout flush, flags flush, arm, blur, TLS enter) is now the floor; each mechanism adds
  its own constant. The next gain is a single per-turn "nothing happened" bit, not faster mechanisms.
* "Fine select under 2,000 at any row count" is not reachable with root handlers that change plain fields: the
  owner re-describes by the design's rule 0. The gui-bench port should select through row routes or flags; the target
  should be stated for that program, or for the turn without the frame (the frame alone is ~1.1k for two rows).
* Measuring wire-path cells without a frame (`signal_cost`) hides where damage is computed; direct wires look 5.7x
  better without the frame and 1.25x with it. Report both.

## Next (in order, with predictions)

1. Take the per-turn constants back (idle 356 -> ~300): enter TLS only when direct entries exist, inline the arm
   check, fold blur into `set_focus`'s callers; fast path for one-range plans (+85 -> +15).
2. Direct damage (avenue 1): colour write + frame 957 -> ~400 at 256 rows; then numeric labels with fixed advance as
   direct glyph-id stores.
3. By-reference update (avenue 8) and the gui-bench fine port on flags; re-run the sweep's fine/coarse cells and the
   mount-once floor (must stay within 1.02x).
