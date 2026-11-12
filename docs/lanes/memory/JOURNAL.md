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
