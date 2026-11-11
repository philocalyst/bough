# Card: a frame log that is free when off, and the dashboard stall trace (F2)

```
lane:            pacing
state:           CARD (frozen before any measurement; the log's code was written and smoke-run first, see below)
owner:           pacing session, round 6
predecessor:     lead/round6-base aa9943e; nightly-2026-10-17
question:        Where does the time between input and commit go per turn, and what produces the long gaps
                 (AWAGY dashboard: 75.7 ms in one of three runs, all three phases otherwise ~4 ms)?
change:          Instrumentation only. bough-metal `framelog` (feature, off by default): per-turn host-clock
                 timestamps (the input's own time, dispatch, first GPU sync, settle, describe, layout, frame,
                 surface pick, slot write and release, end), the publication sequence, the publisher's take,
                 GPU-done and commit times, notes for every known stall mechanism (ring waits, GPU waits, residency
                 drains, surface preparation, atlas uploads, trims, keep-warm blits, superseded publications), and
                 a measurement-only CVDisplayLink vsync recorder. A probe (pace_probe) runs AWAGY's two workloads
                 through Application::run with events posted to the process's own queue.
implication:     A trace attributes each gap to a mechanism in this in-process reproduction. It cannot reproduce
                 what only window-server HID input or the AWAGY observer would add; if the 75.7 ms gap does not
                 recur, the result is the mechanisms that do produce gaps and their sizes, not its cause.
platforms:       macOS 15 / Metal / M3 Pro / 120 Hz.
workloads:       pace_probe dashboard x 10, pace_probe scroll at 1,000 / 3,000 / 5,000 / 7,000 rows x 3.
gates:           Free when off: without the feature every entry point is an empty inline body (costly arguments
                 behind the const `framelog::ON`), so the default build's instruction cells must be unchanged
                 (checked with the X4 card's G5 cells). The log must account for every posted event.
thresholds:      instruction cells of the default build <= 1.01x.
measurement:     per-phase p50 / p95 / p99 / max of turn time, queueing, input-to-commit and commit intervals;
                 the five largest commit gaps of every run with their causes; load average per batch; runs under
                 the measurement lock.
stop conditions: none beyond the lock; unlocked runs are labelled as such.
do not revisit:  as the X4 card.
```

Recorded before freezing: the frame log, the driver and the probe were written and smoke-run (three dashboard runs
at load average ~80, while another lane held the measurement lock; they were single-window diagnostics and are
reported only as "under load, unlocked").
