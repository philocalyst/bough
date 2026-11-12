# bough's heavy scenes in gui-bench

The standing workloads (`docs/lanes/bench/SCENES.md`) ported to gui-bench (github.com/zortax/gui-bench, commit
0fd4510) so bough and other frameworks run the same scene under the same harness: fakeland's probes on Linux
(ilo-prod), one session, validity recorded. Round 7, bench lane.

* `overlay/apps/shared/scenes.rs`: the shared spec (geometry, colours, data, keys, command line). Every port draws
  from it; `overlay/runner/src/scenes.rs` mirrors what the runner probes (layout check points, the hover target,
  the regions each probe watches) and the measurements: idle, hover (the fan-out), theme, edit, scroll (sheet),
  resize.
* `overlay/`: everything copied over gui-bench 0fd4510 (plus the platform lane's `apps/bough` registration): the
  runner's scene kinds, each framework's port (`apps/{iced,egui,gpui-kit}/src/scenes/`), bough's app
  (`apps/bough`, which runs `comparisons/bough-guibench-scenes` for the scenes and the platform lane's
  `comparisons/bough-guibench` for gui-bench's own scenarios).
* `DEVIATIONS-<fw>.md`: per framework, the features used for each part of the spec and every deviation.
* `PORT-BRIEF.md`: what the porting agents followed.
* `sync.sh`, `remote.sh`: copy the overlay to the remote checkout; build an app and run one quick cell.
* `flatten.py`: a run's scene cells as metric rows with each report's validity.

Status (2026-11-12): sheet and canvas ported to iced, egui, gpui-kit and bough; ide, dash and chat not yet.
