# Card: one commit per refresh at the deadline, from a time-constraint publisher

```
lane:            pacing
state:           CARD (frozen after the X4 A/B batch `ab1`, before any of this card's code)
owner:           pacing session, round 6
predecessor:     lane/pacing/round6 at the X4 amendment-2 commit + the caret-blink idle fix (built from the same tree,
                 selectable at run time in frame-log builds); nightly-2026-10-17; release probe, dist cells
question:        Does pacing the *commit* instead of the *frame* keep frame-per-event's input latency (p50 and p99
                 within 1 ms) under a loaded machine while committing at most once per refresh?
change:          X4's latch runs the frame on the main thread at T - lead; under load (1-minute average 50-92) its
                 timer fired 1.0-1.6 ms late at the median and 3.6-4.4 ms at p99, so discrete input (typing,
                 clicks) lost a refresh at p99 against frame-per-event (input-to-vsync p99 17.9-19.2 vs 9.3-10.3 ms).
                 Here the main thread draws as soon as input is drained (frame-per-event's latency), and the timing
                 moves to the publisher thread, which is the only place a deadline matters:
                 1. The host gives the presenter the refresh phase (target vsync T, period) at every display-link
                    callback; the link runs while presents come faster than one per 2 refreshes, or while something
                    animates, and pauses after 3 idle callbacks (zero idle wakeups, as X4).
                 2. With a fresh phase the publisher commits at most once per refresh, at T - 0.5 ms (the window
                    server takes a commit until ~0.3-1.3 ms after T: compositor-deadline note), taking the newest
                    job posted by then (older ones were superseded and their surfaces freed); without one (sparse
                    input) it commits at once, as today.
                 3. The publisher thread takes time-constraint scheduling (period one refresh, computation 0.5 ms,
                    constraint 1 ms) once pacing starts, so its deadline wake does not depend on the load.
                 4. Input animations are stepped and drawn at the link callback (a full refresh before their
                    deadline, robust to main-thread lateness) and committed at the deadline.
                 X4's latch stays selectable (`BOUGH_PACE=latch`, frame-log builds); the measurement picks the default.
implication:     Passing establishes the same as X4's card, with discrete input at frame-per-event latency. It does
                 not reduce main-thread or GPU frames for discrete input (one per drain, so up to two per refresh at
                 240 events/s): it caps commits, not simulations.
platforms:       macOS 15 / Metal / M3 Pro / 120 Hz.
workloads:       as X4's card, plus the hover sweep (pointer sweeping the toolbar, ~32 crossings/s).
gates:           G1-G6 of X4's card unchanged (presents <= refreshes and 2+-commit refreshes <= 1 %; no lost or
                 reordered input; zero idle wakeups from bough; pixel gates; instruction cells <= 1.02x; BOUGH_CHECK).
thresholds:      input-to-vsync-after-commit p50 and p99 per dashboard phase <= frame-per-event + 1.0 ms (as X4);
                 scroll: input-to-vsync p50 no worse than X4's latch + 1 ms.
measurement:     A = frame-per-event, B = X4 latch, C = this card, one ABC-interleaved batch under the lock, load
                 recorded; instruction cells on the default build.
stop conditions: latency or G1 missed -> REJECTED with numbers; X4 latch kept or not by its own numbers.
do not revisit:  as X4's card.
```
