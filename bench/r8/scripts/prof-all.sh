# the profiles of bough (non-timing). Order: headless record phase, then the gui-bench cells (record, then stat groups), then analysis.
source /root/work/r8/bench/env.sh
B=/root/work/r8/bench
DO_STAT=0 DO_REC=1 bash $B/prof-headless.sh
for spec in "sheet10k sheet 10000" "canvas5k canvas 5000"; do
  set -- $spec
  bash $B/prof-cell.sh $1 $2 $3 record
  for g in cyc br l1 l2 ind; do bash $B/prof-cell.sh $1 $2 $3 stat:$g; done
done
bash $B/prof-cell-an.sh sheet10k sheet 10000 hover theme
bash $B/prof-cell-an.sh canvas5k canvas 5000 hover
