# Round 3: common brief for every agent

Read this first; your own prompt adds your area.

## Where things stand

`canonical` @ 42bc651 is the fused system: bough-core (Bough tree), bough-text, bough-metal (`fused`), bough-mem
(one GPU-visible region), cephal layout, and the façade `crates/bough`. Scenes: comparisons/bough-heavy, -scenes
(lab, animate, blink, micro), -vlist. It beats zgx on every cell measured so far — 6–28× fewer instructions per frame,
1.5–4.5× less CPU, 26–65 % less memory (STATUS.md §1). It covers only zgx's desktop scenes. "Obliterate zgx and zgui on
**every single benchmark**" is the goal, so round 3 is about coverage and the remaining floors, without
giving up any existing cell.

Read before writing code: `STATUS.md` (numbers, gaps, soundness audit), `DESIGN.md`, `API.md`,
`research/04-reformulation-method.md` (method, below), `research/00-bbtree-synthesis.md`, the RESULTS.md of the crates
you touch, `crates/bough-metal/INTERFACE.md` if you touch the GPU, `crates/bough/LAYOUT.md` if you touch layout.

## Method (the user's standing instruction)

Every attempt is a **structural reformulation**, not tuning. Work from Ben Joffe's fast day-of-week
(https://www.benjoffe.com/fast-day-of-week, summarised in research/04): change the domain so the cost vanishes, fold
offsets into one constant, widen so corrections disappear, find the value that was there all along, pad to powers of two
with phantom slots, bend the input range, prove exhaustively, distinguish latency from throughput, read the asm.
There is always a rethink that opens a new horizon if you are willing to bend the rules. One mechanism per job, no
fallback branch that taxes the common case. SIMD and parallelism are core design elements, not bit twiddling or
"parallelise over blocks": find where the representation makes the work naturally wide (lanes = records, = tiles,
= glyphs, = candidates), and where GPU or other cores can own a phase outright. Zig/Odin-style memory: allocate at
mount or never, reuse, rings, fixed carved columns. No dynamic dispatch, no dirty flags at runtime — compile them out
(monomorphization, const generics, const fn, `offset_of!`, typed routes). Use `unsafe` where it pays, with a
`// SAFETY:` argument; public API stays sound (the `sig!` audit in STATUS §4 is the bar). Iterate: build, measure,
rethink, rebuild. Deep refactors are welcome.

## Environment

```
cd <your worktree>
export RUSTC_WRAPPER= PATH=~/.local/share/cargo/bin:$PATH        # rustup nightly-2026-10-17 (rust-toolchain); the nix cargo on PATH is wrong
export CARGO_TARGET_DIR=/Users/mileswirht/projects/bough/target/<your branch>
export ZGX_FONT_FILE=/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf
```

zgx's headless drivers are prebuilt in `/Users/mileswirht/projects/bough/target/zgx-headless` (`bench/zgx/build.sh`
rebuilds them). Slope scripts: `bench/integrate/slope.py`, `bench/layout/slope.py`, micro `bench/integrate/micro.py`,
windowed `bench/integrate/windowed.py`. Instruction counts come from `proc_pid_rusage` (`ri_instructions`); zgx's
`scripts/perf/suite.py` uses `/usr/bin/time -l` on macOS. References: `~/projects/_ref/{zgx,zgui,bbtree,ripple,archive}`,
cephal at `~/Downloads/ilo-prod-work/cephal`. Linux hosts for CPU-only runs: ilo-prod (root@87.99.136.215, use
`/root/work/env.sh`), mothership (miles@, nushell login: wrap in `bash -lc`, disk nearly full). Baselines already
measured: `bench/baselines/{TARGETS.md,macos/SUMMARY.md,linux/SUMMARY.md}`.

## Rules

- Work only in your worktree and branch. Commit often, with messages ending in
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never touch `canonical` or other agents' branches; the
  lead merges.
- Never modify `~/projects/_ref/*` sources. Build zgx or zgui variants in a copy (scratch dir) or with an out-of-tree
  `CARGO_TARGET_DIR`; adapters/drivers for zgx go in this repo (`bench/zgx/`).
- The user is working on this Mac. Windowed runs: ≤ 12 s each, Accessory activation, never steal focus, and only
  while holding the lock directory `/private/tmp/claude-501/bough-windowed.lock` (`until mkdir <lock>; do sleep 2;
  done; …; rmdir <lock>`), so agents' windows never overlap. Do not run zgx's full windowed suite
  (`bench/windowed/`): it needs permissions the user has not granted yet.
- Disk is ~60 GB free and four agents build at once: keep one target dir, avoid building profiles you don't use,
  `cargo clean -p` what you no longer need, and stop to report if free space drops below 15 GB.
- Load average is high from other activity; instruction counts are the reliable column, CPU % is indicative.
- Every existing gate must stay green (`cargo test --workspace --release`, zero warnings in our crates), and no cell in
  STATUS.md §1 may regress by more than noise. If your change touches a shared path (rt.rs, app.rs, elem.rs, the
  presenter), keep the diff focused so the lead can merge four branches; prefer new modules.
- Correctness gates are mandatory for each new feature: pixel-exact vs full redraw at ring ages 1–4 where pixels are
  involved, retained == fresh mount, differential tests against a simple oracle for data structures.

## Deliverables

1. Code on your branch, every gate green.
2. `<area>/RESULTS.md`: what you tried (including rejected reformulations and why), the mechanism you kept, and tables
   against zgx/zgui in their own units with the exact commands.
3. Final report: the tables, what each structural idea bought, honest gaps.
