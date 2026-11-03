#!/bin/bash
# job.sh <S> <wasm> <seed> <budget name> <ev> <backlog> <bps> <burst> <load>
S=$1; wasm=$2; seed=$3; bn=$4; tag=s${3}_${4}_L$9
python3 worlds/practice15/gen20.py $seed $9 $5 $6 $7 $8 $S/e20/$tag.toml >/dev/null
A3_STATS=1 ./target/release/competitive_net_sim run-world $S/e20/$tag.toml --load-scale $9 --program $wasm --team-log $S/e20/$tag.team --log $S/e20/$tag.simlog > $S/e20/$tag.out 2>&1
a3truth5/target/release/logdump $S/e20/$tag.team 65536 --all > $S/e20/$tag.txt 2>/dev/null
echo "$tag fail=$(grep -c 'program failed' $S/e20/$tag.out) $(grep A3STATS $S/e20/$tag.out | tail -1 | sed 's/A3STATS //') log=$(stat -c%s $S/e20/$tag.team) | $(python3 tools/eggcheck.py $S/e20/$tag.toml $S/e20/$tag.txt | tr '\n' ' ')" > $S/e20/$tag.res
rm -f $S/e20/$tag.simlog
