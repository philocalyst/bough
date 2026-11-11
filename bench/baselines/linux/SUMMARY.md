# Linux baseline: zgx and zgui headless benchmarks

Host: ilo-prod (NixOS, kernel 6.18.54), AMD Ryzen 7 3700X 8C/16T, 62 GB.
Raw results: `bench/baselines/linux/` (copied from `/root/work/gui-bench/results/` on ilo-prod).

## Environment

Full output in `env.txt`.

- zgx: https://github.com/zeronsh/zgx at 06debe9d
- zgui: https://github.com/zortax/zgui at 951a0ea3
- rustc 1.97.1 (8bab26f4f 2026-07-14), LLVM 22.1.6, x86_64-unknown-linux-gnu
- Load before runs: ~0.2 idle. Timing runs go through `bench` (flock on /root/work/.bench.lock, cores 0-3,8-11). Builds go through `build` (cores 4-7,12-15).
- perf: `/root/work/bin/perf` points at a missing nix store path. Used nix perf 7.3.1 (`/nix/store/bw8frr0nk26m740a2mwaw5yi06mgxakb-perf-linux-7.3.1/bin/perf`) placed first on PATH. Verified it counts `instructions:u`.
- Native libs from nix: dbus, wayland, libdrm, libxkbcommon, vulkan headers/loader dev outputs, libclang (`LIBCLANG_PATH`). Runtime libs: libstdc++ (gcc-16.2.0-lib) and libfontconfig (fontconfig-2.18.3-lib) via `LD_LIBRARY_PATH`. The setup is in `/root/work/gui-bench/scripts/env_bench.sh`.
- No GPU adapter on this host: `/dev/dri/card0` exists, but there is no GL or Vulkan ICD. No drivers or software rasterisers were installed.

## Commands

```
# setup
git clone https://github.com/zeronsh/zgx && git -C zgx checkout 06debe9d
git clone https://github.com/zortax/zgui && git -C zgui checkout 951a0ea
source /root/work/gui-bench/scripts/env_bench.sh   # env.sh + nix perf/python3 + native libs

# zgx prebuild (build cores), via suite.py's own build()
build python3 scripts/zgx_prebuild.py            # --all scene set, release profile

# zgx suite
bench python3 scripts/perf/suite.py                 # default scenes  -> FAILED (GPU), see below
bench python3 scripts/perf/suite.py --only mount,script,anime,list-state,style,widgets,app-   # -> app-* FAILED (GPU)

# zgx headless, CPU-only, RENDER=0, per run:
bench perf stat -x, -e instructions:u,cycles:u -o X.perf.csv <example> env RENDER=0 ...
  heavy_headless  ZGX_MODE={idle,stream,scroll,both} TICKS={300,600}
  workload_headless ZGX_MODE={idle,stream,scroll,both} TICKS={300,600}
  animation_headless FRAMES={600,1200}
  micro_bench SCENE={state,vscroll-fixed,vscroll-variable}

# zgui
build cargo build --release --locked -p zgui-bench --bin zgui-bench
build cargo build --locked --package xtask
bench cargo xtask perf
```

## zgx: suite.py, default scenes

`/root/work/gui-bench/results/zgx_suite_default.txt`. Failed at the first scene (heavy-both). No rows printed.

Error:
```
thread 'main' panicked at crates/zgx-desktop/examples/heavy_headless.rs:72:6:
GPU: GpuError("No suitable graphics adapter found; noop not requested, vulkan not requested, metal not requested, dx12 not requested, gl drivers/libraries could not be loaded, webgpu not requested")
```

Skipped on this basis: heavy-stream, heavy-scroll, heavy-both, lab-stream, lab-scroll, animate, layout, frame, text.

## zgx: suite.py, non-GPU scenes

`/root/work/gui-bench/results/zgx_suite_nongpu_noMicro.txt`. Instruction counts are the least of 3 runs. Spread is (max-min)/min over the 3 runs.

| scene | instructions | spread | time |
|---|---:|---:|---:|
| mount | 296.8 M | 0.05% | 0.08 s |
| script-update | 3843.8 M | 0.01% | 0.46 s |
| anime | 3733.5 M | 0.00% | 0.40 s |
| script-layout | 2974.1 M | 0.01% | 0.27 s |
| list-state | 830.8 M | 0.02% | 0.11 s |
| style | 1332.1 M | 0.00% | 0.16 s |
| widgets | 146.8 M | 0.00% | 0.06 s |

Not run: app-mail, app-dashboard, app-sketch, app-code (`apps_bench`), failed:
```
apps_bench.rs:35:84: a GPU: GpuError("No suitable graphics adapter found; ... gl drivers/libraries could not be loaded ...")
```

Also not run: `state`, `core` (slow, `--all` only). The `--all` run was not attempted.

micro-* scenes via suite.py failed with the same GPU error (suite.py does not set RENDER=0 for them). Their RENDER=0 numbers are in the micro section below.

Earlier failures, resolved by environment only: `mount_bench` could not load libstdc++.so.6; `zgui-bench` could not load libfontconfig.so.1. Both are fixed by `LD_LIBRARY_PATH` (see Environment).

## zgx: headless, CPU-only, RENDER=0

Files: `headless/*.perf.csv`, `headless/*.stdout`. Per-frame values are the slope between the 300-tick and 600-tick runs, which cancels process startup. Animate is the slope between 600 and 1200 frames.

| scene | instr, small | instr, large | instr/frame | cycles/frame |
|---|---:|---:|---:|---:|
| heavy idle | 27,618,317 | 27,775,176 | 523 | 112 |
| heavy stream | 1,053,516,904 | 2,006,215,993 | 3,175,664 | 1,537,978 |
| heavy scroll | 72,018,431 | 116,146,327 | 147,093 | 79,161 |
| heavy both | 1,104,848,603 | 2,108,810,881 | 3,346,541 | 1,603,384 |
| workload idle | 10,761,733 | 10,976,506 | 716 | 453 |
| workload stream | 148,138,908 | 292,777,121 | 482,127 | 289,468 |
| workload scroll | 26,930,635 | 43,403,403 | 54,909 | 28,845 |
| workload both | 163,373,363 | 323,607,808 | 534,115 | 325,803 |
| animate (update+flush) | 147,766,934 | 289,501,595 | 236,224 | 105,444 |

The 600-tick heavy both run reports 0.466 ms/tick, 842 layout nodes/tick, 209.8 damage rects/tick. Animate reports 26.7 us/frame CPU.

## zgx: micro_bench, RENDER=0

Files: `headless/micro-*.stdout` (JSON, one line per figure) and `headless/micro-*.perf.csv`. Medians are process CPU time per operation over 2000 ops.

| scene | write median | frame median | frame p10-p90 |
|---|---:|---:|---:|
| state | 3,139 ns | 5,518 ns | 5,156-5,897 ns |
| vscroll-fixed | 2,332 ns | 8,654 ns | 8,101-9,791 ns |
| vscroll-variable | 12,000 ns | 13,760 ns | 1,684-25,526 ns |

## zgui: cargo xtask perf

Files: `zgui/xtask_perf.stdout`, `zgui/xtask_perf.stderr`, `zgui/performance.md` (generated), `zgui/perf-runs/1793519595.tsv`.

This is a timed ratchet: measurements are compared with recorded bands. It is not an instruction-count benchmark. I did not verify whether it reports instruction counts. It ran 7 headless scenarios (xtask's help text says five).

Exit status: 1. `PERF failed: 10 measurements outside their bands`.

| measurement | measured | band | result |
|---|---:|---:|---|
| hover.crossing | 274.09 us | 266.0 | REGRESSED |
| scroll.translation | 98.78 us | 61.6 | REGRESSED |
| kitchen.click | 68.87 us | 15.1 | REGRESSED |
| kitchen.keystroke | 765.03 us | 407.4 | REGRESSED |
| kitchen.untouched_turn | 130.98 us | 30.8 | REGRESSED |
| kitchen.glide_tick | 1451.89 us | 854.0 | REGRESSED |
| kitchen.tab_traversal | 774.06 us | 533.4 | REGRESSED |
| shell.tooltip_crossing | 2234.34 us | 1750.0 | REGRESSED |
| shell.drag_step | 517.15 us | 420.0 | REGRESSED |
| shell.drag_hit_rebuilds | 1.00 | 0 | REGRESSED |

Other measurements within band, as printed: idle.turn 0.11 us, cold.first_frame 89.55 ms, kitchen.resize 3.77 ms, thread.resize 14.22 ms, shell.resize_step 1515.87 us, scroll.recycle 400.24 us.

The generated `performance.md` was regenerated by this run in the checkout, as the harness does.

## Failures and skips, consolidated

- zgx default suite: GPU adapter missing, all GPU scenes. Exact error above.
- zgx apps_bench (app-mail, app-dashboard, app-sketch, app-code): GPU adapter missing.
- zgx suite.py micro-*: GPU adapter missing (no RENDER=0 in suite.py). Replaced by the RENDER=0 direct runs above.
- zgx `--all` (state, core): not run.
- zgx lab-*, heavy-*, animate, layout, frame, text: need GPU, not run through suite.py. heavy and animate were run in headless CPU-only mode via RENDER=0 instead.
- zgui: `cargo xtask perf` completed and failed its ratchet on 10 measurements (see above). Not a crash.
- Earlier runtime library errors (libstdc++, libfontconfig): fixed by environment, not code.
