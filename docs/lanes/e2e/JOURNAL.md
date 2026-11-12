# Lane `e2e`, round 8: journal

Branch `r8/e2e` (worktree `wt/r8-e2e`), from `r8/expr` `8722481a7b`, with `r7/platform` `74e665786c` and
`r8/raster` `e066757840` merged (`3851bd6f14`; the raster's copy of the seam is kept as `docs/r8/SEAM-raster.md`).
Nothing is pushed. Remote work: `ilo-prod:/root/work/r8/e2e` (`env.sh` there; every command through `lo`, which is
`taskset -c 8-15 nice -n 19`), and `mothership:/home/miles/work/r8/e2e` for counters and micro-timing.

The brief: (1) a gui-bench app for bough on the new core (`bough-x`, CPU span raster, a Wayland host), scenario by
scenario (rows, virtual, table, typing, search, ticker, spinner, resize), each passing the harness's layout check
and measured in the real harness beside the façade build; (2) the startup chain as one reformulated thing, app
share at most 5 ms and the same at 100,000 rows; (3) commit on input even when a frame callback is pending, in a
portable host layer, gated by pixel checks under a burst. Targets: `ROUND8.md`, Amendment 5.

Newest entries last. Every experiment has its prediction and kill criterion written before the run.

## 1. The façade's startup, taken apart (2026-11-12; the cmp-2 binary, `74e665786c`, sha256 52e30d0a171c0672)

Host: ilo-prod, load 9 to 12 from other tenants, CPUs 8-15 at nice 19, fakeland `71695e7` `serve --renderer pixman
--output 1280x800@60`, the app under `strace -f -ttt -T` (so every number below is inflated by strace: about 40 µs
per system call; the *order* and the *proportions* are what this run is for, the clean numbers come from the
harness). Traces: `/root/work/r8/e2e/dis/{rows3000,virt100k}.strace`.

| stage | rows 3,000 (ms since execve) | virtual 100,000 |
|---|---|---|
| execve to the first application system call (dynamic linking of libgcc_s, libm, libc: 13 failed `openat` probes of the nix library path, then relocations) | 0 to 4.0 | 0 to 4.0 |
| **`fc-match -f %{file} sans-serif` spawned and waited for (font discovery), before the Wayland connection** | 6.6 to 61.0 (51 ms in the child) | 7.0 to 62.0 (54 ms) |
| open and read `DejaVuSans.ttf` (757 KB) | 61.1 to 62.0 | 62.0 to 62.9 |
| `connect()` to the compositor | 62.4 | 62.9 |
| registry, binds, `dlopen` of libxkbcommon | 62.4 to 67.6 | 62.9 to 67.5 |
| **`fc-match` spawned a second time** (the application's `Config` is made twice: once to learn the window size, once at the real scale) | 67.7 to 115.7 (47 ms) | 67.6 to 111.8 (43 ms) |
| the font read and parsed again | 115.8 to 131.3 | 112.0 to 112.9 |
| mount (a 118 MB anonymous reservation, then the records; rows mounts all 3,000) | 131.4 to 174.3 (43 ms) | 112.9 to 116.4 (3.5 ms) |
| memfd, mmap, first raster, first `sendmsg` with the commit | 174.3 to 179.8 (5.4 ms) | 116.5 to 121.5 (5.0 ms) |
| wait for the frame callback | 180.0 to 191.3 | 121.7 to 138.6 |

Findings.

1. **Font discovery is a child process, twice.** Unstraced and warm, `fc-match` takes 8 to 10 ms on this host
   (16 ms measured through `taskset`/`nice`, of which 6.5 ms is the wrapper), so about 18 ms of the 54 ms
   spawn-to-first-buffer of the rows-3,000 cell; cold or under contention it is the 115 to 150 ms the harness
   records before `connected` in four cells of cmp-2 (rows-100, ticker-fine-1, search, virtual). It runs even
   though `BOUGH_FONT` is exported in the harness's environment. Nothing about it scales with rows.
2. The Wayland connection opens only after the first `fc-match`: the compositor could have been working on the
   registry meanwhile.
3. Mount is the part that scales: 43 ms at 3,000 rows (all mounted), 3.5 ms for the virtual list.
4. First raster plus commit is 5 ms either way.
5. Under this run virtual-100,000 reached its first commit *sooner* than rows-3,000 (121 ms against 180 ms),
   yet cmp-2 records 564.9 ms for it (connect at 115 ms, first buffer at 548 ms). So the 434 ms between connect
   and first buffer in cmp-2 is not mount: it is not reproduced here and is most likely the second `fc-match`
   (and the font read) under a cold page cache or a loaded host. Open; it disappears with the reformulated chain,
   and the cold/warm split below will say whether anything like it remains.

So the façade's 65.7 ms is roughly: 1 ms process start, 18 ms font discovery (two children), 2 ms font parse,
1 ms connection and globals, 30 ms mount (unstraced), 5 ms raster and commit, about 10 ms waiting for the latch.

Also inherited from this host (the lead's report of the heavy run): the façade's scenes binary loses its
connection at sheet-10,000 during the scroll probe ("Connection reset by peer"; hover, edit and theme had passed
in the same cell). Not reproduced by this lane yet. The app log shows no protocol error message, and a reset on
the client's side means the compositor closed the socket with data unread; the likely cause is the compositor's
send buffer to a client that does not read while it is busy (a scroll frame of the 10,000-row sheet takes tens of
milliseconds and the probe sends four axis events per refresh). The new host drains the socket before anything
else on every pass and coalesces input (newest state wins), so it cannot fall behind by more than one frame of
events; the burst gate (section 4) covers it.

## 2. The startup chain, reformulated: predictions (written before any of it was built)

What a first frame is made of when nothing is done that the first frame does not need:

| # | stage | prediction (ilo-prod, warm, unloaded core) | kill |
|---|---|---|---|
| F0 | **the harness's floor**: a static client with no runtime that connects, binds, and commits one solid 1280x800 buffer with its configure acknowledgement. Spawn to first buffer | 2 to 3 ms, of which most is not the client: `setpriv` exec, the client's exec, two round trips, and the kernel's first touch of 4 MB of shm pages (about 1,000 page faults) | if the floor itself is above 5 ms the target "app share at most 5 ms" is restated as "at most the floor plus 2 ms" |
| F1 | process start: static, no dynamic loader, no constructors | within 0.3 ms of F0's | a static std binary more than 1 ms slower to `main` than a freestanding one: then drop std's runtime |
| F2 | connection: `get_registry` and `sync` in the first write; all binds, the surface, the toplevel and its first commit in the second; nothing else until the configure | 2 round trips, under 0.5 ms of client time | a third round trip before the first buffer |
| F3 | text: no font file opened, no shaper constructed; glyph coverage and advances for the first frame come from a prepared image in rodata | 0 system calls, under 50 µs for the first frame's glyph placement | any `openat` of a font before the first commit |
| F4 | mount by exposure on `bough-x` | under 0.2 ms (about 190,000 instructions at any list size, expr journal section 5) and within 10 % between 100 and 100,000 rows | grows with rows |
| F5 | first raster by CPU spans straight into the first `wl_shm` buffer, done while the compositor answers the toplevel commit | under 0.7 ms including the page faults of the buffer | above 2 ms |
| F6 | the first buffer attached, damaged and committed in the same write as `ack_configure`; no frame callback awaited; no GPU, no worker thread | one `sendmsg` | a wait of any kind between the configure and the commit |
| F | spawn to first buffer, rows 3,000 and virtual 100,000 | at most F0 + 1.5 ms, the two within 10 % of each other | above 5 ms, or the two differ by more than 10 % |

The harness's `first_present` adds the wait for the latch (uniform over one refresh, so about 8 ms on average at
60 Hz plus the 1 ms latch offset) to every framework; it is reported, never chased.

What this needs from other lanes (sent to the lead with the first report):
* `text`: the prepared glyph image as a build product of the application's declared fonts and sizes (this lane
  builds a first form by hand, section 3), and the lazy path for what the image does not hold.
* `threads`: the mapped scene image. With mount at about 0.1 ms the map does not pay for the *mount*; what it
  could still pay for is the first raster (a mapped first frame is a `memcpy` of what differs from a solid fill).
  Asked: can the image carry tiles?
* `expr`: the element layer in the façade's idiom; until then the views here are written against the core.

## 3. Interaction: predictions (written before the app existed)

| # | cell | prediction | kill |
|---|---|---|---|
| I1 | rows hover, input to commit, 100 and 3,000 rows | at most 0.05 ms, the same at both sizes | above 0.1 ms, or size-dependent |
| I2 | rows scroll, CPU per frame | at most 0.5 ms (full repaint of the exposed rows by spans) | above 1 ms |
| I3 | table select, update10, swap, input to commit | at most 0.05 ms | above 0.15 ms |
| I4 | table remove and insert, input to commit and CPU per frame | at most 0.3 ms | above 0.6 ms |
| I5 | typing: handling and CPU per frame | at most 0.05 ms | above 0.15 ms |
| I6 | idle RSS | at most 8 MiB | above 20 MiB |
| I7 | commit on input under a burst of k inputs inside one refresh: the frame the compositor latches shows the newest input; presented == full redraw of the newest state on every latch; damage sent equals the tiles that differ from the previous commit | holds for k = 1..16 and ring sizes 2..4 | any stale latch, or any differing pixel |
