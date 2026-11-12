# The five bough-suite failures on the round-6 base

Card: `cards/2026-11-11-base-failures.md`. Base: `aa9943e7ae` (lead/round6-base), the first test run after the `api`
(ec1d34aae0) and `floor` (81e393a919) merges: 220 passed, 5 failed, 2 ignored, all five failures in
`comparisons/bough-suite`. State reached: **GATED**. All five are fixed at their cause; no gate was changed, loosened or
skipped, and nothing went to `failures.md` (no failure needed a mechanism redesign).

Each failure was first reproduced alone on the base (`cargo test --release -p bough-suite --test <t> -- --exact <name>`),
with the same message as the lead's run.

## 1. `anime_wires_and_lanes_draw_the_same_pixels`: lanes were off the geometry grid

*Symptom.* "ring 0: wires != lanes, first diff Some((140, 0, 255, 0))": at pixel (140, 0), byte 0 (blue) is 255 in
the wires variant and 0 in the lanes variant. Each variant on its own passes (retained == full redraw == fresh
mount), so the two describe different geometry, not different damage.

*Cause.* `canonical` (0ee0aff3c7, "exact geometric damage") put every quad and bar edge on a 1/256-px grid
(`rt::snap`; `rt.rs`: "quad and bar edges are multiples of 1/256 px"). `api` introduced `lanes` without the grid. The
merge routed the wires' `.span` operand through canonical's `Rt::quad_geom`, which snaps, while
`lanes::write_lanes` (`crates/bough/src/lanes.rs:58`) kept writing raw `ox + l.x * s`, `oy + l.y * s`.

At the last step (t = 3.4 s), dot 687 (x = 687 % 40 * 20 = 140) has phase 0.001667, so y = 300 * p = 0.5002 px. The
wire snaps it to 0.5, the lane keeps 0.5002. The solid kind's pixel-centre test covers row 0 (centre 0.5) for
y0 = 0.5 and not for y0 = 0.5002: blue 255 for wires, background 0 for lanes. That is the reported diff.

*Fix.* `write_lanes` writes x, y, w, h on the grid (`snap`), and the mount snaps the radius as `quad_instance` does.
The dots are then the same boxes in both representations. Reverting the fix brings the failure back (the base run).

*Cost (instructions only; a load-robust count, not a timing).* `anime_bench`, 1,000 lanes, 600 frames, three
interleaved A/B runs: lanes 91.5 k → 96.5–96.7 k instructions/frame (+5.1 k, about 5 per lane, +5.6 %). Wires,
the control, read 982–984 k both before and after. In the assembly of `patch_lanes::<f32, AnimeLanes closure>` the
loop body gains an `ldp` that reloads the 256 and 1/256 constants, plus `fmul.2s` / `frinta.2s` / `fmul.2s` on (x, y). The
compiler hoists the constant (w, h) snap out of the loop. The loop has no bounds check (hoisted), no dynamic dispatch
(the closure is inlined), and is not vectorised, before or after: it writes 48-byte instances and calls `cosf`. Its
other spills (constants reloaded across the `cosf` call) were already there.

## 2 and 3. `layout_screen_*`: a re-describing root mounted once

*Symptoms.* `layout_screen_remounts_and_resizes_are_pixel_exact`: `assertion failed: a.frame()` (the first remount
drew nothing). `layout_host_mount_equals_fixed_mount`: "host != fixed mount at Some((0, 0, 0, 28))": the host shows
nothing (0) where the fixed mount shows its background `c(20, 22, 28)` (blue byte 28).

*Cause.* `LayoutHost` changes plain fields (`salt`, `show`) in its handlers, so only a re-description can mount or
unmount the screen. On `api` it was one of only two suite views without `FIXED` (the other is `Teardown`). The
merge removed `FIXED` everywhere and added `Config::once()` to every suite config, including `lb_config`
(`comparisons/bough-suite/src/lib.rs:903`). With `mount_once` set, `App::redescribe` does nothing, so the turns
are no-ops.

*Fix.* `lb_config` no longer sets `once()`. `LayoutScreen`, the fixed mount it also configures, is unaffected
because it is never changed after mount. The same defect broke `layout_screen_bench`, which asserts `a.frame()` after
each remount (found by reading; it was not run on the base). A short smoke run now completes.

## 4. `teardown_and_mount_are_pixel_exact`: the same cause for `Teardown`

*Symptom.* "teardown (rr): nothing was compared": no step drew a frame.

*Cause.* `Teardown` removes its rows by a re-description, which drops the `Option` when `show` is false. The lists
gates' `cfg()` gained `once()` in the merge (`tests/lists.rs:16`), so writing `show` changed nothing and no frame
had damage.

*Fix.* The teardown gate uses a config without `once()` (`cfg_redescribing`). The other lists gates keep `cfg()`:
their views were `FIXED` on `api`. `teardown_bench` had the same defect (`app_sized` sets `once()`, and the bench
asserts `bough.len() == 0` after the removal; found by reading, not run on the base). It now builds `config_sized`
with `mount_once: false`, and a short smoke run completes.

Across `api`'s 99 `impl View` blocks, only `Teardown`, `LayoutHost` and `crates/bough/tests/describe.rs`'s `Doc`
lacked `FIXED`. `Doc`'s test does not set `once()`, so there are no further victims of the blanket `once()`.

## 5. `nested_scrolls_…`: `App::copy_plan` sliced an empty tile-row range

*Symptom.* Panic at `crates/bough/src/app.rs:580`: "slice index starts at 12 but ends at 11".

*Cause.* Scroll by copy (floor, round 3b) clears the repair marks of the tiles wholly inside the scrolled clip C:
columns `ix0..ix1`, rows `iy0..iy1`, where `iy0 = ceil(y0 / 32)` and `iy1 = floor(y1 / 32)`. The column range
was guarded (`if ix0 < ix1`) but the row range was not. Before the merge, only full-size list viewports scrolled by
copy, so C always spanned a tile row. The merge ported scroll by copy into `listview`'s scroll path, and a nested
list's viewport is cut by its parent's clip (`Rt::lr_place_region`: `vpr = box ∩ parent clip`). Instrumented, the
failing step's C is `[0, 364, 220, 372]`: an inner group's 52-pt viewport cut by the outer viewport's bottom edge
(y = (16 + 170) * 2 = 372) to an 8-px strip inside tile row 11, so `iy0 = 12 > iy1 = 11`.

*Fix.* The row range is clamped first and guarded like the columns: `if ix0 < ix1 && ry0 < ry1`. When C holds no
whole tile, no repair mark is cleared. That is the plan's own rule: the tiles meeting C keep `repair(age)` plus this
epoch's marks, and the copy is unchanged. The gate passes at every ring age and on the windowed ring.

*Negative control.* With `--features bough/sabotage_copy` (the copy one pixel off), the `virtual` gates fail:
`nested … step 4`, `grid … step 2` and `plain … step 12`. So the nested gate does exercise the copy.

## Runs on the fixed tree (release, this Mac, direct-Metal presenter)

| command | result |
|---|---|
| `cargo test --release -p bough-suite` | 32 passed, 0 failed |
| `cargo test --release -p bough` | 30 passed, 0 failed |
| `cargo test --release -p bough-vlist` (scroll-by-copy users) | 8 passed, 0 failed |
| `cargo test --release -p bough-app-mail-dash -p bough-app-sketch-code` (lanes users) | 24 passed, 0 failed (as on the base) |
| `cargo test --release -p bough-suite --features bough/sabotage_copy --test virtual` | 3 failed as required (plain, grid, nested); 2 passed (not copy-dependent) |
| `cargo check --workspace --all-targets` | passes; the only warnings are the 2 pre-existing dead-field warnings in `bough-gpu` (the hygiene sub-lane removes them) |

Not run: the whole workspace's tests (only the crates above), and the `wgpu` presenter (`--features bough/wgpu`).

## Other findings (not fixed here)

* `rustfmt --edition 2024 --config skip_children=true <file>`, the command CONTRIBUTING prescribes, reformats whole
  files: the tree has no `rustfmt.toml`, and its files follow a wide style that no single configuration reproduces.
  Under `max_width = 160, use_small_heuristics = "Max"`, the closest one, the five files touched here already
  deviate in 0–75 lines each. The edits here add no new deviation under that configuration, and the files were not
  reformatted. A committed `rustfmt.toml` would make the rule usable.
* The committed `Cargo.lock` is stale. It lacks the `source = "git+https://github.com/zeronsh/zgx?rev=06debe9d…"`
  lines for `zgx-cosmic-text`, `zgx-harfrust` and `zgx-workload` (the merge moved them from paths to git revs), so
  every build rewrites it, and `cargo metadata --locked --offline` fails ("cannot update the lock file"). A
  `--locked` CI job would fail on it. It was not committed here because root manifests are the coordinator's.
* Seven app and scene crates set `once()` in their shared `config()` (mail-dash, sketch-code, widgets, zgui, heavy,
  scenes, vlist). Their views were `FIXED` on `api`, so behaviour is preserved. But `Config::mount_once` is documented
  as "a harness option, never an application's", and the app ports (mail-dash, sketch-code) are applications.
