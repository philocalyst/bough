# Working in this repository

Two implementations share it. **bough** (the workspace at the repository root: `crates/`, `comparisons/`, `bench/`,
`vendor/`) is the active one. **`reactivity/`** is the preserved clean-room GPUI rework, with its own workspace and
the workflow in the second half of this file. The design both answer to is `designs/reactivity/` (start with
`final/00-model.md`). The work list is `research/AUDIT.md`.

## bough workflow

- **Toolchain.** `rust-toolchain.toml` pins nightly-2026-10-17. Put rustup's proxies first on `PATH` and clear
  `RUSTC_WRAPPER`. A system cargo that ignores the pin fails on `#![feature]`.
- **Targets.** Give each concurrent lane its own `CARGO_TARGET_DIR`; never share one between worktrees.
- **Application code and new benchmarks use `bough::prelude` only.** `bough::raw` (coordinates, templates,
  `Mounter`) is for the existing scene ports. Port them when the prelude can express them (AUDIT §11).
- **No `FIXED`-style switches on `View`.** Views re-describe after their handlers (design 01 §2). Mount-once exists
  only as the harness option `Config::once()` for benchmarks that measure the wire path alone.
- **Measure alone.** Nothing else heavy runs during a measurement: no builds, no tests, no other benchmark. Use a
  lock file when an agent runs measurements, and have every other process check it before starting heavy work.
  - Report every cell, including losses.
  - Preregister thresholds before measuring.
  - Keep a negative control (a deliberately broken variant that must fail the gate).
  - Reject a change that regresses any fixed cell past its limit, whatever it wins elsewhere (AUDIT §10).
- **Record attempts.** Each area's `RESULTS.md` says what was tried, kept and rejected, and why. Rejected attempts
  stay recorded. AUDIT §15 re-reads them for merit.
- **Gates are pixel-exact.** Retained frames must equal a full redraw at every ring age, and a fresh mount must equal
  the retained tree. A new optimisation lands with its oracle and a mutant that fails it.
- **Unsafe.** Every `unsafe` keeps a `// SAFETY:` argument. New unsafe invariants get a test of their own.
- **Vendored crates** (`vendor/`) are plain source copies. Every local edit is listed in `vendor/README.md`. Grafts
  into parley are twin-gated against unmodified upstream (`crates/bough-text/tests/parley_twin.rs`).
- **Formatting.** Format only intended files with `rustfmt --edition 2024 --config skip_children=true`.
  `cargo fmt --all` reaches into vendored path dependencies.
- **Paths.** No absolute paths in manifests or scripts. External comparison sources are pinned git revisions
  (zeronsh/zgx, ppmpreetham/BUI), and scripts take `BOUGH_REF` for local reference checkouts.
- **Commits.** Say what was verified and what was not. Push to this repository (`main`, `bough/*`).

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
