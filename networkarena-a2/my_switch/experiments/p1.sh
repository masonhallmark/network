#!/bin/bash
# p1.sh wasm -> one line: per Part-1 world at x1 and x1.5: F1worst/F2worst, loops flag
cd /mnt/c/Users/Mason/Documents/Classes/CS356/network/networkarena-a2
P=$1; line=""
for w in a2p1-dumb-004 a2p1-grid-009 a2p1-ring-020; do for x in 1 1.5; do
 o=$(./target/release/competitive_net_sim run-world worlds/practice/$w.toml --program $P --score --load-scale $x 2>&1)
 f1=$(echo "$o" | grep -E "^ F1" | awk '{print $5}'); f2=$(echo "$o" | grep -E "^ F2" | awk '{print $5}')
 r=$(echo "$o" | grep -E "OVERALL" | awk '{print $2}'); lp=$(echo "$o" | grep "no loops" | awk '{print $5}')
 line="$line | ${w:5:4}@$x $f1/$f2 ${r:0:1}${lp/0/}"
done; done; echo "$line"
