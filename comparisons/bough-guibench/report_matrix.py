#!/usr/bin/env python3
"""Turn private cell records into self-contained public tables without local paths/log transcripts."""
import argparse,json,re
from pathlib import Path

def reason(root,c):
    if c.get('mount_failure'):
        d=json.loads((root/(c['mount_failure']['log']+'.json')).read_text()); text=d['stderr']
    else:
        fail=next((g for g in c.get('gates',[]) if g['code']),None)
        if not fail:return 'pass'
        d=json.loads((root/(fail['log']+'.json')).read_text());text=d['stderr']
    if 'panicked at' in text:
        text=text.split('panicked at',1)[1].split('\n',1)[-1]
    text=text.split('note:')[0].split('\n')[0].strip()
    return re.sub(r'/Users/[^\s:]+','<local source>',text)[:180].replace('|','/') or str(d['code'])

def number(c,phase):
    r=c.get(phase) or {};m=r.get('median')
    if m is None:
        codes=[s['code'] for s in r.get('samples',[]) if s['code']]
        return 'failed '+str(codes[0]) if codes else 'mount failed'
    return f"{m:,.0f} [{r['low']:,.0f}, {r['high']:,.0f}]"

def render(root,title):
    root=Path(root);cells=json.loads((root/'cells.json').read_text());scales=json.loads((root/'scaling.json').read_text())
    out=[f'# {title}', '', 'Calling-thread instructions per operation: median [min, max] of three slopes, each from 8 and 32 operation/undo repeats. Oracle work excluded. R1 counts CPU frame submission/completion, not GPU instructions. Counts taken under concurrent load. A failed pixel cell is not a speed win.', '']
    groups={}
    for c in cells:
        key=(c['scenario'],c['variant'],c['plain'],c['caps'],tuple(c['extra']))
        groups.setdefault(key,[]).append(c)
    for key,cs in groups.items():
        out += [f"## {key[0]} / {key[1]} / {'plain' if key[2] else 'normal'} / {key[3]} {' '.join(key[4])}", '', '| rows | operation | R0 instructions | R1 instructions | exponent R0/R1 | ideal exponent | pixels / semantics |', '|---:|---|---:|---:|---|---:|---|']
        for c in cs:
            ex=[]
            for ph in ['R0','R1']:
                match=next((s for s in scales if s['key']==[key[0],key[1],c['op'],key[2],key[3],list(key[4]),ph]),{})
                ex.append(f"{match['exponent']:.2f} ({match['verdict']})" if 'exponent' in match else 'unsupported range')
            ideal=int(c['op'] in ('update10','replace','clear','filter','front-churn','moves','signals','fanout'))
            out.append(f"| {c['rows']:,} | {c['op']} | {number(c,'R0')} | {number(c,'R1')} | {' / '.join(ex)} | {ideal} | {reason(root,c)} |")
        out += ['']
    out += ['Gate failures remain recorded even when instruction samples succeed. Missing slopes mean the process failed or timed out; they are not zeros. See the main report for mechanisms and scope.','']
    return '\n'.join(out)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('root');p.add_argument('output');p.add_argument('--title',default='Round 6 instruction and correctness cells');a=p.parse_args()
    Path(a.output).write_text(render(a.root,a.title))
