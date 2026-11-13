# flatten.py RUN_DIR OUT_DIR : a gui-bench run as three TSVs (metrics, validity, cells). Same as zortax's
# tools/flatten.py (copied from /root/work/zortax/tools), plus the split fields that scene reports carry.
import json,glob,os,sys
root=sys.argv[1]
run=json.load(open(root+"/run.json"))
os.makedirs(sys.argv[2],exist_ok=True)
M=open(sys.argv[2]+"/metrics.tsv","w"); V=open(sys.argv[2]+"/validity.tsv","w"); C=open(sys.argv[2]+"/cells.tsv","w")
M.write("scenario\tframework\tvariant\trows\tupdates\treport\tsection\tmetric\tmvariant\tunit\tbetter\tn\tmedian\tmean\tmin\tmax\tstddev\thz\n")
V.write("scenario\tframework\tvariant\trows\tupdates\treport\tvalid\tcheck\tstatus\tvalue\tlimit\tmessage\thz\n")
C.write("scenario\tframework\tvariant\trows\tupdates\tdone\tskipped\terrors\thz\n")
for c in run["cells"]:
    k=c["cell"]; hz=k.get("hz", k.get("refresh_hz", ""))
    key=[k["scenario"],k["framework"],k["variant"],k["rows"],k["updates"]]
    C.write("\t".join(map(str,key+[",".join(c["done"]),json.dumps(c["skipped"]),json.dumps(c["errors"]),hz]))+"\n")
    d=os.path.join(root,c["dir"])
    for f in sorted(glob.glob(d+"/*.json")):
        b=os.path.basename(f)[:-5]
        if b=="cell": continue
        try: j=json.load(open(f))
        except Exception as e: V.write("\t".join(map(str,key+[b,"UNREADABLE","","","","",str(e),hz]))+"\n"); continue
        va=j.get("validity") or {}
        for ch in va.get("checks",[]):
            V.write("\t".join(map(str,key+[b,va.get("valid"),ch.get("name"),ch.get("status"),ch.get("value"),ch.get("limit"),(ch.get("message") or "").replace("\t"," ").replace("\n"," "),hz]))+"\n")
        for s in j.get("sections",[]) or []:
            for m in s.get("metrics",[]) or []:
                M.write("\t".join(map(str,key+[b,s.get("name"),m.get("name"),m.get("variant"),m.get("unit"),m.get("better"),m.get("n"),m.get("median"),m.get("mean"),m.get("min"),m.get("max"),m.get("stddev"),hz]))+"\n")
