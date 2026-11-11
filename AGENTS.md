# Working in this repository

Two implementations share it. **bough** (the workspace at the repository root: `crates/`, `comparisons/`, `bench/`,
`vendor/`) is the active one, worked as a relay of lanes (`docs/LANES.md`). **`reactivity/`** is the preserved clean-room GPUI rework, with its own workspace and
the workflow in the second half of this file. The design both answer to is `designs/reactivity/` (start with
`final/00-model.md`). The work list is `research/AUDIT.md`.

## bough: lanes, ownership and evidence

Read `README.md`, `CONTRIBUTING.md`, `docs/LANES.md`, `docs/AVENUES.md`, `docs/SETTLED.md` and the AUDIT sections your
lane cites before starting work. Preserve pinned baselines and settled exclusions.

- **Lanes.** Each session owns one lane's `docs/lanes/<lane>/` (public reports and frozen cards) and its ignored
  `runtime/lanes/<lane>/` (scratch, logs, handoffs). The lanes are `platform-host`, `platform-capabilities`,
  `surface-api`, `async-entities`, `text`, `style-motion-media`, `gpu`, `pacing`, `memory`, `components`,
  `developer-loop`, `verification` and `independent-review`. Their backlogs are in `docs/AVENUES.md`.
  - Don't overwrite another lane, the shared docs (README, AUDIT, BRIEF, STATUS), the gates or root manifests.
    Shared changes go through a reviewable patch that the coordinator integrates.
  - No session accepts its own result: `independent-review` replays it.
- **Continuation.** `runtime/lanes/<lane>/STATE.md` is the canonical continuation. Before returning, save the
  decision, the state reached (CONTRIBUTING's states) and the next bounded card.
- **Cards first.** Freeze a card (`docs/CARD.md`) before building or measuring. Record dependencies, outcomes, costs
  and negative results.
  - Public reports must be understandable without the runtime logs.
  - Private logs, transcripts and local absolute paths stay in `runtime/`.
- **Toolchain.** `rust-toolchain.toml` pins nightly-2026-10-17. Put rustup's proxies first on `PATH` and clear
  `RUSTC_WRAPPER`; a system cargo that ignores the pin fails on `#![feature]`.
- **Targets.** Give each concurrent lane its own `CARGO_TARGET_DIR`; never share one between worktrees. Preserve other
  sessions' uncommitted work and worktrees.
- **Measure alone.**
  - Nothing else heavy runs during a measurement: no builds, no tests, no other benchmark. A session that measures
    holds a lock file for the whole batch, and every other session checks it before heavy work.
  - Report every cell, including losses.
  - Preregister thresholds.
  - Keep a negative control (a deliberately broken variant that must fail).
  - Reject a change that regresses any fixed cell past its limit, whatever it wins elsewhere (AUDIT §10).
- **Surfaces.** Application code and new benchmarks use `bough::prelude` only. `bough::raw` (coordinates, templates,
  `Mounter`) is for the existing scene ports. Views re-describe after their handlers; the only mount-once switch is
  the harness option `Config::once()`.
- **Gates are pixel-exact.** A retained frame equals a full redraw at every ring age, and a fresh mount equals the
  retained tree. A new optimisation lands with its oracle and a mutant that fails it.
- **Code hygiene.**
  - Every `unsafe` keeps a `// SAFETY:` argument, and new unsafe invariants get their own test.
  - Vendored crates (`vendor/`) are plain copies, with every edit listed in `vendor/README.md`. Parley grafts are
    twin-gated against unmodified upstream.
  - Format only intended files (`rustfmt --edition 2024 --config skip_children=true`). `cargo fmt --all` reaches
    into vendored path dependencies.
  - No absolute paths in manifests or scripts. Competitor sources are pinned git revisions; scripts take `BOUGH_REF`
    for local reference checkouts.
- **Authority.**
  - Remote hosts need an explicit, live allocation from the user; an idle server is not one.
  - Don't message outside people or other sessions without the user's direction.
  - This repository is public. Everything committed or written in a PR is visible.
- **Commits.** Say what was verified and what was not. Integrations go to `main`; lane work goes on
  `lane/<lane>/<topic>` branches.

---

## reactivity/ workflow (the clean-room GPUI checkpoint; unchanged)

The clean-room library is the standalone `reactivity/` workspace. Original
`crates/gpui*` code and feature branches are references; qualify changes against
the actual clean-room composition before adoption. Designs live in
`/root/designs/reactivity`, mirrored by `rework/`.

- Keep the root model/window exclusively borrowed. Immutable resource leases
  have explicit scene lifetimes. Do not introduce Rc, Cell or RefCell into the
  production window engine. Prefer safe Rust; justify any new unsafe invariant
  and test it independently.
- Create unique worktrees and targets with fail-fast shell commands. Verify a
  destination does not exist before creating it. Prefer `worktree add --no-checkout`
  and sparse checkout of `reactivity` and `.cargo` in the newly created worktree.
  Never run sparse-checkout setup on an existing worktree after creation fails.
- Freeze a clean commit before a build/test gate. Do not edit that worktree until
  its compilers, tests and before/after source checks finish. Build inputs include
  `.cargo`, `clippy.toml`, workspace manifests/lock/toolchain and all first-party
  and vendored source used by the build. Keep failures and corrected attempts.
- Format only intended files with `rustfmt --edition 2024 --config skip_children=true`.
  `cargo fmt --all` traverses path dependencies and has modified vendored source
  accidentally; do not use it for this workspace.
- Give each concurrent lane its own target. Cross-worktree target reuse has
  returned an identical stale Rust executable for changed source whose mtime
  preceded the baseline build. Clean changed first-party packages before such
  reuse, or use a fresh target. Profile selection matters: `cargo clean -p gpui`
  does not clean release artifacts; use `cargo clean --release -p gpui` for a
  release comparison. Verify the changed library was rebuilt, not merely the
  final executable. Preserve rejected stale captures. Pinned Cargo's
  `-Z checksum-freshness --config
  'build.fingerprint="content"'` passed an actual old-mtime 17→19→17 control.
  [Cargo documents](https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#checksum-freshness)
  that build-script `rerun-if-changed` still uses mtimes. Clean the GPU package
  after overlays, compare generated WGSL against the typed shader, and verify
  generated WGSL/SPIR-V bytes inside the executable that ran. Inspect Cargo JSON
  freshness and actual executable identities; source guards alone are insufficient.
- Treat execution and measurement separately. Preserve full source/toolchain,
  features, input guards, executable hashes and exact workload gates. Report
  failed gates and fallbacks. Software Vulkan and private X11 establish neither
  hardware GPU performance nor display scanout latency.
- Inventory shipping code with `reactivity/tools/core_lines.py`: seven first-party
  libraries and library build scripts; exclude tests, comments, blanks, examples,
  tools, evidence and vendors. Report dependency/vendor, generated shader, whole
  machine-code, stack and retained-capacity costs separately. Do not forecast
  final line counts or count helper code twice.
- Mothership is available as `ssh mothership` (user miles). Build/copy work takes
  a shared lock and runtime/profile measurements an exclusive lock at
  `/home/miles/gpui-ce3-compute-20261024/host.lock`. Keep at least 8 GiB free and
  release/reap every owned process. Its AST adapter has no render node; current
  Vulkan measurements use a software driver. Preserve source/evidence archives
  before removing regenerable caches.
- Keep public tests on ordinary library interfaces. Test exact semantic authority,
  failed newer candidates and older eligible acknowledgements, resource lifetime,
  numerical boundaries and actual native accessibility. A bounded mutant should
  falsify any new optimization certificate. Never substitute tolerance or reduced
  work for an established exact/workload gate.
- Standalone application ports have independent lockfiles. Resolve necessary
  dependency edges without unrelated upgrades, then run their locked offline
  checks against the composed core before claiming those ports still build.
