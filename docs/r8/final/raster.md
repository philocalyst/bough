# raster lane: final (r8/raster a187c31709, on r8/expr a2a2687662)

Checks run (Mac, M3 Pro, release): at aa36acbd04 `cargo test -p bough-paint -p bough-kernel` green (paint lib 2, exact
5, review 8, scenes 2; kernel lib 3, admission 2, exact 4, pool 3, simd 1). After the merge `cargo check --workspace
--all-targets` clean with `comparisons/bough-workloads` excluded (its gui-bench edition needs files the branch lacks),
and `cargo test --release -p bough-paint -p bough-kernel -p bough-expr` green. Kernels and generated GPU functions
equal the round-7 oracle on 12 scenes x 6 block states; GPU mutants caught (rounding 148 words, dropped layer 31,001,
cut one pixel wide 1,309, dropped damage 63,792, damage one record too high added at aa36acbd04); `mutant_tab` caught
at 242aaa0483. bough-kernel's feature mutants were rerun by the closer on main-next 163c02c3b4: core, cut and flat
fail tests/exact.rs, and bough-paint's core, cut, flat, hide, mask and tab each fail a gate. Nothing on the remotes.

Numbers (M3 Pro, dist, canvas 2560 x 1600, 19,476 records in 14 columns; wall under load is indicative only):
- back to back, load 20-26 (`bench/results/r8/raster/canvas-b2b-2.txt`): full frame on one core 4.48 G instructions,
  287-315 ms; 5+1 cores 66-77 ms; GPU resident 20.1 ms GPU time. Round-7 generic kernel on the same pixels (raster
  only): 21.1 G, 1.71-1.98 s; GPU 31.1 ms. Hover (1,576 tiles): one core 3.41 G against 17.06 G; resident 16.8 ms.
- corner tables (load 12-15): CPU full frame one core 585 -> 316 ms (7.62 G -> 4.41 G), six threads 150 -> 93 ms;
  GPU 16.8-18.9 ms against 22.5 closed form; 158 keys, 4.9 MB at 16 bits.
- damage by changed layer and visibility (load 34 rising to 96, `canvas-damage-1.txt`): hover 1,576 -> 527 tiles,
  6.9 ms GPU; theme 4,000 -> 3,628 tiles, 19.3 ms; one-record hover (30 tiles) one core 40-43 us, GPU 254-498 us wall.
- start-up: mount 3.7-6.8 ms; Metal compile and 19 pipelines 2.5 s for new source, 6 ms from Metal's cache; first
  frame 31-37 ms against the presenter's 60 (journal section 3, load 12-20).

Kept: per-archetype CPU kernels and GPU functions from one leaf definition through wgsl-rs, word-exact; a cover as a
per-record predicate; in-place record rewrite with GPU-side damage; corner tables on the CPU (1.85x); damage by
changed layer and visibility (3x fewer hover tiles); against section 1's R4/R5 (verdicts mine, from canvas-b2b-2):
one core 4.7x fewer instructions than the generic kernel (predicted >= 1.3x), GPU 0.65x its time (<= 0.8x); the
modifier chain as one paint record (`style.rs`, SEAM R9); the band interface `shade_into`/`worker`/`tile_weight`.
Refuted: corner tables <= 9 ms GPU (16.8-18.9) and <= 2 MB (4.9); per-corner tap leaves (22-24 ms at 100 % hits);
struct copies or inlining as the GPU's cost (0 %; forced inline 31 ms); never slower than the Metal presenter as
section 3 stood (hover 23-30 ms against 12.5, theme 23-34 against 19.2); the plan's static executor choice (one-record
hover sent to the GPU: 248 us against 43 on one core); R6's hover in <= 1 ms GPU (6.9 ms; verdict mine).
Review: closed with tests at aa36acbd04: R01-R05, K01, K02, K08. R06 in part: the driver compares the CPU's
predicted cost (tile weights) with the GPU's round trip for damage it has looked at, constants written for the M3
Pro; a large prediction still comes from the last frame's damage per word; gated, not measured. Standing: R06 rest,
R07, R08, R09 (direct Metal), R10 (the driver picks in the frame shown), R11, K03-K07.
Open: stable prefix per damaged tile (asked by the lead; not designed; medium-large); the text lane's faces as the
atlas with Glyph records (small; a line record with per-tile entries is large); one rounding definition with e2e's
bough-span (medium); calibrate and measure the driver (small); bin deltas (R07, medium); pick in the frame drawn
(R10, medium); per-archetype GPU dedupe and wgpu/Vulkan (large); a `Let` combinator for shared operands (small).
Questions for the user: none recorded by the lane.
Lesson: time the generated source with parts removed before making a part cheaper (18 of 24 ms was one integral).
