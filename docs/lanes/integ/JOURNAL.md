# Integration lane (round 7): journal

Branch `round6` in `wt/integ`. Brief: `bough/runtime/lead/ROUND7.md`, section "integ": merge the round-6 lane
branches in the order of `wt/lead/docs/CHECKPOINT-2026-11-12.md` section 5 (steps 2 to 7 and 9; gpu and the memory
WIP are the compute lane's), clear each step's pre-merge items, run the suites with `BOUGH_SETTLE_AUDIT=1` after
each, commit. Never push.

## How each step is run

* Mac (M3 Pro, shared with every other lane, load 20 to 30): `runtime/integ/suite.sh TAG` =
  `BOUGH_SETTLE_AUDIT=1 cargo test --release --workspace --no-fail-fast` (GPU mode), log in
  `runtime/integ/test-TAG.log` (ignored), totals summed from the `test result` lines. A full build of the
  workspace's tests, examples and bins takes 50 to 60 minutes under this load; the tests themselves about 10.
* ilo-prod (x86-64 Linux, `/root/work/r7/integ`, nice 10, `-j6`): the platform-independent crates,
  `cargo test --release -p bough-core -p bough-mem -p bough-text --lib --tests`, and `vendor/cephal`'s own suite
  when a step touches it. Source goes as a git bundle through rex (`bench/remote/xfer.sh` of lane bench), jobs
  through `bg.sh`. Their examples are not built there: they read macOS counters (`proc_pid_rusage`).
* Conflicts are resolved first in a scratch worktree (rerere records the resolutions; the merge in `wt/integ`
  replays them), so the slow Mac builds and the conflict work overlap.

## Steps

| step | lane | commit on round6 | Mac suite | ilo-prod | notes |
|---|---|---|---|---|---|
| 2 | core `2c77f0a1ed` | `173a740bdd` | 243 passed, 1 failed (intermittent, see below), 2 ignored | cephal passed; core/text did not build (fixed in `f723b65e7f`) | dir columns carved 2x, kernels' clamp 2*(m_page+1), gpu_bounds walks dir_len, ledger |
| - | portability | `f723b65e7f` | n/a (no aarch64 change) | bough-text builds; 3 font/word-list tests fail there | damage-lab NEON symbol aarch64-only; bough-text objc2 dev-deps macOS-only |
| 3 | store `fea9e15453` | `2db93d2120` | 149 passed, **101 failed** in GPU mode ("region offsets are u32") | 84 passed, 4 failed (x86, known) | memory's Region::heap kept; instance view reaches ibase + max_id |
| 3 fix | GPU mode | `8acb08b722` | **250 passed, 0 failed**, 2 ignored | 84 / 4 (known) | `Col::off` usize (store block past 4 GiB at `Caps::reserve`); GPU offsets checked |
| - | r7/platform-fixes | `39693ad1f3` `4c14023690` `ab06410a0e` `36cf1d13f5` | (with step 4) | | scroll box by copy; scroll_band KF_MASK; rehover; ScrollBox re-described |
| 4 | layout `83bc9b0f83` | `2208a85e70` | 251 passed, 2 failed (zgui input gallery-l, transforms: caused by `4c14023690`) | 84 / 4 (known); cephal 66 / 0 | describe.rs cursor fix dropped (store removed the cursor); lay_diff via `text_of` |
| 4 fix | revert `4c14023690` | `12804f6899` | running | | bisected: gates pass with `39693ad1f3` alone, fail with `4c14023690` |

## Open items found

* **Intermittent pixel failure** after step 2: `bough-heavy` `correctness`
  `log_pane_ring_of_lines_is_pixel_exact_at_every_ring_age`, "ring 3 tick 29: presented != full redraw at
  (64, 160, 38, 51)", once in the full suite; 0 failures in 70 reruns of the binary (alone and four at once). The
  CPU side is deterministic; the surface a frame repairs (so its ring age) follows GPU completion, so a timing
  path is the likely carrier. Not reproduced, cause not found.
* **macOS-only code in platform-independent crates** (amendment 1): bough-core's examples and
  bough-damage-lab's `measure.rs` (`proc_pid_rusage`, `pthread_set_qos_class_self_np`), bough-text's examples
  (`proc_pid_rusage`). Tests are fine with `--lib --tests`.
* **bough-text on x86-64**: `exact.rs` `english_words_and_sentences` (`n > 100 || !full()`), `pool.rs`
  `heavy_numeric_labels_over_consecutive_ticks` ("most heavy updates must be digit patches"),
  `random_edits_all_paths` ("every path exercised: [0, ...]") and, from step 3, the store lane's
  `moved_label_patches_as_one_allocated_there` ("the digit patch path was exercised after moves (0)"). The font
  is the same embedded zgx font on both hosts; the cause is that `GlyphPool::set_text_d`'s equal-length patch
  (`patch16`) is NEON-only, so on x86-64 every label update takes the slow path: correct output, but the tests
  assert the fast path ran. A missing x86 twin (core avenue C20), not a merge regression. The exact.rs one was
  not traced.
* **The first store-step suite was killed** (SIGTERM at 01:45: the core lane's `pkill -f "cargo test --release
  --workspace --no-fail-fast"` matched it); the second ran out of disk (the Mac at 100 %). Since then the suites
  build with `CARGO_PROFILE_RELEASE_DEBUG=0` (the target stays at 2 to 3 GB instead of 12).
* **GPU mode after the store merge**: with `Caps::reserve` (2^15 pages) `StoreCaps::of` gives 2^22 ids and the
  CPU-only store block passes 4 GiB of address space; `Col` kept u32 offsets. Fixed with 64-bit offsets (the block
  is a lazy mapping); the lead's preferred form (no worst-case reservation per column) is left to store/memory.
