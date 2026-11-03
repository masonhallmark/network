# Truth run candidate 4: basic, broad logging + a reporting system built to deliver events (NOT uploaded)

Built from `a3truth4/` (from `a3truth3/`). Logger 7 by default (`A3_LOGGER=n` at build time changes it; re-test first).
Read with `a3truth4/target/release/logdump team.log 65536 --all`. Same record format as truth2/3.

## What it logs (small and broad, 5-8 KB per run in the practice worlds)
HEARTBEAT, LINK_DOWN/UP, LINK_QUALITY, PORT_STATS, CUSTOMER_RATE, CUSTOMER_GAP, SCAN_SUSPECT, SWITCH_SILENT, BUDGET_STATS, EXT OBSERVE.
No raw samples, flow, profile or top-talker records. Raw punts are off.

## Reporting (what changed from truth3)
- Events (link changes, link quality, gaps, scans, heartbeats) ride in **3 reports each**, in separate send slots; the logger drops copies by hash.
- Reports are forwarded by installed routes (data plane); only the logger punts them. TTL 8.
- Routes go to a neighbour strictly closer to the logger by the map (cannot loop). Among those, the one with the lowest cost wins.
  Cost = the neighbour's advertised distance + a congestion penalty (0-3) from the port's drops, so a link whose report queue is starved
  by customer traffic (like 6->7 in truth run 1) is avoided by switches that have another way.
- Route adverts: when a switch's distance changes (link down or congestion), it tells each live neighbour in a 41-byte packet (as the bucket allows).
  With no closer neighbour alive it detours through the others (loop-free by adverts).
- Send slots staggered by switch id; slot = max(14 / (0.3 * events/s), 140 B / (0.9 * bytes/s)) clamped to 1..8 s; final flush 37.6 s + 0.1 s * id.
- Built-in audit: HEARTBEAT carries the count of events the switch made; the logger writes BUDGET_STATS (events lost per origin).

## Results (practice worlds, 0 program failures everywhere; del = audited events delivered)
| world | origins heard | del |
|---|---|---|
| run-a/b/c (loads 1.0-2.0), many, huge, 100 customers, mid, tight, tiny, many_tiny, leak3, healthy | 15/15 | 100% |
| burst only 60 B | 15/15 | 100% |
| 5 events/s, 30 B/s | 15/15 | 92% |
| 8 B/s refill | 15/15 | 90% |
| 3 events/s, 20 B/s | 12/15 | 81% |
| provided 6-switch truth worlds, loads 1.0-2.0 | 6/6 | 100% |
Planted events checked independently in the logs (7-8 link fail, pause, scan, 14-9 gap): 14/14 at rich, mid, tight; 13/14 at tiny
(the 50-address scan record is missed: the switch's own sampling loses punts at 10 events/s, not a report loss). Link 1-6 (permanent and 1.6 s):
both ends' down/up in the log at normal and tiny budgets. Killing switch 8, 6 or 14 at 15 s: every neighbour's LINK_DOWN and SWITCH_SILENT arrive.

## Not guaranteed
No acks. Copies cover loss of a report, not a starved link for the whole run or a budget too small to send (below about 20 B/s).
The audit counts events from the last heartbeat that arrived; an origin whose heartbeats all fail shows nothing.
Switch 7 (the logger) dying ends the log. Scan detail at low event budgets is sampled, not complete.
