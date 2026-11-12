# zortax gui-bench on ilo-prod: handoff (r6)

Status as of 2026-11-12 ~01:45 EST: builds done, quick matrix (run 1) still running. No results collected yet, no second run started.

## Where things are
- Host: ilo-prod (NixOS, 16 cores, 62 GB RAM, /root has 1.9 TB free). GPU: PCI 1a03:2000 (ASPEED BMC VGA, no 3D). Software Vulkan via Mesa lavapipe only. `vulkaninfo` not installed.
- Work dir: /root/work/zortax
  - gui-bench: /root/work/zortax/gui-bench, commit 0fd45107d64b3617f87dd4714f4ac1f63886cbc7 (2026-11-12 02:03:25 +01:00, main)
  - fakeland: /root/work/zortax/fakeland, commit 71695e7c5315c916649f46c1e32376cce75befa3 (2026-11-12 00:52:37 +01:00, main)
  - Nix env: /root/work/zortax/env (env.nix pins nixpkgs f6bf1059f02576e0b913c7b49d5e4eb6dda9cc22; result symlink). Source /root/work/zortax/env/env.sh before any cargo command.
  - Rust: nightly-2026-04-16 via rustup, RUSTUP_HOME=/root/work/zortax/rustup, CARGO_HOME=/root/work/zortax/cargo.
  - Local path patch for runner: /root/work/zortax/gui-bench/runner/.cargo/config.toml (points fakeland at ../../fakeland). zgui app builds against the GitHub feat/fakeland-marks branch (no local patch).
  - Build script: /root/work/zortax/build.sh. Logs: /root/work/zortax/logs/ (build-*.log, build-all.log, quick-1.log).
- rex: ilo-prod session `bough-r6` (session:01a1ccd3-828f-72bc-8d3d-490dde484869). My pane is window "zortax-latest", block:01a1ce5c-3c66-75ff-811c-63d7b3116a2d. Other agents use other windows in this session; do not type into them.

## The run in progress
Launched in my pane (bash), after the dioxus-skia build:
```
cd /root/work/zortax/gui-bench && nice -n 19 /root/work/zortax/gui-bench/runner/target/release/gui-bench --quick --out /root/work/zortax/results/quick-1 > /root/work/zortax/logs/quick-1.log 2>&1; echo "QUICK1_RC=$?"; echo QUICK1_END
```
Output per run goes to /root/work/zortax/results/quick-1/ and the runner's stdout/stderr to logs/quick-1.log.

Tell it finished: `rex -S ilo-prod capture -s session:01a1ccd3-828f-72bc-8d3d-490dde484869 -b block:01a1ce5c-3c66-75ff-811c-63d7b3116a2d --trim | grep QUICK1_END`. The line `QUICK1_RC=<n>` gives the exit code. As of the last check the marker had not appeared.

Collect: capture the log tail, then the results tree, e.g. `cat /root/work/zortax/results/quick-1/run.json` and per-cell `cell.json`. Copy with `rex ... capture` (the pane is text-only; no direct scp, and ssh is not allowed).

## Builds (all release, nice -n 19, -j8)
| Target | Result | Seconds |
|---|---|---|
| fakeland CLI | ok | - |
| gui-bench runner | ok | ~75 |
| zgui | ok | 167 |
| gpui-kit | ok | 234 |
| gpui-kit-fast | ok | 330 |
| freya | ok | 118 |
| freya-056a5b4 | ok | 86 |
| vizia | ok | 69 |
| floem | ok | 129 |
| iced | ok | 82 |
| egui | ok | 93 |
| dioxus | ok | 137 |
| dioxus-skia (feature build) | ok | - |
All 11 app builds ok (10 apps plus dioxus-skia). No build failures.

## Re-run from scratch
See RUNBOOK.md next to this file.
