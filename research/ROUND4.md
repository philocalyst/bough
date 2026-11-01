# Round 4: direction from the user (2026-11-01)

This overrides ROUND3.md wherever they differ. ROUND3.md's rules (worktrees, locks, gates, trailer, no regressions,
Joffe method) still apply.

## What the user asked for

> virtualization shouldn't be a thing that is special-cased into something like lists, but rather built deeply into
> the core as part of damage tracking, just like how transformations and reflection should be, first class primitives,
> reflection especially for the inspector … ideally should be even cleaner to use than facet, all typed, and require no
> opt in from the component library itself. bevy has a lot of inspiration here.
>
> for text please just vendor parley, and graft any performance fixes to it rather than doing something from scratch.
>
> shaders stay in wgsl-rs, and … vendor winit and wgpu to strip out any startup costs or needless driver or anything,
> and ensure a pure wgpu pipeline is more performant and uses all the private and public apis.
>
> the goal for shaders/inspector/transforms/etc, is that they're built in, so those multi thousand line PRs just fall
> out. the inspector is a good thing to work on because it's a great chance to test out the apis you've been iterating
> on. some of the warning and cruft in the interface should be stripped, but otherwise you should be able to see if
> the apis actually improve life or not.

Also: "still missing some pretty big streamlinings for performance", and no new agents: the four running ones are
redirected, so **do not spawn subagents**.

## Principles

1. **Built in, not bolted on.** Virtualization, transforms, reflection, transitions, shaders and the inspector fall out
   of the core representation. If a feature needs a thousand-line special case, the core is missing a primitive; find
   it.
2. **Virtualization is damage.** Today the measured/virtual list is a special mechanism. Instead, every clip or
   viewport is a region, and content outside every visible region is not materialized: not mounted, laid out, shaped
   or drawn. When a scroll, transform, resize or reveal exposes area, that exposure is damage, and it materializes the
   records intersecting it through the same machinery that repaints them. Any container is then virtual for free: a
   long column, a grid, a document, a tree view. The list becomes a case of nothing.
3. **Transforms are a core record attribute.** A scroll offset is a translation, so page offsets generalize to
   transforms. Three things follow from that one attribute:
   * damage is the tile cover of the transformed ink;
   * hit testing applies the inverse;
   * shaders apply it per instance.
4. **Reflection is typed and needs no opt-in.** Component libraries write no derives and no registration. It must be
   cleaner than facet, with bevy_reflect as inspiration. The runtime already knows a great deal about every view, so
   reflection should come from there:
   * the element types (builders are typed);
   * every wired field (`sig!` gives an `offset_of!` plus the field's type);
   * every record's kind, style, layout and ink.

   The inspector reads and edits through it.
5. **Transitions are built in.** A transition is a typed operand: a value plus a curve, driven by the frame clock
   through the same wires.
6. **The inspector is the API's test.** It is an ordinary bough app built only on the public API, opened on any running
   bough app. It picks elements by pointer (hit testing), shows the record tree, styles, layout boxes and ink, damage
   tiles per frame, and the reflected state, and it edits that state live. Record every friction point you hit while
   writing it, and fix the API rather than working around it. Strip warnings and cruft from the public interface.
7. **Text is vendored parley.** Vendor parley (and fontique, harfrust/swash, skrifa as needed) into the repo. Then
   graft bough-text's performance mechanisms into it:
   * the dense step table for simple scripts, with digit patches;
   * byte slots;
   * the ring of lines;
   * line-local re-wrap with its convergence point;
   * hot and cold paragraph halves;
   * table caches, variable-font instances, and the delta-recorded text model.

   bough's text path then becomes parley plus those grafts. Keep every benchmark win, or report the delta honestly.
8. **The GPU path is vendored wgpu with shaders in wgsl-rs.** Vendor wgpu (core, hal, types, naga) and winit. Strip
   their startup cost and needless driver work. Make the pure wgpu pipeline at least as fast as today's direct-Metal
   presenter (`bough_metal::fused`) on every metric: CPU slopes R0/R1, the per-frame floor, the drag probe, GPU time,
   footprint. Reach the platform's private and public APIs through the vendored hal: IOSurface/CAMetalLayer
   presentation, shared events, residency sets, pre-armed command buffers. Shaders are written in wgsl-rs, and naga
   compiles them at build time, not at runtime.

## Where to look

| topic | references |
|---|---|
| inspector | `~/projects/gpui-ce3` branch `origin/inspector`; `~/projects/gpui-ce-inspector` (Loupe, branch `codex/loupe-production`, `crates/gpui_inspector`); archive (`~/projects/_ref/archive`) `inspector-ui-sol`, `inspector-pointer-sol`, `inspector-contained-owner-sol`, `typed-catalog-sol` |
| reflection | `~/projects/gpui-ce3` `origin/reflection` (`examples/learn/trait_reflection.rs`, `src/inspector.rs`); facet; bevy_reflect |
| transitions | `~/projects/gpui-ce3` `origin/main`: `crates/gpui/src/transition.rs`, `style_transitions.rs`, `examples/learn/transitions.rs` (unkeyed transitions keyed by tracked caller location) |
| parley | `~/projects/gpui-ce3` branches `parley_stack_01_core` … `parley_stack_06_editable_text` (how gpui-ce integrates parley) |
| wgpu / wgsl-rs | gpui-ce3 `origin/main` `crates/gpui_wgpu`, `crates/gpui_render`, and the root `Cargo.toml` pin `wgsl-rs` (schell/wgsl-rs rev b2de16a1b8); archive `codex/wgpu30`, `wgsl-buildtime-sol`, `local/gpu-scroll-copy` |

Read references with `git show <branch>:<path>` / `git -C <repo> log`; never modify those repos.

## Ownership (one agent each; merge `canonical` often, since these overlap)

| agent / branch | owns |
|---|---|
| `api` | virtualization-as-damage in the core (2), reflection (4), and the interface cleanup (6, the API side) |
| `input` | transforms (3), transitions (5), and the inspector app (6) built on `api`'s reflection as it lands |
| `floor` | vendored wgpu + winit, wgsl-rs shaders, and the pure wgpu pipeline (8); scroll-by-copy is built there, not in Metal |
| `text3` | vendored parley with the grafts (7); inline formatting, the editor view and bidi come through parley |
