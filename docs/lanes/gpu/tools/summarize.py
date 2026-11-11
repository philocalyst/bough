#!/usr/bin/env python3
"""Produce path-free public measurement tables from the private R6 logs."""
import csv,json,pathlib,re,statistics,sys
src=pathlib.Path(sys.argv[1]); dst=pathlib.Path(sys.argv[2]); dst.mkdir(parents=True,exist_ok=True)
def write(name,rows):
    if not rows:return
    with (dst/name).open('w') as f:
        w=csv.DictWriter(f,fieldnames=list(rows[0])); w.writeheader();w.writerows(rows)
def jsonlines(name):
    p=src/name
    return [json.loads(x) for x in p.read_text().splitlines() if x.startswith('{')] if p.exists() else []
slopes=jsonlines('slopes.jsonl')
write('slopes.csv',[dict(scene=r['scene'],mode=r['mode'],**{k:v for key,keys in [('ms_med_min_max',['ms_median','ms_min','ms_max']),('kc_med_min_max',['kc_median','kc_min','kc_max'])] for k,v in zip(keys,r[key])}) for r in slopes])
pairs=jsonlines('pairs.jsonl')
write('pairs.csv',[dict(scene=r['scene'],mode=r['mode'],pair=r['pair'],a_ms=r['a'][0],b_ms=r['b'][0],a_kc=r['a'][1],b_kc=r['b'][1],a_effective_mhz=r['a'][2],b_effective_mhz=r['b'][2],ms_ratio=r['ms_ratio'],kc_ratio=r['kc_ratio']) for r in pairs])
groups={}
for r in pairs:groups.setdefault((r['scene'],r['mode']),[]).append(r)
write('pairs-summary.csv',[dict(scene=s,mode=m,**{key+'_'+stat:fn([r[key] for r in rr]) for key in ('ms_ratio','kc_ratio') for stat,fn in [('median',statistics.median),('min',min),('max',max)]}) for (s,m),rr in groups.items()])
raw=[]; tiles=[]
for phase in ('timing','stats'):
    for p in sorted((src/phase).glob('*.txt')):
        scene,mode=p.stem.split('-',1)
        for line in p.read_text().splitlines():
            if not line.startswith(('R6_TIME ','R6_TILE ')):continue
            fields=dict(re.findall(r'(\w+)=([^ ]+)',line))
            row={'scene':scene,'mode':mode}
            row.update({k:float(v) if k=='ms' else v if k=='case' else int(v) for k,v in fields.items()})
            (raw if line.startswith('R6_TIME ') else tiles).append(row)
write('kernel-samples.csv',raw);write('tile-samples.csv',tiles)
groups={}
for r in raw:groups.setdefault((r['scene'],r['mode'],r['frame'],r['cycle']),{})[r['case']]=r['ms']
derived=[]
for (scene,mode,frame,cycle),r in groups.items():
    derived.append(dict(scene=scene,mode=mode,frame=frame,cycle=cycle,whole=r['whole'],bin=r['bin'],tile=r['repair']-r['bin'],cull_stage_store=r['stage']-r['bin'],shade=r['repair']-r['stage'],copy=r['copy'],copy_marginal=r['whole']-r['repair'],empty=r['empty']))
write('kernel-decomposition.csv',derived)
groups={}
for r in derived:groups.setdefault((r['scene'],r['mode']),[]).append(r)
write('kernel-summary.csv',[dict(scene=s,mode=m,**{k+'_'+stat:fn([r[k] for r in rr]) for k in ('whole','bin','tile','cull_stage_store','shade','copy','copy_marginal','empty') for stat,fn in [('median',statistics.median),('min',min),('max',max)]}) for (s,m),rr in groups.items()])
groups={}
for r in tiles:groups.setdefault((r['scene'],r['mode']),[]).append(r)
def pct(xs,p):return sorted(xs)[min(len(xs)-1,int((len(xs)-1)*p))]
write('tile-summary.csv',[dict(scene=s,mode=m,tiles=len(rr),frames=len({r['frame'] for r in rr}),scan_mean=statistics.mean(r['scan'] for r in rr),scan_p95=pct([r['scan'] for r in rr],.95),scan_max=max(r['scan'] for r in rr),hit_mean=statistics.mean(r['hit'] for r in rr),hit_p95=pct([r['hit'] for r in rr],.95),hit_max=max(r['hit'] for r in rr),raw_layers_per_pixel=sum(r['raw_layers'] for r in rr)/sum(r['pixels'] for r in rr),executed_layers_per_pixel=sum(r['executed_layers'] for r in rr)/sum(r['pixels'] for r in rr),certified_uniform_fraction=statistics.mean(r['uniform'] for r in rr),pixel_uniform_fraction=statistics.mean(r['pixel_uniform'] for r in rr) if all('pixel_uniform' in r for r in rr) else 'not_measured') for (s,m),rr in groups.items()])
write('cpu.csv',[dict(scene=r['scene'],mode=r['mode'],render=r['render'],median=r['instr_med_min_max'][0],minimum=r['instr_med_min_max'][1],maximum=r['instr_med_min_max'][2]) for r in jsonlines('cpu.jsonl')])
