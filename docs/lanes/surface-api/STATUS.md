# surface-api round 6: status

Branch `lane/surface-api/round6`. Re-established from the branch on 2026-11-11 (21:10) after the lanes were re-staffed;
`HANDOFF.md` predates the Fable agent that built the items below and is superseded by this file.

## Inherited (Fable agent, commits `facc86d06d`, `5d3b95a556`, `4c67f26160`), verified by this session

All runs: `cargo test --release -p bough --test <name>` on HEAD `88ed56ff5b` (lead/core-r6 and round6 merged).

| item | built | test (passes) | mutant (fails its test) |
|---|---|---|---|
| scroll containers reconcile their content (`ScrollBox`, `ScrollY` keep the view, re-describe port and content) | yes | `propagation_scroll` (7 tests, rings 1..4, retained == fresh == presented) | `--features mutant_scroll_skip`: 6 of 7 scroll tests fail, and 3 route tests (verified) |
| `On::update` refreshes a route's captured closure | yes | `propagation_routes::route_captures_follow_the_description` | `--features mutant_route_stale`: that test fails (verified) |
| routes resolve their receiver along the owner chain (list rows, entities, an enclosing owner) | yes | `propagation_routes` (11 tests: row/entity handlers, removed owners, slot reuse, nested owners, mount error) | `--features mutant_route_owner`: `drag_ends_with_its_element`, `a_removed_row_is_never_dispatched_to` fail (verified) |
| only handled owners re-describe (hover colour, wheel, glide, keystroke re-describe nothing; a row handler re-describes the row) | yes | `propagation_routes::grain::a_turn_redescribes_the_owners_it_handled` (counts `view` calls) | none |
| hover follows the tree under a resting pointer (finding 8) | yes | `propagation_routes::grain::hover_follows_a_scroll` (rings 1..4) | none |
| route ops / `HoverBg` / `Focusable` / `NoHit` seal (closure bytes; hover colour as paint) | yes | `seals`, `propagation_routes` | `BOUGH_SEAL_MUTANT=1`: `route_captures_follow_the_description` fails (verified) |

Not verified by the predecessor and not yet by this session: the whole `bough` suite on this HEAD, the mount-once
slope floors (heavy stream, lab both, vlist) against the base. Fixed here: `cargo check --workspace --all-targets` did
not build (`bough-app-sketch-code` assigned the now-private `Spec::abs`; `c2ae86085a`), and the lead's
`examples/signal_cost.rs` did not compile (`NumFmt::new` arguments).

## Baseline numbers of this HEAD (instructions, calling thread)

* `signal_cost` (one signal written per `App::turn`): 16 rows once 1,096 / re-describing 32,244; 256 rows once 11,133 /
  re-describing 513,280 (colour cell). Idle turn: once 853 (16) / 10,933 (256); re-describing 32,743 / 515,764.
* `redescribe_cost` (one plain field per turn, 1,000 rows): owned 648 / row, static 614, bare 233, once 181 / turn.
* gui-bench R0 per operation (`headless --blocks 3 --repeats 8`): fine select 14.9k (10) / 15.7k (1,000) / 15.8k
  (10,000); rows hover 4.0k (10) / 4.5k (1,000); coarse select 28.0k (10) / 1.48M (1,000); coarse `--plain` select
  27.3k (10) / 1.48M (1,000).

## This session's work

Owner grain and owner seal (`grain.rs`, `describe::OwnerSeal`), record state flags with variant tables (`flags.rs`),
direct wires (`direct.rs`); propagation design in `PROPAGATION.md`. Numbers, gaps and avenues: `HANDOFF.md`.
