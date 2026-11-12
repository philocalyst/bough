# Card: repair backends (who executes a frame's repair), chosen per frame by one cost model

```
lane:            memory
state:           CARD
predecessor:     e81ddcaaf0 (region views candidate + probes)
question:        Can a frame's repair be executed by any of several pixel-identical backends, chosen per frame from
                 the damage and the device's state, so that a window holds GPU memory only while GPU work pays?
mechanism:       `Repair` backends behind one choice: (a) CPU tiles: a NEON rasteriser writing the damaged tiles of
                 the target IOSurface directly (kinds modelled exactly: solid, rounded, border, glyph; the blend is
                 one f16 FMA, as the GPU's); (b) GPU on a live queue (today's pre-armed stream); (c) GPU after
                 re-creating the queue. The presenter releases its queues (the driver's 69 MiB) after an idle
                 threshold. One cost model per frame: tiles x (CPU ns/tile | GPU ns/tile + wake cost when the queue is
                 cold or the GPU idle); a tile with an unmodelled kind forces the GPU for the frame.
situations:      counter click, caret blink, hover, one label change, a toast, a progress bar, a clock, a cursor in
                 an editor, an idle window, an occluded window, a window on another Space, thermal pressure or
                 battery (CPU cheaper than waking the GPU), first frame of a small window, a scroll (GPU), a resize
                 (GPU), a large animation (GPU).
gates:           every pixel gate with the CPU path forced on (`BOUGH_REPAIR=cpu`) against the GPU reference
                 (`reference_pixels`); a start-up self-check (a fixed probe scene rendered both ways once, cached per
                 device + OS build) disables the CPU path on any mismatch; mutant: the two-roundings blend must fail
                 the gates and the self-check.
measure:         counter click, caret blink, hover, one label change: instructions per frame, wall time input ->
                 surface written, footprint of a process that never submitted to the GPU vs 122.5 MiB today; first
                 frame after idle at queue-release thresholds {0.1 s, 1.25 s, 10 s keep-warm}; instruction slopes of
                 the STATUS cells (10x spread, median of 5) must stay within 1.02x (the GPU path is unchanged there).
thresholds:      counter active < 20 MiB, idle < 10 MiB (lead's targets); STATUS cells <= 1.02x.
stop:            a modelled kind that cannot be made exact -> name the operation; the CPU path excludes it.
```
