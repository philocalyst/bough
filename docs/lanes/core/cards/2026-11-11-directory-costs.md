# R6 core: measure the matrix, attribute the directory's superlinear cells, fix them cheapest first

lane: core; state: CARD; predecessor: 30cdd35181 + d6ef6b5e29 (lead's three defect fixes, cherry-picked)

Question: which instructions make a structural edit O(pages), and does each go away without taxing N = 1e3?

Change, in order, one commit each: (0) run the 204-cell `--cost-r6` matrix three times, no lock (instruction
counts); (1) attribute a front insert at 1e6 to `reindex_pages`, the directory column splices,
`Prefix::invalidate_from`, `fix_chunks` by counters around each; (a) `invalidate_from` honours its index;
(b) `fix_chunks` only recomputes chunks whose membership changed; (c) `Page::dir` not written eagerly;
(d) experiment: chunked directory, fanout 16/32/64, flat GPU columns kept derivable and their derivation charged.

Gates: `cargo test --release -p bough-core` (differential vs `oracle.rs`), harness oracle asserts, one new test
per fix that fails on the obvious wrong version (mutant run and recorded).

Thresholds: every existing cell at N = 1e3 <= 1.02x its baseline median after each change (cells below 200
instructions are counter-dominated: reported, judged at +4 instructions); a fix must cut its attributed share at
1e6 by >= 10x or it is reported as a loss. The chunked directory is rejected if any 1e3 cell passes 1.02x.

Negative control: the harness's omitted-move mutant; per-fix mutants named in each commit.

Stop: an oracle mismatch (diagnose before any table); a 1e3 regression that survives one revision.
