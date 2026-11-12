# raster lane, round 8: journal

Branch `r8/raster` (worktree `wt/r8-raster`), started 2026-11-12 from `r7/compute` `78635ba4c4`. Nothing is
pushed. Host for every number unless stated: M3 Pro (6P + 6E), macOS 15, shared with the other lanes; the
1-minute load average is recorded next to each number. Commands run at `nice -n 15`, `CARGO_BUILD_JOBS=2`.
Headless only (no window). ilo-prod is reserved; nothing here ran there.

The brief: ROUND8.md sections 4, 6 and 7. A paint is a type composed from typed primitives; an archetype
(paint type plus its expression types) gets one monomorphised CPU kernel and one generated GPU function from
the same expression code; `Frame`-stage expressions are evaluated while shading; associated constants fix
the plan at compile time; the scene's columns are the GPU's buffers, and culling, binning, damage and picking
run there. `crates/bough-kernel` (round 7) is the exactness oracle and the fallback archetype and is not
extended.

## 0. Why the round-7 kernel was stopped (so this lane does not rebuild it)

The generic kernel interprets 32-word commands with a kind field. Three consequences, each visible in its
own journal:

1. The compiler sees nothing of the UI. Every tile entry is decoded (`load_at`: 20 to 32 words) and then
   switched on its kind, per row on the CPU and per pixel thread on the GPU. A card that is always "shadow
   under rounded body under border" is four unrelated commands to it.
2. State reaches pixels through restaging. A hover is: re-apply records, stage commands (330 instructions
   per glyph), bin, raster. That is why the canvas hover cost 14.1 M instructions against the presenter's
   236 K: the work is proportional to the commands of the damaged area, not to the one word that changed.
3. Each groove (palette table, kernel transitions, scroll copy) was added as a mechanism of its own.

What is kept: integers from one source (`kernel.rs` leaf functions are the only place a pixel's arithmetic is
defined), the occlusion cut, and writing words into the memory the compositor reads.

## 1. Predictions and kill criteria (written before any kernel code existed)

Scenes: **canvas** (2560 x 1600 px; 20 clipped frames, 5,000 layered nodes: cards with one or two shadows,
gradients, bordered boxes, labels, clipped groups, 1,000 component instances whose fill reads hover and
theme; chrome with a 505-row layers list; pan moves content and clips) and **anim** (hundreds of rounded
boxes, each moving, resizing, rounding and recolouring on its own phase from one `Time` word).
Baselines are the generic kernel's executors on the *same staged pixels* (`cpu::raster` with the cut and flat
rows, one core and the pool; its Metal executor), plus the façade's workloads where they build.

| id | hypothesis | prediction | kill |
|---|---|---|---|
| R1 | per-archetype kernels are exact against the oracle | 0 differing words: archetype CPU kernel == generated GPU function == `cpu::reference` of the plainly staged scene, on both scenes, across hover, pan, theme and time samples; four mutants caught | any mismatch whose fix needs executor-specific arithmetic |
| R2 | generic composition cannot reach WGSL through wgsl-rs's own generics | a generic `impl<C: Expr> Expr for Sel<C>` is dropped or rejected by the macro (it sees tokens, not trait resolution) | if it works, use it and delete the const assembly |
| R3 | const-assembled source works: each expression and paint type carries its code as an associated const tree; the per-archetype GPU function is rendered from it | the rendered WGSL validates under naga for every archetype of both scenes; direct MSL rendering equals naga's translation of the WGSL on pixels | a combinator that cannot be expressed as (leaf name, children) |
| R4 | CPU, full canvas frame, one core: specialisation and the static cut inside a composed paint pay | >= 1.3x fewer instructions than `cpu::raster` on the same pixels | under 1.15x: per-archetype CPU kernels do not matter for full frames, only for interaction |
| R5 | GPU, full canvas frame: no 32-word decode per entry per pixel thread | GPU kernel time <= 0.8x the generic kernel's on the same pixels | slower than the generic kernel |
| R6 | a hover is a uniform write: no restage, no CPU work per record | GPU-resident path: <= 1,000 instructions of our own on the frame thread plus Metal's fixed cost (<= 200 K in all), GPU time <= 1 ms; CPU path >= 1.5x fewer instructions than restage + bin + `cpu::raster` over the same damage | more instructions than the generic path on either executor |
| R7 | animation is one word per frame | zero turns; frame-thread instructions on the GPU path grow < 10 % from 600 to 6,000 animating elements; GPU time <= 2 ms at 600 | frame-thread cost proportional to elements |
| R8 | the pick runs where the scene is | pick == CPU reference pick on 10,000 pointer positions; one word read back; pick pass <= 50 us of GPU time | a mismatch, or a pick that needs the CPU to walk records |
| R9 | damage can be computed on the GPU by evaluating each record under the old and the new uniform block | retained frames == full redraws over a 200-step random interaction script (hover, pan, theme, time) with the CPU sending only uniform words | a frame where retained != full |
| R10 | the plan is const data | `PLAN` is evaluated in a `const` item (asserted at compile time) and printed; a scene type with no GPU-class archetype never creates a Metal pipeline | a plan input that only exists at run time (other than area) |
| R11 | monomorphisation stays small | <= 150 KB of text for both scenes' kernels | over 500 KB |

## 2. What the type system can and cannot carry to the GPU

(filled in below as measured)
