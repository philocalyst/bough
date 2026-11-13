# Text lane, round 8: journal

Branch `r8/text` (worktree `wt/r8-text`), from `r8/expr` `9469bf18f1`, fast-forwarded to `0e0ec02e9e` (the expr
lane's green gates). Nothing is pushed. Brief: `bough/runtime/lead/ROUND8.md` with Amendments 1 and 2, and the
lead's lane brief of 2026-11-12. Newest entries last. Every experiment has its prediction and kill criterion
written before the run.

Documents this lane owns:

* `docs/r8/TEXT-API.md`: the author-facing programs (façade idiom), what the types infer, the gap table against
  gpui-ce and the web, the order to close it, the questions for the user.
* `docs/r8/TEXT-SEAM.md`: the boundary with layout (answer to `wt/r8-layout/docs/r8/LAYOUT-SEAM.md` section 5:
  accepted with five amendments), the proposal to the raster lane (line records over a shared glyph column,
  `Digits` as a glyph source, atlas and positioning rules), the seams with surface and threads.
* this journal.

Provenance unless a line says otherwise: the shared M3 Pro (aarch64, macOS 15.6), `nice -n 15`,
`CARGO_BUILD_JOBS=2`, toolchain nightly-2026-10-17, profile `dist`. Unit: instructions retired on the calling
thread (`bough_mem::pmu`). ilo-prod was reserved when the lane started and freed during it (Amendment 3): section 5 has its counters.

## 0. Baselines (recorded by round 7; not re-measured here)

From `wt/r7-bench/bench/results/r7/workloads/mac-6deab7e4e7/` (M3 Pro, `dist`, `REPS=32`, load 15 to 27, so the
instruction column is the one to trust), `crates/bough-text/RESULTS.md` and the round 7 journals.

| cell | instructions | source |
|---|---:|---|
| chat mount (2,000 messages, 436,280 characters of wrapped text, 23,593 records) | 964.4 M | once-render.tsv |
| chat resize, one step (re-wraps about 2,274 paragraphs, 33,255 allocations) | 186.8 M | once-render.tsv, alloc.tsv |
| chat steady p50 / hover / theme | 779,840 / 144,921 / 3.14 M | once-render.tsv |
| ide mount (20,133 one-line labels, 114,901 glyphs, 14,585 token spans over 3,000 lines) | 409.7 M | once-render.tsv |
| ide steady p50 / hover (4,312 wires) / theme (25,000 wires) / resize | 404,671 / 804,335 / 6.65 M / 63.1 M | once-render.tsv |
| number write, whole turn (`signal_cost`, `SC_STABLE=1`) | 6,000 | MEASUREMENT.md |
| of which text (render 92, patch 68, marks) | 236 | RESULTS.md 527-534 |
| label mount, text part / operand lowering | 2,490 / 620 | r7 core journal 657-674 |
| ASCII shaping, NEON | 7.6 per glyph + 210 | RESULTS.md 722 |
| slot kernel, mixed paragraphs | 7.0 per byte | RESULTS.md 214 |
| paragraph re-wrap at a new width (1,000 paragraphs) | 1,950 each | RESULTS.md 145 |
| shaped-string cache hit (round 5, rejected) against re-shaping a short label | 212 against 148 | RESULTS.md 489-500 |
| glyph raster, 14 px, cold, with atlas pack | 26,600 per glyph | RESULTS.md 735 |

gpui-ce's parley path for the same strings: its headless bench is
`~/projects/gpui-ce/crates/gpui_ce_parley/benches/text_pipeline.rs` (criterion; only `layout_multilingual_cold`
shapes, the warm cases hit its result cache and measure cloning). Not built yet by this lane.

What the inventories found that the design must answer (details in TEXT-API section 4):

* The façade has no shaped-string cache because for a short covered label a hit cost more than shaping. So a
  memo must not be justified by instructions on short ASCII; its value is shared storage, uncovered text, the
  break table and the build-time image. T1's kill line is set against re-shaping for that reason.
* The façade's resize re-wraps every paragraph and rewrites every run header at every step, exposed or not.
* gpui-ce is O(document) per edit even on cache hits (whole-text hash, per-paragraph source maps, deep clones,
  grapheme re-segmentation), its paragraph caches are 512-entry FIFOs, its storage trait forces a contiguous
  `&str`, its atlas is keyed by the exact `f32` size, and static text cannot be selected.

## 1. Predictions and kill criteria (written 2026-11-12 before any code of this lane ran)

Smallest real form: new modules under `crates/bough-text/src/stage/`, measured by
`crates/bough-text/examples/stage_bench.rs` on corpora regenerated with the chat and ide scenes' own generators
(`comparisons/bough-workloads/src/{chat,ide}.rs` on `r7/bench`: same seeds, same syllable table), at the
scenes' sizes (13 px at scale 2 for chat bodies).

| id | hypothesis | prediction | kill |
|---|---|---|---|
| T1 | a shape memo keyed by (font, features, text) whose hit is two chained lines (bucket, then entry header with the key bytes inline) | hit at most 60 + 0.6 per byte instructions; miss adds at most 15 % to shaping | a hit costs more than re-shaping at 16 bytes (about 330), or more than two dependent loads to the glyph range |
| T2 | `block(w)` over a memoised break table by binary search per line, returning the validity interval | at most 150 instructions for the chat's median paragraph at a new width; on a sweep of the chat's bubble width in 1 px steps under 5 % of paragraphs leave their interval per step; text-side cost of a step under 30,000 instructions for 2,274 paragraphs (façade: about 4.4 M in re-wrap alone, 186.8 M in all) | more than 25 % re-broken per step, or `block` above 600 |
| T2b | `guess(w)` from byte length and a per-font mean advance | total extent error under 3 % on the chat's 2,000 messages at three widths; per-row error under one line for 90 % of rows | not better than the layout lane's running mean on the same corpus |
| T2c | break tables are small | at most 2 bytes per character of prose, so under 1 MiB for the chat | above 4 bytes per character |
| T3 | a number is a glyph source: digits placed by arithmetic from the value, no shaping, no column | the reference kernel's pixels equal shaping and placing the formatted string for 100,000 values at three sizes in every font tested whose digits are fixed-advance and kern-free; glyph selection at most 25 instructions per digit; a write is one word plus the damage of one rect | a font in common use (Arial, Helvetica, Menlo, DejaVu Sans) for which neither fixed digits, `tnum` nor a synthesised cell gives the shaper's pixels |
| T4 | appending to a paragraph costs the appended glyphs | at most 25 instructions per appended byte plus 300 per append, flat from 1 KiB to 100 KiB of existing text apart from the prefix compare (at most 0.05 per existing byte when the source is a plain `String`) | cost per append growing with length beyond the compare |
| T5 | an edit in a document touches one line | typing one character in a 60-byte line: at most 1,500 instructions of text work, within 10 % between 1,000 and 300,000 lines; a newline at most 3,000 | dependence on document size above 10 % |
| T6 | constant strings are memo entries that exist before the program starts (a position-independent memo image in rodata), so their mount is a lookup | at most 40 instructions per constant label at mount with the key computed at compile time (façade: 2,490 + 620); the image needs no parse: header check only | above 200 per label, or the key not folding to a constant in the disassembly |
| G1 | glyph word layout: three words per glyph self-contained, against two words plus a per-strike entry table | the three-word form needs at least 15 % fewer instructions per glyph in the reference kernel's setup | under 5 %: take the two-word form for its memory |
| S1 | chat, text side of first paint on the new core: only exposed messages get glyphs | at most 100,000 instructions for the exposed paragraphs (about 20, 4,000 characters) including their break tables; the size-only sweep of the other 2,000 at most 6 M, off the interaction path | first paint above 1 M |
| S2 | ide, text side of first paint: one line record per code line with a style index per glyph, not one label per token | at most 150,000 instructions for about 60 exposed lines, 40 tree rows and 30 terminal lines (façade mount 409.7 M in all, of which text about 20,133 x 3,110 = 62.6 M) | above 1.5 M |

Not built in this pass and predicted only in TEXT-SEAM: atlas residency by exposure (B6), the scalable
archetype for animated size (B5), the exactness gate for quarter-pixel positioning on both executors (B4).

## 2. Log

* 2026-11-12. Read the round's documents, the façade's text and element code, `bough-expr` and `bough-x`.
  Inventories of gpui-ce's text system and of `bough-text` plus the recorded baselines made (section 0).
  `TEXT-API.md` written against the façade's idiom after Amendment 2 arrived (the first plan, an API in the
  core's own terms, was dropped). The layout lane's `Measure` proposal accepted with amendments
  (`TEXT-SEAM.md` part A). Fast-forwarded to `0e0ec02e9e` at the lead's instruction.

## 3. What was built (commits `414016c59d`, `9ebb8a9d89`)

`crates/bough-text/src/stage/` (about 900 lines), measured and gated by `examples/stage_bench.rs`, compared
with parley by `examples/stage_parley.rs`. Not wired into `bough-x` or the façade: this is phase A.

* `memo.rs`: shaping memoised by (font, text). Open addressing over 64-byte buckets of eight (tag, offset)
  slots; an entry is a six-word header followed by its key bytes, placed so a short entry does not straddle a
  line; glyphs are ranges of two shared lanes in font units (`gid: u16`, `x: i32`, so one entry serves every
  size). No pointers anywhere, so the memo is also an `Image`: `to_image` writes it, `Image::new` checks a
  header and reads it in place. The key is a `const fn`.
* `brk.rs`: `block(w)` in two forms, both returning (lines, widest, `[lo, hi)`): `block_bytes` reads the pen
  lane and the text and stores nothing; `block` runs over a break table (`build`: separators classified eight
  bytes at a time into a bitmask, only edges visited). `guess` is the O(1) estimate.
* `flow.rs`: a growing text; `doc.rs`: a document as blocks of lines under two lanes of prefix sums;
  `digits.rs`: the k-th glyph and pen of a number by integer arithmetic, and the font check; `line.rs`: the
  two glyph forms and a reference kernel for each.

Gates (all in the bench; the run stops at the first failure): a memo hit's glyphs are the shaper's; the break
table by bitmask equals the glyph-by-glyph walk (every message, and texts with leading, trailing and double
spaces, lengths around 8, a hard break); `block_bytes` == `block` over the table == the walk, with the same
line starts, at 52 widths; the interval is exact and tight at both ends (the breaks hold at `lo` and at
`hi - 1` and differ at `lo - 1` and at `hi`); a text appended six bytes at a time equals the text shaped and
broken whole; every image lookup equals the build's; both glyph forms emit the same boxes and cover the same
pixels; digit glyph ids equal the shaper's for 300,000 formatted numbers. Mutants, each stopping the run at a
gate: `STAGE_MUTANT=hi` and `lo` (an interval one unit too wide), `join` (an append that restarts at the
appended bytes, losing the pair step across the join). Two unit tests (`Doc` against a `Vec<String>` over
20,000 random edits; digits against `format!`).

## 4. Results against the predictions

`bench/results/r8/text/stage-mac.tsv` (Arial, the façade's default face on macOS) and `stage-mac-dejavu.tsv`;
M3 Pro, `dist`, load average 12 to 20, instructions of the calling thread, median of five batches, bracket
subtracted. Corpora: the chat scene's 2,000 message bodies (436,280 characters; mean 218 bytes, 39 words, 2.46
lines at the 1280 pt window) and the ide scene's 3,000 code lines, from the scenes' own generators. Glyph
raster is in none of these numbers.

| id | cell | measured | prediction | verdict |
|---|---|---:|---|---|
| T1 | hit, key hashed at the call, 8 / 16 / 32 / 64 / 256 bytes | 112 / 129 / 166 / 234 / 456 | 60 + 0.6 per byte | missed by about 1.7x; about 100 + 1.4 per byte |
| T1 | hit, key known | 82 / 90 / 107 / 139 / 147 | | |
| T1 | hit over re-shaping (193 / 234 / 350 / 563 / 1,898) | 0.58 / 0.55 / 0.47 / 0.42 / 0.24 | kill above 1 at 16 bytes | not killed (the first version was at 0.93 to 1.01: its key assembled bytes one at a time, 6.6 instructions per byte; now one load and one widening multiply per 8 bytes) |
| T1 | miss over shaping (12.1 against 7.8 per byte) | 1.55 | at most 1.15 | missed: copying the lanes, the entry, the hash and the probe are 4.3 per byte |
| T1 | dependent lines to the glyph range | 2 by construction (bucket, entry with inline key) | 2 | holds; as misses: section 5 |
| T2 | `block` at a new width, no table / over a table / the walk | 374 / 198 / 448 per paragraph | at most 150 | missed (2.5x and 1.3x); not killed (600) |
| T2 | 1 px sweep: paragraphs outside their interval per step | 2.1 % (0.2 % change height) | under 5 % | holds |
| T2 | 1 px sweep: cost of a step for 2,000 paragraphs | 41,500 (no table); 27,700 (table) | under 30,000 | missed without the table, held with it |
| T2 | the r7 harness's sweep (69 pt per step) | 78.7 % outside; 908,000 per step | none written | intervals do nothing at that step size; the step costs `block` for nearly every paragraph |
| T2b | `guess`: total extent error at 2000 / 2560 / 3200 px windows | -1.7 % / -0.7 % / -0.3 % | under 3 % | holds |
| T2b | `guess`: rows exact / within one line | 93 to 98 % / 100 % | 90 % within one | holds; the running mean of the first 15 rows gets 19 to 38 % of rows exact and 37 to 71 % within one (its total is as good: +1.3 %, +0.1 %, -1.7 %) |
| T2c | break bytes per character | 2.155 (table); 0 (no table) | at most 2 | table form missed by 8 %; moot for byte-shaped text |
| T3 | glyph and pen per digit | 19.2 | at most 25 | holds (33.6 before the per-record part was hoisted) |
| T3 | arithmetic equals the shaper on the formatted string | glyph ids always; pens only where the face does not kern digits | equal in every face with fixed, kern-free digits | **killed as written**, see below |
| T4 | append of 6 bytes at 1 / 10 / 100 KiB | 715 / 700 / 693 | 25 per byte + 300 = 450 | flat in length (holds); 1.6x the constant (missed) |
| T4 | the same through `set` with a plain `String` | 1,993 / 6,524 / 52,807 | compare at most 0.05 per existing byte | missed 10x: the compare is 0.52 per byte |
| T5 | one character typed, 1,000 / 300,000 lines | 1,202 / 1,257 (Arial); 1,548 / 1,626 (DejaVu) | at most 1,500, within 10 % | holds for Arial; 3 to 8 % over the bound on DejaVu (lines with ligatures take the token path); size dependence 4.6 to 5 % |
| T5 | newline (split and join, both halves re-shaped) | 657 / 844 | at most 3,000 | holds; 28 % dearer at 300,000 lines (the suffix adds over the prefix lanes) |
| T6 | constant label at mount: image lookup, key known | 77 | at most 40 | missed (1.9x); kill 200 not reached; shaping the same labels costs 168 |
| T6 | does `key()` of a literal fold | 49.0 with a `const` key, 49.0 with `key("..")` inline | folds | holds |
| T6 | opening the image | 95 instructions | header check only | holds |
| G1 | kernel setup per glyph, placed / shaped | 12.3 / 21.1 (ratio 0.58) | placed at least 15 % cheaper | holds, and the criterion was the wrong one, see below |
| S1 | chat, 20 exposed messages (4,727 characters, 54 line records) | 94,600 cold, 65,600 warm | at most 100,000 | holds (with a break table it was 160,000 to 235,000: missed, and the reason the table was removed) |
| S1 | size-only measure of the other 1,980 | 4.02 M keeping nothing; 5.90 M through the memo warm, 7.47 M cold | at most 6 M | holds keeping nothing; missed through a cold memo |
| S2 | ide, 130 line records (2,486 characters) | 132,400 cold, 98,000 warm (Arial); 172,600 / 127,300 (DejaVu) | at most 150,000 | holds on Arial, missed on DejaVu |

"Cold" is a fresh memo per pass: growth and the kernel's first touch of its memory are inside the cell, as
they are at a process's first paint. "Warm" is the same memo cleared. The difference (about a third) is not
text work and is what a mapped image removes.

### Against parley on the same strings

`bench/results/r8/text/parley-mac.tsv`: DejaVu Sans 26 px, the twin driver of `tests/common/twin.rs`, retained
layouts with allocations reused. "Upstream" is unmodified parley 0.11.1; gpui-ce pins 0.12 and adds its own
caches, clones and per-call O(document) work on top (section 0), none of which is in these numbers, and
gpui-ce itself was not built. Parley does more than the staged path (itemisation, bidi, fallback, line
metrics, alignment); the staged path here covers only what the table shaper covers.

| cell | upstream parley | grafted parley (round 7) | staged (DejaVu) |
|---|---:|---:|---:|
| shape and break a chat message, per byte | 1,968 | 273 | 12.1 (miss) + 419 per paragraph (`block`) |
| re-break one paragraph at a new width | 18,708 | 18,709 | 419 (and only when its interval ends) |
| 20 exposed messages from nothing | 9.49 M | 1.37 M | 94,800 |
| append 6 bytes at 1 KiB / 10 KiB | 2.55 M / 20.1 M | 328,000 / 2.55 M | 711 / 706 |
| 130 one-line texts from nothing | 7.14 M | 2.22 M | 172,600 |

### What the measurements changed

1. **The break table is gone for byte-shaped text.** Building it cost 12 instructions per byte, more than
   shaping (7.8), and it was 60 % of a paragraph's first measure. The pen lane already is the prefix sum a
   line search needs, so `block_bytes` reads pens and text and stores nothing. It costs more per call than
   the table form (374 against 198) and nothing to build; the table pays for itself only after about 15
   re-breaks of the same paragraph. The table stays as the general form (non-byte clusters, hard breaks, a
   line-break machine's opportunities) and as the oracle. A label never builds one in either form.
2. **Tabular is a definition, not an equality.** Arial's digits share an advance but the face kerns "11" by
   152 units, so 5.2 % of numbers set by the shaper differ from the cell (by up to 608 units, 7.7 px at 26 px,
   for runs of ones). A tabular number must not do that; `.tabular()` therefore *means* "digits on the cell,
   no kerning among digits, point and minus", and equals the shaper only in faces that do not kern them. A
   number that is not tabular needs the 12 x 12 kern table per pair: still arithmetic, CPU stage because its
   width moves. Of the seven macOS faces checked, only Arial has a cell under the table shaper: SF and Georgia
   have proportional digits (they need `tnum` through the full shaper, or a synthesised cell), Menlo's digits
   are under a contextual rule, and Helvetica, Times New Roman and Verdana have no simple-script table at all.
3. **The table shaper covers few real faces.** Item 2's last sentence generalises: on this Mac the fast path
   exists for Arial and DejaVu and for none of the system UI faces. So the general path (the vendored
   parley's shaper) is what most applications' text will take, at 273 instructions per byte grafted or 1,968
   upstream, and there the memo is worth 100x per hit, not 2x. The façade's round-5 finding ("a shaped-string
   cache costs more than re-shaping") is true only inside the table shaper's coverage.
4. **The line record should name the memo's own lanes.** G1's prediction held (the placed form's kernel setup
   is 9 instructions per glyph cheaper) and was beside the point: producing the placed form costs 31 per glyph
   at every mount, re-break and size (S1: 247,000 against 94,600), setup is under 5 % of painting a glyph
   even with a naive blit, and the shaped form is 6 bytes per glyph, size-independent and position-independent
   (so a constant string's glyphs can be rodata). Recommended to the raster lane: the shaped form, with the
   pixel position computed where the glyph is binned (TEXT-SEAM B is to be revised accordingly). The number
   still missing is the GPU's cost of that multiply and slot load per glyph per tile.
5. **A plain `String` that grows costs a compare of the whole old text** (0.52 instructions per byte: 53,000
   at 100 KiB) before the append's 700. An append-aware source (`TextState`, or the relation journal of a
   list of lines) does not. This is the one place where "state is plain fields, the framework compares" has a
   cost proportional to the size of the state; it is small in wall time and stated here so it is a choice.

## 5. Counters on ilo-prod

`bench/results/r8/text/stage-ilo-miss.tsv`, `stage-ilo-fill.tsv`, `parley-ilo.tsv`: ilo-prod (Ryzen 7 3700X,
Zen 2), `taskset -c 8-15 nice -n 19`, `dist`, commit `9ebb8a9d89`, DejaVu Sans, load average 3 to 8 (shared
host). Counters of the calling thread in groups of three events (`BOUGH_PMU`); on Zen 2 `cache-misses` are L2
misses and there is no last-level-cache event. The shaper has no vector kernel on x86-64 (the NEON kernel is
aarch64 only), so shaping is 14.8 instructions per byte there against 7.8 on the Mac. The three mutants stop
at their gates there too. Per unit of the cell (per text, paragraph, step, append or the whole cell):

| cell | instructions | cycles | L1d misses | L2 misses | branch misses |
|---|---:|---:|---:|---:|---:|
| memo hit, hashed, 16 bytes | 144 | 71 | 4.0 | 0.1 | 0.0 |
| memo hit, key known, 16 bytes | 90 | 42 | 1.7 | 0.0 | 0.0 |
| re-shape, 16 bytes | 332 | 129 | 2.2 | 0.0 | 0.0 |
| memo hit, hashed, 256 bytes | 483 | 282 | 12.7 | 0.0 | 0.1 |
| re-shape, 256 bytes | 3,616 | 848 | 5.4 | 0.0 | 1.0 |
| `block` at a new width, no table, per paragraph | 466 | 397 | 12.2 | 5.4 | 6.5 |
| `block` over a table, per paragraph | 259 | 139 | 5.2 | 0.4 | 1.7 |
| the walk over a table, per paragraph | 530 | 162 | 5.5 | 0.1 | 2.5 |
| 1 px sweep step, 2,000 paragraphs (2.2 % re-broken, no table) | 53,940 | 40,082 | 1,189 | 579 | 472 |
| append 6 bytes at 1 / 10 / 100 KiB | 688 / 686 / 683 | 261 / 314 / 375 | 1.7 / 4.2 / 4.1 | 0.1 / 0.5 / 0.5 | 2.7 / 2.3 / 2.9 |
| `set` with a `String` at 100 KiB | 14,582 | 8,238 | 3,325 | 43 | 4.6 |
| type one character, 1,000 / 300,000 lines | 1,817 / 1,890 | 938 / 725 | 13.3 / 0.8 | 0.5 / 0.0 | 6.0 / 5.6 |
| constant label from the image, key known | 83 | 22 | 0.2 | 0.0 | 0.0 |
| kernel setup per glyph, placed / shaped | 12.2 / 25.8 | 4.1 / 9.7 | 0.2 / 0.2 | 0.0 / 0.0 | 0.0 / 0.1 |
| chat, 20 exposed messages, cold / warm | 113,981 / 98,375 | 36,488 / 44,064 | 2,319 / 1,386 | 3.5 / 76 | 55 / 46 |
| chat, size-only measure of 1,980 messages | 7.19 M | 3.27 M | 14,507 | 2,075 | 20,351 |
| ide, 130 line records, cold / warm | 155,203 / 132,455 | 60,307 / 68,428 | 3,462 / 1,971 | 110 / 79 | 72 / 38 |
| parley upstream: re-break one paragraph | 22,799 | 13,914 | 120 | 38 | 45 |
| parley upstream: 20 exposed messages | 10.37 M | 3.37 M | 19,955 | 333 | 12,069 |
| parley grafted: 20 exposed messages | 1.45 M | 0.79 M | 12,019 | 646 | 4,493 |
| parley upstream / grafted: append 6 bytes at 10 KiB | 22.4 M / 2.87 M | 8.27 M / 1.54 M | 111,242 / 13,537 | 6,515 / 1,991 | 25,837 / 9,403 |

What the counters say that the instruction counts did not:

* **The no-table `block` loses on the memory hierarchy.** Per re-broken paragraph it takes 12 L1 misses, 5.4
  L2 misses and 6.5 branch mispredictions (it touches the text and the pen lane at scattered places and its
  byte scans end unpredictably) against 5 L1 misses, 0.4 L2 misses and 1.7 mispredictions for the table form
  (five sequential lines), and 397 cycles against 139. Section 4's item 1 stands for a paragraph's *first*
  measure and for memory; for a paragraph that is re-broken repeatedly the table is 2.9x cheaper in cycles and
  pays for its build (about 2,300 cycles) after nine re-breaks. Under ROUND8's cost model (chained lines
  first) the right structure is in between and is not built: the end pens of segments alone (4 bytes per
  word, sequential), made the second time a paragraph's interval ends.
* A memo hit is 2 to 5 L1 misses in a working set a little over L1's size and no L2 miss; the two dependent
  lines of the design are what is being paid.
* The sweep step is miss-bound (0.74 instructions per cycle): of its 579 L2 misses about 240 are the 44
  re-broken paragraphs and the rest is the bench's own per-paragraph bookkeeping read at random.
* `set` at 100 KiB costs 3,325 L1 misses per append: the compare walks the whole text through the cache.
* Document size does not show in typing: 1,817 against 1,890 instructions, and fewer misses at 300,000 lines.

## 6. Refuted or failed, in one place

1. Own: a break table as the cached form of a paragraph (TEXT-SEAM A1 as first written). Dearer to build than
   shaping; removed for byte-shaped text.
2. Own: T3 as written (arithmetic equals the shaper). False in a face that kerns digits.
3. Own: G1's criterion (kernel setup) as the thing that decides the glyph form.
4. Own: "no break table" as the whole answer. It wins the first measure and the memory; on ilo-prod it loses
   every later re-break by 2.9x in cycles and 13x in L2 misses (section 5).
5. Own predictions missed: T1 hit (1.7x) and miss ratio (1.55 against 1.15); T2 `block` (2.5x without the
   table); T2c (the table's bytes); T4's constant (1.6x) and the compare (10x); T6 (1.9x); S1 through a cold
   memo; S2 on DejaVu; T5's bound on DejaVu.
6. Own: the first key function (byte-assembled) made a hit as dear as shaping; `#[cold]` on the memo's miss
   path made the path that *is* first paint compile for size.
7. Brief: "a number write should be a word". True of a tabular number in a box whose size does not depend on
   it (the expr lane's `Num` already measures it: the exposed edit, 1,566 instructions of settle). Not true of
   a proportional number, whose width feeds layout: that one is a Turn-stage loop over digits.
8. Brief: "constant strings shaped at build time into glyph-instance columns in rodata". The lookup is built
   and measured (77 instructions, the key folds); **the build step that produces the image is not built**, and
   a constant label's mount saves 90 instructions of shaping, not the 3,110 the façade spends: most of that
   was never shaping.

## 7. Second pass: text on the new core (after Amendments 5 and 6)

Priorities were reset by the lead: the scoreboard is gui-bench, the `e2e` lane's app has no real text, startup
must not parse or discover fonts, typing and search are singled out. The branch now sits on `r8/expr`
`054b83c929` (merged), with `r8/e2e` `249cd1e47f` merged before it.

### 7.1 What was built (commits `df8abc60b0`, `6907f1fe03`, `69f1c0da75`)

* **`crates/bough-face`** (no dependencies, `no_std` + alloc, about 600 lines): a face at a size as a prepared
  image read in place (`Face::new` is `const`), `Face::shape` (tokens and pens by arithmetic over the bytes:
  the table shaper's token model with its pair steps deduplicated into row and column classes, ligatures
  replayed), `Glyphs` (shaped lines as memoised ranges of two shared lanes, 6 bytes per glyph; a line is a
  two-word `Run`), `Face::spans` (the kernel: coverage rows cut to a clip, the pen rounded by one integer
  rule), `num::Num` (a number's glyphs and pens by arithmetic, with a least digit count for `{:05}`).
  `data/DejaVuSans-14.face`: 26,015 bytes, baked on the Mac and, byte for byte the same, on ilo-prod from the
  benchmark's own font file.
* **`bough_text::stage::bake`** (and `examples/bake.rs`): the build step. It writes the image from the table
  shaper's model and the façade's rasteriser. What it does *not* bake is strings: section 7.3 says why.
* **`bough-x/src/text.rs`**: `Line<C>` (operands: a `Run` and a colour expression) and `Digits<D, P, V, C>`
  (a `NumArch` with `PAD`), `TextCx` (what a backend reads). `Backend::line` has a default that draws
  nothing, `NumArch::PAD` defaults to 0, so existing backends compile unchanged. The oracle raster paints both
  from real coverage when given the faces and lanes.
* **Scenes and gates**: `scenes/rows.rs` (gui-bench's `rows` and `table` shape) and `scenes/search.rs` (its
  `typing` and `search` shape: a one-line input with a caret, a filtered list), `tests/text.rs` (five tests:
  retained == full redraw == immediate == fresh mount over random scripts in mount-once and re-describing
  mode; typing, deleting and caret moves through the real event path; nothing unexposed is shaped at 100,000
  rows; re-entering rows and caret moves shape nothing), `--features mutant_line` fails at step 5 (the first
  form of the scene did not catch it: every colour change of a line coincided with its row's fill changing,
  so a text-only colour word was added). `crates/bough-text/tests/face.rs` holds the image to the shaper and
  the rasteriser on 4,000 random Latin-1 and ligature strings at two sizes, refusals included.

What the types decide in `scenes/rows.rs`, none of it written: "Row " and "rows" are shaped once and found
again; a row's label is shaped when the row is exposed; the index, the value and the count are never shaped;
every colour is an expression over the block, so hover and the text tint cost no text work at all.

### 7.2 Bounds and results

I did not write predictions of my own before running `text_x`: this pass was built and measured in one go,
against bounds that others had written first. Those are what the verdicts below refer to:

| id | bound | from |
|---|---|---|
| F3 | text on the way to the first frame: no font file opened, under 50 µs for the frame's glyph placement | e2e journal section 2 |
| F4 | mount by exposure within 10 % between 100 and 100,000 rows | e2e journal section 2 |
| I5 | typing: handling at most 0.05 ms | ROUND8 Amendment 5, e2e journal section 3 |
| A6 | rows entering a filtered result: their labels were shaped before, so re-entry shapes nothing | ROUND8 Amendment 6 as relayed by the lead |

`bench/results/r8/text/text_x-{mac,ilo,ilo-cold}.tsv`, `stage_start-{mac,ilo}.tsv`; commit `69f1c0da75`;
ilo-prod at `taskset -c 8-15 nice -n 19`, load 5 to 7. Below pixels: a step ends at the damage bitmap.

| cell | Mac instructions | ilo-prod instructions / cycles | ilo-prod, every line cold: cycles / L1 misses / L2 misses | verdict |
|---|---:|---:|---:|---|
| rows mount, 3,000 rows | 82,870 | 72,324 / 31,472 | 91,651 / 806 / 1,053 | F4 holds |
| rows mount, 100,000 rows | 83,952 | 72,405 / 37,613 | | F4 holds (1.3 % apart on the Mac, 0.1 % on ilo-prod) |
| lines shaped at mount; glyph state | 14; 6,356 bytes at both sizes | | | asserted by `nothing_unexposed_is_shaped` |
| idle | 42 | 43 / 19 | 1,692 / 10 / 44 | |
| hover a row | 2,720 | 3,603 / 1,113 | 18,583 / 134 / 346 | |
| text colour alone, every exposed line | 1,700 | 2,015 / 640 | 10,420 / 80 / 192 | no text work: the block compare and the delta |
| scroll three rows | 3,924 | 4,354 / 1,456 | 19,161 / 127 / 301 | |
| scroll a page (24 rows enter, 72 records) | 17,703 | 20,476 / 6,155 | 28,895 / 236 / 369 | 740 per row, memo hits included |
| label edit on an exposed row | 1,378 | 1,496 / 649 | 14,478 / 103 / 322 | one line shaped |
| value edit on an exposed row | 1,021 | 1,148 / 425 | 11,596 / 78 / 223 | nothing shaped |
| **type a character** (no filter) | 3,422 | 3,694 / 1,251 | 28,081 / 132 / 481 | I5 holds below pixels: 1,251 cycles warm; with every line cold 28,081 cycles, about 8 µs at this host's clock, against 50 µs |
| caret move | 2,345 | 2,489 / 817 | 21,725 / 119 / 364 | no memo traffic and no row described (asserted); the cost is the event path and the settle |
| type a character with the scene's filter, 3,000 / 100,000 items | 833,286 / 27.7 M | 924,605 / 30.8 M | | the filter is this scene's plain scan of every label per key, not text: see below |
| narrow and widen (24 rows re-enter) | 198,728 | 227,290 | | A6 holds: one line shaped in 64 steps (the empty query); text per re-entering row is a memo hit, about 100 instructions |

Text on the way to the first frame (24 rows and a header; `stage_start`):

| | Mac | ilo-prod |
|---|---:|---:|
| **with the image**: shape the frame's lines | 44,546 instructions, 2.0 µs | 39,395, 4.2 µs |
| with the image: and lay every glyph's coverage into the frame | 563,840, 25 µs | 649,424, 40 µs |
| without: read the font file | 29 µs | 78 µs |
| without: shaping tables, file cache warm / built from the font | 163 µs / 1,015 µs | 148 µs / 1,604 µs |
| without: construct the shaper | 23 µs | 27 µs |
| without: strike and rasterise the frame's glyphs | 128 µs | 161 µs |
| without: shape the frame's lines | 1.5 µs | 2.6 µs |
| **without, in all** (warm / cold), before font discovery | 345 µs / 1.2 ms | 417 µs / 1.87 ms |

F3 holds: 40 µs on ilo-prod with a deliberately plain blit, no system call (the image is a `static`). Font
discovery, which the façade does twice through a child process (about 18 ms warm, 115 to 150 ms cold: e2e
journal section 1), is the large term the image removes; parsing and rasterising are 0.4 to 1.9 ms.

### 7.3 What this pass changed or refuted

1. **The build step bakes the face, not the strings.** The brief asked for constant strings shaped at build
   time. Section 4 measured what that saves: 90 instructions per label. A face baked at a size makes *every*
   covered string cheap at run time (12 instructions per byte, no font machinery), needs no knowledge of the
   program's strings, and is what removes the first frame's 0.4 to 1.9 ms and its font discovery. The first
   frame of the rows scene shapes 14 lines in 2 to 4 µs. A string image on top would save about 1 µs.
2. **`Num` needed a pad.** `Row 00012` is a constant and a five-digit number; without `P` it would have been
   a shaped string per row and memo traffic while scrolling. With it, scrolling 100,000 rows shapes nothing.
3. **Typing is 3,700 instructions on the new core**, of which text is the one line's shaping; the façade's
   cell is 0.43 ms of handling. The caret is a word of the block.
4. **Search is not a text problem.** With the labels in the memo a row re-entering the result costs a hit.
   The 0.9 M (3,000 items) and 31 M (100,000) instructions per key in this scene are its naive filter: a
   lower-cased substring scan of every label. That is the expr lane's incremental filter to replace; an index
   on the text side (trigrams over the labels, built off the interaction path) is the other half and is not
   built.
5. **The glyph memo's garbage is not collected.** Every keystroke and every label edit leaves a dead run. The
   lanes grow by 6 bytes per glyph of every distinct line ever shown (9 KiB after the whole `text_x` run).
   `Glyphs::clear` exists; the policy (clear and re-describe when the lanes pass a budget: one full repaint)
   is not wired.

### 7.4 Not done in this pass, and where it stands

* **The parley shaper behind the memo** (most real faces miss the table shaper): not built. What the layout
  lane asked, the worst case of an exact `block(w)` for a dozen exposed rows of uncovered text, from section
  4's and 5's parley rows on ilo-prod: grafted parley, 303 instructions and 142 cycles per byte to shape plus
  22,800 instructions and 14,600 cycles per paragraph to break; a dozen 218-byte messages are about 1.1 M
  instructions, 0.55 M cycles, 0.15 ms. Upstream parley: 2,178 and 899 per byte, about 5.9 M instructions,
  2.5 M cycles, 0.7 ms. A later re-break of the dozen is 0.27 M instructions.
* **The structure between no table and a full break table** (segment end pens on the second re-break): not
  built.
* **The editor path beyond one line**, selection, `TextState`, IME: not built. The field in `scenes/search.rs`
  is the façade's default form (a `String`, the caret in plain state).
* **`text(..)`, `para(..)`, `text_input(..)` as elements over `Element::drive`**: waits for the expr lane's
  element layer; `scenes/rows.rs` and `scenes/search.rs` are written against the core, as its own fixtures
  are.
* The memo of section 3 (`bough_text::stage::memo`) and `bough_face::Glyphs` are two copies of one design;
  the first carries break tables and the image form, the second is what the core uses. To be folded into one.

## 8. Next, in order

1. The `e2e` lane takes `bough-face` and `bough_x::text` (its backend implements `Backend::line` and reads
   `NumArch::PAD`); the committed image replaces its hand-built `bough-glyph`.
2. The raster contract (TEXT-SEAM revision 2) answered by the raster lane.
3. The glyph memo's collection policy; then the full shaper behind the memo, off the interaction path.
4. Elements (`text`, `para`, `text_input`) on the expr lane's element layer; `Measure` answered by
   `block_bytes` for `para`.
5. The filter index for search; selection and `TextState`.

## 9. The first pass's list of next steps (superseded by section 8)

1. Wire a `Line` record into `bough-x` (worktree copy first, then through the expr lane): a paint whose
   operands are a memo range and a palette; the chat and ide scenes' text on the new core with the layout
   lane's `Measure` answered by `block_bytes`. This is what moves the shared objective.
2. The general path: the vendored parley's shaper behind the same memo for text the table refuses, off the
   interaction path (threads lane), with the break opportunities as a bitmask lane.
3. The build step for the image (run the first mount at build, write the memo, `include_bytes!`), with the
   layout lane's shapes and the expr lane's columns if they take the same route (ROUND8 reformulation 3).
4. The raster seam: revise B2 to the shaped form, agree binning by sub-range, then quarter-pixel variants and
   the exactness gate on both executors.
5. `TextState`, the selection scope and hit testing (TEXT-API order 3 and 4), on line records.
6. Atlas residency by exposure; the scalable archetype for animated size.

## 10. Third pass (the last batch): predictions, written before the code

The lead's order: the raster seam (fallback ready), the glyph memo collected and the two memos folded, the full
shaper behind the memo off the interaction path, elements when the expr lane publishes its layer.

| id | hypothesis | prediction | kill |
|---|---|---|---|
| C1 | dead runs are bounded by clearing the lanes when they pass a budget and re-describing what is exposed (the exposed records are the only holders of runs) | glyph state never exceeds the budget plus one turn's growth over 20,000 keystrokes and label edits; the pixel gates hold across every collection | any frame after a collection that differs from a full redraw, or state above twice the budget |
| C2 | what a collection costs | one re-description of the exposed records plus re-shaping the lines on screen: at most 80,000 instructions on the rows scene; its damage is every text record's rect (the runs' words change though their pixels do not); at a 256 KiB budget it happens once per 2,000 keystrokes or more | above 300,000 instructions, or more often than once per 500 keystrokes |
| C3 | one memo: `bough_face::Glyphs` with the shaper as a parameter serves the table shaper, the prepared face and the full shaper; `bough_text::stage::memo` (and its string image, refuted in 7.3) is deleted | the stage bench's memo cells within 10 % of section 4's on the Mac (hit at 16 bytes about 129 hashed, 90 keyed; miss about 12 per byte) | a hit more than 1.3x dearer than before |
| P1 | text the prepared face refuses is shaped by the vendored parley on a worker and lands as runs in the same lanes, drawn from a slot table that grows | the frame that first shows such a line costs the interaction path at most 2,000 instructions more than a covered line (a queue push); the line is blank until the worker's result is taken at a later turn; taking it is one re-description | the interaction path waits for the worker, or opens a font |
| P2 | the worker's cost for a first uncovered line (20 to 40 characters: Cyrillic, Greek, Arabic), fonts already loaded in the worker | at most 400,000 instructions and 0.3 ms on ilo-prod for shaping (from section 5's 303 per byte plus fixed costs), plus 27,000 per new glyph rasterised | above 3 ms |
| P3 | the first uncovered line of a process pays the font machinery once | at most 5 ms on the worker (font context, the face's tables); stated, not hidden | above 50 ms |
| R1 | if the raster refuses line records on the GPU: a line expanded to one glyph record each at mount | boxes and texels of the expansion equal what `Face::spans` covers, glyph for glyph (a test); cost about 30 instructions per glyph (G1) | any pixel differing between the expansion and the line record on the oracle |

## 11. Third pass: results, the independent review's findings, and what is parked

The round was closed (Amendment 7) while this pass was in flight. What landed, against section 10's
predictions; Mac M3 Pro, `dist`, load 8 to 38 (instructions are the column to trust), working tree of the
commit that adds this section.

| id | outcome |
|---|---|
| R1 | **Holds.** The raster lane took the fallback for its executors (a line expanded to one `Glyph` record per glyph at mount) and will take the line record on the GPU later. `Face::expand` gives the boxes and texels; `tests/face.rs` holds the expansion to the line's spans at three cuts. For that binding the face's coverage became one byte image at a common stride: **stride 256** (the image is now 31,248 bytes; format magic `BFC2`). The sink-side adapter itself is not built: `bough-x`'s sink gives every row of a list one shared shape, and an expanded line is a list of records each with its own rect; where `Glyph` records are emitted (the raster's scene) is where `expand` is called. |
| C1 | **Holds.** `bough_x::text::collect` (clear when over a budget, re-describe what is exposed). Over 20,000 keys of ever-new lines under a 24 KiB budget the memo stays under twice the budget and frames checked along the way equal the full redraw (`the_glyph_memo_is_collected_and_frames_stay_exact`). Runs now carry the memo's generation: a run from before a clear draws nothing instead of another line's glyphs. `clear` gives the memory back; `bytes` and `over` count capacity and the table. |
| C2 | **Cost holds, frequency in between.** One collection on the rows scene: 41,091 instructions, 389 tiles damaged (every text record's rect: the run words change though the pixels do not). Frequency: a never-seen 30-character line costs about 250 bytes, so a 256 KiB budget lasts about 1,000 such keystrokes (predicted 2,000 or more; kill under 500); typing that revisits lines hardly grows the memo at all (3 KiB in 4,000 keys). |
| C3 | **Folded; the hit got dearer.** One memo, `bough_face::Glyphs`, with the shaper as a closure (`line_with`); `bough_text::stage::memo` and its string image are deleted (the bench keeps break tables beside it). Hit at 16 bytes: 148 hashed, 107 keyed, against 129 and 90 before: 1.15x and 1.19x (predicted within 1.10x; kill above 1.3x). The extra is the richer return (a `Run` with its face and generation, the advance). Miss: 12.7 per byte against 12.1. |
| P1, P2, P3 | **Not built: parked as a design** (below). |

### The independent review (`bough/runtime/lanes/independent-review/recent-20261112/REPORT.md`, batch 4)

| finding | state |
|---|---|
| T01 Critical: document edits could store invalid UTF-8 | **Closed.** `Doc::splice_at` and `Doc::split` refuse, with ordinary asserts, an offset inside a character at either end, and a newline in a splice. The review's two probes and four more are `stage::doc::tests::edits_inside_a_character_are_refused`. The property test's corpus was ASCII, as the review said. |
| T02 Critical: safe glyph helpers used `get_unchecked` on caller-supplied ids | **Closed.** `line::each_shaped` and `line::place` index the strike's slots checked (an unknown id has no ink); test `unknown_glyph_ids_have_no_ink`. `Glyphs::find`'s unchecked reads are of offsets the memo itself stored; `brk`'s unchecked loops assert their lengths first, as the review noted. |
| T03: padding repeats the leading digit above ten positions | **Closed.** Positions beyond a `u32`'s ten digits are zeros; decimals are capped at nine and digits drawn at 32. The review's probe is `num::tests::padding_beyond_ten_digits_is_zeros`. |
| T04 High: glyph identity had no generation or collection | **Closed in its first form**: generations on runs, a collector, memory returned, honest `bytes`. **What stands**: the collector is stop-the-world for text (clear, re-describe, repaint text rects); it is not exposure-aware or pinned, and publishing new lanes with the records that reference them is atomic only because both happen on one thread in one turn. An in-place compaction that rewrites the run words of line columns (a typed walk) would avoid the repaint and is not built. |
| T05 High: the fast path is a restricted language | **Partly closed.** What an application sees is now exact and stated: `Glyphs::line` refuses text the prepared face does not cover (a code point beyond Latin-1, a control, a pair that needs contextual shaping); `bough_x::text::line_or_stand_in` then draws the face's stand-in (`Face::shape_lossy`: each uncovered code point as `?`, an unkerned advance where a pair is refused) and says the line is not exact. The fixtures use it, so typed text no longer disappears (`refused_text_is_still_drawn`); the caret follows the stand-in's advances. **What stands**: there is no full shaper on the new core, so other scripts are question marks, not text; cluster-aware caret and selection, bidi, composition, fallback fonts and rich spans are not integrated; the memo's key is (face index, text) and does not carry features, language or direction. |
| T06: streaming and document cliffs | **Stands, as described.** `Flow::set` compares the whole old text (0.52 instructions per byte); `Flow::append` re-shapes from the last space, so one ever-growing unbroken token is O(its length) per append; a `Doc` block bounds lines, not bytes, so one huge line moves all its following bytes per edit, and a block split rebuilds the prefix lanes. What would close it: an append-aware source type instead of the compare; a restart point at the last *shaping-safe* glyph rather than the last space; blocks bounded in bytes with a huge line as its own chunked block. |
| T07: prepared assets need admission limits | **Closed.** `Face::check` (a `const fn`, so a `static` face is checked at compile time) validates the section sizes, every token's row and column class, every row of every slot's coverage against the atlas, every ligature entry, the scale and the stride; `Face::new` is that check or a panic. `tests/admission.rs` corrupts the committed image 3,000 ways: each is refused or, if admitted, cannot make shaping, spans, expansion or any slot's rows read out of range. The first run of that test found a hole in the check itself (a slot with height and no width), now closed. Size limits are refusals at bake time (`NoImage::TooLarge`: a glyph box over 255 pixels or a bearing beyond 127), and a line is at most 65,535 glyphs and a face index at most 255 (asserted). |

### Parked: the full shaper behind the memo (design, with section 10's predictions P1 to P3 unmeasured)

* A worker thread owns the vendored parley's font and layout contexts and the fonts' bytes. A job is
  (key, face family and size, the text's bytes copied); nothing of the application is borrowed.
* Its result is glyph ids and pens in 26.6 pixels (parley positions are pixels at the size, so such a face's
  scale is 1/64) and the coverage of glyphs not yet rasterised, into a *grown* face: the same slot and atlas
  layout as a prepared face, with a slot table that grows (16-bit slots) instead of 256 fixed tokens. A run
  names it by its face index, so `Line` and the kernels do not change.
* The interaction path: `line_or_stand_in` draws the stand-in and queues the job once per key. When results
  are taken (at the start of a later turn) they are entered into the memo under the exact key (`line_with`
  with a closure that copies), and the rows that showed the stand-in are described again. With no way yet to
  name those rows, that is one `redescribe`; the stand-in's key differs from the exact key so the two
  coexist until then.
* What the user sees until it lands: the stand-in (question marks for other scripts, slightly wrong spacing
  for refused Latin pairs), then the exact line; widths can change, so a content-sized box re-lays out once.
* Expected cost, from sections 4 and 5: about 303 instructions per byte to shape on the worker with the
  grafted parley (2,178 upstream), 27,000 per glyph rasterised for the first time, and the font machinery
  once per process.

### Parked: `text`, `para`, `text_input` as elements

The expr lane's element layer was not published before the round closed. The mapping is direct: `text(src)`
drives the sink with a `Line` (or `Digits` for a number operand with `.tabular()`), its `Measure` is the
line's advance (not asked when the box is definite); `para` answers `Measure::block` from `brk::block_bytes`
and emits one `Line` per line of the answer; `text_input` is the field of `scenes/search.rs` (a `String` in a
one-row relation, the caret a word of the block) with selection as two more words. Prediction for when it is
built: the façade's programs in TEXT-API P6, P8 and P12 run unchanged in source, and a `text` element costs
what its `Line` costs in `text_x` (about 740 instructions per row of three records at mount).

### For the e2e lane: the editor path it asked for

What exists: `scenes/search.rs`'s field: edits as plain `String` mutations that push `Update(0)` on a one-row
relation (so one row is described again and one line shaped), the caret as the block word `CaretX` computed
from the advance of the text before it (a caret move shapes nothing and describes nothing), keys through
`World::text` and `World::key_down`. Selection is not built: it is two more block words (the pixel x of each
end) and a fill whose rect reads them, which needs a paint whose geometry is a Frame-stage operand; the
oracle kinds have none except `Outline`'s offset.

### What I would do next, and what I would not do again

Next: the full shaper behind the memo (above); elements on the expr layer; in-place compaction of the memo
through a typed walk of line columns; the search index. Not again: caching a derived structure before
measuring what it costs to build (the break table); a gate whose only reader of a mechanism is masked by
another (the first text mutant, which the row's fill hid); a property test over an ASCII corpus for code
that handles UTF-8; `get_unchecked` in a safe function on the strength of where its inputs usually come
from; a pass built and measured with no predictions written first (section 7.2).
