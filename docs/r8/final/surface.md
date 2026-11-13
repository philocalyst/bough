# surface lane: final (r8/surface dc6dbfb9e5, on r8/expr a2a2687662)

Checks run (Mac): `cargo check --workspace --all-targets` clean; `cargo test --release -p bough-x` (anim 2, events 12,
reflect 7, review 7, sheet 4, table 1) and `-p bough-ax` (11) pass; mutants mutant_fast, mutant_removed (bough-ax)
and mutant_reflect (bough-x) fail their gates.

Numbers (events_x, dist, M3 Pro, load 19-34; identical at 100 / 10,000 / 1,000,000 rows): allocations per event 0
(were 4 for pointer-onto-row and click). Instructions dispatch / with settle: pointer onto a row 1,270 / 2,113;
same row 954 / 1,000; click 3,397 / 4,288; key 120 / 324; text into a focused field 233 / 1,023. Warm under 0.3
microseconds; 3,300-15,000 cycles after a cache sweep. sheet_x hover 6,826 (6,909 before events existed).

Kept: reflection in the core, zero cost unused, tied to its store schema; source locations per site; rows on
demand; events as pick -> place -> trampoline with captured closures; zero-allocation event path over handler
columns only; focus that follows its item through insert/remove; a press cancelled when its slot shows another row;
moving records hit where drawn; publication revision so an accessibility no-op is five compares; accessibility
projection (100,000-row sheet under 2,000 nodes; theme/hover/glide project 0 nodes).
Refuted: events cost nothing on hover (+685, of which ~430 column walk and ~430 allocations: both removed); press
identity check free (click 2,663 -> 3,397); revision bumped by every list pass.
Review: closed with probes as tests: C01 (events part), C03, C05, C06, C09, S01, S02 (part), S03, S04. Standing:
S02 rest (typed semantic components), S05 (item keys; no native adapter), C08 (lexical parents; Tab cannot be
declined), hover leave lost on a reused slot.
Open: NSAccessibility adapter with lazy children (large); item keys in relations (medium); typed semantic components
(medium); lexical parent positions, key-up/repeat/function keys (medium); text seam (medium); cold line counts
(small); leave on slot release (small); press identity without a path walk (small).
Questions for the user: screen-reader window policy; typed comparisons by default; NSAccessibility first; should
`Rel` carry an item key and generation so focus, presses and accessibility nodes name items, not positions.
