Pre-upload checks used for truth5 (see submission/truth5/SUBMIT.md).
- limits_job.sh <S> <tag> <world> <load|-> <wasm>: runs one world with A3_STATS=1 (the simulator prints max call input/output/fuel; the patch in src/wasm.rs is gated by that variable) and counts "program failed".
- eggs20_job.sh <S> <wasm> <seed> <budget name> <events/s> <backlog> <bytes/s> <burst> <load>: builds a 20-thing world (worlds/practice15/gen20.py), runs it, and scores the log with tools/eggcheck.py.
- packet_survival.py <decoded simlog>: report packets on the wire vs punted at the logger (decode with tools/decode).
Limits that bite (src/wasm.rs): 4,096 B input and output per call, 4,000,000 fuel per call, 128 MB memory, 4,096 counter/register slots.
