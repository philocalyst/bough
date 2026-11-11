# Settled: what not to reopen without a new representation

Each line closes a particular route, not the goal behind it. Before reopening one, name the representation change
that removes the stated reason. Full evidence is in `research/AUDIT.md` §15 and the cited RESULTS files.

**Closed in bough:**

* **Per-row hover signals.** O(rows) per crossing; hover is a paint write on two records (`crates/bough/INPUT.md` §4).
* **A separate hit index.** The record tree's page summaries already are one: 12.7 ns among 1,000 cards (INPUT.md §4).
* **Swallowing content events in the event monitor.** Measured worse (INPUT.md §4).
* **Per-column bar strips.** 32–65 K instructions/tick for −30 % tiles (bough-metal RESULTS).
* **Shaped-string caches on the update path.** Hashing and lookup cost more than ~8 instructions/glyph of table
  shaping (bough-text Attempt 5). A *mount-time* cache is still open (lane 4, RT5).
* **GPU shaping.** Exact, but not cheaper (bough-text Attempt 3).
* **Ligatures inside the SIMD lanes.** bough-text III.4.
* **Present paths with large commit costs.** CAMetalLayer drawables, CAMetalDisplayLink and AVSampleBufferDisplayLayer
  cost +292–700 K/frame; many small layers ~37 K per changed layer (bough-metal §12.3). Hidden-flip of *fixed*
  layers was rejected for safety, but zgx's timed tile-flip protocol reopens it (lane 5, RT1).
* **Interval lists, summed-area tables and bounded rect lists for damage.** All lost to tile planes on CPU
  (bough-damage-lab RESULTS).
* **The per-tile sequential bin scan (v7).** Failed. Its named successor, 2D binning with a parallel scatter, is open
  (lane 5, RT2).

**Closed in the archive's clean-room GPUI** (`archive/RESEARCH_BRANCHES.md`): rejections specific to its
rebuild-per-frame pipeline, which bough doesn't share.

* Text-witness replay.
* The delivery sweep.
* Region batching.
* Precise scroll repair.
* GPU text packets and tiles.

Their GPU sample-classification work (mask grid, partitions, pixel-uniform tiles) missed a 20 % target with real
8–13 % gains; it is open for bough's kernels (RT6, RT7).

**Closed by the design** (`designs/reactivity/final/00-model.md` §6, `approaches/`):

* hooks or positional state;
* a `cx` threaded through application code;
* a public `'ui` brand;
* universal MVCC;
* a general `#[component]` macro;
* `.children(...)`, `WeakEntity`, a public `Task`;
* any per-view static switch that changes semantics.

**Corrected claims:** the AWAGY "13 of 480 clicks dropped" was wrong. The driver sent 467 in that run and bough
counted all of them (`bench/awagy/README.md`).
