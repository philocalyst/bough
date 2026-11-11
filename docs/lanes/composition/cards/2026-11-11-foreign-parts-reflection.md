# Card: foreign records, part addressing and override, the reflection gap list

```
lane:            composition
state:           CARD
owner:           Claude (Fable), round 6
predecessor:     aa9943e7ae (lead/round6-base; nightly-2026-10-17, default features, profile dist for instruction cells)
question:        Can interleaved foreign content, overridable external components and inspector reflection be one
                 mechanism (addressed records + typed patches at mount), on stable-Rust techniques, free when unused?
change:          (1) a foreign record kind: layout leaf, record with ink/clip/paint position, three compositing modes
                 (Under: destination-out hole in bough's surface, the foreign layer beneath; Over: record without
                 pixels, the layer above; Sampled: host pixels through the image kernel), a host callback with the
                 placement whenever it changes, hit policy, external content-changed damage.
                 (2) `Part<S>` keys: `.part(KEY)` at the author's site, `.restyle(KEY, f)` / `.intercept(KEY, f)` at the
                 consumer's, resolved at mount and at re-description through a scope chain in the Mounter.
                 (3) docs/lanes/composition/DESIGN.md with the gap list.
implication:     Passing shows the record tree can carry content it does not paint with exact damage and ordering,
                 and that override needs no component macro. It does not show a real webview, IOSurface zero-copy
                 sampling, AX, or cross-process surfaces.
platforms:       macOS 15 / Metal (direct presenter). bough-gpu (wgsl) is not ported in this card.
workloads:       new tests crates/bough/tests/{foreign,parts}.rs; examples/foreign_layers (windowed, 2 s, unfocused);
                 existing suites of bough, bough-core, bough-metal; slope.py bough 0 heavy stream both, lab stream both.
gates:           presented == full redraw at every ring age with foreign records (incl. inside a scroll view, scroll by
                 copy); hole pixels are (0,0,0,0) and bough content painted after the hole is intact; placement handed
                 to the host equals layout box ∩ clip, in paint order; fresh mount == retained.
                 Negative controls: the punch disabled (mutant: Under mounted as Over) must fail the hole gate; a
                 placement scan that ignores scroll must fail the placement gate; compile-fail doctests for part misuse.
thresholds:      every slope cell after/before <= 1.02; existing pixel gates unchanged and green.
measurement:     instructions (slope.py, 5 reps interleaved A/B, no lock needed); no wall-time or GPU-time claims.
budget:          this session; this Mac only.
stop conditions: a slope cell > 1.02 that cannot be removed -> restructure so the mechanism is behind the element,
                 else REJECTED; a kernel change that alters any existing pixel -> REJECTED.
do not revisit:  many small layers per element (bough-metal §12.3: ~37 K per changed layer) as the general present
                 path: a layer is used only for content bough cannot paint; general #[component] macro, hooks,
                 ambient context (SETTLED, design).
```
