# expr lane: final (r8/expr ded8ff4e25: the lane's WIP c6fce8e823 with r8/surface and r8/layout merged and finished by the closer)

Checks run: ilo-prod at c6fce8e823 and 911a16b590 (the surface merge), `cargo test --release -p bough-x -p bough-expr
-p bough-lay` (and bough-ax) green, mutants failing. Mac at ded8ff4e25 (release): bough-x anim 2, el_small 2, events
12, reflect 7, review 7, review_core 12, sheet 4, sheet_el 4, table 1; bough-lay extent 3, oracle 4; bough-ax 11 + 1
ignored; mutants delta (el_small, sheet_el), journal (events), moves (sheet), sweep (anim, sheet_el), reflect, ax's
fast and removed, lay's gap and valid all fail. sheet_el is the façade's 10,000-row sheet: retained == full redraw
every step, == immediate and == fresh mount whenever no transition is in flight. No miss counters.

Numbers (M3 Pro, dist, instructions per step p50 of 32; identical at 100 / 10,000 / 1,000,000 rows but insert):
- sheet at 13e9ed3fa4 (load 18): idle 53, hover 6,909, theme 31,010, exposed edit 1,433, unexposed 905, select 694,
  still frame 6, re-describing idle 16,087; insert 11,071 (100 rows) to 7.5 M (1,000,000: the app's Vec::insert);
  mount 234-270 K, 33.7-43.4 us wall; 764 records, 41,728 bytes of columns at any size. Façade recorded at
  13253be95f (not re-run): mount 3.58 G / 232.6 ms, hover 15.38 M, theme 57.35 M, idle 241.
- hot uniforms (479687b4f1, load 5.6): glide 174 for 252 tiles (was 631); endless animation 159 / 2,049 per frame
  at 32 / 2,048 records, no record evaluated; pointer words 8.7 per reading record per event.
- final tree (lane's last command, load 96 at its end; identity with c6fce8e823 unverified): idle 68, hover 8,049,
  theme 31,030, exposed edit 2,612, unexposed 2,086, still frame 11, mount 219 K / 28.0 us. The review fixes cost
  +1,179 per edit, mostly in `part_journal_only` (359 -> 1,538; cause not attributed).
- compile (7f20c17d42, load 5.7, one run): 0.130 / 0.158 / 0.175 s, 12.5 / 11.8 / 11.9 KB per distinct component at
  N = 8 / 32 / 128. Element-layer sheet on typed layout: type-checks in 1.8 s, release codegen killed after 3 h 32 min unfinished;
  on layout as data (the close): the test builds in 4 s.

Kept: a turn is one block compare plus an epoch word (no write barrier); damage as eval(old) != eval(new) over
exposed pages, exact for opaque functions and vectorised (about 550 per page of 64); one column per call site, no
per-record dispatch; one view, four sinks; lists as a ring on a relation's journal; Time never differentiated; a
world-wide column table (block to page in three dependent loads at any depth, was five); linear compile time for
flat components. Element layer (el.rs): `fn view(&self) -> impl Element<Self>`, `div`/`row`/`text`, `sig!` by
`offset_of!`, captures as record words, `.transition`, indexed `each`, rows laid out by bough-lay; holds the oracles
on a small view and on the façade's sheet (scenes/sheet_el.rs, two stated differences); mount describes again once
the view's fields are known (the first description read them as zero).
Refuted: "opaque closures have the weakest derivative"; "Frame state costs no per-record CPU work" (about 9 per
exposed record); two chained lines (four, then five through nested stores); own predictions: mount 187 K vs <= 60 K,
hover 7,173 vs <= 4,000, theme 30,755 vs <= 12,000, unexposed edit 1,067 vs kill 500; endless frame flat in records
(2,049 at 2,048); the element layer on typed layout for a view of the sheet's size (codegen did not finish; layout as data fixes it).
Review: closed with the probes as tests/review_core.rs: C01 (view identity at the entry points; the close made the
store tree private, the slot table, Store::paint and Store::delta crate-private), C02, C03, C04, C05, C06, C10, C11,
C13; C09 on r8/surface; L03 in bough-x's list (Uniform extent, sizes refused where described). In part: C07 (4,096
columns, list rows and heights admitted; paths of six u16 and 16-bit reader incidences stand), C12 (bytes_all).
Standing: C08 (ancestry by overlap), C10's purity half, a 64-bit hash as the list memo's key.
Open: the element-layer sheet measured against the core's hand-written sheet (small); runs inside a row and the
layer in e2e's app (medium each); text on the element layer through bough-face (medium);
an incremental search filter (large; 30.8 M a keystroke at 100,000 items); the order lane, designed, and arenas
(medium); the composition note, cited but never written, and the edit regression (small). Journal section 10 is
written (the close).
Questions for the user (none recorded by the lane): is a run-time view check enough for C01; may components bound types?
Lesson: scaling measured on flat generated components said nothing about one deep real view; compile it early.
