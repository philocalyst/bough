#!/usr/bin/env python3
"""Run from repository root after building core_bench. Prints the three baseline contract failures."""
import os
import subprocess
from pathlib import Path

root=Path.cwd()
common=Path(subprocess.check_output(['git','rev-parse','--git-common-dir'],text=True).strip()).resolve()
lock=common.parent/'runtime/measure.lock'
assert not lock.exists(), 'Wait for the shared measurement lock before compiling the witness'
target=Path(os.environ.get('CARGO_TARGET_DIR', root/'target'))
libs=list((target/'release').rglob('libbough_core-*.rlib'))
assert len(libs)==1, 'Use the lane default-feature target, with exactly one core library'
lib=libs[0]
args=['rustc','--edition=2024','-C','lto=thin','-C','opt-level=3',
      'docs/lanes/core/witness-r6.rs','--extern','bough_core='+str(lib)]
# The pinned Cargo emits separate rmeta; older layouts embed metadata in the rlib.
if lib.with_suffix('.rmeta').exists(): args+=['--extern','bough_core='+str(lib.with_suffix('.rmeta'))]
for directory in sorted({p.parent for p in (target/'release').rglob('*.rlib')}):
    args+=['-L','dependency='+str(directory)]
out=root/'runtime/lanes/core/witness'
out.parent.mkdir(parents=True,exist_ok=True)
subprocess.run(args+['-o',str(out)],check=True)
subprocess.run([str(out)],check=True)
