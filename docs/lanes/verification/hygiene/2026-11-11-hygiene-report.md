# verification/hygiene, round 6: report

Card: `cards/2026-11-11-hygiene.md`. Base `aa9943e7ae` (`lead/round6-base`), toolchain nightly-2026-10-17, macOS arm64.
State reached: tasks 2 to 6 landed; task 1 (the failing tests) belongs to another session and was not touched.

## What changed

| task | change |
|---|---|
| 2 warnings | `crates/bough-gpu/src/fused.rs`: removed the never-read `Kern::region` (a second retain of the region buffer, still held as `region_raw`) and `Pipes::{bgl0, bgl1, layout}` (plain data kept only after pipeline creation). `cargo check --workspace --all-targets`: no rustc warnings. Two build-script notices remain ("no Metal compiler", bough-metal and bough-gpu); they depend on Xcode being installed and are not code. |
| 3 paths | `bench/awagy/tools/sizes.sh` (hard-coded `/Users/philocalyst/projects/bough`: now `BOUGH_ROOT`, default the checkout), `bench/floor/first_pixel.py` (`/Users/mileswirht/...`: `BOUGH_REF`), `bench/awagy/tools/run.py` (`~/projects/_ref`: `BOUGH_REF`), `bench/floor/windowed_gpu.py` (`/private/tmp/claude-501/...` lock: `BOUGH_WINDOWED_LOCK`, default `$TMPDIR/bough-windowed.lock`), `crates/bough-text/examples/report.rs` (a literal `~/...` that `std::fs::read` never expanded: argument or `BOUGH_REF`), two comments in `comparisons/bough-zgui`. Manifests had none (competitor sources are pinned git revisions). |
| 4 CI | `.github/workflows/bough.yml`, unexecuted. There was no other `.github/` in the tree to disturb (only a vendored softbuffer's under `reactivity/vendor`). |
| 5 gate | `bench/gate.sh` (see its header). |
| 6 SAFETY | 31 sites in `crates/bough/src` gained a `// SAFETY:` or `# Safety` (comments only). |

## Evidence

* `cargo check --workspace --all-targets`: exit 0, no warnings besides the two notices.
* `bench/gate.sh --base lead/round6-base` (changed crates `bough bough-gpu bough-text bough-zgui`): hygiene PASS, check PASS, tests PASS (43 test binaries ok), sabotage PASS (vlist gates pass as-is; 6 of 8 fail with `--features bough/sabotage_copy`).
* Negative controls: an absolute path added to a script fails hygiene; an unused variable added to `bough-mem` fails check; the sabotage step fails when the gates do not detect `sabotage_copy`.

## Left, with reasons

* `Cargo.lock`: the committed lock lacks `source = "git+...zgx..."` on `zgx-cosmic-text`, `zgx-harfrust`, `zgx-workload`; cargo rewrites it on every command, so `--locked` fails. Not committed (root files belong to the coordinator): regenerate it.
* Release builds warn about output filename collisions: `i18n_bench` (bough-text-bench, bough-suite), `text_bench` (bough-vlist, bough-suite), `label_churn` (bough-zgui, bough-text-bench). `cargo check` does not show it. One binary overwrites the other in `target/release`, and the bench scripts pick them by name, so a rename needs the scripts updated; left.
* Clippy is not clean (`cargo clippy --workspace --all-targets --no-deps`): deny-by-default `mut_from_ref` in `bough-core/src/prefix.rs:50` and `bough-metal/src/scene.rs:211,367`, then dozens of warnings. The CI clippy step reports and does not gate.
* SAFETY: nine `unsafe fn` trait-method impls (`Headed::at` in entity, list, reader, signal; `Items::len` in reflect) rely on the trait's contract and were left. The scan is a heuristic (a comment within four lines above or two below); fn-pointer types are not counted.
* Absolute paths in documents (BRIEF, STATUS, RESULTS files, research notes) and recorded logs under `bench/results`, `bench/baselines` are evidence or shared docs and were left.
* CI cannot be run here; whether a hosted macOS runner exposes Metal for the pixel gates is unverified.
