# Claim candidate: the TTL ladder (NOT submitted)

**Files to upload to Canvas "A3 - Claim":** `switch.wasm` and `detect.wasm` in this folder (sha256 in `hashes.sha256`). A claim uses 1 of 30; it can win at most one egg.

## The bet
Truth runs 1 and 2 both show one customer at switch 4 sending 64-byte UDP packets whose TTL cycles through small values (raw samples 3, 4, 5, 6, 7; profile `ttl<10 = 100%`).
Its range is named Mapper in `customers.txt`. The simulator has a `ttl_ladder` option for exactly this ("packet i starts at (i mod N) + 1"). This is the anomaly I have the most evidence for.
See `../../EGG_CANDIDATES.md` (item 8).

## How it works
- **Switch** (`switch/src/lib.rs`): switch 4 is the logging switch; every other switch installs nothing. Switch 4's data plane counts data packets entering from customer ports
  by TTL (a counter array of 32 slots; no punts, so no event budget is used and no report has to cross the network, which in truth run 2 mostly failed).
  Its controller writes, once a second in which some packet had TTL 0-9, the counts for TTL 0-15 (34 bytes), and one 9-byte total at 39.3 s. Worst case about 1.4 KB of the 24 KB cap.
- **Detector** (`detect/src/lib.rs`): true when, among TTL 0-9, at least 4 values each have at least 3 packets, the largest count is at most 4 times the smallest, there are at least 30 such packets,
  and they appear in at least 3 different seconds. A constant low TTL, a few stray packets, or one dominant value are not a ladder.

## Practice
`worlds/practice15/gen_claim_ttl.py` writes `worlds/practice15/claim_ttl/` (10 runs + `answers.toml`) in the layout of the provided practice eggs (5 true; false = 2 off, 2 constant low TTL, 1 ladder at another customer;
loads 1.0/1.25/1.5; each dealt afresh; every other anomaly seen in the truth logs switched on). Run it as the server would:

    python3 scripts/practice-check.py --switch claims/ttl_ladder/switch.wasm --detect claims/ttl_ladder/detect.wasm \
        --worlds worlds/practice15/claim_ttl --egg 31

Results: 10 of 10 right. A harder set (36 runs: ladders N 6-10 at 0.01-0.1 Mbit/s, starts up to 20 s, loads to 2.0, constant TTL 1-9, a ladder at another customer, a ladder at six other switches): 36 of 36.
No false positives on the 15-switch worlds, the many-customer worlds, the 3-law and 20-thing worlds, or the provided practice truth worlds (log 9 bytes, detector false).
Worst program use: input 857/4096 B, output 63/4096 B, fuel 0.14M/4M.

## What this bet depends on (not known)
1. The ladder customer attaches to switch 4 in claim runs, as in both truth runs. If it moves, this says false (tested: a ladder at switch 0, 3, 5, 9, 10 or 12 gives false).
2. The egg is "a customer at that switch sends a TTL ladder", and the false runs are off, constant-low-TTL or a ladder elsewhere. A grader whose near misses differ (a very short ladder, say) may differ; a ladder shorter than 30 packets or 3 seconds would be called false.
3. The 10 real claim runs have a ladder on 4 or 5 of them; mine copy the provided eggs' layout, which is a guess.
