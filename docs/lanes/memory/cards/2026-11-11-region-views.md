# Card: the region as address space, the GPU's view of each column as its used prefix

```
lane:            memory
state:           CARD
owner:           memory session, round 6
predecessor:     d892f974da (ledger instrumentation on aa9943e7ae); nightly-2026-10-17; release profile; direct
                 Metal presenter (the wgpu-hal presenter keeps the one-buffer region and is not changed)
question:        Can the region cost what its columns use instead of what `Caps` reserves, with no capacity
                 panic below the record format's own limits, and no cost on a frame path?
change:          Today the region is one storageModeShared MTLBuffer of `Caps::region_bytes` (APP_CAPS: 20 MiB);
                 the residency set makes all of it resident and charged (ledger: 20.03 MiB resident, 0.05-2.2
                 MiB in use on the AWAGY windows). vmprobe showed (a) a buffer joining a residency set is wired
                 whole, (b) a no-copy view over a prefix of an mmap wires only the prefix, (c) a longer view over
                 the same base can replace it. So:
                 * the region's CPU memory is an anonymous mmap reservation (lazily backed); GPU-read columns
                   start on page boundaries and reserve up to the record format's limits (instances 2^18, pages
                   2^15, glyph slots 2^22, run headers 2^20, strikes 2^8; spaces and clips as configured);
                 * the presenter wraps, no-copy, the header + frame slots (buffer 0, as today) and, per column,
                   the page-rounded prefix that holds its mask group's capacity (a power of two of elements);
                   the kernels read each column's GPU address from a column table in the header instead of
                   `region + offset`; masks are the committed capacities;
                 * before a GPU frame, `App` hands the presenter each mask group's high-water mark (a handful of
                   loads and compares); a group past its capacity gets new views at the next power of two
                   (at least double), the header is rewritten and the residency set updated (draining the
                   pre-armed frames, as a bin growth does today). The CPU addresses never move.
                 * `Region::heap` uses mmap (alloc_zeroed with 16 KiB alignment writes every page today).
                 Kernel change (shader.metal, fused kernels only): owned by lane `gpu`; prototyped here.
implication:     Passing establishes that the region's resident cost follows content on this machine and OS,
                 that `Caps` stops being a memory knob and a panic source below the format limits, and that the
                 indirection costs nothing measurable on the CPU frame path. It does not establish anything for
                 the wgpu-hal presenter (WGSL has no buffer addresses: one binding per column would be needed),
                 nor GPU time on scenes other than those measured.
platforms:       macOS 15, M3 Pro, direct Metal.
workloads:       memory: the ledger batch (counter, dashboard, scroll 100/1K/7K, heavy; 3 runs; t = 3 s and 9 s).
                 CPU: instruction slopes (bench/integrate/slope.py method, release builds of A = d892f974da and B,
                 3 reps, median) of heavy stream, heavy scroll, lab both, animate, vlist scroll.
                 GPU: ABAB pairs (bench/floor/gpu_pairs.py, PAIRS=5) of heavy both and vlist scroll, if the GPU
                 time tool builds here; otherwise "not measured".
gates:           pixel: cargo test --release -p bough (all tests), -p bough-heavy (correctness, gpu_bounds),
                 -p bough-scenes (correctness), -p bough-vlist (correctness), -p bough-mem, -p bough-metal.
                 New: a GPU test that mounts 70,000 quads (past APP_CAPS's 65,536 instances; DEFAULT's 8,192),
                 each a distinct colour, and checks the pixel at every quad's centre against the colour the CPU
                 gave it (an oracle independent of the views), after growth through several doublings.
                 Negative control: feature `sabotage_views` (bough-metal) never grows a view; the new test must
                 fail with it.
thresholds:      every instruction cell B/A <= 1.02; GPU pairs median ratio <= 1.02 where measured; region
                 resident <= 2 x in use + 12 pages on every ledger case; phys_footprint (active) lower by >= 15 MiB
                 on counter, dashboard and the three scroll cases; steady-state allocation count per tick not
                 higher than the base batch.
measurement:     memory under the lane's ledger protocol; instructions are load-robust; GPU pairs and windowed
                 runs hold the measurement lock.
budget:          the rest of this session.
stop conditions: overlapping no-copy views rejected by Metal, or the pre-armed stream breaking under a view
                 change -> METHOD_BOUNDARY with the reason; any instruction cell > 1.02 -> REJECTED.
do not revisit:  SETTLED's present paths; round 5's driver-block knobs.
```
