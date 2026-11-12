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
thread (`bough_mem::pmu`). ilo-prod is reserved; cache and branch-miss columns are empty until the lead frees
it.

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
