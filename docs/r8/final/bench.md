# bench lane: final (wt/lead/bench-r8, uncommitted; measured bough r7/platform 74e665786c, not r8/expr)

Checks run: every run directory re-checked by `tools/run_status.py` (`results/RUN-STATUS.tsv`): all eleven COMPLETE
(runner rc 0, results line, announced cells = tabled cells, flat tables present). Every IDENTITY.txt names the
round-7 façade from 74e665786c: plain 52e30d0a171c0672 (55.5 MB; matrix, sweeps) or scenes 4c9a8a961d4fe29c (70.1 MB;
heavy); runner e7fb33095c6f86d3 (fixed) or 0a347d1bc50ca5b5 (GB_TIMING); `results/EFFECTIVE-SOURCE.txt` hashes
sources, overlays and binaries. `tools/results_doc.py` died on `f()` without `nd`; the closer's helper fixed that line
and ran it: RESULTS.md (63 KB) produced, `results_tables.md` byte-identical. Dashboard: the lane's last publish, v11.

Numbers (ilo-prod, Ryzen 7 3700X, no GPU; fakeland pixman; bough one CPU-executor thread into wl_shm, others lavapipe
with 21-53 threads; wall-time p50 in ms from the runner; pinned to CPUs 0-7 from heavy-1 on, SMT siblings shared):
- cmp-u1, unthrottled (load mean 4.7, 1.0-17.6): rows 3,000 hover 0.99 (iced 5.35); typing 0.25 (zgui 4.06); resize
  5.80 (iced 8.20); table remove 3.73; spinner 10,000 CPU/frame 1.15 (zgui 12.07); idle RSS 34.8 MiB (iced 168.5).
  Lowest valid value in 35 metrics, behind in 3: spinner and ticker fps (bough paces itself at 62.5; freya 153).
- heavy-u1 (load 12.7): behind in 11, lowest in 14. Sheet 10,000 hover 19.40 (egui 9.95), theme 27.91 (11.19), idle
  RSS 331.3 MiB (149.0); canvas 1,000 hover 83.29 (iced 17.56); canvas 5,000 theme 1,151 (egui 71.46), resize 385.5.
- cmp-2, fixed 60 Hz, unpinned (load 9.1, 0.2-21.6): rows hover 11.52 total, 0.93 input to commit; behind in 2 of 37
  (resize 17.22 vs iced 14.41). heavy-1: canvas 5,000 hover 925.8 (invalid report), sheet 10,000 hover 29.37.
- sweep-60/120/240: bough hover 13.31 / 7.00 / 3.74, app share 1.09 / 0.98 / 1.03; typing handling 0.31 / 0.26 / 0.29.
  cmp-v1, vrr 48-144 (load 10.2): rows hover 4.10, app 1.11; 195 of 587 reports invalid (`wake_lateness`).
- Profiles (perf stat, user mode, process scope, PMU ~75 % duty; instructions per operation): sheet 10,000 hover 76.4 M
  (IPC 1.12), theme 139.0 M (1.40); canvas 5,000 hover 6.60 G (1.82; shadow erf/exp ~60 % of self time); headless rows
  hover 7.59 M (2.26; memmove 46 %); table ops 1.06-27.8 M. Twelve flamegraphs in `results/profiles/svg/`.

Kept: runner timing patch (GB_TIMING fixed | unthrottled | vrr, hash in each identity record); unthrottled as the
primary view; missing validity counts as unchecked, never valid; start-up unchecked and unranked; estimates labelled,
dotted and outside every rank; per-run identity, effective-source hashes and a run-status re-check.
Refuted: "bough virtual-100,000 start-up is 7th at 564.9 ms" (cmp-2 load; pinned unthrottled 39.1 ms, first of 11);
estimate ranges as ~95 % intervals (held 71-77 % in hold-out tests; worst factor x20 to x6,406).
Failures, reported not rerun: floem layout check in 16 of 17 cells of every matrix; vizia typing in cmp-u1; bough
sheet 10,000 scroll/resize in heavy-1 and heavy-v1 (connection reset by peer), resize in heavy-u1. cmp-3 not run.
Review: addressed B02, B03 (stated; no ABBA blocks), B04, B05 (labelled; hold-out tests), B06 (fixed in collect.py),
B07, B08, B09 (scripts exit with the runner's code after the runs; runs re-checked), B12. Partly: B01 (stated at the
top; the scoreboard still measures 74e665786c), B14 (cells bound to a hash and the layout check; no independent pixel
check, trace or failure mutants). Standing: B10 (e2e's fast paths), B11 (caveat only), B13.
Open: the matrix with bough-x beside bough (medium): its registration (`patches/gui-bench-bough-x.patch`) and main's
build of it pass the runner's checks in all eight scenarios (e2e journal section 10); the closer copied
`patches/gb-timing.patch`, `gui-bench-bough-x.patch`, `bench-runner-overlay.diff` and `runner-cell-all-diffs.patch`
from ilo-prod into `bench-r8/patches/`; commit bench-r8 and republish (small); cmp-3 or randomised blocks on a quiet host, richer animation (medium).
Questions for the user: a quiet ilo-prod window for cmp-3; a GPU host for B02; keep estimates on the page at 71-77 %.
Lesson: a cell with no validity check under load ranked bough 7th where pinned it is first.
