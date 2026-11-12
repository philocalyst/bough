#!/usr/bin/env python3
"""to_dashboard.py RESULT.tsv [DATA.json] : put a workloads result file (bench/scenes/run.sh or baseline.sh output)
into the dashboard's data.json as its "workloads" section, then rebuild the page.

The result file's identity lines (`# host=...`, `# commit=...`, `# env ...`) become the section's provenance; each
scene's `# scene=... model="..."` line its model; every phase row its numbers. Run from anywhere:
    python3 bench/scenes/to_dashboard.py bench/results/r7/workloads/<dir>/once-render.tsv
"""
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
DEFAULT_DATA = os.path.join(ROOT, "bench", "r6", "dashboard", "data.json")


def parse(path):
    ident, scenes, cur, header = [], [], None, None
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line:
            continue
        if line.startswith("# scene="):
            m = re.search(r'scene=(\S+) model="([^"]*)" fanout=(\d+)', line)
            cur = {"scene": m.group(1), "model": m.group(2), "fanout": int(m.group(3)), "phases": [], "settings": line[2:]}
            scenes.append(cur)
        elif line.startswith("# records=") and cur is not None:
            cur["records"] = line[2:]
        elif line.startswith("#step"):
            continue
        elif line.startswith("#"):
            ident.append(line.lstrip("# ").strip())
        elif line.startswith("scene\t"):
            header = line.split("\t")
        elif header and cur is not None and line.split("\t")[0] == cur["scene"]:
            f = dict(zip(header, line.split("\t")))
            num = lambda k, t=float: t(f[k]) if f.get(k, "-") not in ("-", "") else 0
            cur["phases"].append({
                "phase": f["phase"], "steps": num("steps", int),
                "instr_p50": num("instr_p50", int), "instr_p95": num("instr_p95", int),
                "wall_us_p50": num("wall_us_p50"), "wall_us_p95": num("wall_us_p95"),
                "footprint_mib": num("footprint_mib"), "peak_mib": num("peak_mib"),
            })
        elif "panicked" in line and cur is not None:
            cur["failed"] = line.strip()[:160]
    return ident, scenes


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    res = sys.argv[1]
    data_path = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_DATA
    ident, scenes = parse(res)
    for s in scenes:
        s["note"] = (s.get("records", "") + "; fan-out " + f'{s["fanout"]:,}').strip("; ")
    with open(data_path, encoding="utf-8") as f:
        data = json.load(f)
    data["workloads"] = {
        "note": "The five standing workloads (docs/lanes/bench/SCENES.md): instructions per step on the calling thread "
                "(p50) by phase, and wall time per step (p50), from one run. Bars share one log scale across scenes. "
                "Instructions are the stable column; wall time on the shared Mac moves with its load.",
        "identity": " · ".join(ident),
        "source": os.path.relpath(res, ROOT),
        "scenes": scenes,
    }
    with open(data_path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=1, ensure_ascii=False)
        f.write("\n")
    subprocess.run([sys.executable, os.path.join(os.path.dirname(data_path), "build.py")], check=True)


if __name__ == "__main__":
    main()
