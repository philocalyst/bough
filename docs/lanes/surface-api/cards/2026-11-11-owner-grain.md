lane: surface-api · state: CARD · owner: the Opus session that took the lane over (2026-11-11, 21:40) · predecessor: 88ed56ff5b
question: can a handler cost what it changed (owner grain), and a state flag flip be a paint write, as general mechanisms?
change A (owner change detection): a handler re-describes its receiver only if (1) a plain byte of the receiver changed
  (a compile-time plan from `type_info`: inline scalars compared as bytes, `Copy + PartialEq` fields by value; a field the
  plan cannot prove, e.g. `Vec`, `String`, `Box`, keeps today's "always") or (2) a cell (`Signal`, `List`, `Entities`,
  `Reader`) its last description read *plainly* (not through `sig!`) was written: such a read subscribes the owner
  with a wake wire. Serves: signal-only handlers, `App::poll`, timer turns, no-op keys, hover routes, row edits that set
  row signals, a root handler that changes state its view does not show, async completions, drags, typing, scrolls.
change B (owner seal): a re-describing owner compares its whole description with its retained seal first and steps
  over the walk when equal (scroll boxes, lists, `Option`/`Vec`/`each` parts seal).
change C (flags): per-record state flags read by variant tables resolved at mount (`.when(flag, ..)`; `hover_bg` an
  instance); a flip is a paint/style write on the flipped records; list items carry flag columns for range flips.
gates: after each operation retained == fresh mount == presented at rings 1..4 (`tests/owner_grain.rs`, `tests/flags.rs`)
  + the bough, suite, awagy, zgui test crates. Mutants: a plain field left out of the plan; a plain cell read not
  subscribed; a flag flip not applied to one of a range's records; each must fail its test.
thresholds: signal_cost re-describing within 1.2x of once; gui-bench fine select and rows hover flat in N; no-op key
  handler < 1,000 then < 300; mount-once slopes (heavy stream, lab both, vlist, 10x spread, median of 5) <= 1.02.
stop: a mechanism that cannot be made exact lands as its exact subset, the rest written down; no gate is loosened.
