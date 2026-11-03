# Claim candidate: link 1-6 flaps (NOT submitted)

**Upload to Canvas "A3 - Claim":** `switch.wasm` and `detect.wasm` here (sha256 in `hashes.sha256`). Uses 1 of 30 claims; can win at most one egg.

## The bet
Truth runs 1 and 2 both show link 1-6 down for 1.63 s twice: down at 5.91 and 17.95 s, up at 7.54 and 19.58 s (S1 logged it in run 2, S6 in run 1; identical times). The simulator's `fail` plant does this.
See `../../EGG_CANDIDATES.md` item 1.

## How it works
- **Switch** (`switch/src/lib.rs`): switch 6 is the logging switch (and one end of link 1-6), so no report crosses the network. Its data plane counts keepalives per ingress port (a 64-slot counter array)
  and punts 1 keepalive in 4 so the controller learns the switch on each port. Every 100 ms the controller marks a port silent after 5 keepalive periods without one.
  Each change writes 7 bytes `[tag][port][peer][at_ms u32]` (tag 1 = silent, at = last keepalive heard; tag 2 = heard again). Every other switch installs nothing.
- **Detector** (`detect/src/lib.rs`): true when the port whose peer is switch 1 has at least 2 silences of at least 1.0 s that ended with the link coming back.
  Down once, down and never back, a lossy link, brief blips, or another link flapping are all false.

## Tests (all pass)
- Unit tests of both crates (`cargo test` in `switch/` and `detect/`).
- `worlds/practice15/gen_claim_flap.py` -> `claim_flap/` (5 true, 5 false: off, down once and back, down and stays down, another link flaps, 1-6 loses 50%): **10 of 10**.
  Run: `python3 scripts/practice-check.py --switch claims/link16_flap/switch.wasm --detect claims/link16_flap/detect.wasm --worlds worlds/practice15/claim_flap --egg 32`
- `worlds/practice15/gen_flap_stress.py` -> `claim_flap_stress/` (40 random runs, loads 1.0-2.0, 2-4 flaps of 1-3 s or 4-8 s, 0-50% wire loss, other links flapping): **40 of 40**.
- Every run has the other truth-log anomalies on (links 1-2 and 13-14 slow, 12-13 and 7-12 lossy, 6-7 cut to 3 Mbit/s, 0-5 down 22 s).
- Provided practice worlds and 15-switch worlds: detector false, no "program failed". Program use: input 137/4096 B, output 85/4096 B, fuel 0.13M/4M; log 28 bytes in a 2-flap run.

## What this bet depends on (not known)
1. The egg is "link 1-6 goes down and comes back repeatedly". If the real egg is a single down-and-up, or the real claim runs flap only once, this says false on the true runs and loses.
   (`MIN_FLAPS` in `detect/src/lib.rs` is the knob; 1 would also call the "down once and back" near miss true.)
2. The real flaps last at least about 0.8 s (a 1.63 s flap is measured as about 1.85 s).
3. My near misses are guesses.
