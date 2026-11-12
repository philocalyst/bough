#!/usr/bin/env python3
"""Round-6 calling-thread instruction matrix. Oracle subprocesses are never timed samples."""
import argparse, hashlib, json, math, os, re, statistics, subprocess, time
from pathlib import Path
from run import scaling

class Matrix:
    def __init__(self, binary, out):
        self.binary = str(Path(binary).resolve()); self.out=Path(out); self.out.mkdir(parents=True,exist_ok=False)
        self.cells=[]; self.serial=0
        (self.out/'provenance.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
            'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'counter':'calling thread thread_selfcounts',
            'repeats':[8,32],'replicates':3,'under_load':True},indent=2))
    def run(self, args, caps='default', timeout=120):
        self.serial+=1; ident=f'{self.serial:05d}'
        env=dict(os.environ, GUIBENCH_CAPS=caps, GUIBENCH_FAILURE_DIR=str(self.out/ident))
        cmd=[self.binary,*args]; start=time.monotonic()
        try:
            p=subprocess.run(cmd,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,timeout=timeout)
            result=dict(code=p.returncode,stdout=p.stdout,stderr=p.stderr)
        except subprocess.TimeoutExpired as e:
            conv=lambda x: x.decode(errors='replace') if isinstance(x,bytes) else (x or '')
            result=dict(code='timeout',stdout=conv(e.stdout),stderr=conv(e.stderr))
        result.update(command=cmd,caps=caps,elapsed_context_only=time.monotonic()-start)
        (self.out/(ident+'.json')).write_text(json.dumps(result,indent=2)+'\n')
        result['log']=ident
        return result
    def cell(self, scenario, variant, n, op, plain=False, caps='default', extra=(), rings=(1,2,3,4), mount_failure=None):
        base=['--scenario',scenario,'--variant',variant,'--rows',str(n),'--op',op,*extra]+(['--plain'] if plain else [])
        c=dict(scenario=scenario,variant=variant,rows=n,op=op,plain=plain,caps=caps,extra=list(extra))
        if mount_failure:
            c.update(mount_failure=mount_failure,gate='mount failure',R0=None,R1=None)
        else:
            gates=[]
            for ring in rings:
                r=self.run(base+['--gate','--ring',str(ring),'--repeats','4'],caps)
                err=r['stderr'].split("panicked",1)[-1].split('note:')[0][-800:] if r['code'] else ''
                gates.append(dict(ring=ring,code=r['code'],log=r['log'],error=err))
            c['gates']=gates
            for phase in ['R0','R1']:
                samples=[]; slopes=[]
                for rep in range(3):
                    totals={}
                    for repeats in ([8,32] if rep!=1 else [32,8]):
                        r=self.run(base+['--repeats',str(repeats)]+(['--render'] if phase=='R1' else []),caps)
                        m=re.search(r'result .*? instructions=(\d+)',r['stdout'])
                        samples.append(dict(repeats=repeats,code=r['code'],log=r['log'],instructions=int(m[1]) if m else None))
                        if r['code']==0 and m: totals[repeats]=int(m[1])
                    if len(totals)==2: slopes.append((totals[32]-totals[8])/48) # per operation, two per repeat
                c[phase]=dict(samples=samples,slopes=slopes,median=statistics.median(slopes) if len(slopes)==3 else None,
                    low=min(slopes) if slopes else None,high=max(slopes) if slopes else None)
        self.cells.append(c); self.save()
        med=lambda ph: (c.get(ph) or {}).get('median')
        print(scenario,variant,n,op,'plain' if plain else 'scroll',caps,'R0',med('R0'),'R1',med('R1'),
              'gate',c.get('gate') or [x['code'] for x in c['gates']],flush=True)
    def save(self):
        groups={}
        for c in self.cells:
            for phase in ['R0','R1']:
                r=c.get(phase) or {}; med=r.get('median')
                if med is not None and med>0:
                    key=(c['scenario'],c['variant'],c['op'],c['plain'],c['caps'],tuple(c['extra']),phase)
                    groups.setdefault(key,[]).append((c['rows'],med))
        exponents=[]
        for key,points in groups.items():
            ideal=int(key[2] in ('update10','replace','clear','filter','front-churn','moves','signals','fanout'))
            if all(n>0 for n,y in points):
                exponents.append(dict(key=key,**scaling(points,ideal)))
        (self.out/'cells.json').write_text(json.dumps(self.cells,indent=2)+'\n')
        (self.out/'scaling.json').write_text(json.dumps(exponents,indent=2)+'\n')


def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--out',required=True)
    p.add_argument('--batch',choices=['table','rest','adversarial'],default='table')
    p.add_argument('--sizes',default='10,100,1000,5000,10000');p.add_argument('--rings',default='1,2,3,4')
    a=p.parse_args();m=Matrix(a.binary,a.out);sizes=list(map(int,a.sizes.split(',')));rings=tuple(map(int,a.rings.split(',')))
    if a.batch=='table':
        configs=[]
        for n in sizes:
            configs += [('rows','fine',n,False,'default')]
            for variant in ['fine','coarse']:
                for plain in [False,True]: configs.append(('table',variant,n,plain,'default'))
        configs += [('table','fine',n,plain,'raised-spaces') for n in sizes if n in (5000,10000) for plain in (False,True)]
        for scenario,variant,n,plain,caps in configs:
            mount=m.run(['--scenario',scenario,'--variant',variant,'--rows',str(n),'--op','mount','--repeats','0']+(['--plain'] if plain else []),caps)
            failure=dict(code=mount['code'],log=mount['log'],error=mount['stderr'][-1000:]) if mount['code'] else None
            ops=['hover','scroll'] if scenario=='rows' else ['select','update10','swap','remove','insert','replace','append','truncate','clear','replace-select']
            for op in ops:
                if n==10 and op=='scroll': continue
                m.cell(scenario,variant,n,op,plain,caps,rings=rings,mount_failure=failure)
        for n in sizes:
            if n<=1000:
                m.cell('table','fine',n,'select',extra=['--mutant'],rings=rings)
    else:
        raise SystemExit('batch implementation follows scenario sources')
if __name__=='__main__':main()
