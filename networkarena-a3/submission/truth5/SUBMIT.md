# Truth run candidate 5: truth4's reporting + every record type, for discovering the eggs (NOT uploaded)

Final file: `switch.wasm`, sha256 starts `a45eba6b6d1b3c6e` (182,192 bytes). Supersedes the earlier truth5 build described below (adds an output cap and congestion detours).

Built from `a3truth5/` (copy of `a3truth4/`). Logger 7. Read with `a3truth5/target/release/logdump team.log 65536 --all`.

## Difference from truth4
- Keeps all record types again: CUSTOMER_PROFILE, FLOW_LOSS, TOP_TALKER, RAW_SAMPLE, RAW_KEEPALIVE (priority below events). Raw punts sparse (1 in 1024 / 4096 packets).
- Events keep their 3 copies, priority, route adverts, congestion-aware routes and the delivery audit (see truth4/SUBMIT.md).
- **Lean mode** (decided at start from `input.observe`): if events/s < 8 or export < 45 B/s, only truth4's record set is made and raw punts are off.

## Results (practice worlds; 0 program failures; events = audited events delivered)
Log 15-38 KB (truth4: 6-10 KB). Events 100% in run-a/b/c, many, mid, tight, tiny, many_tiny, burst-60 B, leak3, healthy; 99% (56 customers), 98% (100 customers);
lean worlds (5 events/s 30 B/s: 92%; 8 B/s: 90%; 3 events/s 20 B/s: 81%) are identical to truth4. Packet survival 88-98% at realistic budgets (same as truth4).
Known cost: with 56-100 customers the many profile records cost 1-2% of events.

## Pre-upload checks on the final file (load 1.25 is the expected truth-run-2 load; 1.0, 1.5, 2.0 also run)
- **Limits.** 115 runs (provided 6-switch truth worlds a/b/c at loads 1.0-2.0, all 30 provided egg runs, our 15-switch worlds incl. 56/100 customers
  and the tight/tiny/harsh budget worlds), with both the submission build and the practice build: 0 program failures, 0 non-zero exits.
  Worst call: input 857 of 4,096 B, output 2,493 of 4,096 B (the logger now writes at most about 2,600 B of records per call), fuel 1.74M of 4M.
- **Twenty planted things** (gen20.py: 8 failures, 7 customer, 5 traffic items; 4 seeds x 6 budgets at load 1.25, plus loads 1.5 and 2.0), scored by tools/eggcheck.py:
  rich/mid/tight (80/40/20 ev/s) 20 of 20; 10 ev/s with 100 B/s or 60 B/s 18 of 20 (misses: the burst on one customer, and the 50-address scan, sampling limits);
  5 ev/s with 30 B/s (lean mode) 14 of 20. At 1.5 and 2.0 the same or better. Every run reports from 14 of 15 switches throughout: switch 4 goes quiet
  after its second link fails because its only other link carries all its rerouted traffic and reports starve behind it (its neighbour's LINK_DOWN still arrives).
- **Regression.** Audited events 100% down to the 10 ev/s, 60 B/s world; 88%/85%/93% at the 5 ev/s, 3 ev/s and 8 B/s worlds; all 15 switches heard in every world.
- **Congestion detour.** A switch whose nearer ports are heavily congested (penalty up to 6) takes a one-hop-longer path; this fixed switches 5 and 6 vanishing
  for 19.6 s when link 6-7 is cut to 1 Mbps.
## Known limits (not fixed)
- Reports still starve behind customer traffic on any link that stays saturated and has no alternative.
- The 50-address scan record and per-customer bursts can be missed at 10 events/s (sampling, not delivery).
- Lean mode (below 8 events/s or 45 B/s) drops profiles, flow loss and raw samples.
- The harness route plan (split/backup) is not logged.
