# Claim candidate: link 1-2 has a slow direction (NOT submitted)

**Upload to Canvas "A3 - Claim":** `switch.wasm` and `detect.wasm` here (sha256 in `hashes.sha256`). Uses 1 of 30 claims.

## The bet
Truth run 2: switch 1 hears switch 2 at exactly 30.0 ms (6.1 and 26.5 s) while switch 2 hears switch 1 at exactly 2.0 ms (18.4 s); every other link 0.5 ms. The simulator's `link_delay` plant does this (separate delay each way).
See `../../EGG_CANDIDATES.md` item 5. (Link 13-14, 25 ms one way, is the sibling; it may be a different egg, and this detector ignores it.)

## How it works
- **Switch:** switch 1 is the logger, switch 2 the other end; every other switch installs nothing. Each keepalive carries its send time, so arrival minus it is that direction's delay. Each end measures the direction toward itself in 1 s windows
  (count, minimum, mean; 100 us units). Switch 2 sends its last two windows to switch 1 in a one-hop report per second (so one lost report costs nothing). Switch 1 writes 7-byte records
  `[tag][second][count][min u16][mean u16]` (tag 1 = 2->1, tag 2 = 1->2): about 550 bytes a run of the 24,576 allowed. No customer packets are punted (30 events/s at most at switch 1).
- **Detector:** true when the median window minimum of 2->1 is at least 10 ms and steady (70% of windows within 3 ms of the median; mean close to the min), and 1->2 is at most a quarter of it (if 1->2's reports never arrived, the slow side alone decides).
  Fast link, symmetric slow link, slow the other way round, congestion (delay varies) and a slow link elsewhere are false.

## Tests (all pass)
- Unit tests of both crates.
- `worlds/practice15/gen_claim_delay.py` -> `claim_delay/` (5 true with 25-40 ms one way; false: off, symmetric 30/30, reversed, 1-2 congested by a 1 Mbit/s cut, 5-6 slow instead): **10 of 10**.
- `worlds/practice15/gen_delay_stress.py` -> `claim_delay_stress/` (40 random runs, loads 1.0-2.0, also mild 2-6 ms asymmetry, 20-40% loss on 1-2, any other link slow): **40 of 40**.
- All runs have the other truth-log anomalies on. Provided practice and 15-switch worlds: false where no 1-2 delay plant exists, true where one does; no "program failed". Use: input 137/4096 B, output 46/4096 B, fuel 0.08M/4M.

## What this bet depends on (not known)
1. The egg is about link 1-2 and the true runs match the truth log (30 ms from 2 to 1, small from 1 to 2). If the real egg is a different link (13-14?), every run has 1-2 slow and this says true everywhere, so it loses.
2. My near misses are guesses; a mild asymmetry (under 10 ms) is called false.
