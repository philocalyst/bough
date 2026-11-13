# text lane: final (r8/text-main 5d7a6bc39a, on r8/expr a2a2687662)

r8/text 89b9e8cc5f is the same 11 commits on an older base (with r8/e2e 249cd1e47f and raster's bough-workloads).
Checks run (Mac, release, on 5d7a6bc39a): `cargo test --release -p bough-x -p bough-face -p bough-expr` green (among
them text 7, events 9, admission 3); `-p bough-text --lib` (4) and `--test face` (2) green; `--features mutant_line`
fails tests/text.rs as it must; `cargo check --workspace --all-targets` finished with no error. The stage bench's gates
and mutants (STAGE_MUTANT=hi|lo|join each stop it) ran on r8/text before the rebase, not on text-main. ilo-prod ran the
same suite green, mutant failing, on an earlier text-main (532d89f6ae, on r8/expr 054b83c929), not on the final commit.

Numbers (instructions on the calling thread, `dist`; Mac M3 Pro load 12-20; ilo-prod `taskset -c 8-15 nice 19`):
- staged chat first paint, 20 exposed messages: 94,600 cold (ilo-prod 113,981); grafted parley 1.37 M, upstream
  9.49 M on the same strings; size-only measure of the other 1,980: 4.02 M (7.47 M through a cold memo).
- append 6 bytes at 1 / 10 / 100 KiB: 715 / 700 / 693 (through a plain `String` 1,993 / 6,524 / 52,807); a character
  typed in a `Doc` of 1,000 / 300,000 lines 1,202 / 1,257 (Arial); `guess(w)` 93-98 % of rows exact; a 1 px sweep
  re-breaks 2.1 % of paragraphs a step.
- on the new core (text_x at 69f1c0da75, ilo-prod load 5-7, to the damage bitmap): rows mount 72,324 / 72,405 at
  3,000 / 100,000 rows; type a character 3,694 (1,251 cycles; 28,081 with every line cold); a search key 0.92 M /
  30.8 M at 3,000 / 100,000 items (the scene's label scan, not text); one memo collection (Mac) 41,091, 389 tiles.
- wall time, first frame's text on ilo-prod: 40 µs with the face image; 417 µs warm / 1.87 ms cold without it.

Kept: a face at a size as a prepared image (`bough-face`, 31,248 bytes, checked by a `const fn`): no font opened
before the first frame; a line as a two-word run over shared 6-byte glyph lanes; `Digits` with a pad (scrolling 100,000
rows shapes nothing); the caret as a block word; `block(w)` with exact validity intervals; flat appends; one memo with
generations on runs and a collector (20,000 keys under twice a 24 KiB budget, frames equal a full redraw); refused text
as a stand-in; `Face::expand` as raster's fallback (equal to the line's spans at three cuts).
Refuted: break table as the cached paragraph form (12 per byte to build, 7.8 to shape); "no table" as the whole answer
(ilo-prod: 397 against 139 cycles per re-break); tabular digits equal the shaper (Arial kerns "11", 5.2 % differ);
the table shaper covering real faces (one of seven macOS faces); placed 12-byte glyphs (31 per glyph to save 9);
baking strings (saves about 90 per label); a `String` compare at 0.05 per byte (0.52); memo hit within 1.10x (1.15x).
Review: closed with tests: T01 (the review's two probes and four more), T02, T03 (the review's probe), T07 (3,000
corruptions of the image; the first run found a hole in the check itself, closed). T04 closed in its first form;
standing: collection is stop-the-world (clear, re-describe, repaint text rects), not exposure-aware or pinned. T05
partly: other scripts draw as `?`, marked inexact; no full shaper, bidi, IME or fallback fonts; the memo key has no
features, language or direction. T06 stands. H05 stands: r8/e2e aafc8f0e47 still has `bough-glyph`, no `bough-face`.
Open: parley behind the memo on a worker (large; parked design, unmeasured); `text`/`para`/`text_input` on the element
layer (medium); selection, `TextState`, IME (large); GPU binning of line records, a search index, quarter-pixel pens
(medium each); segment end pens, ilo-prod re-run (small); `expand` adapter, memo compaction, T06 (size unstated).
Questions for the user (TEXT-API section 5): `TextState` or `String` as default editing state; static text selectable
by default; fonts bundled with system fallback, discovered, or registered; coverage contrast and LCD anti-aliasing.
Lesson: measure what a derived structure costs to build before caching it; never test UTF-8 code on an ASCII corpus.
