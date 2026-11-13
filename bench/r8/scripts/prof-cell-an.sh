# prof-cell-an.sh LABEL SC ROWS PROBE... : analysis of prof-cell.sh runs (record and stat:*) for one cell:
# per probe, flamegraph SVG + top ten symbols of the probe's measured-iteration windows, and perf stat sums over the same windows.
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench; P=$B/results/profiles
INF=/nix/store/qxrk44bap2vnldsmni7882ym9iyl8mvy-inferno-0.12.8/bin
LABEL=$1; SC=$2; ROWS=$3; shift 3
REC=$P/cell-$LABEL-record
CELL=$(ls -d $REC/run/$SC/bough/default/60hz/$ROWS)
mkdir -p $P/svg $P/top $P/collapsed
for PR in "$@"; do
  python3 $B/prof_cell_an.py $CELL $PR > $REC/windows-$PR.txt
  echo "== $LABEL $PR windows: $(wc -l < $REC/windows-$PR.txt)"
  K=$LABEL-$PR
  ( while read w; do $PERF script -i $REC/perf.data --time $w 2>/dev/null; done < $REC/windows-$PR.txt ) | $INF/inferno-collapse-perf > $P/collapsed/$K.txt
  $INF/inferno-flamegraph --title "bough $LABEL $PR (gui-bench cell, CPU executor)" --subtitle "perf record DWARF 999 Hz on the app process, samples inside the harness's measured $PR iterations only; profile, not a timing" --minwidth 0.2 < $P/collapsed/$K.txt > $P/svg/$K.svg
  python3 $B/prof_an.py $P/collapsed/$K.txt $P/top/$K.json
  python3 - $CELL $PR > $P/params-$K.json <<'PY'
import json,sys
j=json.load(open(f"{sys.argv[1]}/{sys.argv[2]}.json")); print(json.dumps({"params":j.get("params"),"iterations":[{k:i.get(k) for k in ("measured","started","finished","ok")} for i in j["iterations"]]}))
PY
  # stat sums
  python3 - $CELL $PR $LABEL $P > $P/stat-$K.json <<'PY'
import json,subprocess,sys,os
cell,pr,label,P=sys.argv[1:5]
out={}
for g in ("cyc","br","l1","l2","ind"):
    d=f"{P}/cell-{label}-stat-{g}"
    cell_g=f"{d}/run/{os.path.relpath(cell, P+'/cell-'+label+'-record/run')}"
    if not os.path.exists(d+"/stat.csv"): continue
    r=subprocess.run(["python3","/root/work/r8/bench/prof_cell_an.py",cell_g,pr,d],capture_output=True,text=True)
    try: out[g]=json.loads(r.stdout)
    except Exception: out[g]={"error":r.stdout+r.stderr}
print(json.dumps(out))
PY
done
