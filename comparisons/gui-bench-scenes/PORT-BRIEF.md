# Porting bough's heavy scenes to another GUI framework (brief for one porting agent)

bough is a Rust GUI framework. Its bench lane measures five "heavy scenes" (a spreadsheet, a design canvas, an
IDE, a dashboard, a chat). To compare bough with other frameworks fairly, the scenes are added to **gui-bench**
(github.com/zortax/gui-bench), a benchmark that runs each framework's app under **fakeland** (a headless Wayland
compositor that injects input and times every frame) on a Linux host, ilo-prod. You port the scenes to ONE
framework. Start with the sheet, then the canvas.

## The spec is the contract

* `comparisons/gui-bench-scenes/overlay/apps/shared/scenes.rs` (in bough's repository) is the shared spec: sizes,
  colours, data, keys, hover targets, the command line. Read all of it. Every app draws the same layout from it, and
  the runner probes fixed pixels (`overlay/runner/src/scenes.rs`: the layout check points, the hover target and
  the regions each probe watches). If your app draws something else at those points, the cell fails its layout
  check and nothing is measured.
* Include the spec with `#[path = "../../shared/scenes.rs"] mod scenes;` (from `apps/<fw>/src/main.rs`; adjust the
  relative path for submodules) and call `scenes::from_env()` at the top of `main` before gui-bench's
  `cli::from_env`: when it returns `Some(scene)`, run your scene and return; otherwise fall through to the
  existing code unchanged.
* Where the framework cannot draw what the spec asks (a shadow, a gradient, a clip, a font size), draw the closest
  thing it can and record it. Never fake a measurement (no caching a screenshot, no skipping work the scene
  describes, no special-casing the probe points).

## Idiomatic, as the framework's own users would write it

Read how the existing gui-bench app of your framework is written (`apps/<fw>/src/**`) and write the scenes in the
same style: its normal widgets, layout, scrolling, hover handling, key handling, theming, and **its virtualised
list where it has one** (the sheet's body is 1,000 to 10,000 rows of 10 cells). Do not hand-roll a renderer or
bypass the framework's update path. If the framework's documentation teaches two ways, pick the one its examples
use and say which.

## Where the files go (bough's repository, your worktree)

Worktree: `/Users/philocalyst/Downloads/designs/reactivity/wt/r7-bench`. The upstream gui-bench (commit 0fd4510,
the version on ilo-prod) is checked out read-only at
`/private/tmp/claude-501/-Users-philocalyst-Downloads-designs-reactivity/3b0ef642-a3f6-4579-a8db-9ce940b28864/scratchpad/bench-r7/gb`
for reading. Your files are an **overlay** copied over that checkout on ilo-prod:

* `comparisons/gui-bench-scenes/overlay/apps/<fw>/src/main.rs`: a full copy of upstream's `main.rs` with the
  `scenes` module and the pre-dispatch added (copy upstream's file first, then edit);
* `comparisons/gui-bench-scenes/overlay/apps/<fw>/src/scenes/{mod.rs,sheet.rs,canvas.rs}`: your scenes;
* `comparisons/gui-bench-scenes/overlay/apps/<fw>/Cargo.toml` only if you need another crate or feature of the
  framework (copy upstream's first; prefer what is already there);
* `comparisons/gui-bench-scenes/DEVIATIONS-<fw>.md`: every place your port deviates from the spec or from how the
  framework would normally be used, and why; and the framework features you used for each part of the spec.

Edit nothing else (not the spec, not the runner, not other frameworks' overlays). If the spec is ambiguous or
wrong for your framework, say so in your report and in DEVIATIONS; do not change it. No git commands that change
state (no commit, add, stash, checkout); the lead commits.

## Build and run on ilo-prod (from the worktree)

```
cd /Users/philocalyst/Downloads/designs/reactivity/wt/r7-bench
export REX_SESSION_ID=session:01a1ccd3-828f-72bc-8d3d-490dde484869
comparisons/gui-bench-scenes/remote.sh build <fw>              # sync your overlay, build, print errors
comparisons/gui-bench-scenes/remote.sh cell <fw> sheet 1000     # one quick cell: outcome, probes, screenshot
comparisons/gui-bench-scenes/remote.sh dev <fw> sheet 1000      # both
SHOTS=<dir> ...                                                # where the screenshot lands (default /tmp)
```

`<fw>` is `iced`, `egui` or `gpui-kit`. A build takes 1 to 5 minutes, a cell about a minute; give the Bash
tool a timeout of 15 minutes. One command at a time for your framework. Read the screenshot (the Read tool shows
PNGs) to check your layout against the spec. A cell's report lists `done` (measurements that wrote a report) and
`errors`; the probes are `hover` (the fan-out), `theme`, `edit`, `scroll` (sheet), `resize`, plus `idle`. A probe
that times out means your app did not change the watched pixels within 5 s of the input: check that the hover
target, the keys and the watched regions match the spec. Do not run a cell larger than 10,000 rows (sheet) or
5,000 nodes (canvas). Never run anything that opens a window on the Mac.

## Done means

1. The sheet cell at 1,000 rows passes the layout check and writes all its reports with no errors (`done` holds
   startup, layout, idle, hover, theme, edit, scroll, resize); likewise at 10,000 rows. Then the same for the
   canvas at 1,000 and 5,000 nodes (no scroll probe there).
2. The screenshot matches the spec by eye (panels, colours, sizes; text may differ in rendering).
3. DEVIATIONS-<fw>.md is complete.
4. Your final message: what you built (which framework features for which part), the cells' outcomes and the
   main numbers the report printed (hover, theme, edit latencies; idle RSS), the deviations, and anything in the
   spec or the scripts that got in the way. Plain text, no emojis.
