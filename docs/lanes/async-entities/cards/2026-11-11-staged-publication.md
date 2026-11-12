# Card: pending as a property of publication (A1, A6, A9, A4, A2, A5)

```
lane:            async-entities
state:           CARD
owner:           async-entities session, round 6 (Claude Fable)
predecessor:     42df2fbb67 (lead/core-r6; nightly-2026-10-17, release profile)
question:        Can bough's async be "fetch high, block low" as a property of publication (staged values, cohorts
                 that publish atomically or never) with zero cost to a turn that touches no async cell?
change:          The one global cohort and the reader-inline staging slot are replaced by per-source cohorts in a
                 slab (held source + its producers + a staging slot per flight), completions addressed by
                 RequestStamp (producer slot, generation, ticket) instead of (reader address, ticket), an injected
                 executor that drives futures (map_async / try_map_async / compute), is_pending as a source on
                 signals and readers, Settled<T> (reader.or), a typed failure table with retry.
implication:     Passing establishes the semantics under the model test's event alphabet (writes, completions in any
                 order, faults, retries, owner removal), not under speculative description (R1): an unwired read of a
                 held signal in `view` still reads intent.
platforms:       macOS / headless App (no GPU needed for the state machine)
workloads:       tests/reader.rs, tests/owners.rs unchanged; new tests/async_*.rs; redescribe_cost 10 100 1000; a
                 signal-write microcell (one wired signal, turn only); commit cost with 10,000 pending readers.
gates:           cargo test --release -p bough; the model property test (A9) over random interleavings in both
                 once and re-describe mode; the alias test (A6) fails under feature `sabotage_alias` (admission by
                 input value equality); cargo check --workspace --all-targets.
thresholds:      redescribe_cost every cell <= 1.02x; signal-write microcell <= 1.02x (target: identical instruction
                 count); a cohort commit's instructions independent of the number of other pending readers (<= 1.05x
                 between 0 and 10,000 bystanders).
measurement:     instruction counts (bough_metal::perf::tinstr), slopes over two repeat counts; no lock needed.
budget:          this session.
stop conditions: an existing reader/owners test would have to change; the non-async cells regress past 1.02x.
do not revisit:  no public Task, no hooks, no ambient context, no start_transition, no fallible Deref (00 §6).
```
