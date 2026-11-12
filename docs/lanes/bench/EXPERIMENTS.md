# bench lane, round 7: experiments

Each entry: the mechanism or question, the prediction and the kill criterion as written before the run (or a
note that it was not), the result with provenance, and the verdict. Losers stay here.

## E1. hotpath's overhead per measured call

* Question: is hotpath 0.30.3 usable on bough's turn, whose cells are 200-600 instructions?
* Prediction (informal, before the run): 100-300 instructions per measured call on the calling thread; unusable
  below ~5 µs per call. Kill (for "usable on cells"): more than 10 % added to an idle turn.
* Result [M3, dist, 5 runs, medians, 2931c0fe50]: +1,513-1,812 instructions per turn for five measure sites
  (~310 per call); idle turn 199 -> 1,756 (8.8x); +2,000-7,600 with allocation tracking.
* Verdict: killed for cells; kept for scene phases (1 % on a 155 K re-describing turn). MEASUREMENT.md.

## E2. Do the in-process counters and perf stat agree?

* Prediction (before the run): instructions within 2 %, cache events within 25 % (different brackets, the
  in-process slope cancels setup differently).
* Result [ilo-prod, eab3927e34 tree, set_ink at 1e5, perf stat slope 2,000 -> 22,000 reps vs in-process median of
  five slopes]: instructions 905.3 vs 908.6 (0.4 %), cycles 881 vs 878, L1d misses 11.7 vs 10.5-12.7, L2 misses
  5.7 vs 6.8-7.4 (-16 % to -23 %), DRAM refills 0.1 vs 0.04.
* Verdict: holds. Either can be quoted, with its bracket named.

## E3. A perf stat group of five events schedules whole (6 counters, 1 for the NMI watchdog)

* Prediction (before the run): five events at run% 100.
* Result: five and four events at 49-52 %, three at 100 %, pinned or not, no other process with perf fds. An
  in-process group of five read complete in every batch.
* Verdict: killed. `pstat.sh` uses three-event groups. Cause not found.

## E4. signal_cost's cells are stable in rows

* Question raised by a surprise: `signal_cost 64` printed 144115188075854459.
* Prediction (after seeing the output, before the fix): first-touch work per row lands inside the measured
  batches; with every row written before measuring, the mount-once number cell is flat in rows within 5 %.
  Kill: it still grows from 16 to 256 rows by more than 10 %.
* Result [M3, dist, 3 runs, SC_STABLE=1]: number write 5,990-6,090 at 1, 16, 64 and 256 rows (flat); string write
  6,500-6,880 at 1-64 rows and 9,960-10,130 at 256 (grows 50 %: the prediction does not cover the string cell).
* Verdict: holds for the number cell; the checkpoint's "number write 8,756 at 256 rows, grows with rows" came from
  the method. The string cell's growth at 256 rows is real and unexplained (next: hotpath on `lay_flush` at
  64 vs 256 rows, or the core lane).

## E5. The sheet's fan-out cost is linear in cells

* Prediction (before the run): hover (200 K wires on `look`, 20 K change a pixel) and theme (all change) cost a
  constant number of instructions per cell from 1,000 to 100,000 cells, within ±10 %.
* Result [M3 under load, dist, RENDER=0, MODE=once, REPS=16 (8 at 30,000 and 50,000), p50; 1,000-10,000 and
  100,000 cells on the 1eca7b5dc9 tree, 30,000 and 50,000 on 43574eaf23, which changed only the root's width and
  the glyph capacity]:

  | cells | hover instr | per cell | theme instr | per cell |
  |---:|---:|---:|---:|---:|
  | 1,000 | 159,972 | 160 | 523,916 | 524 |
  | 3,000 | 456,270 | 152 | 1,478,601 | 493 |
  | 10,000 | 1,506,647 | 151 | 5,062,093 | 506 |
  | 30,000 | 4,608,406 | 154 | 16,695,443 | 557 |
  | 50,000 | 7,661,689 | 153 | 28,331,054 | 567 |
  | 100,000 | 15,306,455 | 153 | 57,344,459 | 573 |

* Verdict: hover holds (151-160 per cell: ~77 per wire whose value does not change). Theme fails the ±10 % bound
  (493 -> 573 per cell, +16 % from 3,000 to 100,000 cells): per-cell cost grows with the working set, the shape a
  cache effect has; ilo-prod cannot run the façade yet, so the miss counters cannot confirm it. Either way, a
  hover that changes 20,000 pixels' worth of cells runs 200,000 wires: the fan-out is all wires on the signal,
  not the dependents that change (the representation the core lane's "hover with 10,000 dependents is a handful
  of vector operations per page" item targets).

## E6. Resize cost of the sheet once the root follows the window

* Not an experiment with a prediction: an observation to explain. With the root at `w_pct(1.0)`, the resize
  sweep's p50 is 36 M instructions at 30,000 cells and 61 M at 50,000, but p95 is 514 M and 946 M.
* Per step (`STEPS=1`, 30,000 cells, RENDER=0, 24 steps): two steps cost ~500 M (514 M at step 7, 1115 x 523
  after 1184 x 480; 499 M at step 18, 1184 x 1000 after 1115 x 957), the others 22-37 M (steps 13-22: 22 M; the
  rest: 36 M). 500 M / 30,000 cells = 16.7 K per cell, about a full relayout with text measurement; 22-36 M is
  ~1 K per cell. No simple rule in the sizes explains which steps spike (not the turns of width or height alone).
  Left for the layout/core lane with the reproduction: `SCENE_SCALE=0.3 REPS=24 RENDER=0 PHASES=resize STEPS=1
  workload sheet`.
