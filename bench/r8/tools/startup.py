# startup.py RUN_DIR OUT.tsv : the startup.json of every cell of a gui-bench run as one TSV (flatten.py skips
# startup because it has no sections). Runs on ilo-prod (python from the zortax env).
import json,os,sys
root=sys.argv[1]
run=json.load(open(root+"/run.json"))
F=["connected_ms","first_surface_ms","toplevel_ms","first_buffer_ms","first_present_ms","visually_complete_ms","responsive_ms","first_input_latency_ms"]
out=open(sys.argv[2],"w")
out.write("\t".join(["scenario","framework","variant","rows","updates","hz"]+F+["exited_early"])+"\n")
for c in run["cells"]:
    k=c["cell"]; hz=k.get("hz", k.get("refresh_hz", ""))
    p=os.path.join(root,c["dir"],"startup.json")
    if not os.path.exists(p): continue
    j=json.load(open(p))
    out.write("\t".join(map(str,[k["scenario"],k["framework"],k["variant"],k["rows"],k["updates"],hz]+[j.get(f) for f in F]+[j.get("exited_early")]))+"\n")
