# Truth run candidate 3: route-based report transport (NOT uploaded)

Built from `a3truth3/` (copy of `a3truth2/` with a new report transport). Logger = switch 7 by default;
`A3_LOGGER=6 cargo build --release --target wasm32-unknown-unknown` builds another logger (re-test it before use).
Read the result with `a3truth3/target/release/logdump team.log 65536 --all`; same record format as truth2.

## What changed from truth2
- Reports are forwarded by an installed route (table 4, REPORT_IP -> port toward the logger), so transit switches do not punt
  or re-pay the 40-byte header; only the logger punts (reason 4). TTL 8 (a report crosses at most about 6 hops).
- The route prefers the closer neighbour whose port has dropped least (drop EMA); it moves when that port is down or clearly worse.
- Urgent records (link changes, gaps, scans) ride in 3 reports in all; the logger drops the copies by hash.
- Send slots: one report per `slot` per switch, staggered by switch id. slot = max(14 / (0.3 * events/s), 140 B / (0.9 * bytes/s)),
  clamped to 1..8 s. A slot sends only with >= 100 B waiting (or after 2 slots). Urgent records may go early (gap >= max(250 ms, slot/4)).
  With >= 20 events/s a full chunk may go early. Final flush: 37.6 s + 0.1 s * id (logger 39.3 s), spaced >= urgent gap.
- The logger samples at one third of the event budget, so it can take in reports.

## Results (practice worlds, load shown; f = program failures, org = switches heard of 15, del = records delivered)
| world | truth3 | truth2 |
|---|---|---|
| run-a/b/c, rich budget, load 1.0-2.0 | org 15, del 85-91% | org 15, del 95-97% |
| tight (20 ev/s, 200 B/s) | org 15, del 89% | org 15, del 75% |
| tiny (10 ev/s, 60 B/s) | org 15, del 84% | org 15, del 71% |
| many_tiny | org 15, del 77% | org 15, del 75% |
| 5 ev/s, 30 B/s | org 15, del 50% | org 13, del 83% (13 heard only) |
| 3 ev/s, 20 B/s | org 14, del 64% | org 11, del 87% (11 heard only) |
| burst 60 B | org 15, del 100% | org 15, del 78% |
| 6-switch provided truth worlds, load 1.0-2.0 | org 6, del 96-97% | same |
Link 1-6 down (permanent and 1.6 s) at tiny budgets: both ends' LINK_DOWN and LINK_UP reach the log. 0 program failures everywhere.
Caution: del is measured over heard switches only, so truth2's s5/s3 figures are inflated; truth3 covers more switches.

## Known weaknesses
- Rich budgets: 6-10 points below truth2 (more, smaller packets; 4-packet report queue on the link into the logger drops some).
- During a failure next to the logger a detour can loop for up to 8 hops (seen at 15-21 s when 7-8 fails).
- Nothing guarantees delivery: no ack, no retransmit. Urgent records are sent three times; lost data is otherwise lost.
