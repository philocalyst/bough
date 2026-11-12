lane: gpu
state: CARD
owner: GPU lane session
predecessor: aa9943e7ae50595f6af026773ee82b46a4b85ae0; nightly-2026-10-17; dist; direct Metal, default features
question: Which persistent or per-frame spatial representation addresses the measured GPU loss without taxing writes or flat scenes?
change: Measurement-only diagnostics of existing kernels and scene drivers; no renderer optimization before DESIGN-R6 review.
implication: Establish current per-frame costs and tile work, then propose a representation. This is not acceptance of an optimization.
platforms: This Mac, Apple M3 Pro, direct Metal; no remote work.
workloads: heavy and lab stream/scroll/both; animate; vlist scroll/fling/jump, default dimensions and input sequences. zgx 06debe9df86dbcb0a82d6074d01f98ace98a65b3 if buildable in a lane-owned scratch clone; otherwise identical bough builds in distinct directories as A/A control.
gates: Existing heavy/lab/animate/blink/micro/vlist/present/gpu_bounds exact gates; sabotage_copy must fail vlist. Diagnostic output must not be used as a pixel reference. Normal and instrumented full-output pixels compared where feasible. No production change in this card.
thresholds: Future candidate: every GPU after/before <=1.0, lab/vlist target <=0.6; every R0/R1 instruction cell <=1.02; exact pixels at every ring age. Diagnostic baseline has no speedup claim.
measurement: gpu_slope.py (3 repetitions), gpu_pairs.py (5 alternating-order pairs), gputime.c. Keep raw per-pair data. Existing bench_frames command-buffer timestamps, five alternating repetitions at frozen scene states, for bin, tile cull/stage, shading and copy; headless present GPU cost is absent, not measured windowed cost. Diagnostic-only per-tile counters measure scanned entries, staged hits, alpha-positive sample layers and a conservative top-opaque-uniform certificate. Separate diagnostic counters from timing binaries. Sample draws 60/120/180 to expose temporal variation. All GPU execution/timing holds the shared measurement lock; reject contaminated timing batches and retain their logs. Check active heavy processes before starting.
budget: Local packages only, four build jobs, lane-owned target. Bound competitor build investigation to one pinned checkout/build attempt plus a necessary correction. No optimization implementation in this card.
stop conditions: Stop after decomposition and DESIGN-R6. If lock or system load prevents valid timing, record UNKNOWN with the exact missing cells; do not infer them from old reports. If instrumentation cannot faithfully describe a kernel, label its limitation. Build failures and measurement failures remain recorded.
do not revisit: No v7 sequential per-tile scan as optimization; no presentation API replacement, per-column CPU damage strips, GPU shaping, or archive uniform fast path without a new representation.

Pre-measurement build note: the checked-in lockfile lacks three zgx git-source lines, so `--locked` fails. Local resolution adds those source identities only (no version upgrades); retain the three-line patch with the lane evidence and restore the root lockfile before handoff. The pinned competitor commit was recovered in a lane-owned scratch clone.

Pre-measurement diagnostic refinement: also count tiles whose final BGRA bytes are all identical, separately from the conservative last-opaque-cover certificate. Assert every certificate implies actual uniform pixels. Tile counters include raw alpha-positive sample layers and those evaluated after the existing per-128-entry-batch opaque floor; these are not geometric bounding-box overlap counts. Command-buffer stage timings retain actual copy exclusions and use zero tile/bin dispatch sizes for isolated cases.
