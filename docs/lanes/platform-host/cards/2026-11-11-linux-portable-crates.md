# Card: the portable crates on Linux x86-64

```
lane:            platform-host
state:           CARD
owner:           platform-host session, round 6 (one session)
predecessor:     bd9978cba8 (main) + 8267af9ecd (first_pixel import fix); nightly-2026-10-17 (rust-toolchain.toml),
                 default features, release profile for tests and benches (thin LTO, as RESULTS files use)
question:        Do bough-mem, bough-core, bough-damage-lab and bough-text (with vendored parley's grafts and their
                 twin gates against upstream parley 0.11.1) build and pass their own tests on Linux x86-64?
change:          only what Linux needs: remove macOS assumptions (Mach/libproc counters, NEON-only kernels without a
                 portable twin, Apple paths). Each removal is listed in the report with how the kernel is gated and
                 what its x86-64 path is. No kernel is rewritten for speed in this card.
implication:     passing establishes that the CPU half of bough (record tree, damage planes, text shaping into the
                 glyph pool, the grafts) is portable and gate-equivalent on x86-64. It does not establish speed parity
                 with aarch64, and says nothing about the presenter or a host.
platforms:       Linux 6.18 (NixOS), AMD Ryzen 7 3700X (Zen 2, AVX2), no GPU needed
workloads:       each crate's own `cargo test --release` (and debug for bough-core, which has debug_asserts);
                 by-product: bough-core `core_bench` instruction counts on x86-64 next to the aarch64 numbers in
                 crates/bough-core/RESULTS.md §6
gates:           every existing test of the four crates, including bough-text's parley twin gates (grafted parley
                 equals unmodified upstream bit for bit). Negative control: a deliberately broken scalar twin of one
                 x86-64 kernel path (a NEON-gated kernel's `not(aarch64)` twin) must fail the differential suite.
thresholds:      correctness: all tests pass (any failure is reported, with its cause, not skipped). The instruction
                 table has no threshold: x86-64 and aarch64 instruction counts are different ISAs and are not a ratio
                 of speed; it only flags kernels whose x86-64 path is scalar where aarch64 is NEON.
measurement:     retired user-space instructions (perf_event_open, instructions:u) on this host, which is shared
                 with other tenants' builds: instruction counts only, no wall-clock claims. Builds under
                 `nice -n 19`, at most 8 jobs.
budget:          this session; this host only (allocated by the user for the round).
stop conditions: a crate that needs a representation change (not a port) to build on x86-64: record it as an
                 obstruction with the smallest reproduction and move on to the next card.
do not revisit:  docs/SETTLED.md (no kernel redesign; portable_simd was tried and dropped in bough-core, RESULTS §9).
```
