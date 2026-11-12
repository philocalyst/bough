# Report: the portable crates on Linux x86-64

Card: `cards/2026-11-11-linux-portable-crates.md`. **State: GATED** (all tests pass, negative control fails as
required). Not yet `INDEPENDENT_REPLAY`.

Host: Linux 6.18 (NixOS), AMD Ryzen 7 3700X (Zen 2), shared with other tenants' builds (load ≈ 15 on 16 threads
throughout). Toolchain nightly-2026-10-17 (rustup from nixpkgs), C toolchain gcc 16.2 (nix). Predecessor
bd9978cba8 + 8267af9ecd. Change commit: d29385c0a0.

## Result

| crate | profile | result |
|---|---|---|
| bough-mem | release | 3 passed |
| bough-damage-lab | release | 2 passed (one is the new Linux counter test) |
| bough-core | release and debug | 25 passed each (differential 7, edges 1, hit 4, nrect 8, scroll_copy 2, shapes 3) |
| bough-text (+ vendored parley grafts) | release | 45 passed, 1 ignored (`upstream_self_consistency`, ignored upstream too) |

bough-text includes every parley twin gate (grafted parley = unmodified upstream parley 0.11.1): style variants 55
cases, font stacks 70, retext 1,400, exhaustive pairs 36,290, text_bench 1,530, workload strings 31,847, short
strings 82,740, random mixed 3,000, all equal. ICU boundary machines 427,575 + 300,000 strings equal.

**Negative control.** `mutants/page-find-last-lt-x86-off-by-one.patch` (an off-by-one in the x86-64 scalar twin of
the NEON `page_find_last_lt`) fails bough-core's differential 7/7, hit 1/4 and shapes. Reverted; the suite is green
again on the restored tree.

## macOS assumptions removed (and how each kernel is gated)

1. **bough-damage-lab `measure`** used `proc_pid_rusage`, `mach_absolute_time`, `pthread_set_qos_class_self_np`. Now
   split by OS: Linux reads `perf_event_open` (`instructions:u`, `cycles:u`, this thread, a two-event group) and
   `CLOCK_MONOTONIC`. Counts on the two OSes are different ISAs and different scopes (macOS: task, user + kernel).
2. **`asm::lab_cull_hw_neon`** wrapped a NEON-only kernel without a gate; gated on aarch64 like the kernel (the form's
   own `repair` already falls back to `cull_hw` elsewhere).
3. **bough-core `core_bench`** had its own `proc_pid_rusage` copy; now uses `measure`.
4. **bough-text dev-dependencies** `objc2*` (only `examples/gpu_shape.rs`, Attempt 3 on Metal) moved to a macOS target
   table; the example is a stub elsewhere. The examples' meters use `measure` on Linux.
5. **bough-text `examples/bench.rs`** called `Shaper::shape_simd`, which exists only on aarch64. Off aarch64 the
   "neon" column measures the scalar kernel.
6. **bough-text tests' data paths.** `/usr/share/dict/words` (now `BOUGH_WORDS`, web2 from nix miscfiles here: 234,937
   words vs macOS's 235,976) and the DejaVu directory in `cache.rs` (now `BOUGH_DEJAVU_DIR` like `common/twin.rs`).
   DejaVu 2.37 from nixpkgs, the version the macOS paths name.
7. **bough-text `pool.rs` path-coverage asserts** required the NEON class patch (`patch16`), which has no x86-64
   twin. On aarch64 they are unchanged; elsewhere they require its absence (so adding a twin forces the test to
   require it) and that every other path is exercised. The tests' exactness checks are unchanged on both.

No kernel failed to build for want of a portable path: every NEON kernel in these crates is `cfg(target_arch =
"aarch64")` with a `not(aarch64)` scalar twin or a scalar tail that does the whole job.

## Coverage lost on Linux (not verified)

* `parley_twin::variable_font`, `cache.rs`'s SF NS table, `font_stacks`' six Apple fonts: skip silently without
  `/System/Library/Fonts`. Partly replaced: `variation.rs` gained `other_variable_font_instances_equal_rustybuzz`,
  the SF NS gate on `BOUGH_VARIABLE_FONT` (Inter 4 InterVariable.ttf here, wght + opsz): 10,390 strings equal per
  instance, 47,710 refused to the full shaper. `font_stacks` ran on DejaVu only.
* macOS still building after these edits: not checked (no Mac on this host).

## Kernels that only vectorise on NEON (instruction counts, by-product)

Raw: `bench/results/platform-host/2026-11-11-*` (two identical runs; the v3 build adds `-C target-cpu=x86-64-v3`).
Table: `tools/isa_table.py` over RESULTS §6 (aarch64, M3 Pro) and this host. `norm` = (Bough x86/aarch64) ÷ (Arena
x86/aarch64); the Arena is scalar Rust on both, so it calibrates the ISA change. Instructions are not time; no speed
claim.

| op | norm range (4 shapes) | the aarch64-only kernel(s) |
|---|---|---|
| cull(full window) | 2.5–3.9 | `nrect::lt_mask` / `lt8`, `bough::lanes_test` |
| repair3+cull, write80+cull | 1.2–2.0 | same cull kernels |
| translate (1016 / 1000 / 156) | 1.9–2.5 | `nrect::translate_union`, `union_all` |
| hit | 1.5–4.7 | `bough/hit.rs` |
| parent, ancestors | 1.1–3.5 | `page::page_find_*` |
| splice8, layout200, subtree_end, children | 1.0–1.4 | (none dominant) |

With AVX2 enabled for the whole build (x86-64-v3) Bough's counts drop ≈ 5 %: the scalar twins do not autovectorise
into anything like the NEON kernels. bough-text's shaping (`shape_simd`, `simd_slots`), its line/space scans and
`patch16` are scalar on x86-64 too (not in core_bench).

## Next

Task 2 of the brief (bough-gpu through naga to SPIR-V, pixel gates on lavapipe). A later card: x86-64 SIMD twins
(SSE4.1/AVX2 behind `is_x86_feature_detected!` or `target_feature`) for the cull, translate, hit and page-find
kernels, gated by the same differential suite and this mutant, measured with this table.

Shared-file note for the coordinator: `Cargo.lock` on main lacks the `source = "git+…zgx…"` lines of the three zgx
packages, so the first build rewrites it (left uncommitted on this branch).
