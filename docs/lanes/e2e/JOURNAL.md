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

## 4. What was built (2026-11-12; `2e0523c35f` and after)

Four crates and the app, none depending on the façade:

* **`crates/bough-glyph`**: a prepared glyph image. `examples/bake.rs` runs the real shaper and rasteriser
  (swash, as `bough-text`) over a character set (Latin-1) at one size and writes advances and kerning in font
  units (kerning = the first glyph's advance inside a pair minus its advance alone, so GPOS classes and the
  legacy table are both in it), the ligatures the shaper forms (DejaVu Sans: ff, fi, fl, ffi, ffl; found by
  shaping every pair and every triple that starts with a ligating pair) and unhinted alpha coverage. The image
  is read in place from rodata (`Image::new` is `const`); placing a string is arithmetic over its bytes.
  Gate `tests/exact.rs`: placement and coverage equal to swash's on 4,013 strings (137 ligature folds, 1,582
  kerned strings) and 195 glyphs; mutants `kern` and `cov` fail. DejaVu Sans at 14 px is 96,813 bytes of which
  76 KB is the kerning square. What it does not hold (other scripts, context beyond pairs and short ligatures)
  is the real shaper's, lazily: **not built**; `Image::covers` is the test a caller makes.
* **`crates/bough-span`**: the CPU span raster as a `bough-x` `Backend`. A column is painted in one pass, each
  record straight into the surface, restricted to a set of 32-pixel tiles (`Spans::cells`: every primitive is
  cut to the runs of damaged tiles it touches, so a glyph that straddles a damaged and an undamaged tile is
  blended once). Kinds that draw themselves through `bough-x`'s new `draw.rs` (`Draws` / `Prims`, the open end
  of the closed set of kinds; the same shape as `bough-paint`'s `Paint` / `Sink`): `Text` (the text is an
  expression's value: a record constant, or a function of block words evaluated when they change), `TextOf` (a
  `Line` of application state named by a versioned handle, never a pointer), `Bar` (a caret or a selection
  inside its box), `Spin` (a rounded box turned by an expression over `Time`).
  **It does not use `bough-paint`'s tile kernels.** Those shade through a Q15 accumulator per tile so that CPU
  and GPU agree word for word; nothing on this path has a GPU twin, and a span written once is several times
  cheaper than an accumulated tile (ROUND8 section 6's "CPU spans" class). They stay the executor for paints
  that need them. Told to the lead as a departure from the brief's wording.
* **`crates/bough-host`**: `wl.rs`, a Wayland client on the wire (libc only: no libwayland, no protocol crate,
  no xkbcommon, nothing `dlopen`ed); `pace.rs`, the ring (per buffer, the tiles damaged since it was drawn; a
  frame goes into the free buffer with the least to repair; the compositor is told the damage since the
  previous *commit*); `keymap.rs`, a reader for the part of an xkb text keymap a Latin layout needs (level two
  by Shift, letters by Shift xor Caps Lock; no AltGr, no second group, no keypad level: stated in the file);
  `run.rs`, the loop.
* **`comparisons/bough-guibench-x`** (binary `bough-bench`): rows and virtual (one view: there is no virtual
  variant to write), table fine (a relation with its journal) and coarse (opaque state, re-describing mode),
  ticker, spinner, typing, search. State is plain structs and handlers are methods; events go through the
  surface lane's dispatch (`on_hover`, `on_click`, `on_key`, `on_text`). The `view` bodies are written against
  the core (rows as tuples of records, rects by arithmetic): **not the façade's idiom yet** (Amendment 2); the
  element layer replaces those bodies and nothing else here.

Binary: 1,070,976 bytes at `dist` on ilo-prod (the façade's: 55,506,840), built in 18 s from nothing.

Gates (`comparisons/bough-guibench-x/tests/pixels.rs`, headless, any platform; Mac, release): the host's ring
and the span raster driven as the Wayland loop drives them, against a model compositor that behaves as
fakeland does (latches the newest commit, holds the buffer on screen, releases a superseded commit, copies only
the damage it was told, accumulated over commits it did not show). After every commit the committed buffer ==
a full redraw; at every latch the screen == a full redraw, and with a ring of three or more == the *newest*
state (I7: commit on input, no stale latch; bursts of 1 to about 6 inputs per latch); every eighth step == a
fresh mount; rings of 2, 3 and 4; resizes and scale 2 mid-script; all six scenarios, 60 to 400 steps each. A
ring of 2 starves under bursts (288 of 400 steps had to wait for a release) and then latches the last
*committed* state, which is the old rule's behaviour; 3 never starves. Mutants, each failing the file:
`mutant_straddle`, `mutant_age`, `mutant_superseded`. Also `filter_equals_the_definition` (the search filter's
refinement stack against `spec::matches` over 3,000 edits).

First run under fakeland (ilo-prod, `fakeland serve --renderer pixman`, rows 3,000): a correct first frame, and
hover on a pointer move, at the first attempt (`/root/work/r8/e2e/smoke/t1.png`).

Found on the way: the façade's spinner is driven by a 16 ms application clock (`Spinner::clock`, 1000 / 16 =
62.5 turns of the crank a second), which is why it stays at 62.5 fps at 120 and 240 Hz in the lead's sweep. It
is not a property of the host. Here the turn is an expression over `Time`, and time advances on frame
callbacks only.

## 5. Startup, first measurement in the harness's own analyzer (2026-11-12, `130e5a9a56`)

`fakeland bench startup` (the preset the runner's `startup.json` comes from: spawn, connect, first buffer,
first present), 15 measured iterations after 2 warm-ups, **warm** (binaries and libraries in the page cache),
ilo-prod with load 11 from other tenants, compositor and app on CPUs 8-15 at nice 19, fixed 60 Hz, pixman.
Medians (minimum in brackets), milliseconds from spawn:

| binary | connected | first buffer | first present |
|---|---|---|---|
| floor (`bough-host/examples/floor.rs`: one solid buffer) | 1.37 (1.16) | 4.17 (3.70) | 15.19 |
| new core, rows 100 | 1.45 | 4.51 (4.17) | 15.15 |
| new core, rows 3,000 | 1.36 | 4.27 (4.15) | 15.24 |
| new core, virtual 100,000 | 1.37 | 4.32 (4.05) | 15.27 |
| new core, table 1,000 | 1.54 | 4.55 | 15.28 |
| new core, search 1,000 | 1.55 | 4.62 | 15.18 |
| façade (`74e665786c`), rows 3,000 | 11.52 | 64.67 (55.45) | 78.62 |
| façade, virtual 100,000 | 11.34 | 35.91 (27.38) | 47.70 |

So spawn to first buffer went from 64.7 ms to 4.3 ms at rows 3,000 (15x) and is the same at 100,000 rows
(prediction F: at most floor + 1.5 ms and within 10 % across sizes: holds; the app is 0.1 to 0.45 ms above the
floor). `first present` is the same 15.2 ms for every binary that commits before the first latch: the preset
spawns at a fixed phase, so it measures the compositor's grid, not the app.

**But the floor itself is 4.2 ms, not the 2 to 3 predicted (F0 missed), and the app's stages are not where
predicted.** The host's own timestamps (`BOUGH_HOST_STATS`, nanoseconds from entering the loop; three runs each,
a build running beside them):

| stage | rows 3,000 | virtual 100,000 | search 1,000 |
|---|---|---|---|
| `connect` returns | 49 to 57 µs | 51 to 75 µs | 49 to 59 µs |
| write 1 (registry, sync) | +4 µs | +4 µs | +4 µs |
| **mount, and the memfd for one buffer** | **+27 µs** | **+27 µs** | **+41 µs** |
| globals arrived (round trip 1) | +7 µs | +8 µs | +7 µs |
| write 2 (binds, surface, toplevel, commit) | +4 µs | +4 µs | +4 µs |
| **first raster into the first buffer** | **+2.63 ms** | **+2.75 ms** | **+2.86 ms** |
| configure (already waiting) | +3 µs | +3 µs | +4 µs |
| write 3 (ack, pool, buffer, attach, damage, commit) | +6 µs | +6 µs | +6 µs |

* F2 (two round trips, under 0.5 ms of client time): holds; the handshake is 20 µs of client time.
* F3 (no font file): holds; no `openat` of a font exists in the binary's path.
* F4 (mount under 0.2 ms, same at any size): holds, 27 µs including `memfd_create`, `ftruncate` and `mmap`.
* F6 (the first commit in the write that acknowledges): holds.
* **F5 (first raster under 0.7 ms) is killed: 2.6 to 2.9 ms.** The floor's solid fill costs the same, so it is
  not the raster: it is the kernel's first touch of the buffer. 1280 x 800 x 4 bytes is 1,000 pages of shared
  memory, each a fault that allocates and zeroes a page: about 2.6 µs a page here. That is 60 % of everything
  between spawn and first buffer, for the floor and for the app alike.
* The 1.4 ms before `connect` is before `main`: fakeland's `setpriv` exec, the app's exec, the dynamic loader.
  Table and search connect 0.18 ms later than rows because their model (1,000 rows of strings) is built before
  the connection opens: to be moved after write 1.

So the chain as reformulated has one stage left that matters, and it is neither mount nor raster nor text: it
is where the first frame's memory comes from. Experiments, predictions written now:

| # | change | prediction | kill |
|---|---|---|---|
| M1 | `fallocate` the first buffer's pages (one call: no faults for allocation), then map with `MAP_POPULATE` | first raster 2.6 ms to at most 1.0 ms | under 20 % gain |
| M2 | transparent huge pages for the memfd (`MADV_HUGEPAGE`, if the host's shmem policy allows) | 2 faults instead of 1,000: at most 0.5 ms | policy is `never`, or no gain |
| M3 | touch the pages from several threads while the main thread connects and mounts | at most 2.6 / cores + 0.1 ms | worse than M1 under load |
| M4 | the model built after write 1 | table and search connect as early as rows | no change |
| M5 | static linking (no loader) | connect 0.3 ms earlier | under 0.1 ms |

## 6. Rows hover, first measurement with the harness's preset (2026-11-12, `130e5a9a56`)

`fakeland bench hover-latency` with the runner's own parameters (`steps.rs`: the pointer enters row 5 from row
1, pixel detector on row 5, 12 repeats, 3 measured iterations after 1 warm-up), ilo-prod, load 4 to 7,
compositor and app on CPUs 8-15 at nice 19, pixman. Medians in milliseconds. Not yet through the runner (its
lane-local rebuild failed on a path; next), so no layout check and no validity record beyond the preset's own.

| timing | binary | rows | total p50 | input to commit p50 | p95 |
|---|---|---|---|---|---|
| unthrottled | new core | 100 | 0.119 | 0.127 | 0.150 |
| unthrottled | new core | 3,000 | 0.074 | 0.078 | 0.111 |
| unthrottled | new core | 100,000 | 0.075 | 0.079 | 0.114 |
| unthrottled | façade `74e665786c` | 3,000 | 0.901 | 0.907 | 1.186 |
| fixed 60 Hz | new core | 100 | 11.107 | 0.081 | 0.114 |
| fixed 60 Hz | new core | 3,000 | 11.100 | 0.085 | 0.112 |
| fixed 60 Hz | new core | 100,000 | 11.072 | 0.133 | 0.144 |
| fixed 60 Hz | façade | 3,000 | 11.097 | 0.870 | 1.052 |

I1 (at most 0.05 ms, the same at every size): **missed by 1.6x** (0.08 ms), and the size-independence is not
shown cleanly yet: the 100-row and 100,000-row cells swap places between the two runs (0.127 / 0.079 and
0.081 / 0.133), which is run-to-run noise on a shared host at nice 19, not size, but three iterations cannot say
so. Against the façade it is 11x to 12x. What the 80 µs is made of is not measured yet (the wake-up of a
niced process, two system calls, the repair of two rows in two buffers, the event path's allocations: the
surface lane's dispatch builds several `Vec`s per pointer event); that split, cold and warm, is next.

## 7. Answer to the threads lane: a pixel section handed to `wl_shm`

Section 5 says the first frame's cost is the first touch of its memory, so an image that carries pixels is
aimed at the right stage. What the first-commit path here accepts and what it does not:

* **Yes, the first `wl_buffer` can be backed by a file the app did not write this run.** `wl_shm.create_pool`
  takes any descriptor the compositor can map shared; the path already creates its pool lazily in write 3, so
  the image's own descriptor and an offset can stand in for the memfd. If the file's pages are in the page
  cache the client touches nothing: no faults, no raster.
* **That buffer is read-only to the app, for ever.** The compositor maps the file shared, so drawing into it
  would rewrite the image. It is committed once, never repaired, and destroyed when released. The ring treats
  it as a foreign buffer: every buffer of its own is still blank.
* **So the cost moves to the first input unless it is paid while idle.** The first frame the app draws itself
  is a full raster into a blank buffer with all its page faults (2.6 ms). The host must draw that buffer right
  after the first commit, before any input (it has nothing else to do), and from then on repairs by damage.
* **Size or scale differ from the image's: the image is not used.** The path already redraws when the first
  configure differs from the size asked for (`redrawn_for_size`); the same branch drops the image.
* **Validity is the hard part, and it is the image's, not the host's.** A pixel section is right only if the
  first frame is a pure function of what the image is keyed on (binary, arguments, size, scale, the block's
  initial words, the face). The host can make it self-correcting at no cost: the idle raster above produces the
  true first frame anyway; compare it with the image's pixels and, if they differ, commit the true frame with
  the difference as damage and rewrite the image. Then a wrong image costs one corrected frame, never a wrong
  window.
* Whether it is worth building: decide after M1 and M2 (section 5). If `fallocate` or huge pages bring the
  first touch under 0.5 ms the image saves that much of about 2 ms and adds a cache to keep right; if the
  kernel cost stays near 2.6 ms it is the largest thing left in start-up.

## 8. Wind-down (Amendment 7): the review's Batch 8, and what the search cell measures

Independent review `bough/runtime/lanes/independent-review/recent-20261112/REPORT.md`, findings H01 to H08 and
B10. State of each at `11a6ee519e` and after:

| finding | state | what was done, or what would close it |
|---|---|---|
| H01 fractional scale inconsistent | **stands** | `run.rs` sizes buffers and scales pointer coordinates by `scale120 / 120` but calls `App::resize` with the ceiling, so at 125 % layout and glyphs are 2x in a 1.25x buffer. Integer scales (1, 2: what the harness's scale switch uses) are consistent. Closing it: `App::resize(w, h, scale120)`, `Geo` in 120ths, a face image per size actually used (or the real rasteriser), rounding rules for rects. Not done: it needs the layout seam's fractional rules, and no cell exercises it. Until then the host should refuse a fractional preferred scale and stay at the integer above it with a viewport; **not built** |
| H02 resize leaks `wl_shm` pools | **closed** | a resize that has to start a fresh memfd now sends `wl_shm_pool.destroy` for the old pool (its held buffers stay valid and are destroyed on release); the old mapping and descriptor are dropped at once. Still open and stated: a fresh memfd per resize step while the old front buffer is held (cost: the new buffer's page preparation each step) |
| H03 keyboard is a hand-built subset | **stands** | by design for this round (nothing `dlopen`ed, static-linkable); listed open in the tally with the alternative: the round-7 host's `xkbcommon-dl` path (`crates/bough-wayland`), or a pure-Rust xkb implementation (kbvm). Missing: groups, AltGr, compose, key repeat; focus loss does not clear a held press |
| H04 the span raster changed alpha semantics | **closed for what it draws** | colours are 0xAARRGGBB, source-over on an opaque surface: alpha 255 is spans, 0 draws nothing, between is blended per pixel; text and turned boxes scale coverage by the colour's alpha. The app's colours are explicitly opaque (`rgb()`). Not built: a translucent *surface* (ARGB buffers), premultiplied operands |
| H05 text duplicated (`bough-glyph` beside `bough-face`) | see section 9 |
| H06 `Line` implicitly `Send`/`Sync` | **closed** | `Line` and `LineRef` carry `PhantomData<*const ()>`; two `compile_fail` doctests hold it |
| H07 / B10 filtering moved into the app | **closed** | `Search::refilter` is now the spec's scan: every keystroke runs `spec::matches` (which lower-cases both sides, allocating) over every label and rebuilds the result; no prefix stack, no precomputed lower case. The earlier refinement stack is deleted, not kept behind a switch. The cell is now comparable work; it is expected to be slower than section 3's I5 for search and is reported as it is |
| H08 author API and runner unfinished; resize remounts the world | **half** | the runner: done for all eight scenarios (section 10). The author API: stands until the element layer (the `view` bodies here are core fixtures). Resize: stands; `Scene::resize` makes a new `World`, which loses focus and press identity across a resize (the search field is re-focused by a synthetic press at its centre, which is a workaround, not a fix). Closing it needs `World::resize` in the core (tiles and spaces re-derived, stores kept) |

Also from B10, stated so nobody reads more into the numbers than is there: the app's text is one prepared
Latin-1 face (anything else is not drawn: the lazy shaper path does not exist); rows are laid out by arithmetic
in the view, not by a layout engine; the binary-size ratio (1.07 MB against 55.5 MB) compares different
profiles and feature surfaces and is not a framework size result (B12).
