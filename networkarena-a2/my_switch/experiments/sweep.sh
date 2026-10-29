#!/bin/bash
# usage: sweep.sh prog.wasm [loads...]
cd /mnt/c/Users/Mason/Documents/Classes/CS356/network/networkarena-a2
P=$1; shift; LOADS=${@:-1 1.5}
for w in worlds/practice/a2p*.toml; do for x in $LOADS; do
  out=$(./target/release/competitive_net_sim run-world $w --program $P --score --load-scale $x 2>&1)
  n=$(basename $w .toml)
  f1=$(echo "$out" | grep -E "^ F1" | awk '{print $5,$6,$7,$8,$9,$10}')
  f2=$(echo "$out" | grep -E "^ F2" | awk '{print $5,$6}')
  lp=$(echo "$out" | grep -E "A1 no loops" | awk '{print $5}')
  st=$(echo "$out" | grep -E "A1 no starv" | awk '{print $5}')
  f3=$(echo "$out" | grep -E "^ F3" | awk '{print $5}')
  echo "$n x$x | F1 $f1 | F2 $f2 | F3 $f3 | loops $lp | starv $st"
done; done
