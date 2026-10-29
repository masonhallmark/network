#!/bin/bash
# b3.sh name "cfg"
S=/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad
cd $S/p3 && touch src/lib.rs && CFG="$2" cargo build --release --target wasm32-unknown-unknown 2>&1 | grep -E "^error" -A8 | head -30
cp target/wasm32-unknown-unknown/release/p3.wasm $S/w_$1.wasm
