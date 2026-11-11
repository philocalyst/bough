lane:            verification
state:           CARD
owner:           one session, branch lane/verification/round6
predecessor:     aa9943e7ae (lead/round6-base), toolchain nightly-2026-10-17, release profile for tests
question:        What causes each of the five bough-suite failures on the round-6 base (the first test run after
                 the api and floor merges), and what is the correct fix for each?
change:          Root-cause each failure (anime wires vs lanes; layout_screen remount/resize and host-vs-fixed;
                 lists teardown; virtual nested scrolls panic in App::copy_plan) against the pre-merge branches
                 (api ae360b7c6d, canonical f1b233f5c6, floor db078e039b) and fix the cause. A defect in a
                 mechanism (damage, presenter, layout, virtualisation) whose correct fix is larger than a local
                 correction is not papered over: it gets a minimal reproduction and a diagnosis in
                 docs/lanes/verification/failures.md and its test stays failing.
implication:     Passing establishes that the five gates hold on this tree as written. It does not establish raster
                 fidelity (every pixel gate compares the GPU path with itself), nor anything about performance.
platforms:       macOS arm64 (M3 Pro), Metal, direct-Metal presenter (default features).
workloads:       `cargo test --release -p bough-suite` (each failing test alone first, then the package), the tests
                 of every other crate touched, `cargo check --workspace --all-targets`.
gates:           the five tests exactly as they are: no gate is weakened, skipped or loosened, no exact comparison
                 becomes a tolerance. Every other bough-suite test stays green. Negative control: for each fix,
                 reverting it alone must make its test fail again (checked by reasoning or by a re-run).
thresholds:      0 failures in bough-suite, or each remaining failure documented in failures.md with a reproduction.
measurement:     none (no performance claim).
budget:          this session only; no remote hosts; one release target dir in this worktree.
stop conditions: a failure whose correct fix needs a mechanism redesign is recorded, not forced green.
do not revisit:  docs/SETTLED.md items; the hygiene sub-lane's scope (warnings, paths, CI).
