# Card: layout work proportional to what can change (lane `layout`, round 6, second pass)

```
lane:            layout
state:           CARD (frozen 2026-11-11, before any change or measurement on this branch)
owner:           one Fable session (layout lane, restarted on lane/store/round6 b68c141937)
predecessor:     b68c141937 (nightly-2026-10-17, release profile, bough features default)
question:        Can a change inside a box cost layout work bounded by the box (not its ancestors' children),
                 an unchanged child of a re-running container cost a few instructions, a cold mount spend
                 far less than ~9,000 instructions per row in cephal, and a moved-only subtree be a
                 translation?
change:          (1) boundaries: what a parent reads of a child's answer, compared exactly, so a change that
                 the parent cannot observe stops at the child; the cases written down. (2) the unchanged-
                 child path of a container re-run as a scan over columns. (3) cold layout: per-phase
                 attribution, then structural cuts (uniform children by spec identity, no per-node hashing
                 or allocation). (4) moved-only rows written as translations.
mechanism:       answers whose parent-visible part did not change end propagation (cephal's spineless
                 scheduler, made exact for overflow, baselines and content-independent questions); serves
                 any GUI: text edits in fixed toolbars, cells of tables, counters in cards, list rows,
                 chat bubbles, progress labels, inspector panes, editors' status bars, menus, tabs,
                 tooltips, form fields, clocks.
workloads:       signal_cost (once, 16/64/256), redescribe_cost (default, MOUNT=1, CHURN=1) at
                 16/64/256/4096 rows where the harness allows; three shapes: column of rows, wrapping
                 flex grid, nested stacks with percent sizes (a new mode in redescribe_cost, not a new
                 harness).
gates:           cargo test --release -p bough --test seals, --test layout; cephal's own tests
                 (incremental == cold over random trees and edits, fixtures) on ilo-prod; a new
                 differential test (incremental == fresh layout over random trees x random edits,
                 thousands of cases) with a mutant that treats a non-boundary as a boundary and must
                 fail it; pixel suites (bough-suite, bough-vlist) at the stage boundary.
thresholds:      every existing instruction cell <= 1.02x; the targeted cells fall by >= 5x or the reason
                 is written down.
measurement:     instructions (bough_metal::perf::tinstr), slopes as the harnesses compute them; no lock.
stop conditions: a shortcut that cannot be shown equal to the full computation is not landed.
do not revisit:  docs/SETTLED.md; Wire/settle/Signal code (lead); Node/instance columns (lane store).
```
