# Lane memory, round 7: journal

Branch `r7/memory` (worktree `wt/r7-memory`), based on `round6` 3b0b00c698 (ledger and region views merged; the
previous lane's WIP `98834fc3bf` and the dormant branch are not in this base). Brief: `bough/runtime/lead/ROUND7.md`,
section "memory". Machine for every number unless said otherwise: M3 Pro (11 cores), 36 GiB, macOS 15 (Darwin
24.6), shared with the other round-7 lanes (load average recorded beside each batch).

This file is the lane's running record: what was tried, the numbers with their provenance, what failed, what is
next. Newest entries at the bottom of each section.

## Plan (2026-11-12, start)

1. Heavy scenes of my own (not waiting for the bench lane): a virtualised feed with avatars, text and photos; a
   board of ~1,500 mounted cards (many layers, text); a gallery of 10,000 distinct images. A headless harness
   (`App::new(.., gpu = true)`, no window, no input events) that samples the in-process ledger, `phys_footprint`
   and `footprint -j` at four moments: after startup, in steady interaction, after a resize storm, idle 5 s
   after the last frame.
2. Media API as code a user would write first (image in a list row, a gallery, a video grid, an avatar that
   updates), then the storage design, with the lead's hypotheses as experiments (prediction + kill criterion
   written before the run).
3. Build it; measure resident bytes per visible pixel and time to first sharp pixel, cold / warm / second launch.
4. Reorganise what the scene ledgers show is resident only because of how it is stored.

## Log

### 2026-11-12 night: scene ledgers, first pass (base round6 3b0b00c698 + harness, release, M3 Pro, load 25-45)

Harness: `crates/bough/examples/scene_mem` (headless, `App::new(.., gpu = true)`, `windowed = true` so the ring
is content-addressed as a window's; four samples: startup, after 600 wheel frames, after 40 resizes, idle 5 s
with the calm trim at 1 s). Tables: `bench/memory/scene_table.py`. One run each (spread not yet measured).

| MiB phys_footprint | startup | steady | after resizes | idle 5 s |
|---|---:|---:|---:|---:|
| feed (2,000 posts, virtual list, 92 records mounted) | 123.8 | 136.7 | 127.3 | 59.2 |
| board (1,200 mounted cards, 8,442 records) | 133.9 | 150.8 | 135.7 | 66.8 |
| gallery-atlas (today's `image()`, 6 distinct PNGs) | 122.4 | 133.9 | 123.8 | 55.0 |

Where (active): the GPU driver's block 72-76 MiB (`IOAccelerator (graphics)`, mapped dirty, reclaimable only
after ~1-5 s without submissions), `IOAccelerator` other 8.1, IOSurfaces 1-2 x 10.8 (board 15.6), malloc 22-30
(Rust live 7.5-12.3), image atlas 4.0 CPU + 4.0 GPU texture. Startup on the main thread: feed 221 M
instructions / 89 ms, board 156 M / 85 ms, gallery 257 M / 93 ms (device and pipelines without `prepare`, PNG
decode and a float resample per image on the main thread).

Defect found and fixed (commit c9a6d451f6): a list inside a scroll box mounted no rows until the first input
(the region measured its exposure before the write-back gave the port's clip its rect; first frame empty).
`lay_flush` now places again when a write-back changed a non-virtual region's exposure.

Defect confirmed: `image()` is PNG-only, decodes on the main thread at mount into one 1024 x 1024 RGBA atlas
plus a CPU copy, never evicts: at 180 x 120 pt (360 x 240 px) eight distinct thumbnails fill it; one 480 x 270 pt
photo takes half. The media design replaces it.

Merged since: round6 12804f6899 (core, store, layout), r7/bench 95dd2d592e.. (five heavy scenes), r7/compute
e914f93070 (the kernel executor is the default).

### E1 (lazy GPU): no Metal device until the estimator wants one. Predictions, written before the run

Mechanism: the kernel's CPU executor draws the first frames into plain-memory surfaces; no device, queue,
library, residency set or driver block exists. Measured here as `NOGPU=1` in `scene_mem` (the App gets a
`MemSurfaces` target; the calm trim now gives back the CPU target's spare surfaces, `Surfaces::release`).

| | now (device at start) | predicted without a device |
|---|---:|---:|
| feed startup instructions (main thread, App::new + first frame) | 221 M | 150 M (± 30) |
| board startup instructions | 156 M | 110 M (± 25) |
| feed time to first frame | 89 ms | 45 ms |
| board time to first frame | 85 ms | 50 ms |
| feed footprint steady / idle 5 s | 136.7 / 59.2 | 45 / 25 |
| board footprint steady / idle 5 s | 150.8 / 66.8 | 55 / 32 |

Basis: device + pipelines are ~60-80 M of today's startup (the feed with 4 records mounted cost 79-84 M); a
full first frame on one core adds 20-60 M. The driver block (72-76), `IOAccelerator` other (8), the GPU atlases
(4.5), bins and Metal's small buffers (~4) disappear; the surfaces stay (heap instead of IOSurface).
Kill: startup instructions down by less than 25 % and time to first frame by less than 30 % kills the startup
half of the case (the memory half is judged on the idle and steady footprint alone).

### E1 results (r7/memory fb632b877f + the NOGPU harness mode; release; M3 Pro, load 29-30; one run each)

Same binary, `NOGPU=1` versus the default (which on this tree is the compute lane's kernel backend with the
Metal device created at `App::new`). MiB phys_footprint, startup / steady / after resizes / idle 5 s:

| | device at start | no device | predicted |
|---|---:|---:|---|
| feed footprint | 30.5 / 42.3 / 113.7 / 45.5 | **23.1 / 25.0 / 42.3 / 42.3** | 45 steady / 25 idle |
| board footprint | 37.5 / 53.8 / 120.6 / 52.9 | **30.8 / 31.8 / 33.2 / 33.3** | 55 / 32 |
| feed startup, main thread | 291 M instr, 1,521 ms (first run after a build: the Metal source compiles) | 215.5 M, **15.2 ms** | 150 M, 45 ms |
| board startup, main thread | 155.7 M, 64.6 ms | **108.9 M, 10.0 ms** | 110 M, 50 ms |

* The merged kernel backend already changes the GPU-mode picture: with the CPU executor taking small frames the
  driver block is not touched until a frame is large (startup 30.5 MiB instead of 123.8 on the old base), but
  the device and its pipelines are still built at start (time) and the first large frame (the resize storm)
  brings the 72-76 MiB block in, which then stays charged for seconds.
* Without a device: time to first frame falls 85-88 %; footprint at every moment is surfaces + heap. The ring
  is one surface (a plain-memory target is never held by a compositor, so the shown surface is repaired in
  place). Startup instructions: board -30 % (verdict: supported); feed -1 % to +24 % (two runs, 215 / 275 M):
  **killed for the feed** — its startup is not the device. Breakdown (`NOGPU`, second run): `App::new` 148 M
  (mount: `image()` decodes the 320 x 214 PNG and resamples it to 960 x 540 px with per-channel float bilinear
  on the main thread), first frame 127 M (one core, 2,000 x 1,400 px: the photo costs ~135 instructions per
  pixel through the kernel's image sampler, flat areas ~10). Board: mount 74 M, first frame 57 M.
* Steady scrolling on the CPU alone is the price: feed 12.9 M instructions and 4.8 ms per 30 pt frame (a
  full-surface memmove for the scroll copy plus the exposed strip), versus ~0.1 M on the GPU path. So the policy
  is: no device until the estimator asks for one (scroll bursts, resize storms), release it when calm.
* Idle footprint after the resize storm (feed 42.3 without a device) is malloc's large-block cache: the CPU
  target's surfaces are `alloc_zeroed` blocks of 6-22 MiB, and the freed ones stay dirty in MALLOC_LARGE (27.4
  MiB) after the storm. Window-sized buffers should be VM mappings (or IOSurfaces), returned on free. Next.
