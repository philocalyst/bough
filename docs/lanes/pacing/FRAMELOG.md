# The frame log

A per-turn record of where input-to-commit time goes, built for the pacing lane's F2 trace and kept as a
diagnostic. Source: `crates/bough-metal/src/framelog.rs`; reader: `bench/pacing/framelog.py`.

## Free when off

`framelog` is a cargo feature of `bough-metal` (and `bough`, which forwards it). Without it every entry point is an
empty `#[inline(always)]` function and every call site whose arguments cost anything (an Objective-C message for a
timestamp, a pick classification) sits behind `if framelog::ON`, a `const false`; the default build compiles to the
same code (checked by the instruction cells, X4 card G5). With it, a mark is one `mach_absolute_time` read and a
thread-local store, a turn adds two syscalls (thread CPU time, minor faults), and a row is a mutex-guarded push into
a preallocated table of 2^17 rows (overflow is counted, not grown).

Enable it with `--features framelog` (the probe: `cargo build --release -p bough-awagy --bin pace_probe --features
framelog`); `BOUGH_FRAMELOG=<file>` writes the table at exit.

## Clock

All times are ns on the host clock (`mach_absolute_time`, uptime without sleep): the clock of `NSEvent.timestamp`,
`CACurrentMediaTime`, `CADisplayLink.timestamp` and `CVTimeStamp.hostTime`. A real event's timestamp is its
hardware time; a synthetic one's (the probe's) is its post time.

## Rows

One row per line, 16 unsigned integers, the first is the tag. Unused words are 0.

| tag | row | words 1.. |
|---|---|---|
| 1 | turn (main thread) | kind; event time; dispatch; first GPU sync returned; settled; described; laid out; frame began; surface picked; slot written and released; end; publication sequence (0 = no frame); flags; thread CPU ns; minor faults |
| 2 | publication | sequence; posted; taken by the publisher; GPU done (as the publisher saw the event); commit begin (just before `setContents:`, where AWAGY's observer timestamps); commit end; committed (0 = held back after a bin overflow); surface; committing thread (0 publisher, 1 main) |
| 3 | vsync | the vsync; the target or output time; when the callback ran; source (0 the host's CADisplayLink, 1 the measurement CVDisplayLink) |
| 4 | note | what; when; a; b |
| 5 | event received by the host | kind; the event's own time; received; sequence (synthetic mouse events carry it as their event number) |

Turn kinds: 1 pointer move, 2 down, 3 up, 4 wheel, 5 key, 6 display-link callback, 7 timer (caret blink),
8 resize, 9 pointer left, 10 a frame turn (the pacer's present), 11 other.

Turn flags: bit 0 a frame drew or published, 1 the epoch had geometry/text damage, 2 paint damage, 3 the input left
something to show (damage, or an input animation started), bits 8–15 the surface pick (1 already on screen, 2 a
held surface published without GPU work, 3 drawn), bits 16.. tiles repaired.

Notes: 1 a queued publication superseded before the publisher took it (a = its sequence, b = the newer one);
2 a frame waited for a free surface (a = µs); 3 trim; 4 keep-warm blit; 5 the display link paused (a = 0) or
started (a = 1); 6 bins grew (a = new capacity); 7 a synthetic event posted (a = kind, b = its time);
8 occlusion at exit (a = visible); 9 held back; 10 a probe phase began (a = index, b = time); 11 process wakeups
and instructions so far (a, b; idle windows come in pairs); 12 the CPU waited for the GPU before writing the
region (a = µs, b = frame); 13 a residency drain (a = µs, b = frames drained); 14 a surface made ready (a = µs, b:
1 first use, 2 purged, 4 was volatile); 15 an atlas upload (a = µs, b: 1 glyphs, 2 glyphs resized, 3 images);
16 a latch armed (a = target vsync, b = latch time).

## Derived quantities (`framelog.py`)

* **Input to commit**: for a turn whose input left something to show, from the event's own time to the commit end
  of the first committed publication at or after the first frame turn that follows it (a superseded publication is
  shown by the next committed one).
* **Input to vsync after commit**: the same, to the first measurement-recorder vsync at or after that commit end.
  It assumes the compositor latches at the vsync; it is not a photon time.
* **Commit intervals and worst gap**: AWAGY's `frames.py` rule over commit-begin times: from the first commit at or
  after the window start to the window end, the trailing gap included in the maximum.
* **Refreshes with 2+ commits**: commits that land in the same vsync interval (all but the last are never shown).
