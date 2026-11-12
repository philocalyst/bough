# R6 core: measure before choosing a structure

lane: core  
state: CARD  
owner: core session  
predecessor: aa9943e7ae50595f6af026773ee82b46a4b85ae0  
toolchain: nightly-2026-10-17, default features, release  
platform: local Apple M3 Pro / macOS, CPU only

Question: which costs grow with unrelated records, and which are required work?

Change: extend the existing core benchmark entry points with an explicit R6 diagnostic mode;
no production representation changes. The user's request for core diagnostics authorizes the
core interfaces here; these are not new application benchmarks on the façade.

Workloads, at exactly 1,000 / 10,000 / 100,000 / 1,000,000 records:

- Fresh bulk-filled pages: insert/remove/move runs of 8 and 64 records at front/middle/end;
  move destinations use positions after removal. Front means record 1 (keep the root).
- Append one record, bulk build from prepared records, repeated append in batches of 8.
- Translate 64 records and half the tree; freeze cold, unchanged, one changed page, all changed.
- Wide shallow topology: parent of the last child, ancestors, root and leaf subtree end;
  repeated depth-32 chains as a contrasting topology.
- Spatially ordered and interleaved coverage: cull 1%, 10%, 100% tiles, both a contiguous band
  and dispersed tile masks; hit a present point and a point inside summaries but outside ink.
  Geometry stays within the current numeric domain at every N (overlapping layers at large N).
- The existing trace replay remains callable; its R6 switch selects the spatial matrix.

Gates: compare records, queries and culls to `oracle.rs`, check structural invariants outside
the measured interval; check snapshots against the source. A deliberately omitted move must
be detected by the same sequence comparison. Core release tests and workspace all-target check.
No changed renderer means pixel gates are not a representation qualification in this card;
they remain mandatory for the later settled implementation.

Measurement: task retired instructions (`proc_pid_rusage`), five fresh samples for mutations,
five batches for repeated queries; publish every sample plus median, counter overhead and
batch size. Setup, verification and destruction are outside the interval; bulk build includes
tree creation but excludes preparing the input and dropping the result. Fresh mutations do
not amortize away splits by repeatedly inserting/removing into warm slack. Report page counts.
Acquire the round measurement directory lock for the complete batch, check it before builds.
Cache traffic is an explicit analytical count/bound in 128-byte hardware lines and 64-byte
layout units, not a claimed cache-miss measurement. Identify dependent load chains separately.

Thresholds: this is a baseline, no speedup claim. Mark asymptotic work exceeding the named ideal;
also flag an observed 10x-N instruction ratio exceeding 2x for log/constant work, 15x for linear
work (descriptive flags, not a replacement for the analytical verdict). Report counter-dominated
cells as such. Later implementation: every fixed STATUS heavy/lab/animate/vlist R0/R1 cell
must remain <=1.02x its paired baseline; gains at large N cannot excuse small-N losses.

Budget: local CPU only, four build jobs, lane-local target; at most ten minutes per measured
matrix attempt. Preserve failures and amend before changing workloads. Stop on an oracle
mismatch; diagnose it without altering production code. Stop after the design report for lead
review and settlement of the first implementation card.

Do not revisit: discarded damage-lab region lists/SAT as a replacement for bitplanes,
universal MVCC, separate semantic runtime trees, or settled presenter exclusions. A motion
certificate is not a replacement for repaint bitplanes and must account for its unused cost.
