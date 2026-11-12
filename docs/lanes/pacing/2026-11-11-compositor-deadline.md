# Where the compositor's deadline is (measured, this Mac)

`crates/bough-metal/examples/latch_cal.rs`, under the measurement lock (1-minute load average 27 → 23), M3 Pro,
built-in 120 Hz ProMotion panel, macOS 15. A CAMetalLayer drawable is cleared on the GPU (finished before the
present), then presented with `-[MTLDrawable present]` at `T − x`, T the display link's `targetTimestamp`; the
drawable's `presentedTime` (the system's record of when it reached the glass) says which refresh showed it. One
drawable outstanding; 24 presents per phase; the window floats and never becomes key.

| x = T − present (ms) | n | shown at T | T + 1 refresh | T + 2 refreshes | dropped | present-call lateness (median) |
|---:|---:|---:|---:|---:|---:|---:|
| 0.25 | 24 | 0 | 0 | 23 | 1 | 37 µs |
| 0.50 | 24 | 0 | 0 | 23 | 1 | 35 µs |
| 1.00 | 24 | 0 | 0 | 24 | 0 | 36 µs |
| 1.50 | 24 | 0 | 0 | 24 | 0 | 34 µs |
| 2.00 | 24 | 0 | 0 | 24 | 0 | 35 µs |
| 2.50 | 24 | 0 | 0 | 24 | 0 | 34 µs |
| 3.00 | 24 | 0 | 0 | 24 | 0 | 36 µs |
| 3.50 | 24 | 0 | 0 | 24 | 0 | 36 µs |
| 4.00 | 24 | 0 | 0 | 24 | 0 | 35 µs |
| 5.00 | 24 | 0 | 0 | 24 | 0 | 34 µs |
| 6.00 | 24 | 0 | 0 | 24 | 0 | 28 µs |
| 7.00 | 24 | 0 | 0 | 24 | 0 | 24 µs |
| 8.00 | 24 | 0 | 22 | 2 | 0 | 93 µs |
| 8.30 | 24 | 0 | 20 | 4 | 0 | 420 µs |

(x = 8.0 and 8.3 ms are presents at the callback itself, V + 0.3 ms and V + ~0.45 ms, V = T − 8.33 ms.)

**Reading.** A present made between ~1.3 ms after a vsync V and the next vsync T is shown at T + 2 refreshes; one
made within ~0.3–0.45 ms after V is (mostly) shown at V + 2 refreshes. So the window server takes what was
committed by a point a fraction of a millisecond **after** each vsync, and shows it two refreshes later
(input-to-photon ≈ time to the next vsync + ~16.7 ms + scanout). Consequences for pacing:

* Committing anywhere inside a refresh interval is shown at the same time; the latest safe commit is just before
  the vsync (or just after, within ~0.3 ms). A frame drawn at the display-link callback (just after V) that
  commits within that sliver catches V's latch; one that misses it waits a whole refresh.
* "Latency to the first vsync at or after the commit" (the frame log's `input→vsync`) is therefore the quantity
  that differs between pacing policies; input-to-photon is it plus a constant (~16.7 ms) on this machine.
* CVDisplayLink's `inNow` is its own wake-up, 11.56 ms before its `inOutputTime`; `inOutputTime` equals
  CADisplayLink's `targetTimestamp` within 4 µs (frame log, same session). The refresh grid used by the
  analysis is the output time.

**Assumption, not measured:** bough publishes IOSurfaces as `CALayer.contents` in its own Core Animation
transaction, not drawables; both go through the same render server and the same window-server frame, so the
deadline above is assumed for bough's commits. A `presentedTime` for layer contents does not exist in public API.
