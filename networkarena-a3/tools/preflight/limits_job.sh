#!/bin/bash
# job.sh <tag> <world> <load or -> <wasm>
S=$1; tag=$2; world=$3; load=$4; wasm=$5
args=""; [ "$load" != "-" ] && args="--load-scale $load"
A3_STATS=1 timeout 900 ./target/release/competitive_net_sim run-world $world $args --program $wasm --team-log $S/pf/$tag.team --log $S/pf/$tag.simlog > $S/pf/$tag.out 2>&1
echo "$tag rc=$? fail=$(grep -c 'program failed' $S/pf/$tag.out) $(grep A3STATS $S/pf/$tag.out | tail -1) log=$(stat -c%s $S/pf/$tag.team 2>/dev/null)" > $S/pf/$tag.res
rm -f $S/pf/$tag.simlog
