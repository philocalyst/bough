# bough-text: results

Two iterations. **Iteration 2** (this section) restructures the whole text update path, value change -> shaped ->
GPU-visible instances -> damage, following `research/04-reformulation-method.md`. **Iteration 1** (the Sonnet
prototype: the table shaper, its exactness gate, the 48-byte instance writer) is kept unchanged below as the
"before".

Machine and method as before: Apple M3 Pro, `nightly-2026-10-17`, single thread, `release` profile. Instructions
(`proc_pid_rusage` RUSAGE_INFO_V4 `ri_instructions`, counter overhead subtracted, best of 7) are the metric; other
agents kept the load average at 45-65 throughout, so ns are indicative only.

    export CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/text
    cargo test --release                          # every gate below (about 60 s)
    cargo run --release --example pipeline        # every iteration-2 number in this section
    cargo run --release --example gpu_shape       # Attempt 3 (Metal compute, readback, cost)
    cargo run --release --example explore         # position classes, ligature kerning facts
    cargo run --release --example bench           # iteration 1's tables (cosmic-text, rustybuzz)

## Iteration 2 headline

Instructions per item. cosmic-text and rustybuzz numbers are iteration 1's (same corpus and harness). "Slots" =
the new GPU-visible output (6 bytes per glyph, below); "+ tile marks" includes the damage output at the damage
lab's granularity.

| workload | cosmic-text (zgx path) | it. 1: shape only | it. 1: shape + 48 B instances | **it. 2: into slots** | **it. 2: + tile marks** |
|---|---:|---:|---:|---:|---:|
| numeric label change (heavy, consecutive ticks) | 9,853 | 196 | 367 (fused) / 463 | **68** | **133** |
| same, from the `f32` value (formatting included) | - | 196 + 757 (`write!`) | 367 + 757 | **92 + 68** | **236** |
| stream line (lab, 105 ch), as a new string | 51,811 | 940 | 3,628 / 3,732 | **1,098** | **1,193** |
| stream line, retained stream (per visible line per tick) | 51,811 | 940 | 3,628 | **267** | (included) |
| log line (heavy, 84.5 ch), as a new string | 41,488 | 877 | 2,922 / 3,134 | **1,052** | **1,148** |
| log pane tick, retained ring of 5 lines | 5 x 41,488 | 4,385 | 14,610 | **1,444** | (included) |
| vlist paragraph (399 ch, wrapped into lines) | 188,618 | 9,169 | 19,630 | **7,557** | **8,058** |
| **one heavy tick: 84 numeric labels** | **891 K** | 16.5 K | 30.8 K (fused) / 38.9 K | **5.7 K** | **11.1 K** |
| one heavy tick from values (formatting included) | - | 80 K (`write!` + shape) | 94 K | **13.4 K** | **19.8 K** |

Per heavy tick that is **80x fewer instructions than cosmic-text's shaping alone** with the instance rewrite *and*
the damage included (11.1 K vs 891 K), 2.8x fewer than iteration 1's fused shaper (which wrote 48-byte instances
and produced no damage at all). Formatting the 84 values with `core::fmt` costs more than everything else
together (757 per label with a reused `String`, 1,249 with `format!`); rendering them straight into the label's
16-byte vector costs 92 (invention B).

## The whole path, as built

    value ──NumFmt::render──▶ 16-byte zero-padded vector  (no String; exact = format! over 402 M f32)
          (or a &str from view code)
    retained Label (64-byte hot line: text head, slot base, pens, cached digit-cell tiles)
          │  vceqq old/new ─▶ changed-lane mask
          ├─ every changed lane digit->digit (position class): ent[base..base+16] = widened bytes   [3 instr]
          │      damage: vminv/vmaxv over changed lanes of the cached tile columns -> one u128 mask per tile row
          ├─ other changes: steps recomputed from the first changed byte (NEON slot kernel when long),
          │      stop at an unchanged suffix; damage O(1): [pen at first change, end pen] through LineBounds
          └─ ligatures / Latin-1: token table (same pair-step kernel over 256 tokens), whole-line marks
    GlyphPool (GPU-visible, SoA): x: i32 (font units) + ent: u16 (token = byte) per slot     = 6 B / glyph
    RunHeader (32 B per run, a *window*): origin, scale, colour, space, strike, base, len, x0
    vertex shader: px = floor(fma(float(x[k] - r.x0), r.scale, r.origin.x + 0.5)) + entries[r.strike][ent[k]].geo
    damage plane: [u128] per 32 px tile row, OR-ed in place (crates/bough-damage-lab, DESIGN §2)

Every box above has its own oracle and differential test (`tests/`):

| gate | domain | result |
|---|---|---|
| `exact.rs` (rustybuzz) | all 6,895,290 strings of length <= 3 over the 190 covered codepoints, every ASCII triple, 146,694 workload strings, 200 K random strings, 68,671 dictionary forms, **and** every string of length <= 7 over `f i l space T y é` (960,799, the greedy-ligature parity cases `fff`, `ffffi`...) | all equal, **all accepted** (the only refusal in the corpus is the em dash of `row_label`) |
| `pool.rs` (reference 48-byte writer + damage oracle) | 126,000 heavy updates over 1,500 consecutive ticks; 60,000 random edits (replace, insert, delete, digit swaps, growth past 16/32/64 slots, ligatures, Latin-1, empty, refused) on 24 labels at 6 sizes with fractional origins and spaces | expanded slots == fresh shape + reference instances, bit for bit; marks contain the oracle's tiles and every changed instance |
| `window.rs` | every visible line of the lab stream for 3,000 ticks (20,953 windows, 1.8 M glyphs); every wrapped line of 2,000 vlist paragraphs (9,134 lines, 637 K glyphs) | each window == that line shaped alone; wrap == `wrap_greedy` |
| `numeric.rs` (`format!`) | every f32 in 30 binades for `{:.1}`, every 5th for `{:.0}`/`{:.2}`, negatives sampled (402,653,183 renders); all heavy values over 5,000 ticks; u32 samples | all equal |
| `gpu_shape` example (CPU kernels) | 195,476 runs x 4 snap settings, 34.9 M glyphs | GPU pens and snapped pixels == CPU, bit for bit |

## Attempt 1: delta text (patch, don't shape) — kept

**Rule bent:** "a changed value is shaped". **Representation change:** a retained label keeps its bytes in a
zero-padded 16-byte vector next to its slot base; a new value is an XOR against it. Which byte changes can move
a glyph is a property of the *table*, not of the string, so it is decided once at font load:

* **Position-equivalence classes** (`Shaper::pos_class`): bytes `a` and `b` are equivalent iff
  `dense[a][*] == dense[b][*]` and `dense[*][a] == dense[*][b]` over all 128 rows and columns (trigger entries
  included). Substituting bytes within their classes leaves every step `dense[s_i][s_i+1]` unchanged (each keeps
  its row and its column, also when neighbours change at the same time), hence every pen, the width and the
  eligibility. DejaVu Sans has 82 classes over its 190 codepoints; **the ten digits are one class** (with
  `$ { }` and the Latin-1 currency signs), the space is one with NBSP and `·`. Digits never kern and are tabular,
  which `examples/explore.rs` prints and `exact.rs` asserts.
* **Equal-length update, every changed lane digit->digit** (5 NEON ops decide it; any other class change is
  checked exactly per lane): the instance rewrite is the 16 widened bytes stored over the label's 16 entry slots
  (`ushll`, `ushll2`, `stp`: unchanged lanes rewrite their own value, lanes past the end stay 0). No step, no
  pen, no glyph id is touched.
* **Anything else** (length change, a class change): steps recomputed from the first changed byte only, the slot
  before it keeps its pen; an equal-length edit whose region is short runs scalar and stops at the last changed
  byte when the pen there comes out unchanged (the suffix keeps its slots); a long region runs the NEON slot
  kernel. A trigger mid-way restores the label exactly (its old text was a valid identity run) and goes to the
  token path.

On heavy's 1,500 consecutive ticks: 96.7% of label updates are digit patches, 2.5% windows, 0.9% unchanged.

| one label update (14 px, best of 7) | slots only | + tile marks |
|---|---:|---:|
| unchanged value | 61 | 66 |
| one or three digits changed (class patch) | 84 | 144 |
| 16-byte label, last digit | 84 | 144 |
| length change `9.9 ms` <-> `10.0 ms` (window from byte 0) | 415 | 518 |
| unit change `12.3 ms` <-> `12.3 us` (window, suffix) | 418 | 528 |

(The micro numbers include a non-inlined call per update; inside a tick loop the patch is 68 / 133 on average.)
The digit patch, as compiled (`one_update::<false>`):

    ldr w8,[x3,#0x1c] ; ldr w9,[x3,#0x14] ; sub x10,x5,#1 ; cmp x10,#0xf ; ccmp ; ccmp ; b.ne slow   ; identity, n == old n <= 16
    and x8,x24,#0xfff ; cmp x8,#0xff1 ; b.hs copy                                                   ; 16-byte load cannot cross a page
    dup.16b v0,w21 ; ldr q1,[iota] ; cmhi.16b v0,v0,v1 ; ldr q1,[x24] ; and.16b v0,v0,v1           ; new bytes, lanes >= n zeroed
    ldr q2,[x19] ; cmeq.16b v1,v0,v2 ; shrn.8b v3,v1,#4 ; fmov x8,d3 ; and x8,#0x8888.. ; cmp ; b.eq done   ; changed-lane mask
    movi.16b v3,#0xd0 ; add.16b v4,v0,v3 ; movi.16b v5,#10 ; add.16b v2,v2,v3 ; umax.16b ; cmhi.16b ; orr.16b
    addp.2d d1,v1 ; fmov x9,d1 ; cmn x9,#2 ; b.eq commit                                            ; all changed lanes digit -> digit
    commit: ldr x8,[x20,#0x20] ; ldr w9,[x19,#0x10] ; add x8,x8,x9,lsl #1
            ushll.8h v1,v0,#0 ; ushll2.8h v2,v0,#0 ; stp q1,q2,[x8]                                 ; THE instance rewrite
            str q0,[x19] ; ldr x8,[x19,#0xb0] ; str q0,[x8]                                         ; new text head

**Losing regime, kept:** the window path costs 415-528 for a 7-byte length change, about 1.1-1.4x iteration 1's
fused shaper (but with damage and 6-byte output). It is 2.5% of heavy's updates (10 instructions per label on
average). Most of it is per-call overhead of the cold path (`is_ascii`, `first_diff`, the pens of the first 16
slots, the text store), not the 7 steps.

## Damage: tile marks (with the damage lab's input) — kept

The damage lab (`crates/bough-damage-lab/RESULTS.md`, DESIGN §2) found that the GPU's cost unit is the 32x32
tile and that per-glyph diffing of sliding text is what made its CPU line lose on lab-stream. So text damage is
emitted as bits OR-ed straight into the frame's plane (`[u128]`, one per tile row), and:

* **class patch:** the changed digits' tiles. The digits do not move and share one *cell box* (the union of the
  ten digit boxes, a per-strike constant), so the tiles a digit at slot `k` can touch, `[t0[k], t1[k]]`, are
  **cached in the label** with the space translation they were computed for (recomputed only when pens move or
  the space moved). Damage = `vminv` of the changed lanes' `t0`, `vmaxv` of their `t1`, one u128 mask, one OR
  per tile row:

        ldur q0,[x19,#0x78] ; bic.16b v0,v0,v5 ; orr.16b v0,v0,v5 ; uminv.16b b0,v0   ; t0 = min over changed lanes
        ldur q0,[x19,#0x88] ; bic.16b v0,v0,v5 ; umaxv.16b b0,v0                     ; t1 = max over changed lanes
        lsl/lsr/csel x2 ; adds ; adc                                                   ; mask = (2 << t1) - (1 << t0)
        ldp ; orr ; orr ; stp                                                          ; plane[row] |= mask

* **window / replace / free / stream line:** O(1), no glyph is compared: the tiles of
  `[min(old, new pen at the first change) + left, max(old, new end pen) + over)` over the line's rows, where
  `LineBounds` (per strike, from outline boxes and each token's smallest step, so negative kerning is covered) is
  conservative by construction. "Everything shifted" is detected in O(1) by the first-difference index (0 for a
  slid line) or, for streams, by construction.
* **moved lines (log pane, scrolled spaces):** the space translation changes; the pane is marked once (clip ∩ old
  ∪ clip ∩ new, per DESIGN §2), never its glyphs.

How the cost came down (heavy tick average, marks only, on top of the slot rewrite):

| formulation | instr / label | tiles over the oracle (heavy, 1,500 ticks) |
|---|---:|---:|
| per changed glyph: NRect union of old/new box -> u128 column mask, clamped row loop | 253 | - |
| per glyph: u64 column mask relative to the line, one shift into u128 at the end | 170 | +0.06% |
| digit-cell span from stored pens (vector min/max over changed lanes) | 114 | +2.0% |
| + straight-line `mark` (no u128 clamps; text store without `memcpy`) | 97 | +2.0% |
| + per-slot tile columns cached in the label | 79 | +2.0% |
| **+ straight-line row OR** (LLVM had vectorised the 1-2 row loop, 15 instr of setup) | **65** | +2.0% (patch path), +4.15% overall |

The cell box gives up exactness at tile granularity (+2.0% tiles on digit patches: about 3.5 tiles per tick, ~0.2
us of GPU at the lab's per-tile cost) for about 6 K CPU instructions per tick; kept. On random edits the O(1)
whole-line rule marks 2.1x the oracle's tiles (mostly ligature/Latin-1 labels, which always rewrite whole); that
is the price of never comparing glyphs, measured, not hidden.

## Attempt 2: the instance pool is the shaper's output — kept

**Rule bent:** "a glyph instance is a 48-byte uber-shader record with its own position, uv, colour, kind".
**Representation change:** structure of arrays, **6 bytes per glyph**: `x: i32` (pen, font units, relative to
the run) and `ent: u16` (strike-entry id). For ASCII the entry id *is the byte* (the strike's entry table is
indexed by token), so the shaper never looks up a glyph id and a label's `ent` column is its string, widened.
Everything constant over a run lives once in a 32-byte `RunHeader` (origin, scale, colour, space, strike, base,
len, `x0`); the vertex shader does the atlas lookup, the scale and the snap. Blocks are power-of-two size classes
with free lists, never cleared (a run draws exactly `len` slots), so text grows in place up to its class.
Strikes are made ASCII-complete when the pool first uses them (95 glyphs rasterized once), which deletes the
readiness check from every ASCII path.

* Exactness: `GlyphPool::expand` is the shader's arithmetic on the CPU; `tests/pool.rs` and `tests/window.rs`
  require it to equal iteration 1's 48-byte writer bit for bit, and `gpu_shape` shows Metal's
  `floor(fma(float(x), scale, ox + 0.5))` equals Rust's `mul_add(..).floor()` on 34.9 M glyphs.
* Cost per glyph for new text: the NEON slot kernel is the prototype's kernel without the `tbl` glyph-id lookup,
  writing `ent` as the widened bytes; 1,098 instructions for a 105-character line (10.5 per glyph including all
  fixed overhead; the task's target was ~15) against 3,628 for iteration 1's fused 48-byte writer.
* Numeric labels: 68 instructions per label (the digit patch), from 367.
* The pool replaced iteration 1's per-16-slot run table (`granule_run`): with runs as windows (invention A) a slot
  can belong to several runs, so the draw list must carry `(run, first, count)` and the GPU expands it (the damage
  lab's §5.4). Removing it and the block clearing took the vlist paragraph from 12.4 K to 7.6 K.

## Attempt 3: shaping on the GPU — rejected (exact, but not cheaper)

**Rule bent:** "the CPU shapes". **Representation change:** the CPU writes only string bytes and an 8-byte run
entry; a compute kernel (one SIMD group per run: gather `dense[a<<7|b]`, `simd_prefix_exclusive_sum` with a
carry, `simd_or` for the trigger flag) writes the pens and the snapped pixels (`examples/gpu_shape.rs`, objc2-metal,
no dependency on bough-metal).

* **Exactness: verified.** 195,476 runs (the workload corpus plus 50,000 random strings) x 4 scale/origin settings:
  34,897,480 pens and snapped pixels equal to the CPU kernels; the 4,872 refusals (non-ASCII, triggers) agree.
* **Cost of one heavy tick (84 labels):** CPU 46 instructions per label to write bytes + run table (3.9 K), plus
  **26.4 K instructions for one compute encoder** in a command buffer the frame already has (41 K with its own
  command buffer and commit); GPU kernel 3.6-9 us; commit to completion 120-136 us of scheduling latency.
  That is about 30 K CPU instructions per tick: iteration 1's level, 5x the pool's 5.7 K, before counting that
  **the CPU no longer knows the positions**, which exact damage, wrapping, intrinsic widths and hit testing need
  (a GPU-side damage/wrap would need a readback a frame later).
* Where it would pay: bulk first-time shaping of megabytes (8.96 MB of text in 1.4-1.8 ms of GPU time, against
  ~68 M CPU instructions), e.g. opening a huge log, if layout can take the width a frame late.

## Attempt 4: ligatures and Latin-1 in the table — kept (eligibility ~100%); SIMD Latin-1 not done

**Rule bent:** "a GSUB rule is a trigger to full shaping". **Representation change:** a 256-token alphabet
(ASCII bytes, Latin-1 codepoints, one token per ligature, pad, phantom) with the same pair-step table,
`step[t<<8|u] = adv(t) + kern(t, u)` (128 KB per font), and a tokenizer that replays the font's ligature lookup
the way HarfBuzz applies it (left to right, at each glyph the ligatures that start with it in the font's order,
first match wins, resume after it), decoding 2-byte UTF-8 for Latin-1 in the same pass.

* DejaVu's set, read from the font: `ffl ffi fl fi ff` (lookup 18, latn `liga`), in that order.
* Validity conditions checked at build (`script::build_tokens`, the model refuses the font otherwise): every live
  GSUB rule is one of the recorded ligatures of one lookup; no active GSUB lookup runs after it; no active
  non-pair, non-mark GPOS lookup covers a ligature glyph (mark lookups need a mark, and none is covered); pairs
  with a ligature glyph are evaluated with the `latn` lookups (all components are Latin letters) and are `TRIG`
  unless plain first-glyph `XAdvance`. In DejaVu **no kerning pair involves a ligature glyph** (`explore`
  checks it), which is also why "ignore ligature kerning" is an undetectable mutation here.
* Gate: the full iteration-1 gate now runs the token path on every string as well and requires it to equal both
  rustybuzz and the dense kernels; every length <= 3 string over the covered set is now **accepted** and equal
  (6,895,290 of 6,895,290; iteration 1 refused 1,140); every length <= 7 string over `f i l space T y é`
  (960,799) is accepted and equal; all 68,671 sampled dictionary forms are accepted (iteration 1: 67,473). Mutations
  caught: ligature order reversed (`ffi` -> `ff`+`i`), ligature advance replaced by `f`'s.

| strings | eligible before -> after | before | token path | rustybuzz + plan |
|---|---|---:|---:|---:|
| Latin-1 phrases (11.3 ch), 6 | 6 -> 6 | 752 (`char_indices` path) | 510 | 18,879 |
| heavy log lines containing `ff` (84 ch), 60 | 0 -> 60 | rustybuzz: 115,241 | 3,865 | 115,241 |
| dictionary words containing ff/fi/fl, 202 | 0 -> 202 | rustybuzz: 20,108 | 736 | 20,108 |

**Losing regime, recorded:** the token path is a second, scalar pass (after the ASCII kernel finds the trigger):
35-45 instructions per character, 4.4x an ASCII line. The ligature cliff went from 130x to 4.4x, but the method's
rule (the same mechanism must be cheapest everywhere) is not met yet. **Not done:** the SIMD UTF-8 decode. The
structural fix, designed but not built: make the slot the *byte* everywhere (glyph on a cluster's first byte,
`PH` phantom slots for UTF-8 continuation bytes and consumed ligature components, drawing nothing). Then Latin-1
decoding is lane-local in NEON (`t_i = f(b_i, b_i+1)`, the next real token is a `bsl` of two shifted vectors,
one phantom at most), a ligature is a local patch of the ASCII kernel's output plus a constant suffix shift
(the ASCII kernel has already found where), and the delta path applies to Latin-1 labels unchanged.

## Attempt 5: caches — rejected (shaping is cheaper than looking up)

| | cache | re-shaping |
|---|---:|---:|
| vlist paragraphs, per-word cache (48-word vocabulary, 99.5% hits; pens re-based, boundary steps re-read: exact, checked) | 27,176 | **7,321** (NEON slots) |
| heavy numeric labels, whole-string cache (74.3% natural hit rate), cost of a *hit* | 212 | **148** (slots) |

Both caches use an Fx-style multiplicative hash (with std's SipHash the word cache costs 36.6 K). Hashing a word
and copying its slots costs more than the ~8 instructions per glyph of shaping it. Caching pays only where
shaping is expensive (full shapers, cosmic-text's word cache), which the table shaper removed. The retained
forms below (deltas, windows, rings) beat caching because nothing is looked up: the shaped slots stay where
they are.

## Invention A: a run is a window onto shaped slots — kept

**Rule bent:** "a text run owns its glyphs". **Representation change:** `RunHeader { base, len, x0 }` draws slots
`base..base+len` with pens `x[k] - x0` (an exact integer subtraction before the fma). A glyph's pen inside a
window depends only on pairs inside it (the window's last glyph keeps its kern with the next byte, which changes
the window's width, never a pen), so a window over a longer text *is* the shaping of its own bytes. Then:

* a wrapped paragraph is shaped once; each line is one 32-byte header (`wrap_slots` reads the pool's `x`): 7,557
  per vlist paragraph including wrap, below iteration 1's shape + wrap (9,169) with the instances included;
* a **stream** (`window::Stream`) keeps its shaped tail; appending shapes only the new bytes (re-reading the last
  glyph's step); the visible lines are re-pointed windows whatever the line breaking does: lab stream 1,870
  instructions per tick for 7 lines, 44 new bytes and 7 line marks, against 6,558 (re-shaping, iteration 1) and
  25,396 (iteration 1 with instances), 51.8 K per line in cosmic-text;
* a **list** of retained lines moves by spaces: heavy's log pane is a ring of 5 line labels, per tick one new
  line shaped, 5 space writes, the pane marked once: 1,444 per tick, against 4,385 / 14,610.

## Invention B: the value never becomes a `String` — kept

**Rule bent:** "text arrives as a string". **Representation change:** a numeric label keeps its format
(`NumFmt`: prefix, `{:.D}` of an `f32` or `{}` of a `u32`, suffix); the value is rendered right to left into one
`u128` register (prepend = shift by 8 and OR; the register *is* the zero-padded 16-byte vector the patch
compares). `v as f64 * 10^D` is exact for an `f32` (24 + 7 bits < 53), and Rust rounds the exact decimal half to
even, so `round_ties_even` gives the digits; `tests/numeric.rs` checks 402,653,183 renders against `format!`
(every f32 in 30 binades) and every heavy value.

| heavy numeric label, from the value | instr / label |
|---|---:|
| `format!()` into a new `String` | 1,249 |
| `write!()` into a reused `String` | 757 |
| `NumFmt::render` (first version, slice copies became `memcpy` calls) | 242 |
| **`NumFmt::render` (one `u128` register)** | **92** |
| `write!()` + pool delta + tile marks | 905 |
| **`NumFmt::render` + pool delta + tile marks** | **236** |

## What did not move, and what is next

* The window path's fixed overhead (about 350 instructions over an unchanged update) is the largest remaining
  per-label item for labels that change length; a 16-byte, vector form of the window for short labels would fold
  it into the patch.
* The token path (Attempt 4) needs the byte-slot / phantom restructure above to stop being a second pass.
* Text damage of whole-line rewrites is O(1) and conservative (+108% tiles on random edits); per-32 px-column
  comparison of old and new pens would tighten it for long lines whose suffix only shifts.
* The draw list must carry `(run, first, count)` and expand runs on the GPU (damage lab §5.4) for windows to be
  free on the GPU side too; nothing here measures that expansion.

## Recommended text pipeline

1. **Font load:** `SimpleScript::build` (classes, kerning matrix, triggers, ligature replay, token table),
   `Shaper::new` (dense ASCII table 32 KB, token table 128 KB, position classes). Refuse the token model unless
   its validity conditions hold.
2. **Strike (font x size):** token entry table (`TokStrike`: 256 x 32-byte GPU entries + damage iboxes),
   `LineBounds`, ASCII glyphs rasterized up front.
3. **Mount a text:** a `Label` (64-byte hot line + cold text) and a pool block of `16 << k` slots; the first
   `set_text` is a replace: the NEON slot kernel writes `x` and `ent` straight into the GPU-visible block (token
   path for ligatures/Latin-1; rustybuzz only beyond Latin-1).
4. **Numeric values:** keep a `NumFmt`; on change render into 16 bytes and `set_text`: a digit patch writes 16
   entry ids and ORs a cached tile span into the damage plane (236 instructions from the value).
5. **Other changes:** the same `set_text`: window from the first changed byte, suffix early-out, O(1) line marks.
6. **Long, growing or wrapped text:** shape once into a block; lines are `RunHeader` windows (`x0`); streams
   append; lists move by spaces.
7. **GPU:** per glyph 6 bytes (`x`, `ent`); per run 32 bytes; the vertex shader computes
   `floor(fma(float(x - x0), scale, ox + 0.5)) + entry.geo` (+ space), the formula verified bit for bit above.
8. **Damage:** bits in the 32 px tile plane, written at update time; nothing rediscovers changes later.

---

# Iteration 1 (Sonnet prototype)

Table-driven shaper for the common case, verified bit-identical against rustybuzz, plus an atlas and
glyph-instance generator. Everything here was measured on an Apple M3 Pro, `nightly-2026-10-17`,
single thread. **Instruction counts** (`proc_pid_rusage` -> `ri_instructions`, counter-read overhead
subtracted, best of 3) are the reliable numbers: the machine was running at load average 50-75 from other
agents during all measurements, so the **ns columns are noisy** (best of 5, but still inflated and jumpy;
trust ratios only loosely, and trust the instruction columns).

Reproduce:

    export CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/text
    cargo test --release                 # full exhaustive gate (BOUGH_FULL=1 forces it in debug)
    cargo test                           # debug: same tests, sampled
    cargo run --release --example bench  # or --profile dist
    cargo run --release --example report # what the trigger analysis found in DejaVu Sans

## Headline

| one heavy tick, ~84 changing numeric labels | instructions | per label |
|---|---|---|
| cosmic-text `Buffer` as zgx drives it (set_text + shape_until_scroll + layout_runs, word cache, per-label retained buffers) | 891 K (release) / 811 K (dist) | 10.6 K / 9.7 K |
| bough-text `shape_into` | 16.6 K | 197 |
| bough-text shape **and** 48-byte glyph instances (two steps) | ~40 K | 481 |
| bough-text fused shape-to-instances (no `GlyphRun`) | ~33 K | 388 |

(zgx reports ~13 K instructions per changed label for text; my mirror of its cosmic path measures 9.7-10.6 K for
shaping+layout alone, so the baseline is the right order and, if anything, kind to cosmic-text.)
Shaping alone is **48-54x** fewer instructions than cosmic-text on the changing-number workload and
**~20-55x** across the other workloads. The instance writer adds 31 instructions per glyph (rasterization
is paid once per glyph per size, 26 K instructions, never in steady state).

## What was built

    src/script.rs   SimpleScript::build(font bytes): cmap/hmtx/GSUB/GPOS -> covered set, gid, advance,
                    class-pair kerning matrix, trigger analysis (ttf-parser). ~9 ms for DejaVu.
    src/shape.rs    Shaper: shape_into (hybrid), shape_scalar, shape_simd (NEON), UTF-8 Latin-1 path; GlyphRun
    src/atlas.rs    swash rasterizer, shelf-packed power-of-two R8 atlas, 48-byte Instance, GlyphCache,
                    instances() and the fused shape_instances_ascii()
    src/wrap.rs     greedy word wrap over a shaped ASCII run (vlist paragraphs)
    tests/exact.rs  the exactness gate        tests/atlas.rs  packing / instance / fused-equality tests
    examples/bench.rs, examples/report.rs, examples/common/corpus.rs (workload strings, generated by the
    pure functions of zgx `comparisons/workload`, depended on read-only by path)

### Output representation
`GlyphRun { gid: Vec<u16>, x: Vec<i32>, width: i32, cluster (implicit for ASCII), upem, size }`, all positions in
**font design units** (HarfBuzz's native output scale), pixel scale applied only when instances are written
(`scale = size/upem`, one fma per glyph). Offsets and y-advances are never stored: a pair that would produce
one is a trigger, so they are zero by construction. A run is size independent.

### How the table works
* Covered set: 0x20-0x7E and 0xA0-0xFF minus U+00AD (default-ignorable) = 190 codepoints in DejaVu Sans.
* **Kerning** is the sum, over the pair-adjustment lookups HarfBuzz enables, of the first glyph's XAdvance, for
  every ordered pair of the covered set, reduced to a **class-pair matrix** (38 row classes x 45 column classes,
  3.4 KB; 1087 of 36,100 pairs are non-zero, i.e. 3%). Row class 0 is an all-`TRIG` row (uncovered bytes and
  single-codepoint triggers); column 0 is the end-of-string pad.
* **Dense expansion** used by the hot kernels: `dense[a<<7|b] = adv(a) + kern(a,b)` as one `u16`, `0xFFFF` = trigger.
  32 KB, ASCII only. Kerning and the end of string stop being separate operations: a glyph's pen step is one
  load, and the trigger test is a single OR-accumulate and a test of bit 15 at the end (real steps are < 0x1000).
* **Trigger analysis** (script.rs header has the argument): enumerate every GSUB/GPOS lookup reachable from the
  default features of the `latn` and `DFLT` scripts (HarfBuzz uses `latn` if the run has a Latin letter and
  `DFLT` for all-Common text, e.g. `12.5%`); a rule can only fire if every one of its glyph positions
  (backtrack/input/lookahead) is satisfiable by covered glyphs, otherwise it is dead; a live rule becomes a
  trigger on its cheapest adjacent codepoint pair (or single codepoint); pair-adjustment lookups are evaluated
  not triggered, unless their values are not plain first-glyph XAdvance (offsets, second-glyph values, device
  tables), their lookup flags skip covered glyphs, or two Common codepoints kern differently under `latn` and
  `DFLT`. A font with legacy `kern`-only kerning, feature variations, or unparsable lookups returns `None`.
* **DejaVu Sans result**: 40 GSUB / 16 GPOS lookups; default-feature lookups are GSUB 3, 4, 18 (latn), 1, 4 (DFLT)
  and GPOS 4, 12-15 (latn), 15 (DFLT). 29 contextual/ligature rules analysed, **24 dead** (the `ccmp` mark-decomposition
  contexts need combining marks), 5 live (the f-ligatures of `liga`, whose cheapest adjacent pairs are `ff`, `fi`, `fl`). The complete trigger set for the
  190 covered codepoints is **three pairs: `ff`, `fi`, `fl`**. No single-codepoint triggers; no
  script-dependent pairs. Kern lookup 14 applies only under `latn`, lookup 15 under both.

## The exactness gate

Reference: rustybuzz 0.20.1, default features, `UnicodeBuffer::push_str` + `guess_segment_properties()` (also
re-run with the script forced to Latin). rustybuzz shapes in font units, so **no rounding is involved on either
side**. The comparison is exact integer equality, per glyph, of: glyph id, cluster (UTF-8 byte offset),
x-advance (kerning included), y-advance (0), x-offset (0), y-offset (0); plus glyph count, `x[i]` equal to the
running sum of reference advances, and `width` equal to their total. Scalar, NEON and hybrid entry points must
also agree with each other bit for bit and on eligibility.

| set | strings | eligible | result |
|---|---|---|---|
| all strings of length <= 3 over the 190 covered codepoints (ASCII + Latin-1) | 6,895,290 | 6,894,150 | all equal |
| the 1,140 refused strings | | | exactly those containing `ff`/`fi`/`fl` |
| all 857,375 ASCII triples (every SIMD lane/tail combination) | 857,375 | - | all equal |
| every workload string (heavy values over 1,500 ticks, rows, labels, 2,000 log lines, lab stream, 400 vlist paragraphs, micro labels) | 146,694 | 146,632 | all equal |
| random strings, length 0-200, mixed ASCII/Latin-1 pools | ~200,000 | - | all equal |
| `f`-free random strings, length 1-200 | ~50,000 | all | must be accepted: all were |
| dictionary words, upper-cased words, 9-word sentences (`/usr/share/dict/words`, every 7th) | 33,711 words | 67,473 / 68,671 forms | all equal |

Release runs the full sets; debug runs a sample (about 1/200 of the triples) to keep the suite at ~25 s.
**Mutation check**: pointing the Latin kern set at the `DFLT` lookups makes the gate fail immediately
(`" -A"`: 739 vs 694), so the gate has teeth. Refused-by-design cases are tested too (control characters, NUL,
DEL, U+00AD, U+0301, CJK, euro, tabs, newlines).

**Assumptions the caller owns** (all outside what the gate varies): horizontal LTR single-script runs shaped with
no language tag (the font's `latn` default language system; Catalan/Spanish/Galician/Romanian/etc. select other
language systems), no user features, no variations, no letter spacing, kerning on.

## Benchmarks

Instructions / ns per string. `hybrid` is `shape_into`: scalar below 16 bytes, NEON from 16. cosmic-text is
`zgx-cosmic-text` 0.19.7 with `shape-run-cache`, one retained `Buffer` per label slot (`set_text` recycling
lines, `Shaping::Advanced`, `shape_until_scroll`, walk `layout_runs`), `shape_run_cache.trim(2)` per pass, DejaVu
Sans through `fontdb` only; wrapped at 688 px for vlist. rustybuzz is 0.20.1; "+ plan" reuses a `ShapePlan`
(a real integration would). Workloads are generated by the zgx workload functions; numeric = 6 tile values (20 px)
+ 30 card values (16 px) + 48 table latency/throughput cells (12 px) per tick over 60 ticks; stream = lab
105-character chunks; log = heavy's log lines; labels = titles, tile/card labels, column titles, ids, workers,
status; vlist = 120 chat paragraphs (avg 399 chars, vlist wrapped by `wrap_greedy` in the bough columns,
whose line counts match cosmic-text's on 120/120 paragraphs).

### `release` profile (thin LTO)

| workload (avg chars) | bough hybrid | NEON only | scalar only | rustybuzz shape() | rustybuzz + plan | cosmic-text (zgx path) | cosmic / hybrid |
|---|---|---|---|---|---|---|---|
| numeric values (7.7) | 206 i / 8 ns | 247 i / 12 ns | 202 i / 8 ns | 38,688 i | 11,579 i | 9,853 i | 48x |
| stream line (105) | 952 i / 51 ns | 948 i / 50 ns | 1,419 i / 53 ns | 199,347 i | 171,991 i | 51,811 i | 54x |
| log line (84.5) | 811 i / 44 ns | 808 i / 68 ns | 1,147 i / 42 ns | 139,582 i | 111,859 i | 41,488 i | 51x |
| short UI labels (9.2) | 211 i / 9 ns | 247 i / 11 ns | 219 i / 8 ns | 41,366 i | 16,025 i | 5,643 i | 27x |
| vlist paragraphs (399, incl. wrap) | 9,187 i / 518 ns | 9,178 i / 828 ns | 11,206 i / 489 ns | 673,826 i | 646,071 i | 188,618 i | 21x |

rustybuzz/cosmic ns on this loaded machine: numeric 2,251 (plan) / 2,571 (cosmic); stream 70,018 / 22,677;
labels 1,164 / 516; vlist 166,766 / 44,318. Treat as order of magnitude only.

### `dist` profile (fat LTO, cgu=1; run before the dead-code cleanup, kernels unchanged)

| workload | bough hybrid | rustybuzz + plan | cosmic-text | cosmic / hybrid |
|---|---|---|---|---|
| numeric values | 212 i | 11,743 i | 9,033 i | 43x |
| stream line | 947 i | 173,842 i | 47,865 i | 51x |
| log line | 812 i | 113,079 i | 38,228 i | 47x |
| short UI labels | 217 i | 16,347 i | 5,005 i | 23x |
| vlist paragraphs | 9,230 i | 654,492 i | 174,089 i | 19x |
| one heavy tick (84 labels) | 16.7 K | - | 811 K | 48x |

### Cost versus length (instructions per string, release)

| bytes | NEON | scalar |
|---|---|---|
| 1 | 213 | 90 |
| 4 | 229 | 144 |
| 8 | 223 | 183 |
| 12 | 241 | 248 |
| 16 | 229 | 284 |
| 17 | 335 | 297 |
| 32 | 346 | 486 |
| 64 | 555 | 864 |
| 105 | 938 | 1,409 |
| 200 | 1,590 | 2,559 |

Scalar is ~12 instructions per glyph plus ~78 fixed; NEON is ~7.6 per glyph (about 121 per 16-byte chunk) plus
~210 fixed. Crossover in instructions is ~12 bytes; the hybrid switches at 16. **In time the two are
indistinguishable at every length I measured** (17 bytes: 9.8 vs 16.7 ns, 200 bytes: 90 vs 86 ns), because the
M3's scalar pipeline retires the dense-table loop at ~4 IPC while the NEON kernel is latency-bound on its
`umov`/`ld1.h` gather. NEON halves the instruction count (which matters if instructions are the metric, and for
sibling-thread/power headroom) but is not faster in wall time on this core.

### Other measurements

| | instructions | notes |
|---|---|---|
| Latin-1 text (UTF-8 decode path, 11.3 chars) | 784 / string | ~70 per char: slow, uses `Vec::push` and `char_indices` |
| instance generation, warm atlas | 31.4 / glyph (411 / run, 5,453 runs, 71,463 glyphs) | 48 B/instance; ~1.6-2.4 ns/glyph |
| shape + instances, numeric label, two steps | 481 | |
| shape + instances, numeric label, **fused** | 388 | no `GlyphRun`; per-byte entry table; 19% fewer |
| swash rasterization 14 px unhinted alpha | 26.6 K / glyph (~4.4 us, cold, incl. atlas pack) | once per (glyph,size); atlas 1024x512, 2% used by ASCII |
| `SimpleScript::build` DejaVu Sans | ~9 ms | at font load |
| counter read overhead | 5.1 K | subtracted |

**Rasterizer choice**: swash (the rasterizer zgx/cosmic already use, so quality is identical). It is by far the
most expensive thing in the crate per glyph (26 K instructions, ~50x a shaped glyph) but runs only on atlas
misses. fontdue/ab_glyph were not compared.

## Hot loops

### Scalar kernel (`Shaper::scalar_ascii`), 24 instructions for 2 glyphs, release

    add   x0, x1, x15              ; s + i
    ldrb  w4, [x0, #1]             ; b  = s[i+1]
    lsl   x5, x4, #1
    orr   x6, x5, x14, lsl #8      ; (a<<7|b) * 2   (a = s[i], carried in w14)
    ldrh  w7, [x11, x14, lsl #1]   ; gid[a]
    ldrb  w14, [x0, #2]            ; c  = s[i+2]
    ldrh  w0, [x10, x6]            ; step1 = dense[a<<7|b]
    lsl   x4, x4, #8
    orr   x4, x4, x14, lsl #1
    ldrh  w4, [x10, x4]            ; step2 = dense[b<<7|c]
    orr   w12, w12, w0             ; trigger accumulate
    orr   w12, w12, w4
    strh  w7, [x17]                ; gid[i]
    ldrh  w5, [x11, x5]            ; gid[b]
    strh  w5, [x17, #2]            ; gid[i+1]
    add   w0, w13, w0
    stp   w13, w0, [x16, #-4]      ; x[i], x[i+1]
    add   w13, w0, w4              ; x += step1 + step2
    add x4/x15/x17/x16, cmp, b.lo  ; loop overhead

### NEON kernel (`simd_kernel::<NARROW=true>`), one 16-byte chunk, ~121 instructions

    ldr   q16,[x5] ; ldur q17,[x5,#1]     ; bytes a[0..16], b = bytes shifted by one
    add.16b v18,v16,v2 (= a-64)           ; second-half table index
    ldp q19..q26,[tbl]                    ; the 128-byte gid table (re-loaded every chunk on purpose)
    tbl.16b v19,{v19-v22},v16 ; tbx.16b v19,{v23-v26},v18     ; 16 bytes -> 16 gids (2 lookups)
    ushll/ushll2 + stp q                  ; widen and store 16 u16 gids
    ushll.8h v18,v16,#7 ; uaddw.8h v18,v18,v17 ; (and the high half)  ; idx = a<<7 | b, 16 lanes
    umov.h w4,v18[0] ; add x5,x10,w5,uxth#1 ... ; ldr h17,[x10,x4,lsl#1] ; ld1.h {v17}[1],[x5] ...
                                          ; the gather: 3 instr/lane, 48 total
    and.16b (valid mask) ; orr.16b v6 (trigger accumulate)
    ext/add x3 per half, dup.8h v19,v17[7], add   ; inclusive prefix sum over 16 u16 lanes
    sub.8h (exclusive) ; uaddw/uaddw2 (add running base, widen to i32) ; stp q x2 ; umov+add (base += last)

The remaining cost is dominated by the 48-instruction gather and ~25-instruction scan.

## Eligibility (fraction of real strings on the fast path)

| workload | strings | eligible | why not |
|---|---|---|---|
| numeric values | 5,040 | 100% | |
| lab stream line (105 ch) | 300 | 100% | |
| heavy log line | 200 | 97.5% | `ff` in the hex checksum (`checksum 00ff1a2b`) |
| short UI labels | 114 | 100% | |
| vlist paragraphs | 120 | 100% | the 48-word vocabulary has no `ff`/`fi`/`fl` |
| vlist author/time/micro labels (exhaustive corpus) | ~5,000 | 100% | |
| all workload strings in the gate | 146,694 | 99.96% | 61 log lines with `ff`, plus the em dash in `row_label` (U+2014 is outside the covered set); a test asserts every refusal is one of these |
| `/usr/share/dict/words` | 235,976 | 96.84% | `ff`, `fi`, `fl` (3.45% of lowercase words) |
| Latin-1 sample (café, naïve, Zürich, ©, ×) | 6 | 100% | scalar UTF-8 path |

## What surprised me

1. **The SIMD kernel's fixed cost, not its throughput, decided everything.** My first NEON kernel took 325
   instructions for a *one-character* string and the scalar loop beat it up to ~24 bytes. Causes found in the
   asm: LLVM pinned six 128-byte lookup tables in registers, ran out of registers and copied ~770 bytes of tables
   to the stack on every call; a `memcpy` call for the tail; a 16-lane scalar gather even for strings with no
   kerning. Fixes: `black_box` on the table pointers so the loads stay in the loop, an overlapping-load tail,
   and then replacing the class-matrix gather *plus* the advance lookups *plus* the end-of-string special case
   by one dense `adv+kern` table (see next point).
2. **Folding the advance into the kern step (`dense[a<<7|b] = adv(a)+kern(a,b)`) deleted three lookups, the
   end-of-string case and the trigger test in one move.** The trigger rides in the same 16 bits as the value
   (bit 15), so "is this string eligible" costs one OR per glyph. Same representation change made the scalar
   loop 12 instructions per glyph. (The class-pair matrix is still the table of record and is what the
   UTF-8 path and the report use.)
3. **Scalar is as fast as NEON in wall time on the M3** at every length from 1 to 200 bytes. The NEON kernel
   is a 2x instruction win only.
4. **DejaVu is almost trigger-free.** The big Latin `ccmp` chain contexts are all dead for text without combining
   marks; the only live rules in the default features are the `fi`/`ff`/`fl` ligatures. Digits never kern;
   only 3% of pairs kern. `DFLT` and `latn` use different kern lookup sets in this font (lookup 14 is `latn`
   only), so correctness depends on the Latin/Common script split; it turns out to be a no-op on Common-only
   pairs here, which the analysis proves rather than assumes.
5. **cosmic-text's word cache makes it much cheaper than raw rustybuzz** on streaming text (51.8 K vs 172 K per
   105-char line) but not on changing numbers (the words are new every tick: 9.9 K vs 11.6 K, about equal to
   raw rustybuzz with a reused plan). `rustybuzz::shape()` without a reused plan costs 27 K instructions of plan
   building per call. My mirror of zgx's path lands at 9.7-10.6 K per label against zgx's reported ~13 K.
6. After shaping gets to ~200 instructions the **instance writer (31/glyph = ~240/label) costs more than the
   shaper**; the per-label fixed overhead (prologue, `reserve`, `is_ascii`, `reset`: ~75 instructions) is also
   a third of a short label.
7. The suite caught nothing: the hard-gate passed first time on 6.9 M strings. I distrusted that and mutation
   tested it (above). Debug builds are the only place I found a bug (an overflowing seed multiply in the test).

## Rules bent, losing regimes

Bent: (a) *domain change*: kerning, advance and the eligibility flag fused into one 16-bit step per ordered pair,
so positions are one prefix sum of loads (Joffe move 1/4); (b) *widen*: u16 lanes with a proven `16 * max_step <
65536` bound, i32 positions; (c) *pad the cycle*: the end of string is column 0 of the same table, no tail
case; (d) *bend the input range*: SIMD/dense path is 7-bit ASCII only (128-entry tables), everything else
goes to a generic path; (e) exhaustive differential proof over the domain of short strings, as the method asks.
I measured throughput (independent strings), not latency chains.

Losing regimes, recorded honestly: strings under ~12 bytes (NEON fixed cost ~210 vs ~90: hybrid threshold is a
length branch, which the method doc warns about; here it costs ~3 instructions and the scalar kernel is the same
dense table, so it is one mechanism with two loop shapes, not a bolt-on); `ff`/`fi`/`fl` text falls back to a
full shaper at 11-40 K instructions (3.5% of dictionary words, 2.5% of the heavy log lines); non-ASCII Latin-1
runs the slow 70 instr/char decode path; the 32 KB dense table is per font (per weight); a new font is a 9 ms
build.

## The 5 most promising structural changes

1. **Patch, don't shape: digit-slot text.** Tabular digits have equal advances and never kern, so a numeric
   label's glyph positions are fixed by its *format*, not its value. Keep the label's instance run in the
   instance pool; a new value that keeps its length rewrites only the 8-byte `uv` of glyph slots whose
   character changed (an 8-byte compare finds them), with no shaping, no positions, no color or geometry
   writes. For heavy's ~84 labels that is roughly a few instructions per changed digit instead of ~480 per
   label (~17 K per tick -> ~1-2 K), and it is exactly the "a write must be the patch" rule applied to text.
   Generalizes to "number formatter writes digits straight into instance memory" (itoa -> `uv`).
2. **Shape into instance memory with a per-strike byte-indexed entry (done in prototype; push it).** The fused
   kernel already drops 19% (388 vs 481). Next: instances in a persistent pool sized in 64-record pages, positions
   as integer pixels relative to a space (the vertex shader adds the space) so the f32 convert/fma/floor and the
   vector shuffle go away, 16-byte glyph records, NEON for 4 glyphs per iteration. Target ~15 instr/glyph for
   shape+write (now ~50).
3. **Cache shaped runs / instances per string and per word.** Streaming text repeats 100% of its words and the
   vlist draws from a 48-word vocabulary: a word -> (gids, relative x) cache with an add of the word origin
   reduces a 399-char paragraph to ~70 lookups + wrap, below even the NEON kernel's 9 K. The same cache keyed
   by whole string covers every static label, status pill and column header (their shaping cost is already
   mostly fixed overhead, so a hash hit at ~30 instr beats it).
4. **Model the ligatures and Latin-1 in the table, in SIMD.** `ff/fi/fl` become table entries (pair -> ligature
   glyph, cluster merge, advance split) instead of triggers: eligibility to ~100% for English, no fallback cliff
   (11-40 K) for 3.5% of words. Do UTF-8 Latin-1 with NEON too (two-byte lead/continuation classification and
   the same dense table indexed by codepoint), taking 70 instr/char to ~8. Add the SIMD `pshufb` port for x86.
5. **Move the prefix sum and the glyph lookup to the GPU.** The instance stores the byte (or the 6-bit class) and
   the run's origin; the vertex shader fetches the atlas entry (`entries[strike][byte]`) and the pen step
   (`dense`) and does the run-local prefix sum in a threadgroup (runs are < 256 glyphs). CPU cost of a text
   change becomes writing the string bytes (1 byte per glyph, no per-glyph instruction at all) plus the
   one-time shaped-eligibility check, which is an OR-reduction already available per 16 bytes (5 instructions
   per chunk with the dense table, or 0 if the eligibility is cached with the string).
