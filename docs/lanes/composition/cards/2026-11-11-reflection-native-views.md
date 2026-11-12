# Card: reflection as a walk of the description (A), native views in the tree (B), parts folded in (C)

```
lane:            composition
state:           CARD
owner:           Claude (Fable), round 6, continuing the stopped composition session
predecessor:     d5bfacc45c (lane/composition/round6 with round6 merged)
question:        Can one zero-cost mechanism serve tests, the inspector, an agent-driven replay server and the
                 accessibility tree, with nothing stored per instance and nothing run per turn; and can native
                 views sit anywhere in the record tree with exact order, clip, scroll, input and teardown?
mechanism A:     reflection = run the description again, on demand, with a probe instead of a mounter
                 (`Element::probe`, default: one opaque record subtree), joined to the retained records by a
                 preorder cursor and the depth column, plus static tables (type_info schemas, type names,
                 closure type names, part keys). The record scan supplies what is retained (geometry, ink, clip,
                 text, paint, routes, wires, focus/hover); the walk supplies what was said (element types,
                 sources, parts, author roles/ids/labels, owners, list rows). Nothing is written at mount for
                 reflection: `Reflect::{types, parts, owners}` and `foreign::Slot::at` are deleted.
situations A:    1 find a button by its text and click it through pointer_down/up; 2 read a counter label after a
                 click; 3 a part by key and index inside a third-party component; 4 an embedded view's owner and
                 its fields; 5 a list row's text and its bound signal; 6 a text field: focus, type, read value;
                 7 a scroll box's content found and scrolled into view; 8 a foreign element in the tree with its
                 placement; 9 hover and focus state flags; 10 a signal read and written by path (agent);
                 11 a session of turns recorded and replayed to identical pixels; 12 a PNG of the retained frame
                 with no window; 13 the accessibility projection (roles, names, values, states, actions);
                 14 the inspector's pick + ancestors + style + source; 15 a covered element refused by click.
mechanism B:     a foreign record keeps its compositor mode; the host is an NSView-level host (sibling views below /
                 above the layer-hosting view) and a layer host; placements ride the frame that draws the hole.
gates:           A: tests/probe.rs (every situation above, headless), mutants: a probe that skips the cursor
                 (attribution gate fails), a selector that ignores the part index; devtools socket round trip
                 test; replay equals the original run pixel for pixel (GPU, offscreen).
                 B: offscreen composite (CARenderer into a texture, no window) of [bough below, CALayer, bough
                 above] equals bough painting the same content (Sampled) within a stated tolerance; existing
                 foreign gates stay green.
thresholds:      signal_cost and redescribe_cost cells (default, CHURN) within 1 % of d5bfacc45c with everything
                 compiled in and idle (3 runs each, the slope harness); MOUNT reported (its noise is ~3 %).
measurement:     instructions only (tinstr). No wall-time or GPU-time claims.
stop conditions: any idle cell > 1.01x that is not removed -> the mechanism moves behind a cargo feature and the
                 card says so; a pixel gate that changes -> REJECTED.
```
