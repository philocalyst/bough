#!/usr/bin/env python3
"""Windowed run sampled by zgx's own sampler (bench/windowed/macsample.swift, built by build.sh): process CPU %,
instructions, footprint, and GPU time per process from the I/O Registry, the GPU's mean active clock from
IOReport (GPU Mcycles/s = GPU seconds x MHz / wall, zgx's suite metric), and the window server's CPU and GPU
over the same seconds (it composites every app: noisy while the desktop is busy). No root, no Screen
Recording. Holds the windowed lock for the run. Usage: windowed_gpu.py <bough|zgx> <scene> <mode> [seconds=10]
(scenes as bench/integrate/windowed.py). Prints one JSON line."""
import json, os, subprocess, sys, time
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "integrate"))
from windowed import BIN, FONT

TOOLS = os.environ.get("FLOOR_TOOLS", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "bough", "target", "floor-tools"))
LOCK = "/private/tmp/claude-501/bough-windowed.lock"

def run(fw, scene, mode, seconds=10.0, t0=1.5, t1=9.0):
    env = dict(os.environ, ZGX_MODE=mode, ZGX_SECONDS=str(seconds), ZGX_FONT_FILE=FONT)
    if fw == "bough":
        env["BOUGH_STATS"] = "1"
    while True:
        try:
            os.mkdir(LOCK); break
        except FileExistsError:
            time.sleep(2)
    try:
        start = time.time()
        p = subprocess.Popen([BIN[(fw, scene)]], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        s = subprocess.run([os.path.join(TOOLS, "macsample"), str(p.pid), str(t0 - (time.time() - start)), str(t1 - t0), "0.25"],
                           capture_output=True, text=True)
        out, err = p.communicate()
    finally:
        os.rmdir(LOCK)
    r = json.loads(s.stdout)
    report = [l for l in (out + "\n" + err).splitlines() if l.startswith("{") or "frames=" in l or "ticks" in l]
    stats = [l for l in err.splitlines() if l.startswith("surfaces:")]
    w = r["wall_seconds"]
    return dict(fw=fw, scene=scene, mode=mode, cpu_pct=r["cpu_percent_one_core"], instr_per_s=r["instructions"] / w,
                footprint_mb=r["mean_footprint_bytes"] / 2**20, gpu_ms_per_s=r["gpu_seconds"] / w * 1e3,
                gpu_mhz=r["gpu_active_mhz"], gpu_mcyc_per_s=r["gpu_mcycles_per_second"], gpu_util_all=r["gpu_utilization_percent"],
                ws_cpu=r["windowserver_cpu_percent"], ws_gpu=r["windowserver_gpu_percent"], gpu_w=r["gpu_system_watts"],
                report=report[-1] if report else "", stats=stats[-1] if stats else "")

if __name__ == "__main__":
    r = run(sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4]) if len(sys.argv) > 4 else 10)
    print(json.dumps(r), flush=True)
