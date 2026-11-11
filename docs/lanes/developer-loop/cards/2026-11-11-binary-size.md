# Card: how small can a shipping bough executable get with no hot-path loss?

```
lane:            developer-loop
state:           CARD
owner:           the round-6 developer-loop session (one owner for every lever below)
predecessor:     aa9943e7ae (lead/round6-base); nightly-2026-10-17 (rust-toolchain.toml); default features (direct
                 Metal presenter); [profile.production] = release + opt-level 3, fat LTO, 1 codegen unit, strip, no
                 incremental (the AWAGY size table's profile)
question:        Which bytes make up a production bough executable, and how far can the counter example and the two
                 AWAGY apps shrink while every hot cell stays within 1.02x of production and every pixel stays the
                 same?
change:          Build configuration first (a new profile under a new name, std rebuilt for size, panic and
                 formatting machinery, per-crate opt-level), then dependency surgery (make heavy capabilities link
                 only when an app uses them), then code-level levers (monomorphisation, core::fmt on always-linked
                 paths, embedded data). Each lever is pulled alone and kept only if it passes the gates. No change
                 to any gate, any semantic, or any shared manifest other than adding a profile with a new name.
implication:     Passing establishes the stripped executable size of counter / awagy-scroll / awagy-dashboard under
                 a profile whose hot-cell instruction slopes are within 1.02x of production and whose presented
                 frames are byte-identical to production's. It does not establish startup time, wall time, GPU
                 time, resident memory or the wgpu presenter (none measured unless a row says so).
platforms:       macOS 15 arm64 (M3 Pro), direct-Metal presenter (bough-metal), AppKit host.
workloads:       size: crates/bough/examples/counter.rs; comparisons/bough-awagy (scroll, the default feature) and
                 (--no-default-features --features dashboard).
                 instructions: the slope method (bench/integrate/slope.py, bench/layout/slope.py) on heavy stream,
                 heavy scroll, lab both, animate, vlist scroll; RENDER=0 and RENDER=1; ZGX_FONT_FILE = zgx's
                 DejaVuSans.ttf.
gates:           G1 pixels, on binaries built with the candidate configuration (not only on release):
                    heavy_shot (ZGX_MODE stream, scroll, both), scene_shot (SCENE lab, animate), vlist_shot
                    (ZGX_MODE scroll, fling, jump) each report 0 differing bytes against a full redraw, AND each
                    dumped frame is byte-identical (cmp) to the production baseline binary's dump for the same
                    inputs; a headless counter driver built the same way (three real clicks through hit testing,
                    count == 3, presented == full redraw, frame byte-identical to baseline).
                 G2 `cargo test --release -p <crate>` for every crate a lever touches.
                 G3 instructions: every listed cell, candidate / production median slope <= 1.02, the two builds
                    interleaved per cell.
                 Negative controls: (a) pixels: `--features bough/sabotage_copy` built with the candidate
                    configuration must make the scroll gates report nonzero differing bytes; (b) instructions: a
                    deliberately de-optimised build (bough-core and bough at opt-level "s") must exceed 1.02 on at
                    least one cell, or the instruction gate is recorded as too weak to judge per-crate opt-levels;
                    (c) an A-vs-A run of the production binaries gives the noise floor of G3.
thresholds:      G3 <= 1.02 per cell (a size win that loses a hot cell is rejected, not traded). Size targets:
                 counter < 1 MiB (1,048,576 B), then < 500 KiB (512,000 B), with G1-G3 passing.
measurement:     size = `stat -f %z` of the stripped executable. Attribution from an unstripped twin built with the
                 same flags: `size -m`, `nm` (sizes from symbol address deltas), a linker map
                 (`-Wl,-map,<file>`), `otool -L`, `strings`, per-crate totals from the map's object files.
                 Instructions = `/usr/bin/time -l` "instructions retired" (proc_pid_rusage), slope TICKS 300/600,
                 animate FRAMES 600/1200, vlist TICKS 600/3600, median of 3 (vlist 5). Build times (secondary,
                 not optimised): wall clock of a clean and an app-only incremental build of the counter with 4
                 jobs on a shared machine (indicative only), generic instantiation counts from the unstripped
                 symbol table.
budget:          this session, this Mac only, no remote hosts, one target dir in the worktree, cleaned as it goes.
stop conditions: a lever that fails a gate is REJECTED and recorded with its numbers. Stop when the counter is
                 under 500 KiB with G1-G3 passing, or when the next lever needs an API or architecture change
                 outside this lane (METHOD_BOUNDARY, with the next card).
do not revisit:  docs/SETTLED.md (nothing there concerns build configuration); no gate is loosened; shared manifests
                 and .cargo/config.toml stay as they are (the recommended profile is described as a patch).
```

The lever order is decided by the attribution (the first report), recorded below as an amendment before any lever
is pulled.
