# Truth run 2: budget-aware build (not yet uploaded)

**File:** `switch.wasm` in this folder, to the Canvas assignment **A3 — Truth run**. Built from `a3truth2/`
(logger = switch 7): `(cd a3truth2 && cargo build --release --target wasm32-unknown-unknown)`.
Read the result with `a3truth2/target/release/logdump team.log 65536 --all` (`cargo build --release --bin logdump` first).
`LOG_FORMATS.md` explains how the log is made and how to parse it.

## Why this build exists
Truth run 1 returned an 8 KB log that was almost all switch 7's own view: the other 14 switches' reports never arrived
(`BUDGET_STATS` showed 136 of 136 lost from switch 2, 49 of 50 from switch 4), and switch 7 lost most of its keepalive samples.
The likely cause: the real export-byte and event budgets are far below the practice world's. Run 1 never recorded them.

## What is different
- **Reads `input.observe`** (events per second, backlog, export bytes per second, burst) and writes it as the first record
  (`EXT OBSERVE`). Keepalive and raw-sample punt rates are set from the event budget.
- **Mirrors the export bucket** and never sends a report it cannot pay for; a report costs 40 bytes plus its payload.
- **Records are prioritised**: link changes, gaps, scans first; heartbeats, quality, port and flow summaries next; rates;
  raw samples last. Each switch holds at most 1,500 bytes and drops the least important, oldest record when full.
- **Relays batch**: a switch puts the records it relays into its own queue and sends them with its own, so one 40-byte header
  carries many records. Reports go up the map to switch 7 by a fixed route (the map is the same every run); no beacons.
- **Scan hunting** punts the first small packet of each new (source, destination) pair instead of every small packet.
- **Longest prefix first** for customers that sit inside another customer's range (cannot be tested here: the simulator rejects
  overlapping customer ranges).

## Checked (practice, as the server would run it)
No `program failed` line, and 0 export-refused reports in every run below; events lost 0 (5 and 12 at the tiny budgets).

| World | Load | Log | Records delivered |
|---|---|---|---|
| 15 switches, practice budgets (80 events/s, 16 KB/s) | 1.0 / 1.25 / 1.5 | 27.4 / 28.1 / 26.6 KB | 96-97% |
| extras world; 23, 56 and 100 customers | 2.0 | 27.6 / 35.0 / 45.6 / 45.0 KB | 95-97% |
| mid budgets (40 events/s, 1000 B/s) | 2.0 | 27.6 / 35.1 KB | 96-97% |
| tight budgets (20 events/s, 200 B/s) | 2.0 | 11.9 / 9.9 KB | 75% / 50% |
| tiny budgets (10 events/s, 60 B/s) | 2.0 | 4.4 / 4.5 KB | 71% / 75% |
| 6-switch practice truth worlds (logger 1 build) | 1.0-1.5 | 12.0-12.9 KB | 97% |

For comparison, the truth-1 build on the same tight and tiny worlds lost 1,952 to 4,849 events and had 550 to 691 reports
refused, and delivered 0-44% of records. Switch 7 killed at 20 s: the run finishes, the log ends at 20 s.
"Records delivered" is records made minus records the logger did not receive, including ones dropped locally by priority.

## Not covered
The real budgets (they will be in the log). A scan lasting under a few seconds at very low budgets may lose its second
(50-address) record. Raw samples are fewer than in run 1, because the event budget now sets the sampling rate.
