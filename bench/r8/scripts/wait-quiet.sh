# wait-quiet.sh MAX_MINUTES THRESH LOGFILE : poll the 1-minute load every 10 s until it is below THRESH (and the
# 5-minute load below 2*THRESH), at most MAX_MINUTES; log each sample and the verdict. Always returns 0 (a run that
# starts on a busy host is recorded as such by run-gb.sh's identity record and the harness's host_load check).
M=${1:-20}; T=${2:-2}; L=${3:-/dev/stdout}
end=$(( $(date +%s) + M*60 ))
while :; do
  read l1 l5 l15 rest < /proc/loadavg
  echo "$(date -u +%H:%M:%S) $l1 $l5 $l15" >> $L
  if awk -v a=$l1 -v b=$l5 -v t=$T 'BEGIN{exit !(a<t && b<2*t)}'; then echo "QUIET at $(date -u +%H:%M:%S) load $l1 $l5 $l15" >> $L; break; fi
  if [ $(date +%s) -ge $end ]; then echo "GAVE UP waiting after ${M} min, load $l1 $l5 $l15" >> $L; break; fi
  sleep 10
done
