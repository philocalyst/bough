# The memory ledger (round 6): who holds every byte

Card: `cards/2026-11-11-ledger.md`. Machine: M3 Pro, 36 GiB, macOS 15 (Darwin 24.6), built-in display at 2x,
direct Metal presenter, release profile. Predecessor `aa9943e7ae`.

## Method

Two views of the same process at the same moment, joined:

* **The system's view.** `footprint -j` (per VM category: dirty, swapped, clean, reclaimable, wired),
  `vmmap --summary`, and `proc_pid_rusage` (`ri_phys_footprint`, `ri_resident_size`). `phys_footprint` is what
  macOS charges the process (dirty + compressed private memory, plus IOKit / GPU memory the process owns whether
  or not it is mapped); RSS counts only pages mapped in the process's address space, clean file pages included.
* **bough's view** (`App::ledger`, `crates/bough/src/ledger.rs`): every region column (len, capacity, and the
  resident and dirty pages under the capacity, by `mincore`), every heap table of bough-core / bough-text / the
  façade / the layout engine (capacities; `mincore` for blocks of 64 KiB and up), the text caches (atlas CPU copy,
  strikes, font copies, shaping tables), and the presenter's GPU objects (each surface: drawn or not, purgeable,
  held by the compositor; atlas; bins; counts; image atlas) plus Metal's `currentAllocatedSize`. The process side
  adds the malloc zones (`malloc_zone_statistics`) and the Rust heap's live bytes (a counting
  `#[global_allocator]`, `bough_mem::count`, installed by the harness).

Workloads (`bench/memory/ledger.py`): the counter, AWAGY dashboard and AWAGY scroll at 100 / 1,000 / 7,000 rows run
by `comparisons/bough-awagy/examples/ledger.rs` (the workloads' own windows through `Application::run_harness`:
same configuration, same input path), driven in-process for 6 s (`App::wheel_points` at 120 Hz, 30 pt per tick;
the counter: one `inc` turn per 100 ms), then calm; heavy is `heavy_workload` (`ZGX_MODE=both`). Samples at
t = 3 s (active) and t = 9 s (calm: 3 s after the last frame, 2 s after the trim). Three repetitions, medians.
Windows never take focus (Accessory policy, `orderFrontRegardless`) and close at 10 s. No synthetic system input.

Raw runs: `bench/results/memory/2026-11-11-base/`; `bench/memory/headline.py` and `table.py` reproduce every
table below from them. `2026-11-11-base-v1/` and `-v2/` are the two earlier batches of the same runs, taken
before two ledger lines were sized correctly (cephal's answer-cache boxes; the instance column's high-water
mark), kept as measured. Windows: 1000 x 700 pt (AWAGY), 480 x 320 pt (counter), 1280 x 800 pt (heavy), at 2x.

## Headline: active (t = 3 s, scrolling or clicking), MiB, median of 3 (range)

| holder | counter | dashboard | scroll 100 | scroll 1 K | scroll 7 K | heavy | scales with |
|---|---:|---:|---:|---:|---:|---:|---|
| **phys_footprint** | **122.5** (122.4–123.5) | **166.2** (165.8–167.5) | **151.4** (151.1–151.8) | **153.6** (153.1–153.9) | **164.6** (163.1–166.4) | **145.9** (145.8–146.5) | |
| **RSS** | **39.8** (39.6–42.2) | **54.2** (53.5–55.4) | **40.2** (40.1–40.4) | **42.7** (42.0–43.0) | **60.8** (60.0–61.5) | **38.2** (37.8–38.5) | |
| GPU driver, unmapped graphics memory | 68.0 | 68.0 | 68.0 | 68.0 | 68.0 | 68.0 | nothing: present from the first GPU submission |
| GPU driver, `IOAccelerator` + `IOKit`, unmapped | 8.0 | 8.0 | 8.0 | 8.0 | 8.0 | 8.0 | nothing |
| IOSurfaces drawn | 2 x 2.34 | 3 x 10.78 | 3 x 10.78 | 3 x 10.78 | 3 x 10.78 | 3 x 15.63 | window size x surfaces drawn |
| the region (one shared MTLBuffer; resident whole) | 20.03 | 20.03 | 20.03 | 20.03 | 20.03 | 2.03 | `Caps`, not content |
| &nbsp;&nbsp;of which in use (columns' high-water marks) | 0.05 | 1.30 | 0.07 | 0.35 | 2.17 | 0.18 | content |
| other GPU objects (glyph atlas 0.5, bins 0.7, counts; dashboard: image atlas 4.0) | 0.83 | 5.23 | 1.20 | 1.20 | 1.20 | 1.30 | window, content |
| small mapped Metal buffers (command / encoder memory, 16–64 KiB each) | 2.8 | 2.8 | 3.2 | 2.9 | 2.9 | 1.9 | nothing |
| malloc zones (dirty) | 12.4 | 23.9 | 12.8 | 15.4 | 26.3 | 12.1 | content |
| &nbsp;&nbsp;Rust heap, live (counting allocator) | 3.1 | 13.2 | 3.4 | 5.3 | 20.0 | not measured | content |
| &nbsp;&nbsp;&nbsp;&nbsp;named by the ledger | 2.3 | 11.5 | 2.5 | 4.3 | 16.8 | 2.6 | |
| &nbsp;&nbsp;frameworks' malloc (zones in use − Rust live) | 5.8 | 6.4 | 7.9 | 5.8 | 6.4 | – | nothing |
| rest: binary and framework images, framework data, page table, stacks | 6.4 | 5.8 | 5.8 | 5.8 | 5.8 | 5.7 | nothing |

How the GPU rows are split: `footprint` charges 92.1 MiB to `IOAccelerator (graphics)` on scroll 7 K;
`vmmap --summary` of the same moment shows 24.1 MiB of it mapped in the process (the region's 20.0, the GPU
objects, the small buffers); the other 68.0 MiB is owned by the process and not mapped: the driver's. Likewise
`IOAccelerator` + `IOKit`: 8.1 charged, 0.1 mapped. The region's "in use" is the sum of its columns' high-water
marks (the instance and transform columns are addressable whole, so their allocators' marks, not their lengths).
Heavy runs the presenter without the counting allocator (its own binary); its Rust heap is not measured.

## Calm (t = 9 s: 3 s after the last frame, 2 s after the calm trim), MiB, median of 3 (range)

| | counter | dashboard | scroll 100 | scroll 1 K | scroll 7 K |
|---|---:|---:|---:|---:|---:|
| phys_footprint | 114.0 (112.6–119.8) | 145.7 (144.2–158.2) | 120.9 (120.9–142.9) | 124.9 (124.8–145.7) | 137.4 (133.2–142.7) |
| surfaces still charged | 1 x 2.34 | 1 x 10.78 | 1 x 10.78 | 1 x 10.78 | 1 x 10.78 |
| trimmed surfaces (volatile, reclaimable, not charged) | 2.3 | 21.6 | 21.6 | 21.6 | 21.6 |
| GPU driver, unmapped (graphics + other) | 60.0 + 8.0 | 68.0 + 8.0 | 60.0 + 8.0 | 60.0 + 8.0 | 60.0 + 8.0 |
| **phys_footprint with keep-warm off** (`BOUGH_KEEPWARM=0`; raw `2026-11-11-base-nowarm/`) | **53.2** (53.0–53.6) | not measured | not measured | not measured | **95.4** (76.9–99.3) |

(Heavy runs for 10 s, so its t = 9 s sample is not calm; it is left out here.)

The driver block is charged for as long as the process keeps submitting GPU work, and the presenter's keep-warm
(a 4-byte blit whenever the GPU has idled 0.8 s, for 10 s after an interaction) keeps it submitting. With
keep-warm off, the block becomes reclaimable (56.9 MiB of `IOAccelerator (graphics)` reclaimable, not charged)
about 1.25 s after the last submission (`floorprobe blit idle:3000`). One more effect of keep-warm: with it, the
trimmed surfaces leave the footprint (reclaimable 21.6 MiB, 3/3 runs); without it, they stayed charged in 2 of 3
scroll runs (32.3 MiB at t = 9). A trimmed surface is removed from the residency set and made volatile, and the
removal seems to take effect only at the next command-buffer submission, which keep-warm supplies; this is a
hypothesis from those runs, not verified.

## Per row (slope from 100 to 7,000 rows, active)

| | bytes / row |
|---|---:|
| phys_footprint | 2,013 |
| RSS | 3,129 |
| malloc zones (dirty) | 2,054 |
| Rust heap, live | 2,533 |
| **heap, by holder** | |
| layout: text leaf state (`LeafCtx`, 392 B each) | 392 |
| layout: answer caches (cephal `NodeCache` box, 360 B, one per text leaf) | 360 |
| layout: layout nodes (`DNode`, 152 B, two per row) | 304 |
| describe: dynamic items (`DynItem`: key, counts, a `Scope` of seven `Vec`s, a tape) | 247 + 32 + 16 |
| runtime: nodes (`Node`, 120 B, two per row) | 240 |
| runtime: text nodes (`TextNode`, 176 B) + its line table (48 B) + its string (10 B) | 234 |
| layout: `LInfo` (3 per row) | 96 |
| layout: queues, stacks, child lists, paragraph hot halves | 128 |
| core: handle table (2 x 24 B) | 48 |
| runtime: reflection type names (2 x 16 B) | 32 |
| unattributed Rust heap (named lines vs the allocator's live bytes) | 357 |
| **GPU-visible (region columns), by holder** | |
| glyph slots: pens `i32` + entries `u16`, 32 slots reserved for "Item N" | 192 |
| pages (2 records x 23 B) | 46 |
| run header | 32 |
| quad instance (the row's background) | 48 |

What the GPU draws a row from is ~320 bytes (the region's in-use slope: 318 B/row); the CPU keeps ~2.5 KiB of
Rust heap beside it (eight times more), of which 0.75 KiB is the layout engine's per-text-leaf state and cache. Nothing per row is in the
surfaces, the driver, or the region's footprint (the region is charged at its capacity whatever the rows).

## Where the 104 MiB between RSS and phys_footprint is (scroll 7 K, active)

phys_footprint 164.6, RSS 60.8. RSS counts only pages mapped in the process's own page tables; the footprint
also charges memory the process owns and has not CPU-touched:

| charged, not in RSS | MiB |
|---|---:|
| the GPU driver's unmapped memory (`IOAccelerator (graphics)` and `IOAccelerator` + `IOKit` not mapped in the process) | 76.0 |
| three IOSurfaces (mapped by Metal, never faulted into the process's page tables: RSS is the same with 2 x 2.3 MiB surfaces, counter, as with 3 x 10.8, scroll 100) | 32.3 |
| the region's pages wired by the residency set and never written by the CPU (20.0 minus the 2.2 in use) | ~17.9 |
| **sum** | **~126** |
| in RSS, not charged: clean pages (`__TEXT`, mapped files, the shared cache's pages) and reclaimable malloc | ~22, the remainder |

So the gap is the GPU: the driver's private block, the surfaces and the wired region, minus clean code pages.

## Findings beyond the table

* **The whole region is resident and charged.** `vmprobe` (`crates/bough-metal/examples/vmprobe.rs`): a 64 MiB
  `newBufferWithLength` (shared) buffer costs nothing until it joins a residency set (or is bound); then the
  footprint rises by its whole size (6 -> 70 MiB with 1 MiB written). A `newBufferWithBytesNoCopy` buffer over
  untouched `mmap` memory does not raise the footprint when it joins the set (7.2 -> 7.2 MiB), but `mincore`
  reports every page resident and the machine's wired page count rises by the buffer's size (+1,024 MiB for a
  1 GiB buffer): the memory is used, only not charged to the process. A no-copy view over a *prefix* of an mmap
  wires only the prefix (64 of 1,024 MiB), and a second, longer view over the same base works and wires its
  length. Capacity as address space therefore does not hold for one buffer; it holds for a buffer per column
  whose GPU view is its used prefix (card `2026-11-11-region-views`).
* **`bough_mem::Region::heap` writes every page.** `alloc_zeroed` with 16 KiB alignment goes through an aligned
  allocation and an explicit zero fill on macOS: a 64 MiB heap region is 64 MiB dirty at once (`vmprobe heapalloc`).
  The crate's doc says the opposite. Headless runs and the overlapped start (`prepare` + `Fused::new_on`) pay
  their whole region.
* **The driver block.** `floorprobe` (round 5's probe, run here): the first GPU submission of the process,
  whatever it is (a 4-byte blit, a one-threadgroup compute dispatch, one with 32 KiB of threadgroup memory, an
  event-released pre-armed buffer), adds 60.8 MiB of graphics-tagged footprint plus 8.2 MiB; a second queue's
  first submission adds 0.2 MiB; a render pass adds 30 MiB more (91.4); a 64 MiB private heap costs nothing until
  used. ~1.25 s after the last submission the 60.8 MiB moves to "not charged" (48.2 MiB stays allocated but
  uncharged) and the 8.2 MiB stays. No API in reach changes the 60.8 MiB; only not submitting does.
* **Steady state allocates.** With the counting allocator: scroll 2.2–3.2 allocations per 120 Hz tick, dashboard
  6.3–6.5, counter 1.7–2.5 (`ledger-harness` lines in each run's `stderr.txt`). Traced (`LEDGER_TRACE=1`,
  `runtime` log): every wheel event clones the hover chain (`App::wheel_points`, 8 B per handle); every
  re-description allocates a `String` per `&'static str` text (`TextSrc::value` returns `String`) and runs the
  view's `format!` closures (dashboard: four per tick while scrolling, because the scroll signal is a field of the
  re-describing root); a text change allocates in `Rt::place_text` / `lines_point` (256 B, 1 KiB) and in
  `Deep::style_id` (4.7 KiB); the keep-warm timer boxes a dispatch closure per arm.
* **A GPU timeout under sampling.** Two of nine heavy runs panicked (`2026-11-11-base-v1/heavy-2`, frame 268;
  `2026-11-11-base/heavy-2`, frame 249, about a second after the t = 3 s sample):
  `GPU frame N did not finish within 5 s` with `kIOGPUCommandBufferCallbackErrorTimeout` on a pre-armed batch.
  `footprint` and `vmmap` suspend the target while they read it; a pre-armed command buffer waiting on its event
  while the process is suspended is the likely cause (not verified). Any long stall of the process (a debugger,
  a suspension) can then take the presenter down: lane `gpu` / `pacing` should know.

## What the ledger says to attack, in order of bytes (an AWAGY window, active)

| holder | MiB | in bough's hands? | lever (card) |
|---|---:|---|---|
| the GPU driver's block | 76 | only by when (and whether) the GPU is used | the keep-warm window (68 MiB charged for 10 s after the last interaction instead of ~1.25 s) and a CPU path for small repairs (round 5's idea, not built). Policy for lanes `gpu` / `pacing`; measured here, not changed |
| surfaces | 32 (3 x 10.8) | yes: how many are drawn, how soon spares are released | a third surface is drawn in every active AWAGY run; one present per refresh (lane `pacing`) makes two enough. Trim exists; its release needs a submission (see above) |
| the region | 20 | yes | **per-column GPU views over an address-space reservation** (`cards/2026-11-11-region-views.md`): ~20 -> ~2 MiB, and `Caps` stops being a limit |
| Rust heap per row | 2.5 KiB/row (17 MiB at 7 K) | yes | virtualization (F6) first; then the layout engine's per-leaf state (0.75 KiB/row) |
| image atlas CPU copy + texture (dashboard) | 8 | yes | upload and drop the CPU copy; size the texture by need |
| font copies | 1.5 | yes | three copies of the face's bytes (the window config's, the leaked shaping copy, the glyph cache's) -> one |
