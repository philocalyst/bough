# Completion run in progress (lead review resumed)

The lead authorized continuous completion and calling-thread instruction counts under load. The root lock repair
is applied in e6d4108a6f; the comparison remains a standalone workspace. [TABLE-CELLS.md](TABLE-CELLS.md) contains
obtained counts and spreads, including failing coarse cells and the new nonopaque comparison. The historical
first-stage report below remains intact; its NM and stop statements describe that earlier stage only.

At N=1,000, coarse plain select R0 is 6,567,603 instructions/op versus fine scroll 12,330. Plain coarse update10 is
4,662,665 and swap 3,311,480. These are three-run repeat slopes with oracle work excluded, not estimates.
Post-replace selection in a plain coarse table fails its semantic gate: On::update does not reinstall the closure,
so its original captured row ID survives re-description. Owner: surface-api. General remedy: update retained route
payloads with description operands, preserving receiver identity. Full size sweep and default-cap follow-ups are running.

---

# Round 6, first rows/table report

State: **CANDIDATE**; coarse scroll content reaches a reproduced **METHOD_BOUNDARY**. Stop for lead review
after the first runnable stage. No framework code was changed. No performance or scaling claim is made yet.

Baseline aa9943e7ae50595f6af026773ee82b46a4b85ae0; frozen card 0691ce0e7b.
Reference gui-bench 13f3cb61687c25f80ea46625680d8e0804f6fbed. Local M3 Pro, macOS, direct Metal,
nightly-2026-10-17, release/thin LTO, default features. Application capacities were preserved exactly.
The source data/layout and CLI files were copied verbatim; SHA-256:

* spec.rs: `724d9e81ad7f79886901a9cb283842e5e584810be1c43a42f4ebcc78d65853d7`
* cli.rs: `856728f01f42c5a244c7d1d673344bc857ade7d7d7e0133a2e9d48a378fddcfa`

## Finding that matters first

**Plain-field updates inside a prelude scroll container do not appear.** At only ten rows, all nine coarse-table
operation cells updated their application state correctly and passed presented == full redraw, but failed
retained == fresh mount. A renderer comparing only its two paths would have certified these stale screens.

Mechanism: `crates/bough/src/template.rs`, `ScrollBox::mount` wraps the entire content in `mount_opaque`, and
`ScrollBox::update` calls only `describe::update_opaque`. It never updates the child element description.
The coarse `Vec<Row>` and selected ID change, but the mounted rows retain their initial text, colour and structure.
The header outside the scroll box does re-describe. Fine rows pass because their List delta and signal wires
patch the already-mounted content without traversing that skipped description.

This contradicts the broad statement that normal re-description makes plain-field state work everywhere. It
also explains an apparent performance advantage that must not be called efficient re-description: the required
work is omitted. The general remedy belongs to **surface-api/core**: retain enough scroll-container tape and
layout ownership to reconcile ordinary child descriptions while preserving its scroll/clip state. Do not fix
it here by putting every coarse row into signals, remounting by hand, or using once().

## Executed cells

`NM` means **not measured**, not zero. These are correctness runs on a machine with concurrent lane builds.
Instrumentation printed instruction/memory/time counters, but they include repeated oracle mounts and were
not taken under an exclusive measurement batch; none is an operation cost or memory slope. Each gate uses scale
1, a 1280×800 window, ring size 4, four operation/undo cycles. Successful cells checked both comparisons after
each of eight transitions. Failures stop at the first differing transition and retain its raw BGRA images.

### rows

| N | operation | R0/R1 instructions | exponent | ideal | semantic / pixels | verdict |
|---:|---|---|---|---|---|---|
| 10 | hover row 5 / row 0 | NM / NM | NM | O(1) | layout points pass; presented/full and retained/fresh pass, 8 transitions | correctness pass at this cell |
| 10 | scroll | NM / NM | NM | O(visible damage) | not run; content is shorter than viewport, reference omits glide | no scroll result |
| 100, 1,000, 10,000 | hover, scroll | NM / NM | NM | O(1) versus mounted N | not run | pending |

Hover's gate proves repair/fresh consistency and exact initial layout points; a separate expected-colour assertion
after crossing is still needed before calling it full interaction conformance.

### table

All rows below are N=10. Every initial gui-bench layout check passed. Fine uses List slots/per-field signals;
coarse uses one ordinary Vec<Row>. Fine selection is delegated at the root because native list-row routes are
refused; the exact escape is in [FRICTION.md](FRICTION.md).

| operation / undo | ideal versus N | fine R0/R1; exponent | fine pixels + semantics | coarse R0/R1; exponent | coarse pixels + semantics | verdict |
|---|---|---|---|---|---|---|
| select / deselect | O(1) | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 79,242 bytes | coarse incorrect |
| F1 update10 / F1 | O(N/10) | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 198 bytes | coarse incorrect |
| F2 swap / F2 | O(1) structure + damage | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 7,524 bytes | coarse incorrect |
| F3 remove / F4 insert | O(1) structure + visible displacement | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 34,020 bytes | coarse incorrect |
| F4 insert / F3 remove | O(1) structure + visible displacement | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 37,973 bytes | coarse incorrect |
| F5 replace / F5 | O(N) | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 36,355 bytes | coarse incorrect |
| F6 append / F7 truncate | O(1,000) changed rows | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 88,362 bytes | coarse incorrect |
| F7 truncate / F6 append | O(1,000) changed rows | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 88,730 bytes | coarse incorrect |
| F8 clear / F8 refill | O(N) | NM/NM; NM | pass ×8 | NM/NM; NM | state and presented/full pass; fresh differs by 66,147 bytes | coarse incorrect |
| N=100, 1,000, 10,000; all above, both variants | as above | NM/NM; NM | not run | NM/NM; NM | not run | pending |

The truncate cell first appends 1,000 through F6, then alternates F7/F6. Its coarse failure occurs when the
reappended rows are required to appear. Every other coarse cell fails on the first operation. No panics from
capacity exhaustion occurred in these runs; the nine reported exits 101 are the deliberate fresh-pixel assertions.
Rows/fine-table successful runs used all four surfaces: `[3,2,2,2]`, or `[3,3,2,2]` for truncate's extra setup
frame. This does **not** discharge the card's separate ring sizes 1, 2 and 3.

### remaining scenarios and resources

| scenario / resource | cells | state |
|---|---|---|
| ticker | N=10,100,1,000,10,000 × K=1,100 × fine/coarse | not implemented; not measured |
| spinner | N=0,10,100,1,000,10,000 | not implemented; not measured |
| typing, search | each at N=10,100,1,000,10,000 | not implemented; not measured |
| virtual | N=10,100,1,000,10,000,100,000,1,000,000 | not implemented; not measured |
| resize | N=10,100,1,000,10,000; size storm and scale 1→2→1 | not implemented; not measured |
| resident/footprint and mount cost per row | all scenario sizes | counters exist; no accepted slopes or mount timings |
| adversarial matrix | all cases in the frozen card | predictions registered; none run |
| linear-select negative control | all four row counts | mutation and analyzer implemented; not executed; locality verdicts remain UNKNOWN |

## Ranked mechanisms for the owning lanes

| priority | mechanism / impact | evidence and owner | general remedy |
|---:|---|---|---|
| 1 | scroll descendants ignore ordinary re-description; affects routine tables, forms and conditional content | reproduced above; `template.rs::ScrollBox::update`; surface-api/core | reconcile children while retaining container state |
| 2 | routes inside List/entity child owners are refused; affects interactive reusable rows | source assertion `input.rs::root_owner_only`, not exercised as a panic in this port; surface-api | routes carry/resolve their actual owner, not the root pointer |
| 3 | fixed per-row space capacity restricts nonvirtual fine tables | source: `listview.rs` creates per-row spaces; `rt.rs::alloc_space` asserts at APP_CAPS spaces=4,096; memory/core | growable space storage and representable IDs; run default failure before any follow-up capacity experiment |
| 4 | structural swap/insert/move cost can depend on distance or total suffix | source: `list.rs::swap` → `permute_with` copies a permutation span; `insert` shifts Vec order; listview replay rebuilds height suffix via `assign_from`; core | delta operations plus an order/height index supporting bounded local edits |
| 5 | F-keys and no-focus window shortcuts missing from the public key surface | source: `input.rs::Key`, `App::key_down`, `backend/host.rs::key_of`; surface-api/platform-host | named keys and window-scope routes independent of focused widgets |
| 6 | re-described nonopaque lists sum every mounted row's record count | source: `listview.rs::LList::update`; surface-api/core | cached subtree span/aggregate updated with structural deltas |

Priority 6 is **not reached through this port's opaque ScrollBox update**. It is a prediction for a corrected
normal path or a non-scroll owner, not a measured explanation for this port's select cost. No extrapolated timing
or capacity failure is reported as executed evidence.

## Validation, cost and limits

Built `bough-bench` and `headless`. All three 10-row applications (rows, table fine, table coarse) opened actual
AppKit windows for bounded two-second smoke runs. Each presented one initial frame and closed cleanly; the
exit redraw check found zero differing bytes. No native F-key/click injection or fakeland run was performed.
Headless operations used the same App pointer/key entry points called by the host, with ordinary frame/update
processing, never direct timed calls to model operation methods.

* Standalone `cargo check --locked --offline --manifest-path comparisons/bough-guibench/Cargo.toml --all-targets`
  passed. First attempt had a comparison-source delimiter error; corrected attempt and both logs are retained.
* Standalone `cargo build --release --locked --offline ... --bins` passed.
* `cargo test --release --locked --offline --manifest-path comparisons/bough-guibench/Cargo.toml -p bough-guibench`
  passed all **11 inherited spec/CLI tests**. Pixel/host gates are separate executions above, not counted as unit tests.
* `cargo check --workspace --all-targets --offline` passed after Cargo filled three missing pinned-git source
  entries. The checked-in root lockfile was restored unchanged; [workspace-lock.patch](workspace-lock.patch)
  contains the exact three-line integration proposal (`git apply --unidiff-zero`). No package version changed. The initial `--locked` check
  failed on that baseline lockfile. A private `--lockfile-path` attempt failed because this Cargo lacks the option.
* Existing warnings remain: missing Metal compiler (runtime shader compilation), two bough-gpu dead-field warnings,
  and unused optional patches in the standalone comparison. No warning-free claim.
* Analyzer arithmetic checks distinguish synthetic constant/linear data and reject nonpositive fits. This is
  **not** the required measured negative control. No scaling qualification yet.
* Inspected arm64 assembly of `Fine::select_pressed` and `Fine::apply` in the release driver: mutant selection
  retains an explicit per-row loop and stack black-box store; ordinary selection branches around it. Slot/chunk
  bounds checks and generation checks remain. Update10 is a scalar, strided loop with allocation/string calls,
  not SIMD; function bodies use direct calls (no `blr` in the inspected range), and save callee-saved registers.
  These are benchmark/framework costs to measure, not changes made to optimize the framework.

Measured executable SHA-256 identities (correctness execution only):

* headless: `906ee88d0331a0641eae4c5b8031cbb6e8a29eec411c402ee140ea991c7df5b1`
* bough-bench: `500401206df13321727cc883ab06f8460003d7c0163ebf77993f8728d28c0bec`

The workload and harness add their own binary/build/storage costs; no shipping-library line count or memory
reduction is claimed. Framework fixes: **none**. Shared root manifest/doc edits: **none retained**. Native
function-key delivery, full operation sequences with post-churn clicks, expected hover pixels, all ring ages,
all larger sizes, isolated R0/R1 slopes, mount/resource slopes and independent replay remain unverified.

## Next bounded card for lead review

G6.2: keep the failing coarse workload intact; independently replay its fresh gate, sweep rows/table sizes and
all ring sizes at unchanged capacities, add expected hover/scroll assertions and a click after replacement to
expose stale handler captures (`input.rs::On::update` retains installed routes). Sample model construction
separately from mount. Then take an exclusive, idle-machine R0/R1 and resource batch including the actual
O(N)-select mutant. Do not begin the other six scenarios or framework repairs before this stage is reviewed.
