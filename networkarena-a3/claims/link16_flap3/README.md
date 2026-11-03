# Claim candidate: link 1-6 flaps THREE or more times (NOT submitted; the 2-flap version, claims/link16_flap, won nothing)

Upload `switch.wasm` and `detect.wasm` here. Same switch as `claims/link16_flap` (switch 6 logs keepalive silences on its ports; 7-byte records); only the detector changed: `MIN_FLAPS = 3` in `detect/src/lib.rs`.

## Why 3
The 2-flap rule won nothing. A one-flap egg would have had true runs like the truth runs (two flaps), which the 2-flap rule calls true, so that case is ruled out. What fits the loss: the flaps are 12.04 s apart (5.91, 17.95 s), so a third falls near 30 s, where our truth logs go dark,
and an egg "flaps three times" with the truth-run shape (two) as a near miss would make the 2-flap rule say true on a false run.
Other explanations remain (different outage lengths, a different link, a different question).

## Tests (all pass)
- Unit tests; `worlds/practice15/gen_claim_flap3.py` -> `claim_flap3/` (true: 3-4 flaps; false: off, exactly two flaps like the truth runs, one flap, down and stays down, 5-6 flapping three times): **10 of 10**.
- `gen_flap3_stress.py` -> `claim_flap3_stress/` (40 random runs, loads 1.0-2.0, 2-5 flaps, loss on 1-6, other links flapping): **40 of 40**.
- Same program use as the 2-flap version (tiny).

## Depends on
The true claim runs having at least three flaps of at least about 0.8 s each on link 1-6. If the real egg is a different question, this loses too.
