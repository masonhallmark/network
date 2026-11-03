#!/bin/bash
# eval.sh <world> <load> <wasm> <logdump>  -> one line
S=${SCRATCH:-/tmp/a3scratch}
K=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$S"
out=$($K/target/release/competitive_net_sim run-world $1 --load-scale $2 --program $3 --team-log $S/e.team --log $S/e.simlog 2>&1)
fails=$(echo "$out" | grep -c "program failed")
$4 $S/e.team 65536 --all > $S/e.txt 2>/dev/null
dec=$($K/tools/decode/target/release/decode $S/e.simlog /dev/null 2>/dev/null | sed -n '/observation drops/,/ingress by/p' | grep "^(" | tr -d '()' | awk -F'[, ]+' '{c[$2]+=$3} END{printf "events_lost=%d export_refused=%d", c[0], c[1]}')
origins=$(grep -E "^ +[0-9.]+s S[0-9]+ " $S/e.txt | awk '{print $2}' | sort -u | wc -l)
echo -n "failures=$fails origins=$origins $dec "
python3 - "$S/e.txt" <<'PY'
import re,sys
sent={};ref={}
for l in open(sys.argv[1]):
    m=re.match(r'\s+[\d.]+s S(\d+)\s+HEARTBEAT\s+(?:reports_sent|records_originated)=(\d+)',l)
    if m: sent[int(m.group(1))]=int(m.group(2))
    m=re.match(r'\s+[\d.]+s S(\d+)\s+BUDGET_STATS\s+events_lost=\d+ reports_refused=(\d+)',l)
    if m: ref[int(m.group(1))]=int(m.group(2))
tot=sum(sent.values()); lost=sum(min(ref.get(k,0),sent[k]) for k in sent)
print(f"records_delivered={100*(tot-lost)//max(tot,1)}%" )
PY
