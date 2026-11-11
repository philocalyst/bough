#!/usr/bin/env python3
"""The memory ledger, measured (lane memory, round 6; card docs/lanes/memory/cards/2026-11-11-ledger.md).

Usage: ledger.py <outdir> <case>... [--reps N] [--at 3,9] [--unmapped] [--no-lock]

A case is `counter`, `scroll:<rows>`, `dashboard` (the ledger harness, `comparisons/bough-awagy/examples/ledger.rs`:
6 s of in-process driving, exit at 10 s) or `heavy` (`heavy_workload`, ZGX_MODE=both, 10 s). For each run:

* the app writes its in-process ledger (`BOUGH_LEDGER`, one JSON line per `--at` second);
* the moment a line appears, this script runs `footprint -j` (categories: dirty / clean / swapped / wired /
  reclaimable; `--unmapped` adds the memory the process owns but has not mapped, e.g. the GPU driver's) and
  `vmmap --summary` on the live process;
* everything is kept raw under <outdir>/<case>-<rep>/ (ledger.jsonl, footprint-<k>.json, vmmap-<k>.txt, stderr).

Windowed runs put load on the GPU and the compositor, so the batch holds the measurement lock
(`BOUGH_LOCK`, default `<repo>/runtime/measure.lock`; a lane worktree points it at the main checkout's: a
directory holding a file `lane`): it waits while
another lane holds it. Binaries: $LEDGER_BIN (default target/release/examples/ledger) and $HEAVY_BIN (default
target/release/heavy_workload); the zgx font for heavy from $BOUGH_REF (default ~/projects/_ref).
"""
import json, os, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TARGET = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
LEDGER_BIN = os.environ.get("LEDGER_BIN", os.path.join(TARGET, "release/examples/ledger"))
HEAVY_BIN = os.environ.get("HEAVY_BIN", os.path.join(TARGET, "release/heavy_workload"))
FONT = os.path.expanduser(os.environ.get("BOUGH_REF", "~/projects/_ref")) + "/zgx/comparisons/fonts/DejaVuSans.ttf"
LOCK = os.environ.get("BOUGH_LOCK", os.path.join(ROOT, "runtime", "measure.lock"))


def take_lock():
    while True:
        try:
            os.mkdir(LOCK)
            with open(os.path.join(LOCK, "lane"), "w") as f:
                f.write("memory")
            return
        except FileExistsError:
            try:
                who = open(os.path.join(LOCK, "lane")).read().strip()
            except OSError:
                who = "?"
            print(f"  measurement lock held by {who!r}; waiting", flush=True)
            time.sleep(15)


def drop_lock():
    try:
        os.remove(os.path.join(LOCK, "lane"))
    except OSError:
        pass
    os.rmdir(LOCK)


def run(case, rep, outdir, at, unmapped):
    d = os.path.join(outdir, f"{case.replace(':', '-')}-{rep}")
    os.makedirs(d, exist_ok=True)
    led = os.path.join(d, "ledger.jsonl")
    if os.path.exists(led):
        os.remove(led)
    env = dict(os.environ, BOUGH_LEDGER=led, BOUGH_LEDGER_AT=",".join(str(a) for a in at))
    if case == "heavy":
        env.update(ZGX_MODE="both", ZGX_SECONDS="10", ZGX_FONT_FILE=FONT)
        cmd = [HEAVY_BIN]
    else:
        kind, _, rows = case.partition(":")
        if rows:
            env["SCROLL_ROWS"] = rows
        cmd = [LEDGER_BIN, kind, "6", "10"]
    err = open(os.path.join(d, "stderr.txt"), "w")
    out = open(os.path.join(d, "stdout.txt"), "w")
    p = subprocess.Popen(cmd, env=env, stdout=out, stderr=err)
    seen = 0
    t0 = time.time()
    while p.poll() is None and time.time() - t0 < 30:
        n = sum(1 for _ in open(led)) if os.path.exists(led) else 0
        if n > seen:
            seen = n
            fp = os.path.join(d, f"footprint-{n}.json")
            args = ["footprint", "-j", fp, "-f", "bytes", "--wired", "--swapped"] + (["--unmapped"] if unmapped else []) + [str(p.pid)]
            subprocess.run(args, capture_output=True)
            with open(os.path.join(d, f"vmmap-{n}.txt"), "w") as f:
                subprocess.run(["vmmap", "--summary", str(p.pid)], stdout=f, stderr=subprocess.DEVNULL)
        time.sleep(0.05)
    if p.poll() is None:
        p.kill()
    p.wait()
    err.close()
    out.close()
    return d


def main():
    a = sys.argv[1:]
    reps, at, unmapped, lock = 3, [3.0, 9.0], False, True
    if "--reps" in a:
        i = a.index("--reps"); reps = int(a[i + 1]); del a[i:i + 2]
    if "--at" in a:
        i = a.index("--at"); at = [float(x) for x in a[i + 1].split(",")]; del a[i:i + 2]
    if "--unmapped" in a:
        a.remove("--unmapped"); unmapped = True
    if "--no-lock" in a:
        a.remove("--no-lock"); lock = False
    outdir, cases = a[0], a[1:]
    os.makedirs(outdir, exist_ok=True)
    if lock:
        take_lock()
    try:
        for rep in range(reps):
            for c in cases:
                d = run(c, rep, outdir, at, unmapped)
                print(f"{c} rep {rep}: {d}", flush=True)
    finally:
        if lock:
            drop_lock()


if __name__ == "__main__":
    main()
