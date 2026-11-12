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
           frame_cost.damage_fraction_p50; and the split from the pooled samples of every measured iteration:
           split.total_p50/p95, split.app_p50/p95 (input to the app's commit), split.after_p50/p95 (commit to presented
           frame: the compositor, the buffer's readiness, the wait for the next refresh), and fakeland's own medians
           of the after-commit parts (latency.commit_to_ready_p50, ready_to_latch_p50, latch_to_present_p50)
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
KEEP["hover"] += [("latency", "commit_to_ready_p50"), ("latency", "ready_to_latch_p50"), ("latency", "latch_to_present_p50")]
KEEP["theme"] = KEEP["hover"]
KEEP["edit"] = KEEP["hover"]
SCENES = ("sheet", "canvas", "ide", "dash", "chat")


def pct(xs, q):
    """Linear-interpolated percentile of a non-empty list."""
    xs = sorted(xs)
    if len(xs) == 1:
        return xs[0]
    k = (len(xs) - 1) * q
    f = int(k)
    c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def split(report):
    """p50 and p95 of total, app (input to commit) and after-commit times over the pooled samples of a latency probe."""
    for sec in report.get("sections", []):
        if sec.get("name") != "latency":
            continue
        samples = []
        for pooled in sec.get("pooled", []):
            samples += [x for x in pooled.get("report", {}).get("samples", []) if not x.get("timed_out")]
        samples = [x for x in samples if x.get("total_ms") is not None and x.get("input_to_commit_ms") is not None]
        if not samples:
            return {}
        tot = [x["total_ms"] for x in samples]
        app = [x["input_to_commit_ms"] for x in samples]
        aft = [x["total_ms"] - x["input_to_commit_ms"] for x in samples]
        out = {"split.samples": len(samples)}
        for name, xs in (("total", tot), ("app", app), ("after", aft)):
            out[f"split.{name}_p50"] = pct(xs, 0.5)
            out[f"split.{name}_p95"] = pct(xs, 0.95)
        return out
    return {}


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
            if probe in ("hover", "theme", "edit"):
                for name, v in split(r).items():
                    yield scene, fw, size, probe, name, v, "count" if name.endswith("samples") else "ms", valid, bad


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = open(sys.argv[2], "w") if len(sys.argv) > 2 else sys.stdout
    out.write("scene\tframework\tsize\tprobe\tmetric\tvalue\tunit\tvalid\tfailed_checks\n")
    for r in rows(sys.argv[1]):
        out.write("\t".join(str(x) for x in r) + "\n")


if __name__ == "__main__":
    main()
