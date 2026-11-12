# IDEAS-OPUS: the record tree and the façade's storage, read from the assembly up

State: `IDEAS` (an Opus collaborator's file for the `core` and `store` lanes, round 6). Nothing here is settled.
Every number says **measured** (with the command that produced it) or **estimated** (with the reasoning).
Numbers attributed to another lane are theirs, not replayed here.

What was read: `ROUND6.md` (all amendments), `wt/lead/docs/lanes/core/{STORE-R6,FOUNDATION-R6}.md`,
`wt/core/docs/lanes/core/{TABLE-R6,COLUMN-PLAN,HANDOFF,COST-R6}.md`, `wt/store/docs/lanes/store/BASELINE.md`,
bbtree `design/{14-blogpost,06-cost-model,07-node-plan}.md` (and `03-decisions.md` by search),
`research/04-reformulation-method.md`, Ben Joffe's "fast time of day" (fetched 2026-11-11: V1 breaks the
divide chain into two independent multiplies, V2 reads the next field from the low half of a product already
computed, V3 pads 60 to 64 so a remainder is a mask, the NEON variant narrows the range so four lanes fit).

Code: branch `opus/tree` from `lead/core-r6` at `59e26e2f4c` (the store branch `1561db0f51` is that commit plus
the sibling-link stage 0a; the core branch `495d504e42` is it plus `translate_pages` and the state-plane
contract). Toolchain nightly-2026-10-17, release profile (thin LTO, debug 1), M3 Pro. Binaries:
`cargo build --release -p bough --example redescribe_cost` and `cargo build --release -p bough-core --example
core_bench` (the brief's `--example bench` does not exist in `bough-core`: its bench example is `core_bench`).
Disassembly: `objdump -d --no-show-raw-insn` (Apple LLVM 17). Instruction counts on the calling thread with
`thread_selfcounts` (`bough_metal::perf::tinstr` or its copy), slopes between two repeat counts so the counter
cancels. The probes are `crates/bough/examples/opus_probe.rs` and `crates/bough-core/examples/opus_core_probe.rs`
on this branch; the two source experiments (A1a, A1c/A1d) are commits on this branch, marked `OPUS EXPERIMENT`
in the code, for the owning lanes to take or drop.

---

## Summary

The core's scans are already close to their floor (the cull spends ~30 instructions per admitted group of 8
records, all NEON, no branch per record). **The time is in the paths around the scans**, and the assembly shows
the same four defects everywhere:

1. **Values are moved, not computed.** The re-description of one unchanged row moved its 330-byte description
   through a look-ahead slot: ~110 stores and reloads per row. Splitting the hot loop from the look-ahead
   (A1a, 20 lines) **measured** 645 -> 420 instructions per row (application row), 617 -> 292 (static), 231 ->
   121 (bare box); with integers sealed as integers and short strings compared as words (A1c/A1d) the
   application row is at **278** (2.3x), churn and mount unchanged within 1 %.
2. **Every applier walks a 5–6 load chain through tables in different index spaces** (tape -> `LInfo` ->
   `Node` -> `Run` -> `Instance`, and `HEntry` -> `Page` -> `clips`), with a bounds check at each hop: 9 on
   the bare box's update path, 29 compiled into the application row's.
3. **Rect algebra outside the kernels is scalar.** `NRect::intersect` outside a NEON loop compiles to four
   `cmp`/`csel` pairs plus a 6-instruction repack (`bfxil`/`orr`/`lsl`) instead of one `smax.4h`.
4. **Bookkeeping is eager and per record.** A one-pixel ink write costs **185 instructions before any damage**
   (measured), of which the store is one; a damage mark of one rect costs **~60** (measured) and every write
   marks two; the edge-band write that `quad_geom` uses costs **573** (measured), more than the old∪new it
   replaced (241).

The formulations in Part 2 attack the representation behind each: position as identity (no chain), the
directory as a packed-memory array (structure edits stop being O(pages)), seals as fixed-stride word records
(re-description as a column compare), layout of repeated templates as a replayed trace, summaries as one word
per page and chunk (every "is anything here ..." question is one AND), and damage from motion certificates
instead of per-record marks.

---

# Part 1: assembly review

Each section: the common path, its instruction count, its dependent-load chain (each load named with what its
address depends on), unpredictable branches, surviving bounds checks, spills, calls not inlined, missed vector
code, and what the minimal code would be. Addresses are from `target/release/examples/redescribe_cost` (lead
base) unless marked `core_bench`. Symbols are shortened (`<ApplyBg>` for the mangled name).

## A1. `describe::update_dyn` for the application row (`Table<0>`: box + integer label + string label)

`0x1000e4b80`, 4,382 instructions, frame `sub sp, sp, #0xa00` (2,560 B), 1,080 stack-relative loads/stores,
24 calls to `RawVecInner::reserve`, 17 `panic_bounds_check` sites.

**Measured** per row per turn (`redescribe_cost 100 1000`, 1,000 rows, the selection moves: two rows change): owned
645, static 617, bare 231 (`once` control: 173 per turn in total). `NOCHANGE=1` (nothing changes): 639 / 611 / 228,
so the cost is the unchanged rows. **Measured** split with `opus_probe` (the same row described and checked in
isolation, 1,000 rows): building the description **57**, building + `SealCheck` **360**; so ~300 is the seal check
and ~285 is the loop around it.

**A1a. The look-ahead slot copies the description on every item (the largest single defect).**
`while let Some(e) = pending.take().or_else(|| items.next())` merges two sources of a ~330-byte `E` (an `LBox`
holding two `Para`, three 84-byte `Spec`s). LLVM materialises `E` in one stack slot that both sources write, so
every item's freshly built fields are stored field by field into it:

```
1000e509c: str	w7, [sp, #0x1b8]          ; one of ~110 stores per item into the merged slot
1000e50a0: str	w7, [sp, #0x394]
1000e50b4: stp	x13, x12, [sp, #0x1c0]
1000e5108: stur	x12, [x1, #0xa4]          ; spec words of the first Para
1000e5110: stur	x12, [x1, #0xac]
  ...                                      ; (lines 1000e509c..1000e525c: 110 instructions)
1000e4f48: str	x26, [sp, #0x2b8]          ; the pending-Some side: the same fields reloaded and re-spread
1000e4f4c: ldr	x8, [sp, #0xf8]
1000e4f50: lsr	x9, x8, #32
```

Minimal code: the unchanged item never leaves registers; it is built, compared and dropped. **Experiment (this
branch, commit A1a): the hot path runs in its own `for e in items.by_ref()` loop where `e` has one source; a
mismatch hands `e` to the existing body through `pending`.** Measured (`redescribe_cost 100 1000`):

| row (instructions per row per turn, 100 / 1,000 rows) | base | A1a | A1a + A1c + A1d |
|---|---:|---:|---:|
| owned (box + `u32` label + `&str` label) | 679 / 645 | 461 / 420 | 343 / **278** |
| static (box + two `&'static str`) | 677 / 617 | 363 / 292 | 367 / **300** |
| bare box | 259 / 231 | 154 / 121 | 154 / **121** |
| `NOCHANGE=1`, 1,000 rows: owned / static / bare | 639 / 611 / 228 | not run | 271 / 293 / 118 |
| churn (remove + insert, per row) | 15,941 / 15,957 | 16,431 / 16,561 | 15,893 / 15,816 |
| mount (`MOUNT=1`, per row at 1,000 rows): owned / static / bare | 27,760 / 27,944 / 4,861 | not run | 27,657 / 27,680 / 4,886 |

At 10,000 rows the experiment build measured 271 / 293 / 118 (owned / static / bare): flat in N, as a per-row cost
should be. (All cells measured on this machine by this session, `redescribe_cost` built from this branch with and
without the experiments: `git stash` of the three source files, rebuild, run, `stash pop`. Mount cells at 100 rows
are omitted: they are the difference of two ~13.3 M-instruction `App`s and move by ±150 K between runs.) A1a alone
cost churn +3 % (the slow path re-runs `sealed_as` once per mismatching item); with A1c/A1d's cheaper checks churn
is 0.3–0.9 % below base. Mount is unchanged within 1 %, as it should be (the experiments touch only the
re-description path). Tests: "Verification".

**A1b. The seal check is a chain of bounds-checked word reads.** `SealCheck::take_word` is
`split_first_chunk::<4>()`: every word is `subs x24, x24, #4; b.lo`, a load, a compare, a `csel` into `ok`
(roughly 6 instructions a word, 13 words a row before specs and strings). The branch is predictable; the cost is
the count. Minimal: a fixed-shape item's seal has a length known from its type (Part 2, F2), so one length check
per item and unchecked loads.

**A1c. Strings are compared through a `memcmp` call.** `bytes()` ends in `head == b`:

```
1000e57d0: bl	0x100264190 <memcmp>       ; labels of 5..15 bytes
1000e57d4: cbz	w0, 0x1000e5cf4
```

bbtree's lesson ("the compiler is not your friend": compare short keys as integers, no `memcmp`) applies
verbatim: two overlapping 8-byte loads per side for 8..=16 bytes, two 4-byte for 4..7.

**A1d. An integer label is formatted to decimal to be compared.** `text(*id)` seals `decimal(id)`: a `umulh` by
`0xCCCC..CCCD` per digit with a store per digit, then the `memcmp` above:

```
1000e5740: mov	x9, #-0x3333333333333334
1000e5744: movk	x9, #0xcccd
1000e5748: umulh	x9, x8, x9
1000e574c: lsr	x9, x9, #3
1000e5754: msub	w11, w9, w11, w8
1000e5760: strb	w11, [x12, x10]
1000e577c: b.ls	0x1000e5740            ; per digit
```

The text is a function of the integer, so the integer is the operand: seal two words. (Joffe's rule: read the
answer from what was already there.) A1c and A1d are one commit on this branch. **Measured** with `opus_probe`:
build + `SealCheck` of the application row 360 -> 253 instructions; in the whole turn (table above) the owned row
went 420 -> 278 on top of A1a. The static row (two `&'static str`, 5 and 15 bytes) moved 292 -> 300: the
word compare saved nothing there and code layout cost 8; worth a look before taking A1c alone. Exactness: both
are the operands themselves (no hash); the existing negative control (`BOUGH_SEAL_MUTANT`, which accepts any
text bytes of the right length) still reaches strings through A1c's `eq || mutant()`, but an integer label no
longer passes through `bytes`, so A1d got its own (commit `ce922c90d7`): with feature `sabotage_seal_int` and
`BOUGH_SEAL_MUTANT_INT=1` the check accepts any integer, and the seals gate fails at its `renumber 2` step
(`a_seal_that_ignores_integers_is_caught`, run and passing). Behind a feature, like `sabotage_copy`, because the
same hook as a runtime flag measured +12 instructions on the owned row (290–292 against 278); compiled out, the
cells are exactly the table's.

**A1e. Specs are compared through the spec table, 84 bytes each, three per row.** `descs.is(ix, s)` is a
bounds check on `specs` (`ldr x11, [x21, #0x2c0]; cmp; b.ls`), an index multiply by 84, then ten 8-byte
`eor`/`orr` and one 4-byte; the first spec of the row is scalar, the Para ones are partly NEON:

```
1000e5358: ldr	x13, [x21, #0x2b8]        ; specs.ptr
1000e535c: mov	w14, #0x54               ; 84
1000e5360: umaddl	x12, w12, w14, x13     ; &specs[ix]          (depends on the seal word ix)
1000e5364: ldp	x13, x14, [x12]
1000e5368: eor	x10, x13, x10
1000e536c: ldr	x13, [sp, #0x480]        ; the new spec's words come back from the stack (A1a's slot)
  ...                                     ; 11 more ldp/eor/orr
1000e53e4: cmp	x10, #0x0
```

Dependent loads per spec: seal word (address: the item's seal offset) -> `specs[ix]` (address: that word). Two
in a chain, each a likely L1 hit. Minimal: no indirection (the spec bytes inline in a fixed-stride seal record,
or a 16-byte compact spec; Part 2, F2).

**Minimal code for an unchanged fixed-shape row.** `opus_probe` variant (d), measured: the whole description
(three specs inline, sizes, colour, the id as a word, the label's first 16 bytes) written as a 288-byte record
and compared with the retained record by 36 `u64` xor/or: **135 instructions** per row, building included; 159
once the check also handles labels longer than 16 bytes (a tail compare against the part's arena, not taken for
these 13-byte labels); 206 with 80-byte labels, against `SealCheck`'s 256 / 316 for the same rows (with A1c/A1d).
Variant (c), dynamic operands only: 85 / 95 / 147. With the loop around it: **~180 a row, against 645 at base
and 278 with the three experiments**; most of what is left is the check's constant operands, which a 16-byte
compact spec would cut by about 60 more (three specs from 84 to 16 bytes).

## A2. `LBox::update` (bare box `0x1000f6ca0`, 256 instructions; application row `0x1000f3988`, 924)

Runs for the row that changed (here: the colour of the selected row). **Measured**: a changed row costs ~3,000
instructions more than an unchanged one (default minus `NOCHANGE` at 1,000 rows, two rows change per turn: base
(645,752 - 639,733) / 2 = 3,010; with the experiments (278,276 - 271,612) / 2 = 3,332), so ~3,600 in all. Most of
it is the slow path around the update (two `sealed_as` calls, a `SealWrite` that interns three 84-byte specs
through `SpecTable::intern`'s hash map, the seal copy) rather than the update itself (estimated from the listing;
not split by measurement). Common path of the bare box's `update` when only the colour changed: **~140
instructions** to the `bl <ApplyBg>` (counted on the listing: entry to `1000f6d84` 58, `..6dbc` 14, `..6dcc` 4,
`..6ed0` 49, `..6f68` 3, epilogue 11).

Dependent-load chain (each line waits for the one before):

```
1000f6cc0: ldp	x8, x0, [x1, #0x20]       ; m.tape, m.ti                         (m)
1000f6cd0: ldr	x8, [x8, #0x8]            ; tape.ptr                             (m.tape)
1000f6cd4: ldr	w23, [x8, x0, lsl #2]     ; lid = tape[ti]                       (tape.ptr, ti)
1000f6cfc: ldr	w8, [x8, #0x24]           ; info[lid].spec_ix   (info.ptr + lid*52)
1000f6d18: ldp	q1, q0, [x8]              ; specs[spec_ix] (84 B)               (spec_ix)
1000f6d9c: ldr	w22, [x9]                 ; info[lid].rtn                        (lid)
1000f6e74: ldr	w0, [x12, #0x28]          ; nodes[rtn].run      (nodes.ptr + rtn*120)
1000f6e8c: ldr	w0, [x12, x13, lsl #2]    ; runs[run].first     (runs.ptr + run*12)
1000f6eac: ldp	s3, s4, [x13, #0x18]      ; inst[first].radius  (inst.ptr + first*48)
```

Six dependent loads to reach the instance (tape -> info -> nodes -> runs -> inst, after the tape's own pointer), in
five allocations with strides 4, 52, 120, 12, 48. At 10 K rows (30 K elements) `info` is 1.5 MB and `nodes` 3.6 MB:
past the 128 KB L1, inside the M3 Pro's 16 MB L2, so each hop of a random access is an L2 hit (tens of cycles) that
the next hop waits for; past ~100 K rows they are DRAM misses. Then `ApplyBg` re-walks `nodes -> runs -> inst` and
adds `htab -> page -> clips` (A4).

Bounds checks that survived: `tape` (1), `info` (3: `1000f6ce8`, `1000f6d88`, `1000f6dec`), `specs` (1),
`nodes` (2), `runs` (1), `inst` (1) = 9 on this path. A runtime `assert_eq!(self.draw, li.kind == 1)`
(`1000f6db8: eor w10, w10, w9; tbnz`) checks what the description's type could carry (Part 2, F11).
`quad_instance` is recomputed to compare radius/kind/border (`fmul`, `frinta`, `fmul`: the 1/256 snap), so the
paint-only path pays a geometry recomputation.

Good: `m.pos += 1; m.kid += 1` is one `add.2d` on a `q` register pair (`1000f6f78`).

Minimal: with the item's records at known slots (F1) the box's instance and side state are loads at constant
offsets from the item's base; the colour compare is one load and one compare; ~25 instructions plus the mark.

## A3. `Para::update` (inlined into the application row's `LBox::update`, `0x1000f3988`)

Before the first `Para` is updated, the `(Para, Para)` children tuple (~240 B) is copied to the stack with
twenty `q`-register loads and stores (`1000f3c64..1000f3cd4`):

```
1000f3c64: ldp	q1, q0, [x23, #0xc0]
1000f3c68: stp	q1, q0, [sp, #0xf0]
1000f3c6c: ldp	q3, q0, [x23, #0x80]
  ... (20 instructions)
```

Then the chain tape -> `info[lid]` (52 B stride) -> `specs[ix]` (84 B compare) -> `info.rtn` -> `nodes[rtn]`
(120 B stride) -> `.text` -> `texts[ti]` (176 B stride) -> `String` heap pointer -> `memcmp`: **five dependent
loads before the bytes**, each with its bounds check (`1000f3dc0`, `1000f3ddc`, `1000f3ed8`). The size assert
costs two `fmul` + two `fcmp` per update (`1000f3df8..1000f3e14`). The integer label is formatted to decimal
again (the `umulh` loop at `1000f3e90`), then `memcmp` (`1000f3f10`), then a call to `ApplyTextColor`.
29 `panic_bounds_check` sites in the function.

Minimal: text identity as `(string id, style id)` in the record's side column (F8); "text changed" is one
compare of two `u32` (an interned or arena-offset id) or, for an integer source, of the integer.

## A4. `ApplyBg::apply` (`0x1000f1500`, 357 instructions, ~150 on the common path)

```
1000f151c: ldr	x1, [x0, #0x1e60]        ; nodes.len  -> bounds check
1000f1534: ldr	w21, [x9, #0x44]         ; nodes[node].color      (node)
1000f1544: ldrsb	w10, [x9, #0x72]       ; .kind (hover bit)      same line
1000f154c: ldr	w8, [x9, #0x28]          ; .run
1000f1564: ldr	w8, [x11, x8, lsl #2]    ; runs[run].first        (run)
1000f1584: umaddl	x8, w8, w12, x11       ; &inst[first]           (first)
1000f15d4: str	w11, [x8, #0x14]         ; the store this function exists for
1000f1600: ldr	x8, [x0, #0x1488]        ; htab.ptr
1000f160c: ldrb	w9, [x8, #0x10]          ; htab[h].live           (h, from nodes[node])
1000f1614: ldr	w9, [x8, #0xc]           ; .generation
1000f1620: ldr	x20, [x8]                ; .page                  -> 4 checks: len, live, gen, null
1000f162c: cmp	w19, #0x3f               ; slot <= 63 (a bounds check the HEntry invariant makes dead)
1000f1638: ldr	x1, [x8, #0x80]          ; page.ink[slot]         (page)
1000f163c: ldrb	w8, [x20, #0x581]        ; page.flags             (page)
1000f16b8: ldrh	w9, [x9, #0x500]         ; page.clip[slot]        (page)
1000f16d4: add	x9, x13, x9, lsl #3      ; &clips[clip]           (clip)
```

Chains: `nodes -> runs -> inst` (3) for the store; `nodes -> htab -> page -> clips` (4) for the damage. Then
`NRect` arithmetic in general-purpose registers:

```
1000f16e8: cmp	w10, w13                 ; intersect with the clip: 4 x (cmp, csel)
1000f16ec: csel	w10, w10, w13, gt
  ...
1000f1708: and	w9, w12, #0xffff         ; repack 4 lanes into a u64: and, lsl, orr, lsl, orr, bfxil
1000f170c: lsl	x9, x9, #32
1000f1710: orr	x9, x9, x8, lsl #48
1000f171c: bfxil	x9, x10, #0, #16
1000f1744: ldrsh	w12, [x0, #0x1570]     ; the window: the same 4 x (cmp, csel) + repack again
```

**Missed vectorisation:** each `intersect` is ~14 instructions where `fmov d0, x; smax.4h v0, v0, v1;
fmov x, d0` is 3. `NRect` is `[i16; 4]`: give its scalar-context ops an `#[inline(always)]` NEON body (or keep
rects in `d` registers end to end). The damage mark then does read-modify-write of `y0`, `y1` and `solid` in
memory (`1000f17d4: strb wzr, [x9, #0x808]`, `1000f17d8..1000f17f8`).

Minimal: instance at the record's slot (F1/F9): compare, store, append `(page, slot)` to the frame's paint
list; the damage of paint-only writes is the visible ink of those slots, computed per page at settle (F10).

## A5. `Rt::quad_geom` (`0x100018380`, ~95 instructions, then a tail call)

LLVM did well here: the 1/256 snap of four floats is two `fmul.2s` / `frinta.2s` / `fmul.2s`, and the
moved-edges test is one `fcmeq.4s` + `mvn` + `addp`:

```
100018398: mov.s	v0[1], v1[0]
1000183a4: fmul.2s	v0, v0, v1
1000183a8: frinta.2s	v0, v0
1000183f0: fcmeq.4s	v1, v1, v2
1000183f8: addp.2d	d2, v2
```

Chain: `nodes[node]` -> `runs[run].first` -> `inst[first]` (3), plus `spaces[space]` (an `[i32; 2]` converted
with `scvtf.2s`). Bounds checks: nodes, runs, inst, spaces (4). The ink is formed with `fcvtps.2s` /
`fcvtms.2s` and clamped with `smax`/`smin`; then

```
100018608: b	0x1000964c0 <set_ink_handle_edges>
```

which is the expensive part: **measured 573 instructions** for a one-pixel x move of a 944 x 30 row
(`opus_core_probe`, random handles over 10,000 records): `set_ink_at` without damage (**185**, measured) plus
`damage_edges` (one strip per moved edge, two here, each `clipped` + `ring.add`, plus the strip set-up: two
`canon`, the emptiness `match`, a four-entry strip array and its loop). The edge-band rule was introduced to save
GPU work (bands around the moved edges instead of old ∪ new); on the CPU it costs 2.4x the old ∪ new write (241,
measured). Both are right for their purpose; the cost belongs to per-record marking (A8, F7).

## A6. `Bough::locate` (inlined; `BlockedPrefix::locate`)

Two branchless binary searches (`csel` per step: good, no mispredicts), over block starts then over the block's
local starts. Each probe's address depends on the previous probe's result: a chain of `ceil(log2(P/32 + 1)) +
ceil(log2(min(P, 32)))` loads (P pages): 0 + 4 at 1 K records, 9 + 5 at 1 M. The core lane's table shows the cost
growing with it: `set-ink-pos` 421 -> 502 instructions from 1e3 to 1e6 against `set-ink-handle` flat at 387
(TABLE-R6, measured by lane core). The handle path avoids `locate` but pays `htab -> page -> page.dir`. Minimal: no
rank query on the hot paths at all (cursors during passes, F1); rank only at the API boundary.

## A7. `Bough::translate_impl` (lead `0x100094000`, 1,200 instructions)

Per whole plain page the lead's version spends ~109 scalar instructions (summary translate with range checks,
two `NRect` unions in general registers, the offset and its four `i16` lanes written one by one); lane `core`'s
`translate_pages` (branch `lane/core/round6`, `5d82a89d80`) brings it to 32 per page (their measurement:
translate-half at 1e6 849,441 -> 271,499). What remains per page: the summary `sqadd`, the offset add, the lane
negate/`uzp1`, `wgen`, two unions, and one more cost the instruction count hides: `(*pp).off`, `offl` and `wgen`
live in the page header, so every translated page is a read-for-ownership of one header line reached through
`d_ptr[d]` (1,000 pages: 1,000 lines). With the offsets as a directory column next to `d_ink` (8 B a page,
read by the same scans), a run translation is a contiguous NEON add over `d_off` and `d_ink` (two pages per
`q` register) that touches no page line: ~4 instructions a page, estimated, and the GPU reads the offset from
the column instead of `gpu_layout::OFFL`. Minimal for a translated *run of pages*: one add to a per-run offset that
the pages reference (a "range tag", which TABLE-R6's ideal-bound column already names); the summaries of a chunk
move by the same delta, so the chunk union is one more `sqadd` (F3 makes a run of pages a contiguous slot range,
so a range tag has somewhere to live).

## A8. `Bough::set_ink_*` (lead `set_ink_at` `0x10008b780`, 688 instructions; `set_ink_handle_edges`, 740)

**Measured** (`opus_core_probe`, 10,000 row records in 157 pages, random handles, a one-pixel move and back,
harness subtracted):

| path | instructions per write |
|---|---:|
| `set_ink_handle` (damage old + new) | 241 |
| `set_ink_handle_edges` (moved x edges, band 2) | 573 |
| `set_ink_handle_nodamage` | 185 |
| `touch_handle` | 94 |
| `set_inks_handles`, 64 a batch, with damage | **326** |
| `set_inks_handles`, 64 a batch, no damage | 173 |
| one damage mark, `Plane::add` of a 944 x 30 rect | 66 |

* **The batch API is dearer than single writes** (326 vs 241) for scattered handles: nearly every write dirties
  a page of its own, so the batch pays a per-page settle (a dirty-list push, `regroup`, the page union from its
  eight `gink`) on top of what a single write pays, and `settle_pages` falls back to per-chunk `fix_chunks` when
  the dirty pages span more than 128 directory entries. Batching pays only when writes share pages; the
  probe's consecutive-records case measured 297 per write in `set_inks_handles` (64 consecutive records a
  batch, ~18 of it the harness's batch building), still above the single write.
* **The non-damage part is 185 for one 8-byte store.** On the path: `canon`, `page_mut` (`wgen += 1`), `abs`
  (a `flags` test then the scalar translate), the equality test, `store_abs` (`translate_checked`), `regroup`
  (`union8` + an OR-fold of eight `u16` clip ids), `union_abs`, `d_ink[d]` store, `chunk_update`. When the move
  shrinks the page union, `chunk_update` falls to `fix_chunks(d, d+1)`, a `union_all` over 64 directory rects.
* **A mark is ~60 instructions however it is written**: the probe's minimal version (tile span, a `u128` column
  mask, one OR per row, y-range kept in registers) measured 58 against `Plane::add`'s 66; the variable `u128`
  shifts and the span arithmetic are the cost. So per-record marking has a floor near 60 per rect, two rects
  per write: that is the argument for F7 (mark motion per page, not per record).

## A9. The cull's inner loop (`core_bench`, `Bough::cull` `0x1000120c0`, 2,780 instructions)

Per admitted group of 8 records (bbox cull, `TILES = false`): `rbit`/`clz`, the `cgroups` byte test, four `ldp
q` loads of 8 inks, four `cmgt.8h`, three `uminp.8h`, `and`, `addv.8h`, the `COMPACT` row load, two `tbl.16b`,
one `stp q`, `cnt.8b`, `add`: **~30 instructions per group, ~4 per record**, no per-record branch.

```
100014ad4: sqsub.4h	v4, v8, v0           ; threshold into page coordinates (one op)
100014ae0: cmgt.8h	v0, v4, v0
100014af8: uminp.8h	v0, v0, v1
100014b08: addv.8h	h0, v0
100014b98: tbl.16b	v2, { v0, v1 }, v2     ; compaction, no branch per record
100014ba0: stp	q2, q0, [x23]
100014ba8: cnt.8b	v0, v0
```

Two small defects in the loop: the threshold `v4` is spilled once per page (`100014af0: str q4, [sp, #0x2d0]`)
and **reloaded on every group** (`100014b4c: ldr q4, [sp, #0x2d0]`) because the out-of-line `group_visible` call
on the clipped-group path clobbers `v0..v7`; and `cgroups` is reloaded per group (`ldrb w8, [x21, #0x580]`,
the output stores may alias the page). Fix: hoist the clipped-group path into a separate loop over the
`cgroups` mask (or make `group_visible` preserve the registers by inlining its common one-clip broadcast case).
The structural cost is elsewhere: TABLE-R6's `cull-1-dispersed` (1 % of tiles damaged, scattered) costs
17.5 M instructions at 1e6 for 10,000 outputs (1,750 per output, lane core measured): page unions are wide, so
scattered damage admits nearly every page. Spatial summaries cannot fix that (lane core's handoff says so
correctly); per-tile record lists (lane `gpu`'s 2D binning) or occlusion floors can.

## A10. `Bough::hit` (`core_bench` `0x1000112a0`, 898 instructions)

**Measured** 390 instructions per hit on the 10,000 overlapping rows fixture (`opus_core_probe`; TABLE-R6:
325–988 on lane core's fixtures). Chain: `c_ink` mask -> `d_ink` block (address: chunk index) -> `d_ptr[d]`
(address: page index) -> `page.gink` -> `page.ink[g]` -> `start_of(d)` (`blk[d/32] + local[d]`): five dependent
loads, the last two only to turn (page, slot) into a position the caller then turns back into a handle. Minimal:
return (page, slot) (or the handle in the page's column) and let the caller ask for a position only if it needs
one.

## A11. `lay_writeback` (`0x100019fc0`, 2,368 instructions)

Per visited node: pop a 16-byte `(u32, f32, f32, bool)` tuple, load `info[n]` (52 B stride; five flag bytes
tested one by one: `ldrb` x4 at `+0x2e..+0x31`), load `eng.n[n].layout` (152 B stride, `+0x70`), two adds,
four compares, then:

```
10001a2e4: ldr	x8, [x20, #0x3a8]        ; stats.nodes_visited += 1 (memory RMW in the loop)
10001a2e8: add	x8, x8, #0x1
10001a2ec: str	x8, [x20, #0x3a8]
10001a390: bl	0x100018380 <quad_geom>   ; -> set_ink_handle_edges (573, A5)
```

then writes back `ax/ay/aw/ah` and four flags into `info[n]`, and `children_into` (a `Vec` extend through the
`DViewRef` iterator) pushes the children. Per quad node the write-back is ~100 instructions of its own plus the
~670 of `quad_geom` + `set_ink_handle_edges`. Store lane measured write-back at ~3,500 instructions per row of
three elements cold. The `stats` counters and `prof.mark` (a `bough_metal::perf::snap` behind a runtime `on`
flag, 8 call sites) belong behind a cargo feature. Minimal (F1, F7, F14): the laid-out rects are a column in paint
order; write-back is a pass over that column that derives inks for 8 slots at a time and appends moved slots to
the frame's motion list.

## A12. A mount path: `LBox::mount` for the application row (`0x1000f2558`, 1,292 instructions)

14 `panic_bounds_check` sites, calls to `Deep::add` (x3), `Rt::alloc_node` (x3: a 120-byte `Node::blank()`
written whole, including an empty `Vec` for `series`), `HEntry::grow_one` (x3) and `RecordInit::grow_one` (x4)
(per-element `Vec` growth checks), `alloc_text` (x2), `fresh_ctx` (x2), `LeafCtx::set_text` (x2),
`TextSys::strike_in` (x2), `perf::snap` (x6, behind `prof.on`), `reflect.elem` (a `&'static str` per handle
into a `Vec`, 16 B per element). Store lane's profile of the 6,000-row mount attributes 47 % of `App::new` to
element mount, two fifths of that inside `realloc`, and ~10 % to spec hashing (`SpecTable::intern`,
`hash_one::<SpecKey>`, `memcmp`). Minimal: F6 (the item's mount image copied, a patch table applied).

## A12b. A note on the store branch's stage 0a (sibling links), read from its source

`lane/store/round6` `eb156f9c1f` replaces `DNode.kids: Vec<u32>` with `first`/`last`/`next` links inside the
152-byte `DNode`s. That removes a heap object per container (good, and measured by store), but it turns a
container's child iteration into a **dependent chain**: child `i+1`'s index is a field of child `i`'s node, so
a container of N children costs N chained loads at a 152-byte stride (the `Vec` gave N independent loads from
one contiguous array). While allocation order equals paint order the stride prefetcher hides it; after churn
(freed node indices reused out of order) it is a chain of misses. Not a defect today, a risk to measure: a
10,000-row column, churn 5,000 rows, then a width change, `lay_compute` instructions and wall time before and
after stage 0a. STORE-R6's end state (children are the depth column's direct children, found by the core's SIMD
depth scan) has no chain; stage 0a is a step toward it only if the links are dropped when the side pages land.

## A13. What the compiler would not do, in one list (for the Fable agents)

| # | where | defect | action |
|---|---|---|---|
| 1 | `update_dyn` | 330-byte item merged through `pending` (~110 moves per row) | split the hot loop (A1a, measured -35 % / -53 % / -48 %) |
| 2 | `SealCheck::take_word` | bounds check per word | one length check per fixed-shape item |
| 3 | `SealCheck::bytes` | `memcmp` call for 5..16 bytes | overlapping word compares (A1c; with A1d measured 420 -> 278 per owned row) |
| 4 | `int_text!` seal | decimal formatting to compare an integer | seal the integer (A1d, same commit as A1c) |
| 5 | `SpecTable::is` | bounds check + index multiply + 84 B per spec | spec bytes inline in the seal record, or a compact spec |
| 6 | `LBox::update`, `Para::update`, `ApplyBg` | 5–6 load chains through 52/120/12/48/176-byte strides, 9–29 bounds checks | position identity + side columns (F1) |
| 7 | `LBox::update` | children tuple copied to the stack (20 `q` ld/st) | the by-value move of `self.ch` into the children's `update`: destructure `self` once at the top, or inline the tuple's `update`, so the fields are read in place |
| 8 | `NRect` ops outside kernels | 4 x `cmp/csel` + 6-instruction repack per op | NEON bodies (`smax.4h`/`smin.4h`) for scalar contexts |
| 9 | `ring.add` | `y0`/`y1`/`solid` read-modify-written in memory per mark | keep a frame's mark bounds in a register-resident accumulator; flush once |
| 10 | `cull_impl` | threshold reloaded from the stack per group; `cgroups` reloaded per group | separate loop for clipped groups |
| 11 | `set_inks_handles` | dearer per write than `set_ink_handle` (326 vs 241) | settle per dirty page with the dirty-group mask, chunks once per dirty chunk |
| 12 | `lay_writeback`, mount | `stats` RMW and `prof` checks in hot loops | cargo feature |
| 13 | `Para::update` | size assert (2 `fmul` + 2 `fcmp`) per update | carry the font size in the type or check at mount only |
| 14 | `Rt::alloc_node` | 120-byte blank `Node` with a `Vec` written per element | F1/F9: no `Node` for quads |

---

# Part 2: formulations

Ranked by expected gain summed over many GUI situations (not one benchmark). Each: the rule it bends, the
representation change, where it pays and roughly how much (with the reasoning), its cost when unused, how it
could overfit and how to see that, and the cheapest experiment that would kill it.

Where the brief's questions land: addressing without an indirection table (F1, and F3 for paint order as slot
order with gaps), bit tricks in handles (F1's packed word), fixed point and derived inks (F14), one-word
"nothing here" tests (F4), text as ids until shown (F8), shared layout of identical subtrees (F5, of which
hash-consing is the degenerate case), descriptions compared and built as words (F2), mount as template plus
patches (F6), GPU-only state (F9), the type system (F11, F2's `MaybeUninit` record, F11's per-kind columns),
parallelism from disjoint pages (F13). Damage per page instead of per record (F7, F10) is the one not on the list
and the one the measurements pointed at.

## F1. Position is identity; handles are opt-in

**Rule bent:** "every element has a handle, every table is indexed by it". bbtree: no pointers a reader must
chase; Joffe: fold the offsets into one constant.

**Representation.** Inside the runtime a record is addressed by `(page, slot)` and every per-record fact is a
column at that slot (STORE-R6's side pages). Passes carry a *cursor*, not ids: the re-description walk already
knows the Bough position of every element in order (`m.pos`); it advances a `(page ptr, slot)` cursor instead,
and for a fixed-shape item the offset of each element inside the item is a compile-time constant
(`Element::NREC` summed by the tuple impls), so `update` reads `side[item_base + K]` with `K` a constant. The
tape shrinks to one entry per dynamic part. The handle table holds only records referenced from outside a pass:
hover/press/capture targets, focus, selection anchors, inspector and accessibility ids, wires (a signal must
find its record after splices), async readers. `attach_handle(pos)` already exists; it becomes the only way a
handle is born, lazily. The handles that remain are one `u64` each: page id (20 bits) | slot (6) | generation
(38), the page at `arena + id * size_of::<Page>()` (the pages are already elements of one carved column): 24 B
-> 8 B per entry, and A4's four checks (`len`, `live`, `generation`, null) plus the dead `slot <= 63` check
become one compare of the stored word's generation bits. (The probe's deferred-write variant uses a word of
this shape, 32-bit generation in the low half: `opus_core_probe`, F10.) Because the word names a slot, the few
handled records still need their word rewritten when the column plan moves them; a per-page "has handles" bit
lets every move of a page without one skip the look, which is the common case F1 creates.

**Pays in.** Every applier (colour, geometry, text, opacity, clip), every mount (no `new_handle`, no `HEntry`
growth, no `fixup_handles` after splits), every structural edit (`fixup_handles` walks the moved slots today),
memory. The chains of A2–A4 go from 5–6 dependent loads to 1–2 (the item base, then independent column loads).
Estimated: the changed row's `LBox::update` + `ApplyBg` from ~290 to ~70 instructions (A2 ~140 and A4 ~150 are path
lengths counted on the listings, not measured; after F1 ~25 and ~45, the remainder being the mark); mount per
element -300 to -500 (handle + node + info allocation and growth checks); memory -24 B/record (`HEntry`) -4 B
(tape) for records nobody references, -120 B (`Node`) and -52 B (`LInfo`) once their fields are columns.

**Cost when unused.** None: records that need handles have them, exactly as today.

**Overfit risk.** An app whose every record is wired or focusable would keep every handle. Detect: count, per
frame of the gui-bench port and the zgx scenes, the records whose handle is dereferenced outside
mount/update/write-back (instrument `handle_ptr` by call site).

**Kill experiment.** That count. If more than ~20 % of records need a handle across the ports, keep handles
mandatory and do only the side-column half (the chain shortening still holds).

## F2. Re-description as a fixed-stride word compare

**Rule bent:** "a seal is a byte stream walked operand by operand through the element tree". Joffe: widen the
representation until the corrections (bounds checks, indirections, formatting) disappear.

**Representation.** An element type whose shape is fixed (no dynamic part, no variable-length operand longer
than 16 bytes) declares `const SEAL_WORDS: usize`. Its seal is a `[u64; W]` record: spec bytes inline (or a
16-byte compact spec), font sizes, colours, integers as integers, short strings as two words plus a length;
a longer string spills its tail to the part's byte arena (and compares it only when the head matched). A part of
fixed-shape items keeps its seals as a column of `[u64; W]` with item `k` at `k * W`. Checking item `k` is:
write the new description's words into a stack record (the description is built anyway: 57 instructions,
measured), xor/or it with the retained record (`W/2` `ldp`, `eor`, `orr`, one `cbnz`), no bounds check inside.
The paint/shape distinction that edits by description need (a zebra stripe is paint, a text is shape) is a
per-type constant mask of paint words: `equal = (new ^ old) == 0`, `shape = ((new ^ old) & !PAINT) == 0`.

**Pays in.** Every turn of every re-describing owner: lists, tables, forms, menus, toolbars, inspector panes,
anything written as plain `view()` code. **Measured** (`opus_probe`, the application row, 1,000 rows): base build +
`SealCheck` 360 instructions per row (253–256 with A1c/A1d; 316 with 80-byte labels); the whole description
including the three specs inline as a 288-byte record + xor **135–159** (variant (d), the upper figure with the
long-label tail check compiled in; 206 with 80-byte labels); the dynamic operands alone 85–95 (variant (c), a lower
bound: it skips the constant operands). The whole per-row turn measured 645 at base, 278 with A1a/A1c/A1d;
**estimated** with F2: ~180 per row with specs inline, ~120 with a 16-byte compact spec: 3.5–5x below base,
1.5–2.3x below the experiments. Most of the 645 -> 278 came from A1a, not from the seal's form; F2's larger value
is what it enables (seal runs as one compare, below). With owners-as-blocks (lane surface-api) the per-turn cost
becomes O(changed owners), and F2 is what each owner pays.

**Seal runs as one compare.** When every item of a part is unchanged, the new seal column equals the retained
column; build the new column first (one pass of the user's closure) and compare the two columns with one
streaming NEON compare (FOUNDATION §3's "seal runs, not items"), the first mismatching record giving the first
changed item. This is a layout of F2, not a second mechanism.

**Cost when unused.** Items with a dynamic part or a refusing operand keep today's byte seal (or none).

**Overfit risk.** Rows with long strings (descriptions, paths, URLs) spill to the arena; the check is then two
words plus a tail compare. Measured with `PAD=80 opus_probe`: record 206 vs `SealCheck` 316, so the form holds
for long strings; a row of several long strings is the case to watch.

**Kill experiment.** `opus_probe` variant (d) (135–159 measured) bounds the check; the killing one is the same
record column inside `update_dyn` for `Table<0>`: kill if the per-row turn does not fall below 200 instructions
(today 278 with the three experiments).

## F3. The directory is a packed-memory array: slots with gaps, paint order is the slot order

**Rule bent:** "the directory is dense; a position is a rank". bbtree: type-stable power-of-two storage that never
reallocates, sentinel padding, searches at full capacity.

**Representation.** Directory columns (`d_ptr`, `d_id`, `d_ink`, `d_len`, `d_min_depth`) are chunks of 64 slots,
each at most 48 live (16 gaps; sentinel `EMPTY` ink, length 0, depth `0xFFFF`). A page insert shifts inside its
chunk (at most 63 entries, and only the moved pages' `dir` fields are rewritten); a full chunk is rebalanced
with its neighbours over a window that doubles until the window is under its density threshold (the PMA rule:
amortised O(log² P) slot moves per page insert, each moved page's `dir` rewritten), and the arrays double only
when the whole directory is over threshold. Inserting a fresh chunk in the middle instead would shift every
later slot, which is today's cost divided by 16, not removed. Record counts live per chunk
(`cstart`, one NEON add over the chunks after the edit) plus the in-chunk lengths. `locate` is unchanged in
shape: "last slot whose start <= pos" skips zero-length slots by construction. Every SIMD scan is unchanged:
sentinel slots fail every `lt_mask` test, so there is still no tail mask. Paint order of two pages is the order
of their slot numbers, so an occlusion floor's "is this record after that one" (FOUNDATION §2) is an integer
compare with no rank query.

**Pays in.** Every structural edit: list insert/remove anywhere, expand/collapse in trees and outlines, drag
reorder, chat or log append at the head, dormant runs being realised (FOUNDATION §1 splits a run: a page
insert), undo of any of these. **Measured prototype** (`opus_core_probe`, the directory part only: a page insert
then its removal at the front): 547 / 944 / 963 / 1,162 instructions at 1e3 / 1e4 / 1e5 / 1e6 records. TABLE-R6
(lane core, measured, warm): `insert8-front` 3,961 / 8,878 / 58,698 / 556,844. Since the 1e3 cell is mostly not
directory work, **estimated** insert8-front at 1e6 ≈ 4,000 + 1,200 + amortised rebalancing (O(log² P): ~200
slot moves at 15,625 pages, ~2,000 instructions) ≈ 7,000: about 80x, and the exponent column goes from 0.98 to
~0.15. `move64-*` and `remove64-*` (0.93–0.98 exponents today) follow.

**Cost when unused.** Scans read up to 64/48 = 1.33x the directory slots (the directory is 26 B per page: at
1e6 records 0.4 MB -> 0.54 MB); every full-capacity column test is unchanged. Bulk build fills chunks to 48.

**Overfit risk.** Adversarial insert positions (always the same slot) trigger ever-wider rebalancing windows; the
PMA bound holds but the constant matters. Detect: TABLE-R6's insert cells at every N plus a hot-spot insert of 1e6
pages one at a time (max and mean per op), and the cull cells with 25 % gaps (must stay within 1.05x).

**Kill experiment.** Replace `dir_insert_blank`/`dir_remove*`/`reindex_pages` with the gapped form behind a
feature, run TABLE-R6's structure and spatial cells. Kill if any spatial cell regresses past 1.05x or the
1e6 insert cells do not fall below 10,000.

## F4. Summary words: one word per page and per chunk answers every "is anything here ..." question

**Rule bent:** "each question has its own structure" (cgroups, flags, F_STATE, wgen, dirty, the state planes'
`any`). bbtree: put what a step needs in the lines that step already loads.

**Representation.** One `u64` per directory slot and its OR per chunk: bits for {written since publish, damaged
since epoch e (per ring slot), transformed, far/wide, any clip, any dormant run, any text, any hit-able record,
any focusable, fully occluded (from the tile floors), state plane f non-empty (16 bits, COLUMN-PLAN)}. Every pass
starts with `chunk_word & want`; inside a chunk, `lt_mask`-style compare of the 64 page words; inside a page, the
existing column scans. A writer that sets a page bit ORs the chunk's word too (one more store in a line it
touches). Relation to COLUMN-PLAN's state planes: their per-page `any` and `changed` words move into this
directory word (16 + 16 bits of a `u64`, or a second `u64` column), so "is anything on plane f here" never
loads the state page, and the chunk OR is the "summaries upward" COLUMN-PLAN defers to directory nodes.

**Pays in.** `freeze_into` (the snapshot a render, inspector or accessibility thread would read each frame; no
façade code calls it yet, so this pays when such a reader lands): TABLE-R6
(lane core, measured) `freeze-idle` 472,435 instructions at 1e6 and 48,148 at 1e5, scanning every page;
with a written-since-publish chunk word it is O(chunks) + O(changed pages): **estimated** ~500 at 1e6 idle
(~1000x) and the changed pages' copies otherwise, provided the directory columns, which `freeze_into` copies
whole today (~0.4 MB at 1e6), are published the same way (a chunk's directory slots copied only when the chunk's
word says its slots changed). Also: focus-within, "any selected in this subtree", selection
counts, next search hit, tab order (`next_state`), hit skipping no-hit pages, write-back skipping untouched
pages, `changed_since`, realisation scans for dormant runs.

**Cost when unused.** One OR per page-level write (the page bit) plus one per chunk; a directory column of
8 B per page (at 1e6: 125 KB).

**Overfit risk.** Bits that are set and never cleared (a page that once held a text stays "any text") make the
summary useless; every bit needs a clear rule at the operation that can clear it. Detect: a randomized test that
recomputes every summary from the records after each operation (the oracle exists).

**Kill experiment.** `freeze-idle` and `freeze-one` with the chunk word: kill if freeze-idle at 1e6 stays above
5,000 instructions.

## F5. Layout of repeated templates as a replayed trace

**Rule bent:** "layout runs the flex algorithm per node". Joffe: the value was there all along; the trace of the
first item is the program for the rest.

**Representation.** The items of a part share a type, so their style ids are usually identical. The first item is
laid out by cephal while recording its float operations and branch outcomes (a trace: `x2 = x1 + w1 + gap`,
`clamp(w, min, max)` with the branch taken, `max(content, min)`); each following item whose leaf measures fall
inside the recorded guards is laid out by replaying the trace: straight-line code, the same IEEE operations in
the same order, so **bit-identical** to cephal (exactness by construction, checked by the existing pixel gates
and a differential test against cephal on every replayed item in debug). Items whose guards fail fall back to
cephal and may record a second trace. Four items per NEON operation is possible because the trace is the same
program on different data. Hash-consing of structurally identical subtrees is the degenerate case (identical
leaf measures give identical outputs: a memo keyed by the measures); the trace is the same idea made parametric,
so rows whose labels differ in width still share it.

**Pays in.** Mount and resize of every list, table, grid, menu, tree, card wall, chat log; any `each`. Store lane
measured cephal's cold pass at ~9,000 instructions per application row (35 % of `App::new` at 6,000 rows).
**Estimated** replay: ~30–60 float ops and guards for a 3-node row, ~150 instructions with loads and stores;
50–60x on the cold pass for rows whose guards hold, which in a fixed-width row layout is every row. A width
change of the window (resize) re-lays every row: the same 50x on the resize path.

**Cost when unused.** One trace recorder per part, active for one item; nothing on non-repeated content.

**Overfit risk.** Wrapping text or content-sized columns make leaf widths differ, which is fine (the trace is
parametric in the leaf measures) until a guard flips (a clamp starts binding). Detect: guard hit rate per part
over gui-bench and the zgx scenes; below ~80 % the recording is wasted.

**Kill experiment.** Hand-write the trace for `redescribe_cost`'s row (row of fixed height, label `w(64)`,
label content-sized) as a Rust function, run it over 10,000 rows with varied label widths, compare the rects
bit for bit with cephal's, count instructions. Kill if not bit-identical or above 500 per row.

## F6. Mount as a copy of the first item's image plus a patch table

**Rule bent:** "mount constructs each element". Joffe: fold everything that does not depend on the item into one
constant (here, a block of bytes).

**Representation.** When a part mounts, its first item's writes are recorded as an image: core records (depth
relative to the part), side-column values (F1), instance bytes, style ids, seal record (F2); plus a patch table
of the offsets that depend on the item's operands (colour, text id, integer, the instance's position from F5).
Items 2..n: one bulk copy of the image into consecutive slots (pages sized from the image; COLUMN-PLAN's
`insert_run_with` takes them), the patches applied, text measured (F8), layout by trace (F5). Independent pages
can be filled by different threads (F13).

**Pays in.** First frame of anything with a list; navigation to a new screen; "load more"; dormant run
realisation during a scroll (FOUNDATION §1: realisation is a mount, so this is the scroll path of every long
list once virtualisation is automatic). Store lane measured 24,516–26,715 instructions per application row; its
profile puts shaping at 1.2 % and `TextSys::select` at 2.1 %, so the floor is low. **Estimated** 1,500–2,500 per
row (10–15x), dominated by text measurement and the patch application.

**Cost when unused.** One image per part (a few hundred bytes), recorded once.

**Overfit risk.** Heterogeneous items (a feed of different card types) get no image reuse unless images are
keyed by item type; keyed by type, the first of each type records. Detect: images recorded vs items mounted per
part in the ports.

**Kill experiment.** In `redescribe_cost MOUNT=1`, time a `memcpy` of a three-record image (core records +
`Node`/`LInfo`/`Instance` bytes) plus the patches against `mount_item`: if the copy-and-patch, without layout and
text, is not below 1,000 instructions per row, the idea is not worth the plumbing.

## F7. Damage from motion certificates, not per-record marks

**Rule bent:** "a moved record marks its old and new ink". Joffe: widen to delete corrections; here, coarsen the
unit of motion to delete per-record marks.

**Representation.** Writes go to the ink column and append `(page, slot)` to the frame's write list (F10's
settle). At settle, a page whose written slots all moved by the same `(dx, dy)` and whose unwritten slots did
not (a reflow below an insert, a scroll inside a page, a slide) marks `union(old) ∪ union(new)` of the moved
slots: two marks per page; when the page's content is otherwise unchanged and the motion is a copy, it records a
per-page copy (scroll by copy generalised from one clip per epoch to one motion per page, FOUNDATION §4).

**Pays in.** Scrolling content that is not a `ScrollBox` run, list reflow after insert/remove/expand, drag
reorder, animated layout (accordion, sidebar), window resize reflow. **Measured basis:** a mark costs ~60
instructions (A8), and a moved quad's write through `quad_geom` is 573 instructions on the edge-band path, ~390
of it marking (A5, A8). A page of 64 rows shifted by an insert above them therefore spends ~25,000 instructions
on marks today against two page-union marks (~120): **estimated** two orders of magnitude on the marking of
coherent motion, and fewer, larger damage rects for the GPU (the tiles are the same: rows tile the page's
union).

**Cost when unused.** One compare per written slot at settle (does its delta equal the page's first delta).

**Overfit risk.** Sparse records in a page (absolutely positioned items) make `union(old) ∪ union(new)` cover
pixels nothing occupied: more GPU work, same pixels. Detect: damaged tiles per frame against per-record marking
on the zgx scenes (must stay within 1.1x).

**Kill experiment.** In `opus_core_probe`, 64 consecutive rows moved by one pixel: per-record marks vs the page
union; then the same over the lab traces' damaged-tile counts.

## F8. A label is `(string id, style id)` until it is shown

**Rule bent:** "a label owns a shaped paragraph from mount". FOUNDATION's relevance: work proportional to what
is shown.

**Representation.** The record's text column holds an offset into a per-window byte arena (or an interned id)
and a style id: 8 bytes. `LeafCtx` (392 B), `TextNode` (234 B) and the glyph block are materialised when the
cull admits the record (it is exposed) or when layout needs a measure that the advance table cannot certify.
Width for layout: a per-strike advance table summed over the string, valid when every glyph of the string is
certified context-free in that font (no kerning pair, ligature or mark rule touches it: a per-font bitset built
once from GPOS/GSUB coverage); otherwise shape (exact either way, since the certificate says the shaper would
produce exactly the sum).

**Pays in.** Every long list, table, log, chat, file browser, autocomplete popup: store lane's ledger has
`lay.eng.ctxs` 1,070 B/row and `rt.texts` 594 B/row at 6,000 rows (measured by them), a row holding two labels;
**estimated** text memory per unshown row 1,664 B -> 16 B (8 B per label). Mount cost of unshown rows loses
`fresh_ctx` + `set_text` (A12).

**Cost when unused.** For shown text, one more indirection at materialisation time, once.

**Overfit risk.** Latin fonts kern common pairs ("To", "AV", "r."), so certification may fail for most strings
in some fonts. Detect: fraction of certified labels over the ports with Arial, SF Pro and the zgx font.

**Kill experiment.** Count certifiable strings in the gui-bench and zgx label corpora for those fonts. Kill the
advance-table half if below 50 %; keep the lazy materialisation half regardless (it needs no certificate when
the width is fixed by the layout).

## F9. State that the GPU reads lives only where the GPU reads it

**Rule bent:** "the CPU keeps its own copy to compare against". Joffe: the value was there all along.

**Representation.** `Node.color`, `Node.{x,y,w,h}`, `Node.radius` mirror `Instance` fields; the instances are
in the carved shared region (`rt.inst`, `Col<Instance>`), which on Apple silicon is ordinary cacheable memory.
Compare against the instance; keep the opacity operand's alpha in its own column and let the shader multiply it
(one multiply per quad on the GPU) so the stored colour is the description's colour. With F1 the instance is at
the record's slot and `Node` has no quad fields left.

**Pays in.** Every paint applier (A4 loses `nodes -> runs`), memory: store lane's ledger has `rt.nodes` at
655 B/row (120 B `Node` x 3 plus slack): **estimated** -400 to -600 B/row, and mount writes one structure fewer.

**Cost when unused.** One GPU multiply per quad.

**Overfit risk.** Write-combined mappings (on other platforms, `platform-host`'s Vulkan) make CPU reads slow.
Detect: keep the read behind the backend's `CPU_READABLE` constant; on a write-combined backend the mirror comes
back.

**Kill experiment.** Read-back cost of `inst[i].color` from the carved region vs a heap array, 10 K random reads
(`ri_cycles`); kill if more than 1.2x.

## F10. Writes are stores; summaries and damage settle once per dirty page per frame

**Rule bent:** "a write keeps every summary exact before it returns". The same rule bbtree bends with D37
(record the node stack only when a split needs it).

**Representation.** `set_ink*` stores the ink and ORs the group bit in `page.dirty` (both in the page's own
lines), appending the page to the frame's dirty list on its first write; settle (before cull or snapshot)
regroups dirty groups, recomputes page and chunk unions once, and marks damage from the logged `(old, new)`
pairs, or from per-page motion (F7). Readers inside a frame (hit tests) see the presented frame's summaries,
which is what the user is pointing at.

**Measured** (`opus_core_probe`, random handles, a one-pixel move and back within a frame of 64 writes): the
logged form costs 75.6 instructions per write without marks (against 185 for today's `set_ink_handle_nodamage`:
2.4x) and 199 with per-record marks (against 241: 1.2x; against 573 for the edge-band path: 2.9x). The version
that snapshots a page's inks on its first write and marks only slots whose ink differs at settle measured 145:
writes that undo themselves within a frame (hover in and out, a layout that settles back) cost no damage.
**So F10 alone is not an order of magnitude**; it is the enabler of F7 and of F13 (settle is a per-page pure
function), and it removes 110 instructions from every write.

**Cost when unused.** A dirty list per frame.

**Overfit risk.** Reads of exact summaries between writes inside one turn (a handler that writes then
hit-tests): must read the settled state or settle on demand. Detect: a debug assertion that no summary is read
while the dirty list is non-empty, over the suites.

**Kill experiment.** Already partly done (the probe). The deciding one: `trace_bench`'s lab traces with settle
per frame: kill if the total is not below today's by 1.5x.

## F11. The description's type carries its shape: associated constants, not runtime checks

**Rule bent:** "the runtime discovers the shape". bbtree D47: plans folded at monomorphisation, derived not passed.

**Representation.** `Element` gains `const NREC: Option<u32>`, `const NKID: Option<u32>`, `const SEAL_WORDS:
Option<u32>` (summed by the tuple impls; `None` once anything dynamic appears) and `LBox`'s paint flag becomes a
type parameter set by `.bg()` / `.rounded()` / `.border()` (the builder already changes `Bg`'s type, so the
API does not change). Consequences: `DynItem.nrec/nkid` and the per-element tape vanish for fixed shapes
(item `k` starts at `base + k * NREC`); `assert_eq!(self.draw, li.kind == 1)` and the `LInfo.kind` load vanish;
splices of fixed-shape items are arithmetic; the seal length check is one compare (F2).

The same move for storage: `Node` is one 120-byte struct for every kind ("cold fields are small"), so a quad
carries text and series fields and a label carries quad geometry; per-kind columns (a quad's paint, a label's
text id) replace it, and F2's stack record is a `MaybeUninit<[u64; W]>` written whole, never zeroed.

**Pays in.** Every re-description and every dynamic part edit (no `DynItem` table walk to sum `nrec`s); memory:
`dyns.parts` 343 B/row (store ledger) -> one column of nothing for fixed shapes.

**Cost when unused.** None (consts are `None`).

**Overfit risk.** Few: it is type information that exists.

**Kill experiment.** Count the fraction of `each`/`Vec` parts in the ports whose item type is fixed-shape; below
50 % it is still correct, just less useful.

## F12. Dynamic items as columns; scopes released by the column plan

**Rule bent:** "each item owns a `Scope` of seven `Vec`s and a tape `Vec`".

**Representation.** With F1 and F11, an item's resources are columns of its records (released when the column
plan clears the slots: COLUMN-PLAN's `clear`), so `Scope` per item and `DynItem` (216 B) disappear for
fixed-shape items; dynamic items keep a compact entry `(nrec, nkid, seal offset)` in a column.

**Pays in.** Every `each`/`Vec` part: store lane's ledger has `dyns.parts` at 343 B/row; **estimated** -330 B/row,
and `release_scope` per removed item becomes a slot-range clear.

**Cost when unused.** None.

**Overfit risk / kill.** Items that own non-column resources (images, async readers) keep a scope; count them.

## F13. Parallelism over disjoint pages, with a measured grain

**Rule bent:** "the UI thread does the work". bbtree's batch lanes: hide latency by interleaving independent
work; pages are the independent unit.

**Representation.** Settle (F10), seal-column compares (F2), mount images (F6), trace replay (F5), write-back and
snapshot copies are functions of disjoint page ranges. A persistent pool with one spin-then-park barrier per
frame; work split by page ranges; below the grain the same code runs on one range on the calling thread.

**Pays in.** Cold start and navigation (10 K rows: store lane's ~250 M instructions today, ~25 M after F5/F6),
jump-scroll realisation, resize of large documents, snapshots of large changes. **Estimated** 4–5x wall on six
P-cores above the grain; nothing below it (a frame's redescribe of 1 K rows after F2 is ~100 K instructions,
~25 µs: under a wake-up's cost).

**Cost when unused.** One branch on the work size.

**Overfit risk.** Benchmarks with huge cold mounts reward it; interactive frames do not. Detect: the grain
measured on this machine, and the frame-time cells (which must not move).

**Kill experiment.** Fork-join round-trip of the pool (wall time, under the measurement lock, a few seconds): if
above 50 µs, raise the grain to where it pays or drop it.

## F14. Geometry in `i32` fixed point (1/256 px), inks derived per page

**Rule bent:** "geometry is `f32` snapped to a 1/256 grid". Joffe: widen to delete corrections.

**Representation.** The laid-out box is `[i32; 4]` in 1/256 px, space-local; inks are derived per page in a
column kernel (`(x - m) >> 8` and `(x + w + m + 255) >> 8`, plus the space offset); a subtree translation is an
integer add with no re-snap; the five copies of geometry (`LInfo.ax..`, `Node.x..`, `Instance.pos/size`,
cephal's `Layout`, the core ink) become one column and derived values.

**Measured, and not a CPU win by itself:** ink from one box, `f32` floor/ceil 23 instructions, `i32` fixed point
27 (the `NRect::new` clamp dominates both); a 64-slot column in fixed point 14.2 per slot (`opus_core_probe`).
**What it buys is exactness and memory:** an `f32` holds the 1/256 grid exactly only below 65,536 px (2^24 /
256), so a long document's or a big canvas's coordinates leave the grid today; `i32` 1/256 is exact to
±8,388,607 px. Memory: **estimated** -40 to -60 B per element once the copies go.

**Cost when unused.** None after the switch; the switch itself touches every geometry writer.

**Overfit risk.** cephal computes in `f32`; its outputs are converted (exactly, if they are on the grid, which
`snap` asserts today). Detect: the existing exactness gates plus a 100,000 px document test.

**Kill experiment.** A document 200,000 px tall: compare snapped `f32` positions with `i32` fixed point at
y > 65,536; if `f32` still lands on the grid there, the exactness argument is void (it should not).

## What the experiments did not support (so nobody builds on it)

* **Fixed-point inks are not cheaper to derive one at a time** (27 instructions against `f32`'s 23; F14 stands on
  exactness and memory only).
* **A hand-minimal damage mark is barely cheaper** (58 against `Plane::add`'s 66): marking cost is per rect, not
  per implementation; only marking fewer rects (F7) wins.
* **Deferring summaries alone is 1.2x, not 10x, once marks are counted** (199 against 241; F10 is an enabler).
* **The fixed-stride seal record is ~1.6x over the already-improved `SealCheck`** (159 against 256), less than the
  2.7x it shows against the base check: most of the re-description win was A1a (the copies), not the seal's form.
* **A1c, on top of A1a, did nothing for the static row's 5- and 15-byte labels** (292 -> 300 per row; A1d does not
  apply there): the owned row's gain (420 -> 278) comes from A1d and from A1c on its 11–13-byte labels, not
  separated.

---

## Ranking, in one table

| rank | formulation | where it pays | gain (measured / estimated) |
|---:|---|---|---|
| 1 | F1 position identity | every applier, mount, structural edit, memory | est. chain 6 -> 1–2 loads; changed-row update ~290 -> ~70; -200 B/element with F9 |
| 2 | F2 seal records | every re-description | measured 645 -> 278 with A1a/A1c/A1d; probe 159 vs 256 (13 B labels), 206 vs 316 (80 B); est. ~120–180 per row |
| 3 | F3 gapped directory | every structural edit at scale | measured directory part 1,162 at 1e6 (no rebalancing) vs 556,844 total today; est. ~80x on insert at 1e6 |
| 4 | F4 summary words | state queries, pass skipping, snapshots (once a replica reader exists) | est. freeze-idle 1e6 472,435 -> ~500; any-state queries one AND per chunk |
| 5 | F5 trace-replayed layout | mount, resize of repeated content | est. cold layout ~9,000 -> ~150 per row |
| 6 | F6 mount images | first frame, navigation, realisation | est. ~25,000 -> 1,500–2,500 per row |
| 7 | F7 motion certificates | scroll, reflow, drag, animation | est. ~25,000 -> ~120 marking instructions per moved page |
| 8 | F8 lazy text | long lists' memory and mount | est. 1,664 -> 16 B per unshown two-label row |
| 9 | F9 GPU-only state | paint appliers, memory | est. -400 to -600 B/row |
| 10 | F10 deferred settle | every write | measured 185 -> 76 without marks; 241 -> 199 with |
| 11 | F11 type-level shape | re-description, part edits | enabler of F1/F2 |
| 12 | F12 items as columns | every dynamic part | est. -330 B/row |
| 13 | F13 parallel pages | cold work above the grain | est. 4–5x wall above the grain, 0 below |
| 14 | F14 fixed point | exactness beyond 65,536 px, memory | measured no CPU win (27 vs 23 per ink) |

## Suggested order (smallest experiment that decides the most, first)

1. **store:** cherry-pick A1a (20 lines) and A1c/A1d (measured: 645 -> 278 per row for the application row);
   then F2's word record for fixed-shape items, killed if the application row's turn does not fall below 200.
2. **core:** F3 behind a feature against every TABLE-R6 cell (kill: any spatial cell > 1.05x, or the 1e6
   insert cells not below 10,000); F4's written-since-publish chunk word on `freeze_into` (kill: freeze-idle at
   1e6 above 5,000).
3. **store:** the handle census of F1 (one instrumented run of the ports) before stage 1 of STORE-R6, so the
   one-index-space stage is built knowing how many handles survive; F9 and F11 ride on the same stages.
4. **lead or store:** F5's hand-written trace for one row type (an afternoon) before any trace recorder.
5. **core:** F10's settle with F7's per-page motion test, on the lab traces' damaged-tile counts.

## Verification (what was run on this branch, what was not)

Commits on `opus/tree` (local, not pushed): `0d3bccf1d4` the A1a experiment (`crates/bough/src/describe.rs`),
`9808e87086` the A1c/A1d experiment (`crates/bough/src/seal.rs`, `crates/bough/src/elem.rs`), `a717bddb2a` the two
probes, `6c1c46bdcc` the probe's long-label variant, `ce922c90d7` A1d's negative control (`Sealer::int`, feature
`sabotage_seal_int`, a test in `tests/seals.rs`), then this file. The three source files keep their existing
(not rustfmt-default) style: rustfmt would rewrite each whole file; the two new probe files are formatted (`rustfmt
--edition 2024 --config skip_children=true,max_width=120`).

Measured, all on this M3 Pro under the round's ordinary load (instruction counts on the calling thread; no wall
time or GPU time was taken, so no measurement lock was needed):

* `redescribe_cost 100 1000`, `NOCHANGE=1 redescribe_cost 1000`, `CHURN=1 redescribe_cost 100 1000`,
  `MOUNT=1 redescribe_cost 100 1000`, at base (experiments stashed) and with the experiments: the A1 table.
  The A1a-only column was measured once, before A1c/A1d existed.
* `opus_probe` (`cargo run --release -p bough --example opus_probe`): (a) 57.0, (b) 359.6 at base and 252.9 with
  A1c/A1d, (c) 84.6, (d) 134.6 instructions per row; (a), (c), (d) identical in both builds. After the long-label
  tail check was added to (c) and (d) (the committed probe): 13-byte labels (b) 256.0, (c) 94.6, (d) 158.6;
  `PAD=80` (80-byte labels) (b) 316.0, (c) 147.0, (d) 206.0.
* `opus_core_probe` (`cargo run --release -p bough-core --example opus_core_probe`), run three times, identical but
  for `hit` (390.0 / 391.4) and v2 without marks (75.6 / 75.7): the A8 table, the F10 variants (v1 145.3, v2 75.6
  without marks and 198.9 with, consecutive records 180.3 vs `set_inks_handles` 297.2), the F3 directory prototype
  (547 / 944 / 963 / 1,162), the marks (66 vs 58) and the ink derivation (23.0 / 27.0 / 14.2). The prototypes are
  models of the formulations, not implementations: the gapped directory omits the window rebalancing and the
  in-chunk length prefix; the deferred writes omit clips, transformed pages and the chunk unions.
* Disassembly read: `update_dyn` (Table<0>, <1>, <2>), `LBox::update` (Table<0>, <2>), `ApplyBg::apply`,
  `Rt::quad_geom`, `lay_writeback_ready`, `LBox::mount` (Table<0>), `Bough::{set_ink_at, set_ink_handle_edges,
  translate_impl}` (redescribe_cost); `Bough::{cull, hit}` (core_bench). Path lengths quoted as "counted" are
  static counts along the listing, not measurements.

Tests: `cargo test --release -p bough` on `ce922c90d7` (every source commit): 14 test binaries, 32 tests, all pass
(including `tests/seals.rs`, the seal gate against a fresh mount at every ring age, and `tests/describe.rs`; the
same suite also passed on `6c1c46bdcc`, before the mutant commit). `cargo test --release -p bough-core` on
`6c1c46bdcc`: 12 binaries, 36 tests, all pass (no core source changed after it). `cargo check -p bough -p
bough-core --all-targets` on `ce922c90d7`: clean.

Seal gate and mutants after `ce922c90d7`: `cargo test --release -p bough --test seals` passes;
`BOUGH_SEAL_MUTANT=1` (text) and `BOUGH_SEAL_MUTANT_INT=1 --features sabotage_seal_int` (integers) are each
caught, at `relabel 5` and `renumber 2`; the feature on with the flag off passes the gate. The workspace check
(`cargo check --workspace --all-targets`) fails, independently of these commits, in
`comparisons/bough-app-sketch-code/src/sketch/mod.rs:1039` (`sp.abs = Some(..)`: `Spec`'s absolute inset became
private fields behind an `abs()` method in `bda4a0a98d`, "Spec as plain bytes"); none of these commits touches
`Spec`. Reported, not fixed (not this branch's file).

Not done: the comparison suites' pixel gates (`bough-suite`, `bough-heavy`, `bough-scenes`, ...), no STATUS cell,
no wall or GPU time, no replay by another session; TABLE-R6 cells are lane core's and were not re-run; store's
BASELINE ledger bytes are lane store's. Formulation gains marked "estimated" are arithmetic on the cited numbers,
not results.

