lane:            iteration
state:           CARD
owner:           iteration session (round 6)
predecessor:     aa9943e7ae (lead/round6-base), nightly-2026-10-17, default features, profile release (bench) / dev+check (builds)
question:        Can the edit -> build -> gate -> measure loop be one command under two minutes that still catches a CPU
                 regression of 2 % and a broken presenter, and which build settings shorten edit-to-gate-result?
change:          bench/quick/run.sh (instruction slopes, re-description, cold mount, startup/shutdown, memory, size,
                 pixel smoke; JSON + table; --base comparison); lane-local build settings (profile, linker, codegen backend).
implication:     passing establishes a fast regression screen, not a replacement for the full slope scripts or the suite.
platforms:       macOS 15 (Darwin 24.6), M3 Pro, Metal, loaded machine (other lanes building).
workloads:       heavy stream/scroll, lab both, animate, vlist scroll (R0, R1); redescribe_cost 1,000 rows; counter and a
                 1,000-row list cold mount; counter and heavy startup/shutdown/memory; production counter size.
gates:           smoke must pass on the base and FAIL with --features bough/sabotage_copy; build settings must keep
                 `cargo check --workspace --all-targets` and the touched gates passing.
thresholds:      instructions flagged above 1.02x baseline; wall times flagged outside the baseline min-max band;
                 a build lever is adopted only if edit-to-gate-result improves and no gate changes verdict.
measurement:     instructions (proc_pid_rusage, no lock); wall times median of 5 under the measurement lock with load
                 average recorded; build times with load average, A/B interleaved where possible.
negative control: QUICK_INJECT=<n> adds a known loop inside the measured window; the comparison must flag it.
budget:          this session; disk <= one full target dir.
stop conditions: a lever that breaks a gate or needs a shared-manifest change is recorded and handed to the lead.
