# Egg candidates seen in truth runs 1 and 2 (written 2026-11-03; evidence, not confirmation)

**See the section "Current status" at the bottom first: it supersedes the table below where they differ.**

Sources: `submission/truth1/result/` and `submission/truth7/result/` (run 2). "Run 2 only" often means run 1 lost that switch's reports.
Customer ids in a log are positions in the contract (address order); my id-to-name mapping is unreliable, so names below are only used where raw samples confirm them.

| # | Anomaly | Where | Evidence | Likely egg | Confidence |
|---|---|---|---|---|---|
| 1 | Link 1-6 goes down 1.63 s, twice: down 5.91 and 17.95 s, up 7.54 and 19.58 s | S1 (and S6 in run 1) | Same 5.91/7.54 in both runs, so timing is fixed. Period 12.04 s; a third flap near 30 s would be missed (switch 1's reports stop). | Link goes down and comes back / flaps | high |
| 2 | Link 0-5 comes back at 36.00 s with no recorded down | S0 | Port was up at 12.24 s (451 kB, no loss), so it was down between then and 36 s; the down record never arrived. | Long outage, or a one-way keepalive gap | medium |
| 3 | Link 12-13 lossy, slow and dropping | S13 (and S12) | Run 2: 40% and 50% keepalive loss, delay 4.8 / 28.9 ms. Run 1: delay 69.5 ms, 172 drops at S12->13. | Link capacity cut / leaky slow link | high |
| 4 | Link 13-14 slow one way | S14 | S14 hears S13 at a constant 25 ms (30.6 and 32.6 s); every other link 0.5 ms | Slow link (link delay) | medium-high |
| 5 | Link 1-2 asymmetric delay | S1, S2 | S1 hears S2 at 30 ms (6.1 and 26.5 s), S2 hears S1 at 2 ms | Slow link, different each way | medium-high |
| 6 | Link 6->7 saturated | S6 | 168 drops (run 1, 14.3 s) and 228 drops (run 2, 26.5 s) | Link capacity drop | medium |
| 7 | One of switch 7's links leaks 10-20% keepalives all run | S7 | 33 records from t=1 s in both runs; port peer never identified | Leaky link (wire loss) next to the logger | high that it is planted; which neighbour unknown |
| 8 | Customer at S4 (id 9) sends 64 B UDP, every packet TTL < 10 (raw samples TTL 3-7); raw samples match the range named Mapper | S4 | Profile `ttl<10 = 100%` in both runs | TTL ladder (traceroute-like probes) | high |
| 9 | Two customers at S1 (ids 12, 26) silent 0.10 -> 20.09 s, then start together | S1 | Run 2 only | Customers that start late / a pause at time 0 | low-medium |
| 10 | One customer at S3 (id 7) silent 1.93 s at 10.09 s | S3 | Run 2 only | A short pause (shape of the handout's 2 s near miss) | low |
| 11 | S7 customers 16 and 21: 8 s on, 8 s off, offset | S7 | Both runs, same times (16: 8.05-16.0 and 24.07-32.0; 21: 12.03-20.1, 28.05-36.0) | Probably baseline customer behaviour (bursty shopping-like), not an egg | low |
| 12 | S7 heavy customer (id 15): 80 pkts/s baseline, bursts to 1,275 pkts/s for about 5 s every 16 s | S7 | Run 2 (12.2-17.3 s, 28.5-33.7 s) | Customer surge / extra flow | low-medium |

## Not seen (no evidence either way)
Scans (no SCAN_SUSPECT in either run), duplicated packets (every dups field is 0), a customer pause of 10 s or more, split or backup routes, a small-queue port,
a switch failure, congestion-control behaviour (no sawtooth anywhere). Many switches' reports never arrived, so absence is not evidence of absence.

## Hints in `customers.txt` (same in both runs)
Three single-host customers inside bigger ranges: Netflix live (10.17.72.200/32, with a "viewers" range), Fortnite player (10.42.152.200/32), Amazon Prime member (10.29.156.200/32).
Mapper has a "target" range instead of a peer. Cam-A, Cam-B, Ping, Ledger, Vault, Backup, Offsite, Sync, Dropbox (and Dropbox-2) are small recurring flows.

## Update after claim 1 (2026-11-03): Egg 7 WON, plus corrections

- **Egg 7, Traffic, "The traceroute" is won** (claims/ttl_ladder). Candidate 8 is confirmed: Mapper -> "Mapper's target" at switch 4 sends the TTL ladder.
- **Candidate 7 (S7 leaky link) is probably an artifact.** Its records say `peer=S255`, which in our code means "peer not learned" (`peer.unwrap_or(255)`), and S7's `events_lost` rises all run (10, 19, 28, 37, 47) because its controller sees ~1,500 pkts/s against an 80 events/s cap.
  The "loss" is keepalive samples that never reached the controller; it rises to 20% exactly when customer 15 surges. Downgrade to low.
- **Candidate 3 vs 6 (12-13, 6->7) may be plain congestion**: Backup (S12) sends ~560 pps of 1,000 B and Cam-A (S6) bursts at ~1,200 pps, which fills a 10 Mbit/s link. A 40 kB drop-tail queue at 10 Mbit/s adds 32 ms (run 2 saw 28.9 ms); 69.5 ms (run 1) would need about 4.6 Mbit/s, so only that sample hints at a capacity cut.
- **Candidates 4 and 5 are upgraded**: `src/sim/plant.rs` has a `link_delay` plant with separate `a_to_b_us` and `b_to_a_us`; 1-2 (30.0 ms one way, 2.0 ms the other, both exact) and 13-14 (25.0 ms) look like it.
- **The five `src == dst, proto 0, size 0` samples at t = 0** are hosts announcing themselves (`announce_pending` in `src/app.rs`), not an anomaly.
- **FLOW_LOSS `entered` counts are sampled** (80 events/s cap), so do not infer a start time from them (an "S10 customer starts at 20.5 s" idea does not hold).
- **Egg vocabulary in the simulator**: plants `fail` (down for N ms or forever), `link_delay`, `wire_loss`, `link_capacity` (steps), `pause`, `side_flow` (steady extra stream, optional proto), `scan`; app options `echo` (every packet sent twice), `ttl_ladder`, `hosts`, `proto`.
- **Unusually named customers are likely egg vehicles** (Mapper was): Netflix live (/32) + viewers, Fortnite player (/32), Amazon Prime member (/32) + peer, Dropbox-2, Cam-A / Cam-B, Ping.

- Built `claims/link16_flap/` (link 1-6 flaps, 10/10 and 40/40) and `claims/link12_delay/` (link 1-2 slow direction, 10/10 and 40/40), 2026-11-03. Neither submitted.
- Built claims/link16_flap3 (3+ flaps) after the 2-flap claim won nothing; 10/10 and 40/40 locally, not submitted.

## Current status (2026-11-03, after claims 1-3; supersedes the table above)

Won: **Egg 7** Traffic "The traceroute" (claim 1, claims/ttl_ladder); **Egg 14** Link "The one-way slow link" (claim 3, claims/link12_delay; detector watched link 1-2 only).
Lost: claim 2, link 1-6 flaps >= 2 (claims/link16_flap). Ready, not submitted: claims/link16_flap3 (>= 3 flaps). Claims used: 3 of 30. Truth runs: 2 of 3.
Categories seen on cards: Traffic, Link (so "Link" covers failures and slow/lossy links).

| Candidate | Evidence | Status | Next |
|---|---|---|---|
| Traceroute at switch 4 (Mapper) | TTL 3-7 ladder | WON (egg 7) | none |
| Link 1-2 one-way delay (30 ms vs 2 ms) | S1/S2 keepalives, run 2 | WON (egg 14) | none |
| Link 13-14 one-way 25 ms | S14 hears S13 at 25.0 ms, run 2 only | probably the same egg as 14 (or a sibling); if the same, it cannot be won again | low priority |
| Link 1-6 flaps | down 5.91 and 17.95 s, 1.63 s each, both runs | real plant; 2-flap rule lost; 3-flap rule built | submit flap3; then a 1-flap, shorter-duration variant |
| Link 0-5 outage | S0 logs LINK_UP at 36.00 s, no down record; S0 heard S5 at 28.56 s, so down within 28.6-36.0 s | medium | build: logger switch 0, one recovered outage of >= 2 s |
| Link 12-13 lossy / congested | S13: 40% and 50% keepalive loss (7.1 s, 27.5 s), delays 4.8-69.5 ms, 172 drops at S12->13 | medium-low (Backup traffic congests it anyway) | build: logger 13, loss windows |
| Link 6-7 saturated | 168 / 228 drops | low (Cam-A bursts fill it) | skip |
| Leak at switch 7 (peer=255 loss) | S7 overloaded its own 80 events/s | probably an artifact | skip |
| Sync pair silent 0.1-20.1 s (S1) | ids 12 and 26, both runs | late start or a pause; low-medium | maybe: customer pause |
| 2 s gap at S3 (cust 7) | run 2 only | low | skip |
| Home users UDP 64 B at S2 | 2 samples, run 2 | low | skip |
| Cust 15 surge at S7 | 80 to 1,275 pkts/s, two windows 16 s apart | low-medium | skip |
| Scans, duplicates, switch failure, CCA sawtooth | not seen (reports mostly lost) | unknown | only via truth run 3 |

Facts that changed our approach: unusually named customers are egg vehicles (Mapper was); `src/sim/plant.rs` lists the plant kinds (fail, link_delay, wire_loss, link_capacity, pause, side_flow, scan) and `src/app.rs` adds echo and ttl_ladder; a claim wins only if the logger sees the egg locally, because reports mostly failed in truth run 2.
- Built claims/link05_outage (0-5 outage, logger switch 0); 10/10 and 40/40 locally; claim link16_flap3 (3 flaps) was submitted and won nothing.
