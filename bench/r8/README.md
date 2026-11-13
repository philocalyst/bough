# Round 8 benchmark material

Scripts, patches, tools, raw tables and the dashboard generator used to run zortax's gui-bench against the
frameworks plus bough (the façade) and bough-x (the new core). Host names and remote paths in the scripts are the
ones of the machines used; adapt them. `patches/gb-timing.patch` adds `GB_TIMING=fixed|unthrottled|vrr:48-144`
to the runner, `patches/gui-bench-bough-x.patch` registers both bough binaries.

`results/cmp-final-fixed` (60 Hz latch) and `results/cmp-final-unthrottled` are the final runs of 2026-11-13 from
commit `cc65ee215f`: identity, load log, runner log, checker output and the flattened tables (cells, metrics,
validity) as compressed archives. `dashboard/explorer/` holds the static pages of the upstream explorer built over
those runs (the published artifact folds them into one page, `dashboard/build_single.py`).
