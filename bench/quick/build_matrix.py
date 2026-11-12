#!/usr/bin/env python3
"""Build times, clean and incremental, for cargo commands under several configurations, interleaved
(bench/quick/README.md "Build times").

    build_matrix.py --cmd "release=cargo build --release -p bough-heavy --bin heavy_headless" \
        [--cmd "gate=cargo build --profile gate -p bough-heavy --bin heavy_headless"] \
        [--clean] [--edits layout,core,heavy] [--reps 1] [--env K=V ...] [--out results.jsonl]

Each --cmd is NAME=COMMAND (the command may set its own environment with `env K=V cargo ...`). First every command
runs once: cold after --clean (the target's profile directory removed; --clean-all removes the target), else as a
warm no-op. Then for each rep and each edit site, for each command in turn: the site gets a fresh public function
appended (its content, metadata and mtime change: a comment-only edit lets incremental check/dev skip everything)
and the command is timed. Commands alternate, so a slow minute of the shared machine lands on all of them. At the
end every edited file is restored byte for byte and each command runs once more, unmeasured, to settle its target.
Every record carries the load average before and after, cargo's own "Finished" time and the units it compiled.
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
    compiled = len(re.findall(r"^\s+(Compiling|Checking) ", p.stderr, re.M))
    tests = re.findall(r"test result: (\w+)\. (\d+) passed; (\d+) failed", p.stdout)
    return dict(wall_s=round(dt, 1), cargo_s=cargo_s, rc=p.returncode, compiled=compiled, load_before=l0, load_after=load(),
                tests=tests or None, tail=None if p.returncode == 0 else (p.stderr + p.stdout)[-1500:])


def profile_dir(cmd):
    words = shlex.split(cmd)
    prof = "release" if "--release" in words else (words[words.index("--profile") + 1] if "--profile" in words else "debug")
    return {"dev": "debug", "test": "debug", "bench": "release"}.get(prof, prof)


def target_of(cmd, env):
    m = re.search(r"CARGO_TARGET_DIR=(\S+)", cmd)
    return m.group(1) if m else env.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--cmd", action="append", required=True)
    ap.add_argument("--clean", action="store_true")
    ap.add_argument("--clean-all", action="store_true")
    ap.add_argument("--edits", default="")
    ap.add_argument("--reps", type=int, default=1)
    ap.add_argument("--env", action="append", default=[])
    ap.add_argument("--out")
    a = ap.parse_args()
    env = dict(os.environ)
    for kv in a.env:
        k, v = kv.split("=", 1)
        env[k] = v
    cmds = [c.split("=", 1) for c in a.cmd]

    def rec(name, cmd, kind, r):
        r.update(name=name, cmd=cmd, kind=kind)
        print(json.dumps(r), flush=True)
        if a.out:
            with open(a.out, "a") as f:
                f.write(json.dumps(r) + "\n")

    for name, cmd in cmds:
        if a.clean or a.clean_all:
            td = target_of(cmd, env)
            victim = td if a.clean_all else os.path.join(td, profile_dir(cmd))
            if os.path.isdir(victim):
                shutil.rmtree(victim)
            rec(name, cmd, "clean", run(cmd, env))
        else:
            rec(name, cmd, "warm-noop", run(cmd, env))
    originals = {}
    try:
        for k in range(a.reps):
            for site in [s for s in a.edits.split(",") if s]:
                path = os.path.join(ROOT, SITES[site])
                if path not in originals:
                    originals[path] = open(path, "rb").read()
                for name, cmd in cmds:
                    with open(path, "a") as f:
                        n = time.time_ns()
                        f.write(f"#[allow(dead_code)]\n#[doc(hidden)]\npub fn quick_edit_{n}() -> u64 {{\n    std::hint::black_box({n % 1000003})\n}}\n")
                    rec(name, cmd, f"edit:{site}", run(cmd, env))
    finally:
        for path, data in originals.items():
            with open(path, "wb") as f:
                f.write(data)
        if originals:
            for name, cmd in cmds:
                r = run(cmd, env)
                print(json.dumps(dict(name=name, kind="settle (unmeasured)", wall_s=r["wall_s"], rc=r["rc"])), flush=True)


if __name__ == "__main__":
    main()
