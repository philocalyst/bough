#!/usr/bin/env python3
"""Run the existing slope/pair instruments, retaining every child output.
Caller owns the coordinator measurement lock; all paths are environment/arguments.
Usage: run_measurements.py slopes|pairs|timing|stats|cpu OUTPUT_DIR
BOUGH_DIST, ZGX_DIST, ZGX_HEADLESS, BOUGH_REF, FLOOR_TOOLS select frozen artifacts.
R6_TIMING/R6_STATS select diagnostic directories. No build occurs here.
"""
import json, os, pathlib, re, subprocess, sys, time
root = pathlib.Path.cwd()
out = pathlib.Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
lock = pathlib.Path(os.environ['BOUGH_MEASURE_LOCK'])
assert lock.is_dir() and (lock/'owner').read_text().splitlines()[0] == 'gpu'
sys.path[:0] = [str(root/'bench/floor'), str(root/'bench/integrate')]
import gpu_slope, gpu_pairs, slope
cells = [(s,m) for s in ('heavy','lab') for m in ('stream','scroll','both')] + [('animate','-')] + [('vlist',m) for m in ('scroll','fling','jump')]
phase = sys.argv[1]
original_run = subprocess.run
serial = 0
def is_heavy(line):
    name = pathlib.Path(line.split()[-1]).name
    return name in ('rustc','clang','clang++','ninja','cc1','kernel_bench') or name.endswith('_headless') or name.startswith(('correctness-', 'gpu_bounds-', 'present-'))
def guarded_run(args, **kw):
    global serial
    # Keep result metadata and all child output. Reject silent process failures.
    serial += 1
    processes = original_run(['ps','-axo','pid,pcpu,comm'],capture_output=True,text=True).stdout
    (out/f'load-{serial:04d}-before.txt').write_text(processes)
    heavy = [line for line in processes.splitlines() if is_heavy(line)]
    assert not heavy, ('heavy work during measurement', heavy)
    p = original_run(args, **kw)
    (out/f'child-{serial:04d}.json').write_text(json.dumps({'args':args, 'returncode':p.returncode,'stdout':p.stdout,'stderr':p.stderr}))
    after = original_run(['ps','-axo','pid,pcpu,comm'],capture_output=True,text=True).stdout
    (out/f'load-{serial:04d}-after.txt').write_text(after)
    assert not [line for line in after.splitlines() if is_heavy(line)], 'heavy work started during measurement'
    assert p.returncode == 0, (args, p.returncode, p.stderr)
    if phase in ('slopes','pairs'):
        m = re.search(r'gpu_seconds=([0-9.]+) clients=(\d+) gpu_mhz=([0-9.]+)',p.stderr)
        assert m and float(m[1]) > 0 and int(m[2]) > 0 and float(m[3]) > 0, ('missing GPU accounting',p.stderr)
    return p
subprocess.run = guarded_run
if phase == 'slopes':
    for scene,mode in cells:
        r = gpu_slope.slope('bough',scene,mode,3)
        print(json.dumps({'scene':scene,'mode':mode,'ms_med_min_max':r[:3],'kc_med_min_max':r[3:]}),flush=True)
elif phase == 'pairs':
    A,B = os.environ['A'], os.environ['B']
    for scene,mode in cells:
        for k in range(5):
            order = (A,B) if k%2==0 else (B,A)
            r = {s:gpu_pairs.one(s,scene,mode) for s in order}
            print(json.dumps({'scene':scene,'mode':mode,'pair':k,'a':r[A],'b':r[B],'ms_ratio':r[B][0]/r[A][0],'kc_ratio':r[B][1]/r[A][1]}),flush=True)
elif phase in ('timing','stats'):
    dist = pathlib.Path(os.environ['R6_'+phase.upper()])
    for scene,mode in cells:
        env = dict(os.environ,R6_PROBE='1',ZGX_MODE=mode,TICKS='200',FRAMES='200',RENDER='1',ZGX_FONT_FILE=slope.FONT)
        binary = dist/pathlib.Path(slope.BIN['bough',scene]).name
        p = guarded_run([str(binary)],env=env,capture_output=True,text=True)
        (out/f'{scene}-{mode}.txt').write_text(p.stdout+'\n'+p.stderr)
        print(json.dumps({'scene':scene,'mode':mode,'returncode':p.returncode}),flush=True)
elif phase == 'cpu':
    for render in (0,1):
        for scene,mode in cells:
            r = slope.slope('bough',scene,mode,render,3)
            print(json.dumps({'scene':scene,'mode':mode,'render':render,'instr_med_min_max':r}),flush=True)
else: raise ValueError(phase)
