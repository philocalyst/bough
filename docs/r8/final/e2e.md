# e2e lane: final (r8/e2e 68fd365e79, code at aafc8f0e47, on r8/expr a2a2687662; on main by 1068e57fd1)

On the branch: gui-bench's app on the new core (`comparisons/bough-guibench-x`, bin `bough-bench`, run as
`bough-x-bench`), a libc-only Wayland client with ring and commit on input (`bough-host`), CPU spans as a `bough-x`
backend (`bough-span`, not bough-paint's tile kernels), a prepared Latin-1 glyph image (`bough-glyph`), `draw.rs`.
Checks run (Mac, release): at 11a6ee519e `cargo test -p bough-guibench-x -p bough-span -p bough-host` pass (pixels 6:
committed buffer == full redraw, screen == newest state at every latch with a ring of 3+, == fresh mount every 8th
step, rings 2-4, resize, scale 2; doctests 2 compile_fail; keymap 1; line 1); mutants straddle, age, superseded failed
it at 2e0523c35f (not re-run after the review fixes); bough-glyph == swash on 4,013 strings. `cargo check --workspace
--all-targets` clean at b092640696 (bough-workloads excluded); at aafc8f0e47 `-p bough-host` and `--test pixels keys`
pass. On ilo-prod the runner ran every probe of rows 100 / 3,000 and virtual 100,000 (0 failed measurements), which it
does only past its layout check. At the close (journal section 10): aafc8f0e47 (sha256 00adf1ed) and main's build
(1068e57fd1, sha256 e325cf1a) through `gui-bench --quick`, all eight scenarios (14 cells) under fixed and unthrottled
timing: the layout check and every app check passed in every cell, every key probe resolved; main's build at load
5-11 had no invalid measurement (the branch's at load 12-28 only `wake_lateness`).

Numbers (ilo-prod, `dist`, fakeland pixman, CPUs 8-15 nice 19, wall time only; no instruction counts):
- startup preset, warm, load 11, at 130e5a9a56: spawn to first buffer 4.27 ms rows 3,000, 4.32 virtual 100,000,
  4.51-4.62 rows 100 / table / search; empty client 4.17; façade 74e665786c 64.67 / 35.91. Host stamps: mount plus
  memfd 27 µs at any size, handshake about 20 µs, first raster 2.63-2.86 ms (first touch of about 1,000 shm pages).
- rows hover, preset by hand (not the runner), load 4-7: unthrottled input to commit p50 0.078 ms at 3,000, 0.079 at
  100,000, 0.127 at 100 (façade 0.907); fixed 60 Hz totals about 11.1 ms for every binary.
- runner, first x1-fixed pass at 11a6ee519e (load 17.6, later killed): first buffer 3.99-4.22 ms; hover input to
  commit 0.107 rows 100, 0.123 virtual (valid), 0.131 rows 3,000 (invalid: wake_lateness, host_load); typing 0.089 ms;
  idle RSS 10.9-11.1 MiB. Table ops other than select/update10 and the search filter: 0 samples (keys unresolved).
  Binary 1,070,976 bytes against the façade's 55,506,840: other profile and features, not a size result (B12).

Kept: first commit in the write that acknowledges the configure; mount by exposure in the benchmarked binary; commit
on input, damage exact across superseded commits (a ring of 2 starves, 3 never does); spans source-over with alpha;
search as the spec's plain scan. Façade: `fc-match` spawned twice; its 62.5 fps spinner is the app's 16 ms clock.
Refuted: empty-client floor 2-3 ms (4.2); first raster under 0.7 ms (2.6-2.9, page faults); hover input to commit at
most 0.05 ms at any size (0.078; 100 and 100,000 swap between runs); idle RSS at most 8 MiB (10.9-11.1, one run).
Review: closed H02, H04 for what it draws (no translucent surface or premultiplied operands), H06, H07/B10 search part,
H08's runner half (all eight scenarios through the runner's checks).
Standing: H01 (fractional scale), H03 (no groups, AltGr, compose, repeat), H05 (bough-glyph stays: the r8/text merge
conflicted in the Backend trait and was aborted), H08's author-API half (views against the core; resize remounts World), B10 rest
(one Latin-1 face, rows laid out by arithmetic).
Open: bough-face for bough-glyph, façade-idiom port, `World::resize` (medium each); bough-span as the default CPU path with
exactness against bough-paint, sheet and canvas ports (large); M1/M4 built but unmeasured, M5, cold start, hover split.
Questions for the user: vendored xkbcommon or a pure-Rust xkb; refuse fractional scales until layout has fractional
rules; is a cached first-frame pixel image worth its validity check (journal section 7)?
Lesson: measure the empty client first: 60 % of first-buffer time was the kernel zeroing shared pages.
