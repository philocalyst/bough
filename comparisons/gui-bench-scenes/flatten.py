#!/usr/bin/env python3
"""flatten.py RUN_DIR [OUT.tsv] : the heavy-scene cells of a gui-bench run as one TSV row per metric.

Columns: scene, framework, size, probe, metric, value, unit, valid, failed_checks. `valid` is the report's own
validity verdict (fakeland's checks: iterations, timeouts, wake lateness, host load...); `failed_checks` names the
checks that failed. Cells that stopped early add a row `probe=cell metric=error` with the error text in `unit`.

The metrics kept per probe:
  startup  first_present_ms (spawn to the first presented frame)
  idle     rss_max (bytes), cpu_ms_per_s, pss_max
  hover, theme, edit
           latency.total_p50 / total_p95 (input to presented frame, ms), latency.input_to_commit_p50 (the app's own
           share: input to its commit, ms), resources.cpu_ms (app CPU per measured iteration), frame_cost.cpu_ms_per_frame,
           frame_cost.damage_fraction_p50
  scroll   scroll_smoothness.smoothness_score, frame_pacing.fps, frame_cost.cpu_ms_per_frame, resources.cpu_ms_per_s
  resize   resize.configure_to_present_p50, frame_pacing.fps, frame_cost.cpu_ms_per_frame, resources.cpu_ms_per_s,
           resources.rss_max
"""
import json
import os
import sys

KEEP = {
    "idle": [("resources", "rss_max"), ("resources", "pss_max"), ("resources", "cpu_ms_per_s")],
    "hover": [("latency", "total_p50"), ("latency", "total_p95"), ("latency", "input_to_commit_p50"), ("resources", "cpu_ms"),
              ("frame_cost", "cpu_ms_per_frame"), ("frame_cost", "damage_fraction_p50")],
    "scroll": [("scroll_smoothness", "smoothness_score"), ("frame_pacing", "fps"), ("frame_cost", "cpu_ms_per_frame"),
               ("resources", "cpu_ms_per_s")],
    "resize": [("resize", "configure_to_present_p50"), ("frame_pacing", "fps"), ("frame_cost", "cpu_ms_per_frame"),
               ("resources", "cpu_ms_per_s"), ("resources", "rss_max")],
}
KEEP["theme"] = KEEP["hover"]
KEEP["edit"] = KEEP["hover"]
SCENES = ("sheet", "canvas", "ide", "dash", "chat")


def cells(run_dir):
    for scene in SCENES:
        base = os.path.join(run_dir, scene)
        if not os.path.isdir(base):
            continue
        for fw in sorted(os.listdir(base)):
            for root, _dirs, files in os.walk(os.path.join(base, fw)):
                if "cell.json" in files:
                    yield scene, fw, os.path.basename(root), root


def rows(run_dir):
    for scene, fw, size, d in cells(run_dir):
        cell = json.load(open(os.path.join(d, "cell.json")))
        for name, err in cell.get("errors", []):
            yield scene, fw, size, "cell", "error:" + name, "", err.replace("\t", " ").replace("\n", " ")[:300], False, ""
        sp = os.path.join(d, "startup.json")
        if os.path.exists(sp):
            st = json.load(open(sp))
            if st.get("first_present_ms") is not None:
                yield scene, fw, size, "startup", "first_present_ms", st["first_present_ms"], "ms", True, ""
        for probe, keep in KEEP.items():
            p = os.path.join(d, probe + ".json")
            if not os.path.exists(p):
                continue
            r = json.load(open(p))
            v = r.get("validity") or {}
            valid = bool(v.get("valid", False))
            bad = ",".join(c["name"] for c in v.get("checks", []) if c.get("status") == "fail")
            have = {}
            for sec in r.get("sections", []):
                for m in sec.get("metrics", []):
                    have[(sec["name"], m["name"])] = (m.get("median"), m.get("unit", ""))
            for key in keep:
                if key in have and have[key][0] is not None:
                    yield scene, fw, size, probe, ".".join(key), have[key][0], have[key][1], valid, bad


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = open(sys.argv[2], "w") if len(sys.argv) > 2 else sys.stdout
    out.write("scene\tframework\tsize\tprobe\tmetric\tvalue\tunit\tvalid\tfailed_checks\n")
    for r in rows(sys.argv[1]):
        out.write("\t".join(str(x) for x in r) + "\n")


if __name__ == "__main__":
    main()
