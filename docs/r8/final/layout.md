# layout lane: final (r8/layout fc830898f4, on r8/expr a37da490a2)

Checks run (Mac): `cargo check --workspace --all-targets` clean; `cargo test --release -p bough-lay` green (extent 3;
oracle 2,000 random trees x 8 widths twice; sheet / 400 ide lines / 400 chat rows; reflection); mutants
`mutant_gap`, `mutant_valid` fail. At the close bough-x's element layer was ported to the kept form (Shape, one layout
function, a Memo per list of elements; r8/expr 464abd2903) and its lists to Uniform and Extent::window; the e2e
binary still places rows by arithmetic. Nothing measured on the remotes.

Numbers (M3 Pro, dist, instructions per row, load 17-35):
- monomorphised place: sheet 25,486; ide line 4,801; chat 56,086; compile +13.4 s / +498 KB (sheet), +204 s /
  +2,748 KB (chat) per row type. Tuple-state form: sheet 28,611; chat 58,794.
- kept form (flat words, one layout function, memo): hit 2,850 (shape hash 2,677); copy of a kept slice 200; miss
  52,730 sheet, 12,657 ide, 91,359 chat, 38,554 chat size-only; compile +0.21-0.54 s, +3.5-17.1 KB per row type.
- cephal on the same trees: cold 346,420 sheet / 298,739 chat; kept tree at a new width 21,600 / 87,079.

Kept: flex arithmetic equal to cephal; validity intervals (41 % of random answers hold over more than one pixel);
layout as data with one layout function; row height kind from the type; i64 extents with a viewport-local frame;
reflection of why an element has its size.
Refuted: monomorphised place cheaper than a key compare (25,486 vs 203); <= 600 for a chat row (56,086); type-level
layout with linear compile time (204 s, 2.75 MB for one chat row type); tuple items fold (28,611); shape hash folds
to a constant (2,677 per row); one flat function beats a retained solver (52,730 vs 21,600); chat resize as a range
check (0.0 %).
Open: Heights (variable rows) in bough-x's lists and layout in the benchmarked binary (large; chat resize estimated
1.4 M vs 181 M);
a sound shape check for Fixed lists without a per-row hash; parametric answers (review L05); sparse exceptions over
uniform spans (L02); guess(w), pending measures, (row, offset) anchor; counters on the remotes.
Review: L03 closed in bough-lay and, at the close, in bough-x's list (Uniform::new, Extent::window, heights refused
where described; r8/expr ded8ff4e25); L02 and L04 stand, stated; L06 not carried forward.
Lesson: read one function's assembly before believing the compiler folds a general algorithm.
