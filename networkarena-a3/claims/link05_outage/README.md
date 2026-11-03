# Claim candidate: link 0-5 goes down for a few seconds and comes back (NOT submitted)

Upload `switch.wasm` and `detect.wasm` here (hashes in `hashes.sha256`).

## The bet
Truth run 2: switch 0 logged `LINK_UP peer=S5` at 36.00 s with no matching down record. Switch 0 heard switch 5 at 28.56 s and switch 5 heard switch 0 at 26.52 s, so the outage began between 28.6 and 36.0 s (at most ~7 s).
The simulator's `fail` plant (down for `for_ms`, or forever if absent) does this. Truth run 1 has only the 28.56 s keepalive from switch 0 (reports lost), consistent with this.
Coverage of this link in the truth logs is thin (switch 0 records at 10.2, 12.2, 25.5, 28.6, 35.7, 36.0 s; switch 5 only at 25.5-26.5 s): another outage in 12.3-26.5 s cannot be excluded.

## How it works
Same program as `claims/link16_flap` but the logger is switch 0 (one end of the link; no report crosses the network): a counter array counts keepalives per ingress port, 1 in 4 is punted to learn each port's peer, and the controller writes a 7-byte record
`[tag][port][peer][at_ms]` when a port goes silent for 5 keepalive periods (tag 1, at = last keepalive) and when it is heard again (tag 2). Detector: true when the port facing switch 5 has at least one silence of at least 2.0 s that ended with the link coming back.

## Tests (all pass)
Unit tests; `worlds/practice15/gen_claim_outage.py` -> `claim_outage/` (5 true: 3-7 s outages at 8, 20, 29.5-31 s; false: off, down and stays down, 1 s blip, other links with the same outage, switch 0's other link 0-1): **10 of 10**; `gen_outage_stress.py` -> `claim_outage_stress/` (40 random runs, loads 1.0-2.0, one or two outages of 2.5-7.5 s): **40 of 40**. Every run has the other truth-log anomalies on (1-6 flapping twice, 1-2 and 13-14 slow one way, 12-13 and 7-12 lossy, 6-7 cut).

## Depends on
The real claim runs having a 0-5 outage of at least ~2 s that ends; if the real egg is "stays down", a short outage, or some other link, this loses.
