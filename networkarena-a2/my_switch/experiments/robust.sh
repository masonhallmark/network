#!/bin/bash
# robust.sh wasm [loads]  -> "pass/N  minF1  failing-list"
S=/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad
cd /mnt/c/Users/Mason/Documents/Classes/CS356/network/networkarena-a2
P=$1; LOADS=${2:-"0.75 1 1.25 1.5 2"}
for w in $S/worlds/*.toml; do for x in $LOADS; do echo "$w $x"; done; done | xargs -P 24 -L1 bash -c '
o=$(./target/release/competitive_net_sim run-world $0 --program '$P' --score --load-scale $1 2>&1)
f1=$(echo "$o" | grep -E "^ F1" | awk "{print \$5}"); f2=$(echo "$o" | grep -E "^ F2" | awk "{print \$5}")
r=$(echo "$o" | grep -E "OVERALL" | awk "{print \$2}"); lp=$(echo "$o" | grep "no loops" | awk "{print \$5}")
echo "$(basename $0 .toml) x$1 $r $f1 $f2 loops=$lp"' | sort > $P.res
n=$(wc -l < $P.res); pass=$(grep -c " PASS " $P.res)
minf1=$(awk '{gsub("%","",$4); print $4}' $P.res | sort -n | head -1)
fails=$(grep -v " PASS " $P.res | awk '{printf "%s@%s(%s) ",$1,$2,$4}')
echo "pass $pass/$n  minF1 ${minf1}%  fails: $fails"
