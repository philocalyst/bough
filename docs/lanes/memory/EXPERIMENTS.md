# Lane memory, round 6: experiments of the lane's own (predicted before running)

Each entry: the mechanism in two sentences, the predicted number, the result that kills it, then the measured
result (losers stay, with their number).

## X1. Release the queues when calm

* **Mechanism.** The driver's ~69 MiB belongs to the command queue that ran work (drvprobe); a presenter that
  releases its queues once a window is calm, and makes new ones (idle, free) in their place, returns it. The next
  frame pays a queue wake that a GPU idle for more than ~1.25 s pays anyway.
* **Prediction.** Counter window calm (t = 9 s, keep-warm on today): 114.0 -> ~45 MiB; scroll 7 K calm 137.4 ->
  ~68 MiB. First frame after a release: ~3.1 ms and ~11 M instructions, against 2.4-2.7 ms / 8.4-8.6 M for a kept
  queue after 2 s idle and ~0.3-0.6 ms / ~0.2 M while keep-warm holds the GPU awake.
* **Kills it.** The footprint does not drop by at least 50 MiB in a windowed process (something else holds the
  block: Core Animation, the residency set, the publisher), or the first frame after a release costs more than
  5 ms.
* **Result.** (pending)

## X2. A small app that never submits GPU work

* **Mechanism.** With the CPU repair backend choosing every frame whose damage is small and modelled, a counter's
  first frame, its clicks, label changes and hovers are all written by the CPU into the ring's IOSurfaces, so no
  queue ever runs work and the driver never allocates its block; with region views the region is what the window
  uses.
* **Prediction.** Counter active 122.5 -> ~25 MiB (driver 76 and region 20 gone, surfaces 4.7 stay, malloc and
  the rest unchanged), idle ~22 MiB; instructions per counter click below the GPU path's by 3-6x (IDEAS-OPUS
  estimate: 15-31 K vs 86-101 K for one tile, before the CA commit).
* **Kills it.** Above 40 MiB active, any pixel gate failing with the CPU backend, or a click costing more
  instructions on the CPU than on the GPU.
* **Result.** (pending)

## X3. Font bytes from a file mapping, once

* **Mechanism.** The face's bytes are copied three times into the heap (the application config's copy, the
  shaping cache's leaked copy, the glyph cache's copy: 3 x 755 KiB for Arial, all dirty). A read-only mapping of the
  font file is clean memory: the kernel can drop and re-read it, and phys_footprint does not charge clean file
  pages.
* **Prediction.** -2.2 MiB of phys_footprint on every window (the three dirty copies), RSS unchanged or up by the
  pages the shaper and rasteriser touch (clean).
* **Kills it.** Less than 1 MiB saved (another copy elsewhere), or a startup instruction regression above 2 %.
* **Result.** (pending)
