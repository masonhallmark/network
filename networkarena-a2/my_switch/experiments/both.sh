#!/bin/bash
# both.sh wasm -> practice+variants (x1,1.5) and random worlds (x1,1.5)
S=/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad
a=$(WDIR=$S/worlds $S/robust.sh $1 "1 1.5" | awk '{print $2}')
b=$(WDIR=$S/worlds_rand $S/robust.sh $1 "1 1.5" | cut -c1-120)
echo "practice $a | random $b"
