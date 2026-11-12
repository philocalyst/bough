# zortax gui-bench on ilo-prod: runbook (r6)

Run everything as root on ilo-prod inside /root/work/zortax. Use bash (the login shell is bash). Long steps go through `rex ... run`/`send`, or background loops, not one blocking call.

## 1. Clone and pin
```
mkdir -p /root/work/zortax && cd /root/work/zortax
git clone https://github.com/zortax/gui-bench.git
git clone https://github.com/zortax/fakeland.git
# latest used in r6: gui-bench 0fd45107d64b3617f87dd4714f4ac1f63886cbc7, fakeland 71695e7c5315c916649f46c1e32376cce75befa3
# to update: (cd gui-bench && git pull) ; (cd fakeland && git pull)
```

## 2. Nix environment
`/root/work/zortax/env/env.nix` is the gui-bench-env buildEnv (copy of the r6 file) with nixpkgs fetched by fetchTarball at f6bf1059f02576e0b913c7b49d5e4eb6dda9cc22. `/root/work/zortax/env/fonts.conf` points at `/root/work/zortax/env/result/share/fonts`.
```
cd /root/work/zortax/env && nix-build env.nix -o result
```
`env/env.sh` sets RUSTUP_HOME, CARGO_HOME, PATH, PKG_CONFIG_PATH, LIBCLANG_PATH, VK_DRIVER_FILES (lavapipe), FONTCONFIG_FILE and CARGO_BUILD_JOBS=8. Source it before building.

Toolchain: `source env/env.sh && rustup toolchain install nightly-2026-04-16 --profile minimal -c rustfmt -c clippy -c rust-src && rustup default nightly-2026-04-16`.

## 3. Local patch for the runner
Create `gui-bench/runner/.cargo/config.toml` (git-ignored) with:
```
[patch."https://github.com/zortax/fakeland"]
fakeland = { path = "../../fakeland/crates/fakeland" }
fakeland-bench = { path = "../../fakeland/crates/fakeland-bench" }
fakeland-trace = { path = "../../fakeland/crates/fakeland-trace" }
fakeland-types = { path = "../../fakeland/crates/fakeland-types" }
```
(paths are relative to runner/, so ../../fakeland is /root/work/zortax/fakeland)

## 4. Build
`/root/work/zortax/build.sh` builds fakeland, the runner, and each app (APPS env var overrides the list). It writes logs/build-*.log and a summary to logs/build-all.log ending BUILD_ALL_DONE. Then build the Skia edition:
```
source env/env.sh; cd /root/work/zortax/gui-bench/apps/dioxus && nice -n 19 cargo build --release -j8 --no-default-features --features skia --target-dir target/skia
```

## 5. Run
Runner from gui-bench dir, under nice, no systemd cap (the justfile's systemd-run scope was not used here):
```
source /root/work/zortax/env/env.sh
cd /root/work/zortax/gui-bench
nice -n 19 runner/target/release/gui-bench --quick --out /root/work/zortax/results/<name>
```
Options (from --help): --scenarios, --frameworks, --variants, --rows, --hz, --repeats, --op-repeats, --iterations, --churn, --resume. The full matrix takes hours (per the gui-bench README). For the fine-variant set requested by the brief use `--scenarios table,ticker --variants fine --rows 1000,10000` and `--scenarios rows,typing,spinner --rows 1000`. Note: --rows applies to every selected scenario; run these as separate invocations.

Outputs: results/<name>/run.json and per-cell directories with cell.json, app.log, screenshot.png, and the fakeland reports (*.json).

## 6. Plots
`python3 gui-bench/plots/all.py results/<name>` (needs matplotlib from the nix env; the env includes it).

## 7. Adding a framework (for bough)
- Create `apps/<name>/` as its own cargo workspace with a binary `<name>-bench`. Its `rust-toolchain.toml` pins the toolchain the rest of the repo uses.
- Share `apps/shared/spec.rs` (layout, colors, data, table ops) and `apps/shared/cli.rs` (command line: `--scenario NAME --rows N [--variant V]`).
- Implement each scenario in `rows`, `table`, `ticker`, `spinner`, `typing`, `search`, `virtual`, `resize`, using the framework's normal widgets, keyed lists, timers and animation APIs. Table ops are F1-F8 with undo keys. Layout check: draw the same pixels as the reference (screenshot.png at fixed points).
- Register the framework in `runner/src/frameworks.rs` (name, app dir, optional build feature, target dir) so the runner finds `apps/<name>/target/release/<name>-bench`.
- Optional fakeland-marks protocol if the framework can emit frame/app marks.
- Build with `cargo build --release -j8` in apps/<name>, and smoke-test one cell before adding it to the matrix.
