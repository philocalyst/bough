# The reformulation method (required reading for every iteration agent)

Source: Ben Joffe, "Fast day-of-the-week calculation", https://www.benjoffe.com/fast-day-of-week (read 2026-11-01).
The user's standing instruction: **there is ALWAYS a structural rethink that opens new performance horizons if
you are willing to bend the rules and think out of the box.** Tuning the existing formulation is not the job.

## What Joffe actually did

The task looks trivial: weekday = (days + 4) mod 7. The obvious code (`rem_euclid`, double mod) is ~3× slower
than the best. He got to 3 instructions (x86: `imul`, `lea`, `shr`) by a chain of *representation changes*:

1. **Change the domain so the expensive operation disappears.** 7 = 2³−1 (Mersenne). Multiplying by
   ⌈2³²/7⌉ makes the remainder appear in the **top bits of the low half** of the product. A modulus became a
   multiply + shift: the answer is read from bits nobody was looking at.
2. **Fold every offset into one constant.** The epoch (+4) and the output format (0..6 vs 1..7) are a
   *rotation* of the 8-segment circle — one added constant `Z`, applied before the shift. No extra ops.
3. **Widen to delete corrections.** Range-correction terms vanish entirely if you compute in 64 bits
   (`(u64(rd)*M + Z) >> 61` covers the full i32 domain). Width is cheaper than branches/fixups.
4. **The value was there all along.** Variant 3: the quotient estimate needed for the correction is the *high
   half of the product already being computed*. Look at what the hardware gives you for free.
5. **Pad the cycle to a power of two** ("nundinal map"): `x mod 7 = (x + ⌊x/7⌋) & 7` — give each week a
   phantom eighth day; then the remainder is a mask. Identity: `x mod D = (x + ⌊x/D⌋) mod (D+c)` for D+c = 2^k.
6. **Bend the input range deliberately.** A restricted valid range enables a stronger identity; then decide
   explicitly whether to widen (to remove the restriction) or keep it (and prove the domain).
7. **Prove over the whole domain**: exhaustive tests for 8/16/32-bit inputs, sampled for 64.
8. **Measure latency and throughput separately** (chain results through XOR for latency), across machines and
   compilers; compilers routinely miss the optimal sequence — read the assembly; generate many parameterized
   variants (he built 280) rather than reasoning about one.

## How it maps to this project (seed list — find more)

| Joffe move | GUI analogue already in the design | Where to push further |
|---|---|---|
| Domain change makes op vanish | Rects stored as `(x0, y0, −x1, −y1)`: intersection = one lane-wise `max`, union = one `min` | Express clip chains, damage accumulation, hit tests entirely in min/max algebra |
| Fold offsets into one constant | Coordinate *spaces*: a subtree's translation is one table entry, not per-node rewrites | Fold scroll + transform + DPI + snapping into one per-space affine applied in the vertex shader |
| Widen to delete corrections | i32/i64 fixed-point absolute coordinates: no float reassociation fixups, exact translation sums | 64-bit packed records so one load carries what a branch would have fetched |
| Value there all along | The GPU instance *is* the paint output — no paint phase; a version stamp *is* the dirty flag (no clearing) | What else is recomputed each frame that some earlier phase already produced? |
| Power-of-two padding / phantom slots | 64-record pages; padding slots hold sentinel values so SIMD scans need no tail masks (bbtree invariant 4) | Phantom tiles/rows so damage planes are whole words; phantom glyph slots so text grows in place |
| Bend the input range | i16 device-pixel rects (±32 K) in hot summaries, exact i32 in cold columns | Which hot fields can drop to 8/16 bits by restricting a domain we can prove? |
| Exhaustive proof | Differential tests: every fast path vs a dumb full-recompute oracle (full damage, full layout, full shaping) | Exhaustive over small domains (all 16-bit rect pairs for rect algebra; all ASCII strings ≤ N for the shaper) |

## Rules for every iteration attempt

- State **which rule you are bending** and **which representation changes**. A micro-optimization of the same
  representation is not an attempt; it is cleanup.
- Keep the dumb oracle and a differential test; correctness over the full domain is non-negotiable.
- Report instructions (deterministic) first, then time; latency and throughput separately; read the asm of the
  hot loop and quote it.
- Record losing regimes. A design that wins big but loses the tiny/flat case must be restructured so the
  *same* mechanism is cheapest there too — the previous GPUI campaign died on bolt-on fast paths that taxed
  the simple case by 5–10%. One mechanism, no fallback branch on the hot path.
