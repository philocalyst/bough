#!/usr/bin/env python3
"""Build times, clean and incremental, for one cargo command under one configuration (bench/quick/README.md).

    build_matrix.py --name NAME --cmd "cargo build --release -p bough-heavy --bin heavy_headless" \
        [--clean] [--edits layout,core,heavy] [--reps 1] [--env K=V ...] [--target-dir DIR] [--out results.jsonl]

--clean removes the target's profile directory first (DIR/<profile dir>, or the whole DIR with --clean-all) and
times the cold build; each edit appends a fresh comment line to its file (so its content and mtime change),
times the rebuild, and leaves it; at the end every edited file is restored byte for byte and one unmeasured build
settles the target. Every record carries the load average before and after and cargo's own "Finished" time.
Edit sites: layout = crates/bough/src/layout.rs, core = crates/bough-core/src/bough.rs (bough-core's root module),
heavy = comparisons/bough-heavy/src/lib.rs (a comparison crate), bin = comparisons/bough-heavy/src/bin/heavy_headless.rs.
"""
import argparse, json, os, re, shlex, shutil, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SITES = {"layout": "crates/bough/src/layout.rs", "core": "crates/bough-core/src/bough.rs",
         "heavy": "comparisons/bough-heavy/src/lib.rs", "bin": "comparisons/bough-heavy/src/bin/heavy_headless.rs",
         "text": "crates/bough-text/src/lib.rs", "metal": "crates/bough-metal/src/fused.rs"}


def load():
    return [round(x, 1) for x in os.getloadavg()]


def run(cmd, env):
    t0 = time.time()
    l0 = load()
    p = subprocess.run(cmd, shell=True, cwd=ROOT, env=env, capture_output=True, text=True)
    dt = time.time() - t0
    m = re.search(r"Finished .* in ((\d+)m )?([\d.]+)s", p.stderr)
    cargo_s = (int(m.group(2) or 0) * 60 + float(m.group(3))) if m else None
    compiled = len(re.findall(r"^\s+Compiling ", p.stderr, re.M))
    return dict(wall_s=round(dt, 1), cargo_s=cargo_s, rc=p.returncode, compiled=compiled, load_before=l0, load_after=load(),
                tail=None if p.returncode == 0 else p.stderr[-1500:])


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--name", required=True)
    ap.add_argument("--cmd", required=True)
    ap.add_argument("--clean", action="store_true")
    ap.add_argument("--clean-all", action="store_true")
    ap.add_argument("--edits", default="")
    ap.add_argument("--reps", type=int, default=1)
    ap.add_argument("--env", action="append", default=[])
    ap.add_argument("--target-dir")
    ap.add_argument("--out")
    a = ap.parse_args()
    env = dict(os.environ)
    for kv in a.env:
        k, v = kv.split("=", 1)
        env[k] = v
    if a.target_dir:
        env["CARGO_TARGET_DIR"] = a.target_dir
    td = env.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
    recs = []

    def rec(kind, r):
        r.update(name=a.name, cmd=a.cmd, kind=kind, env={kv.split("=", 1)[0]: kv.split("=", 1)[1] for kv in a.env})
        recs.append(r)
        print(json.dumps(r), flush=True)
        if a.out:
            with open(a.out, "a") as f:
                f.write(json.dumps(r) + "\n")

    if a.clean or a.clean_all:
        words = shlex.split(a.cmd)
        prof = "release" if "--release" in words else (words[words.index("--profile") + 1] if "--profile" in words else "debug")
        prof = {"dev": "debug", "test": "debug", "bench": "release"}.get(prof, prof)
        victim = td if a.clean_all else os.path.join(td, prof)
        if os.path.isdir(victim):
            shutil.rmtree(victim)
        rec("clean", run(a.cmd, env))
    else:
        rec("warm-noop", run(a.cmd, env))
    originals = {}
    try:
        for k in range(a.reps):
            for site in [s for s in a.edits.split(",") if s]:
                path = os.path.join(ROOT, SITES[site])
                if path not in originals:
                    originals[path] = open(path, "rb").read()
                with open(path, "a") as f:
                    f.write(f"// quick-edit {time.time_ns()}\n")
                rec(f"edit:{site}", run(a.cmd, env))
    finally:
        for path, data in originals.items():
            with open(path, "wb") as f:
                f.write(data)
        if originals:
            r = run(a.cmd, env)
            print(json.dumps(dict(name=a.name, kind="settle (unmeasured)", **r)), flush=True)


if __name__ == "__main__":
    main()
