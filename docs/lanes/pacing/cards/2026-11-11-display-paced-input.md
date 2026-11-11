# Card: display-paced input with a late latch (X4, F1, F13; with the F2 trace)

```
lane:            pacing
state:           CARD (frozen before any X4 code; amendments below, never edits)
owner:           pacing session, round 6 (one owner)
predecessor:     lane/pacing/round6 at the frame-log commits (lead/round6-base aa9943e + feature `framelog`);
                 nightly-2026-10-17; probe in `release`, instruction cells in `dist`; direct Metal presenter
question:        Can the interactive host present at most once per refresh, drop and reorder no input, stay at
                 zero wakeups when idle, and keep input-to-vsync-after-commit within 1 ms of frame-per-event?
change:          The interactive host's input path (crates/bough/src/backend/host.rs `run_interactive`, and a new
                 backend/pace.rs). Today every NSEvent runs a turn and a frame from the event monitor (~240
                 commits/s on a 120 Hz panel; half are never shown), and input-animation frames are "paced":
                 committed at the next sync, which the next input event usually triggers.
                 1. Input queue. The monitor converts the event and pushes it; it does not run the turn.
                    Adjacent pointer moves coalesce to the last; adjacent wheel deltas of the same kind (line or
                    precise), point and sign sum (exact: same-sign clamps compose). Nothing else coalesces: downs,
                    ups, keys and leaves keep their count and order.
                 2. Drain. A run-loop observer (before waiting, common modes) drains the queue once AppKit has
                    delivered everything already queued: one turn per item, eagerly (state is current as soon as
                    the main thread is free; only frames are deferred).
                 3. Pacer. After a drain that left damage: if the display link is not running and the last
                    present was at least one refresh period ago, present now through the publisher thread (the
                    sparse case keeps frame-per-event's latency and cost, RESULTS 12.2). Otherwise mark pending and
                    run the display link. Its callback (vsync V, target T) steps input animations (glides, fades),
                    then, if anything is pending, arms one latch at T - lead: the queue is drained again and ONE
                    frame is drawn and published through the publisher (late latching: input is sampled as late as
                    the deadline allows; the GPU and commit fit in `lead`). A callback with nothing pending and
                    nothing animating pauses the link (zero idle wakeups).
                 4. Input-animation frames take the same latch, so their commits follow the vsync, not the next
                    input event (the scroll p95 mechanism of the F2 trace).
                 5. The publisher thread runs at user-interactive QoS (the latch's lead must bound its wake; the F2
                    trace measured post-to-take delays up to 17 ms under load at default QoS).
                 frame() vs frame_paced() (BRIEF 4.3 #1): the host is the turn's source and picks. Input-sourced
                 frames (events and the motion they started) publish in the same refresh (frame()); clock-only
                 frames (the caret blink, run_window*'s scene clocks) stay paced (frame_paced()). Applications
                 never call either (Application::run). App's API is unchanged by this card; folding the two
                 into one public entry point is recorded as a follow-up, not built.
implication:     Passing establishes, on this Mac (M3 Pro, built-in 120 Hz ProMotion panel, macOS 15), for input
                 posted to the app's own event queue (not window-server HID input): presents <= refreshes, no
                 dropped or reordered input, the input-to-commit and input-to-vsync-after-commit distributions
                 against frame-per-event, zero idle wakeups from bough. It does NOT establish input-to-photon (no
                 photon or compositor-latch measurement exists here; "vsync after commit" assumes the compositor
                 latches at the vsync), AWAGY's own numbers (its driver posts global HID input, not allowed this
                 round), or any other platform.
platforms:       macOS 15 / Metal / Apple M3 Pro / built-in 120 Hz display.
workloads:       pace_probe dashboard (AWAGY's schedule: prepare, 1 s idle, then typing, Refresh clicks, toggle
                 clicks, pointer move + wheel line, 240 events/s, 2 s each) x 10 per build;
                 pace_probe scroll at 1,000 / 3,000 / 5,000 / 7,000 rows (wheel lines at 144/s, reversing every
                 0.5 s, 5.5 s; AWAGY's window: 3 s from the first commit after a 1 s warm-up) x 3 per build;
                 pace_probe idle (5 s after mount); dashboard with a 12 s idle tail (after the keep-warm window);
                 the storm test (headless, in-process).
gates:           G1 presents <= refreshes in every phase window of every run, and refreshes holding 2+ commits
                    <= 1 % of refreshes (frame log: commits vs the measurement recorder's vsyncs).
                 G2 no dropped or reordered input. (a) windowed: every posted event is received, per kind.
                    (b) storm test: a seeded random storm (moves, same- and mixed-sign wheels, clicks, keys) pushed
                    through the queue in random batches and drained into a real App: click routes == clicks, the
                    text field == the typed sequence, the scroll offset == the sum of the deltas.
                    Negative control: feature `sabotage_coalesce` (adjacent clicks coalesce) must fail (b).
                 G3 zero idle wakeups from bough: no link callback, timer, turn or frame in an idle window (frame
                    log), process wakeups (proc_pid_rusage) reported; powermetrics needs sudo and is not used.
                 G4 pixel gates unchanged: cargo test --release -p bough, -p bough-scenes, -p bough-zgui.
                 G5 per-frame instruction cells within 1.02x of the predecessor (slope method, dist): heavy
                    stream, heavy scroll, lab both, animate, vlist scroll, RENDER=1 and 0.
                 G6 BOUGH_CHECK at the end of a probe run: the surface on screen == a full redraw.
thresholds:      G1-G4, G6 exact. G5 <= 1.02x per cell.
                 Latency (per dashboard phase, median over the 10 runs of each run's percentile):
                 input-to-vsync-after-commit p50 and p99 <= frame-per-event + 1.0 ms. Input-to-commit is reported,
                 not gated (deferring by design raises it for storm inputs).
                 Targets reported against, not gating this card: worst dashboard gap <= 16.7 ms (2 refreshes);
                 scroll commit-interval p95 <= 8.6 ms at every row count (AWAGY's bar is 8.4 at its observer).
measurement:     Frame log (feature framelog) in both builds; A = predecessor probe, B = X4 probe, run ABAB
                 under the measurement lock, load average recorded per batch. Instruction cells by the slope
                 method (bench/integrate/slope.py) on dist builds of both, interleaved per cell. A third variant
                 (B with the frame drawn at the link callback instead of the latch, env, framelog builds only)
                 is measured to show what the latch is worth.
budget:          this session; this Mac only; no remote host.
stop conditions: G2-G5 fail and no bounded fix -> REJECTED with the numbers. Latency threshold missed at every
                 lead tried in [1.5, 4] ms -> REJECTED for latency, presents result kept as a finding. The lock
                 unavailable -> report the gates and the unlocked numbers labelled as such.
do not revisit:  RESULTS 12.3's present paths (CAMetalLayer drawables, CAMetalDisplayLink,
                 AVSampleBufferDisplayLayer, many small layers); the publisher thread and pre-armed stream
                 (RESULTS 14.2); the content-addressed ring; per-row hover signals; a separate hit index.
```
