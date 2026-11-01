#!/usr/bin/env python3
"""Windowed adapters, 10 s runs: process CPU time per second, retired instructions per second and
phys_footprint, sampled from outside with proc_pid_rusage(RUSAGE_INFO_V4) at t=1.5 s and t=9 s
(startup and shutdown excluded). Usage: windowed.py <bough|zgx> <scene: heavy|lab|animate|blink> <mode> [seconds=10]
(mode is ignored by animate and blink; vlist: scroll|fling|jump). The adapter's own report line (ticks / animation_frames fps / blink_frames)
is printed last."""
import ctypes, os, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
# bough's dist binaries (built with CARGO_TARGET_DIR=target/fusion) and zgx's headless drivers (bench/zgx/build.sh)
BOUGH = os.environ.get("BOUGH_DIST", f"{ROOT}/target/fusion/dist")
ZGXH = os.environ.get("ZGX_HEADLESS", f"{ROOT}/target/zgx-headless")
ZGXBIN = os.environ.get("ZGX_DIST", "/Users/mileswirht/projects/_ref/zgx/target/dist/examples")
BIN = {("bough", "heavy"): f"{BOUGH}/heavy_workload", ("zgx", "heavy"): f"{ZGXBIN}/heavy_workload",
       ("bough", "lab"): f"{BOUGH}/lab_workload", ("zgx", "lab"): f"{ZGXBIN}/component_workload",
       ("bough", "animate"): f"{BOUGH}/animate_workload", ("zgx", "animate"): f"{ZGXBIN}/animation_workload",
       ("bough", "blink"): f"{BOUGH}/blink_workload", ("zgx", "blink"): f"{ZGXBIN}/blink_workload",
       ("bough", "vlist"): f"{BOUGH}/vlist_workload", ("zgx", "vlist"): f"{ZGXBIN}/vlist_workload"}
FONT = "/Users/mileswirht/projects/_ref/zgx/comparisons/fonts/DejaVuSans.ttf"

class V4(ctypes.Structure):
    _fields_ = [("ri_uuid", ctypes.c_uint8 * 16)] + [(n, ctypes.c_uint64) for n in (
        "ri_user_time ri_system_time ri_pkg_idle_wkups ri_interrupt_wkups ri_pageins ri_wired_size "
        "ri_resident_size ri_phys_footprint ri_proc_start_abstime ri_proc_exit_abstime ri_child_user_time "
        "ri_child_system_time ri_child_pkg_idle_wkups ri_child_interrupt_wkups ri_child_pageins "
        "ri_child_elapsed_abstime ri_diskio_bytesread ri_diskio_byteswritten ri_cpu_time_qos_default "
        "ri_cpu_time_qos_maintenance ri_cpu_time_qos_background ri_cpu_time_qos_utility ri_cpu_time_qos_legacy "
        "ri_cpu_time_qos_user_initiated ri_cpu_time_qos_user_interactive ri_billed_system_time "
        "ri_serviced_system_time ri_logical_writes ri_lifetime_max_phys_footprint ri_instructions ri_cycles "
        "ri_billed_energy ri_serviced_energy ri_interval_max_phys_footprint ri_runnable_time").split()]

libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
class TB(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]
tb = TB(); libc.mach_timebase_info(ctypes.byref(tb))

def snap(pid):
    r = V4()
    assert libc.proc_pid_rusage(pid, 4, ctypes.byref(r)) == 0
    return r

def run(fw, scene, mode, seconds=10, t0=1.5, t1=9.0):
    env = dict(os.environ, ZGX_MODE=mode, ZGX_SECONDS=str(seconds), ZGX_FONT_FILE=FONT)
    if fw == "bough":
        env["BOUGH_STATS"] = "1"  # the ring counters (frames, skipped, surfaces) on exit
    start = time.time()
    p = subprocess.Popen([BIN[(fw, scene)]], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(t0 - (time.time() - start)); a = snap(p.pid); ta = time.time()
    time.sleep(max(0, t1 - (time.time() - start))); b = snap(p.pid); tb_ = time.time()
    out, err = p.communicate()
    stats = "".join(l for l in err.splitlines() if l.startswith("surfaces:"))
    out = "\n".join(l for l in (out + "\n" + err).splitlines() if l.startswith("{") or "frames=" in l or "ticks" in l and not l.startswith("surfaces:"))
    cpu_ns = ((b.ri_user_time + b.ri_system_time) - (a.ri_user_time + a.ri_system_time)) * tb.numer / tb.denom
    dt = tb_ - ta
    ticks = out.strip().splitlines()[-1] if out.strip() else "?"
    return dict(fw=fw, scene=scene, mode=mode, cpu_pct=100 * cpu_ns / 1e9 / dt, instr_per_s=(b.ri_instructions - a.ri_instructions) / dt,
                footprint_mb=b.ri_phys_footprint / 2**20, peak_mb=b.ri_lifetime_max_phys_footprint / 2**20,
                resident_mb=b.ri_resident_size / 2**20, wakeups_per_s=(b.ri_pkg_idle_wkups - a.ri_pkg_idle_wkups) / dt, intr_wakeups_per_s=(b.ri_interrupt_wkups - a.ri_interrupt_wkups) / dt, report=ticks, stats=stats)

if __name__ == "__main__":
    r = run(sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4]) if len(sys.argv) > 4 else 10)
    print("{fw} {scene} {mode:7s} cpu {cpu_pct:6.2f}% of a core  instr/s {instr_per_s:14.0f}  footprint {footprint_mb:6.1f} MB (peak {peak_mb:6.1f})  rss {resident_mb:6.1f} MB  idle-wakeups/s {wakeups_per_s:6.1f}  interrupt-wakeups/s {intr_wakeups_per_s:6.1f}  {report}".format(**r), flush=True)
    if r["stats"]:
        print("   ", r["stats"], flush=True)
