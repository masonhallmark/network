# Truth run candidate 7 = truth6 + two cheap diagnostic records (NOT uploaded)

File: `switch.wasm`, sha256 starts `f7e36e9783fe79ea` (200,426 bytes). Built from `a3truth7/` (a copy of `a3truth6/`; truth5 and truth6 files are untouched).

## New records (both EXT, 16 bytes each)
- **PORT_SATURATION** (ext 5): every 5 s, per port, written only when the port's report queue refused a packet or its customer queue refused 20 or more.
  Reports wait in a queue of about 4,000 bytes that is served only when no customer packet waits, so a saturated port starves them without
  dropping them; the customer-queue refusals are the sign. (A starved switch cannot report its own saturation; see DELIVERY.)
- **DELIVERY** (ext 6): written once by the logger at about 39.3 s, locally (no transport): per origin switch, report packets taken in, distinct records
  taken in, duplicate copies dropped; origin 255 is the total. A switch whose reports starve shows as a low packet count (switch 4 had 17 against about 30).

## Before/after (truth6 -> truth7, same worlds, load 1.25; raw lines in tools/preflight/before_after_6to7.txt)
Log +0.26 to +0.58 KB (1.1% to 2.2%). Report bytes on the wire +0.02 to +0.38 KB (0.1% to 0.9%). Events delivered, packets reached, switches heard and eggs found are
unchanged in all 8 pairs.

## Checks on this file
123 limit runs (provided 6-switch truth worlds a/b/c at loads 1.0-2.0, all 30 provided egg runs, 15-switch worlds incl. 56 and 100 customers and the tight/tiny/harsh budget
worlds, the 3-law and 20-thing worlds; submission and practice builds): 0 program failures, 0 non-zero exits; worst call input 860/4096 B, output 2,493/4096 B, fuel 1.74M/4M.
Twenty planted things (4 seeds x 6 budgets at 1.25, plus 1.5 and 2.0): identical to truth5 and truth6 (20/20 at 80, 40, 20 ev/s; 18/20 at 10 ev/s; 14/20 lean at 5 ev/s).

## Findings made while testing
- Both new bodies were first 12 bytes where the type needs 13; the length guard in `rec()` refused them silently (log stayed intact), a test run showed the records missing, and the padding was fixed.
- A report queue holds about 4,000 bytes, so starved reports are not dropped (drops would show 4 packets): the first form of PORT_SATURATION, firing only on report-queue drops, never fired.
- Switch 4's silence in the 20-thing world is starvation, not a send bug: its reports are injected (about one a second) and wait in the queue of the saturated link.

## Extra pre-upload checks on this file (all through `competitive_net_sim run-world`, except the init test)
- Native `init_with` test (`cargo test`): no customers, 80 customers (it keeps 60), a /32 host inside a /24 (the simulator rejects overlapping ranges, the real contract has them),
  no observe section, no harness, every switch id, tiny and rich budgets: no panic, a timer goes through. (The overlapping case is not run end to end through the data plane.)
- 40 randomized worlds (events/s 3-80, backlog 2-64, bytes/s 20-16,000, burst 60-up, load 1.0-2.0; 20-thing, 3-law, many, huge, 100-customer and run-b worlds): 0 failures; worst output 2,495/4096 B, fuel 1.74M/4M.
- Keepalive period 20, 50, 250, 1000 ms on the 20-thing world: 0 failures; events 95-100%; packets reached 70-71% at the extreme periods with 10 events/s.
- Module interface identical to truth1's accepted file: no imports; exports init, memory, on_link_event, on_punt, on_timer.
- `--until-ms 1` load check (what scripts/practice-check.py does) passes.
- **Report size cap (not fixable by us, not told to the program):** `[harness] report_max_bytes` refuses a report bigger than the cap at its source. Our reports are up to 221 B.
  With a cap of 256 B no change; 200 B: events 73% at 10 events/s, 100 B/s; 150 B: 62%; 100 B: 38%. The default cap is the world's packet size (probably 1000 B), so this is a guess, not a known problem.
  Measured hedges, same worlds (events delivered at 10 ev/s, 100 B/s; no cap / cap 200 / cap 150): truth7 98/73/62; all reports at 100 B of records 91/91/91 (and 2 fewer eggs found); alternating 221 B and 141 B reports 98/92/90 with no other change.
