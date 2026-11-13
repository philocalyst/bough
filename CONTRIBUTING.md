# Contributing to lorlei

lorlei is the project's new name; the code, crates, paths and older documents still say bough (see `AGENTS.md`, which also
holds the contribution policy in short form and the full roadmap).

Welcome contributions: a construction (a mechanism, a representation, a port), a gate, a measurement, an independent
replay, a reusable tool, or a documented boundary (something that provably doesn't work, and why). Start from an
issue or a lane (`docs/LANES.md`) that names the platform, the target and the baseline. Don't claim a win from a
single run, a microbenchmark that skips the frame path, or a machine with other load.

## What a PR carries

* **The claim and its scope.** What changed, on which platforms and backends, and what it does *not* establish. For
  example: "headless pixel gates on Vulkan (lavapipe); no hardware GPU time, no scanout latency".
* **Pinned baselines.** The predecessor commit, toolchain, features and build profile, and for comparisons the exact
  competitor commit (zgx, zgui, BUI, the AWAGY apps).
* **The card it executes** (`docs/CARD.md`), frozen before building or measuring: change, baseline, workloads,
  thresholds, controls and stop conditions.
* **Gates.**
  * Pixel-exact: a retained frame equals a full redraw at every ring age, and a fresh mount equals the retained tree.
  * Semantic and scene snapshots where they exist.
  * A deliberate mutant (negative control) that must fail them.
* **Measurements.**
  * Taken alone on an idle machine.
  * Every preregistered cell reported, losing ones included.
  * Instruction counts for CPU work, ABAB pairs for GPU time.
  * The machine, display and refresh rate.
  * Raw output committed under `bench/results/`.
* **Reproduction commands**, and an **independent replay receipt** (`docs/lanes/independent-review`) before
  integration of anything that claims a win.
* **What was not checked**, and any history that could not be replayed.

For application code and new benchmarks, use `bough::prelude` only. Don't add a per-view static switch to `View`.
Record vendored-crate edits in `vendor/README.md`. Put no absolute paths in manifests or scripts.

## Research states

Keep them distinct in issues, PRs and lane reports:

| state | meaning |
|---|---|
| `LEAD` | an idea with a reason, no card yet |
| `CARD` | a frozen card: baseline, workloads, thresholds, controls, stop conditions |
| `CANDIDATE` | an implementation exists; gates not yet run |
| `GATED` | correctness gates pass, including the negative control |
| `MEASURED` | every preregistered cell measured alone and reported |
| `INDEPENDENT_REPLAY` | someone else reproduced the gates and cells from a clean checkout |
| `INTEGRATED` | merged to `main`, with BRIEF/STATUS/AUDIT updated |

Durable outcomes that are just as useful:

| state | meaning |
|---|---|
| `REJECTED` | measured, missed a threshold |
| `METHOD_BOUNDARY` | the approach cannot reach the target, with the reason |
| `UNKNOWN` | inconclusive |

A merged PR doesn't certify a general speedup, platform parity or a ranking against another framework. Claims like
those need the design's "surpasses" bar (`designs/reactivity/final/10-quality.md` §5): no feature reduction, lower
p50 **and** p99 input-to-present, no unaccepted regressions, and reproduction on two hardware classes.

## Credit and licenses

Name the inherited construction (bough's own, the archive's clean-room GPUI, zgx, zgui, parley and so on), the new
component, the integration, and who independently verified it. Renamed or reformatted variants of an existing result
aren't separate advances. Keep the notices of vendored and reused material. Never commit credentials, private
correspondence, session transcripts, or local operational logs.
