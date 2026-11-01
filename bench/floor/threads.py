#!/usr/bin/env python3
"""Per-thread retired instructions of a running process between two instants (proc_pidinfo
PROC_PIDTHREADCOUNTS, xnu's per-thread counters by performance level; no root for own-user processes).
Usage: threads.py <pid> <t0> <t1> [frames]: prints each thread's instructions per second (and per frame if
the frame count over the window is given), largest first."""
import ctypes, sys, time
libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
libc.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]

def snapshot(pid):
    ids = (ctypes.c_uint64 * 1024)()
    n = libc.proc_pidinfo(pid, 28, 0, ids, ctypes.sizeof(ids))
    out = {}
    for tid in ids[: max(n, 0) // 8]:
        info = (ctypes.c_uint8 * 112)()
        name = ""
        if libc.proc_pidinfo(pid, 15, tid, info, 112) > 0:
            name = bytes(info[48:112]).split(b"\0")[0].decode(errors="replace")
        buf = (ctypes.c_uint64 * 21)()
        if libc.proc_pidinfo(pid, 34, tid, buf, ctypes.sizeof(buf)) <= 0:
            continue
        levels = buf[0] & 0xFFFF
        out[tid] = (name, sum(buf[1 + 5 * l] for l in range(min(levels, 4))))
    return out

def diff(a, b):
    rows = [(b[t][0] or "?", t, b[t][1] - a.get(t, ("", 0))[1]) for t in b]
    return sorted(rows, key=lambda r: -r[2])

if __name__ == "__main__":
    pid, t0, t1 = int(sys.argv[1]), float(sys.argv[2]), float(sys.argv[3])
    frames = float(sys.argv[4]) if len(sys.argv) > 4 else 0
    start = time.time()
    time.sleep(max(0, t0)); a = snapshot(pid); ta = time.time()
    time.sleep(max(0, t1 - t0)); b = snapshot(pid); dt = time.time() - ta
    tot = 0
    for name, tid, d in diff(a, b):
        tot += d
        if d / dt > 1e5:
            print(f"{name[:40]:40s} #{tid % 100000:5d} {d / dt / 1e6:8.2f} M/s" + (f" {d / (frames * dt) / 1e3:8.1f} K/frame" if frames else ""))
    print(f"{'total':40s}        {tot / dt / 1e6:8.2f} M/s" + (f" {tot / (frames * dt) / 1e3:8.1f} K/frame" if frames else ""))
