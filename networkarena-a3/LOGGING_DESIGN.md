# A3 logging design (draft)

Goal: one compact, self-describing log that a detector can parse, that fits
24 KB in a claim (64 KB in a truth run), and that records *changes and
summaries*, not steady state.

Numbers marked (practice) come from `worlds/practice`; the real network may
differ. Read the real budgets from `input.observe` at init instead of
hardcoding them.

## 1. Constraints that shape the design

| Constraint | Value | Source |
|---|---|---|
| Log cap | 64 KB truth, 24 KB claim, a write that does not fit is lost whole | SUBMITTING.md |
| Events to a controller | 80 / s / switch, backlog 64 (practice) | `[observe]` |
| Report bytes | 16,000 B/s sustained, 8,000 B burst per switch (practice); each report is charged 40 B header + payload | `src/observe.rs` |
| Report size | at most one customer packet | `docs/a3-logging.md` |
| Report queue | 4 packets per port, behind customer traffic, so bursts drop | `docs/a3-logging.md` |
| Run length | 40 simulated seconds, 4,000 ticks of 10 ms | handout |
| Anything not yet written at 40 s | lost | `docs/a3-logging.md` |
| Output per controller call | `max_output_bytes` = 4,096 (default) | `src/wasm.rs` |

Average claim budget: 24,576 B / 40 s = about 600 B/s. At 4 to 6 bytes a
record, that is about 100 records a second for the whole network, and the
log can't afford any steady-state heartbeat per switch.

## 2. Record format

```
byte 0      [ type:4 | switch:4 ]      type = b >> 4, switch = b & 0x0F
bytes 1-2   t10: u16 LE                observation time in 10 ms ticks (max 4000)
bytes 3..   body, length fixed by type
```

- Switch ids are 0 to 14, so a nibble holds them (15 is spare).
- 16 types fit in the other nibble; type 15 is an escape (next byte = extended type).
- Body length is fixed per type, so there is no length byte. The cost is that the
  detector's length table must match the switch program exactly. Put the table
  in one shared module that both crates import.

### Time field options

| Option | Header size | Notes |
|---|---|---|
| A. u16 ticks | 3 B | simple, no wrap in 40 s. **Recommended.** |
| B. 12-bit time packed with type and switch | 3 B | same size as A, with 4 spare flag bits (for example "late", "estimated") |
| C. u8 ticks, wraps every 2.56 s | 2 B | needs a HEARTBEAT at least every 2.5 s to rebuild the high bits. Late reports from another switch can land on the wrong side of a wrap, so reconstruction is ambiguous. Saves 1 B a record. |

### Seq byte: probably drop it from the log

- The seq number is needed for flood de-duplication, `(origin, seq)`, and that
  lives in the **report packet** and in logger memory (`broken_link` does this).
  By the time a record is written, duplicates are already gone, so the detector
  gains nothing from it.
- Its other use is spotting lost reports. A per-switch counter in a periodic
  summary does that for less (see type 0 below) and tells you how many were lost.
- Net: record 3 B of header, not 4.

### Received-at-logger time

Observation time is what the detector reasons about. A delta
`(write time - t10)` saturating at 255 ticks is a cheap extra byte, useful for
measuring report latency. Keep it for truth runs (flag bit or a separate type),
and drop it from claims.

## 3. Event types

Sizes are the body only; add 3 for the header.

| # | Type | Body | Why |
|---|---|---|---|
| 0 | HEARTBEAT | `reports_sent:u16, events_lost_est:u16` (4 B), both cumulative | Time anchor every 5 s from **every** switch (switch nibble = origin). Proves the origin was alive until t, and its counters let the logger count lost reports. |
| 1 | LINK_DOWN | `peer:u8` (1) | Keepalive silence on a port. Time = last keepalive heard, not time of detection. |
| 2 | LINK_UP | `peer:u8` (1) | Keepalives resume. Lets the detector tell "stays down" from "went down and came back" (the egg 21 near miss). |
| 3 | LINK_QUALITY | `peer:u8, loss_pct:u8, delay_100us:u16` (4) | A link that is up but working badly: keepalive loss rate, one-way delay `arrived_ns - sent_ns`. Emit only when it crosses a threshold. |
| 4 | PORT_DROPS | `peer:u8, drops:u16` (3) | From `ev.port_drops`: queue overflow per port since the last timer. Congestion vs failure. |
| 5 | CUSTOMER_RATE | `cust:u8, pkts:u16, kbytes:u16` (5) | Per-customer window summary from data-plane counters, which cost no event budget. The detector looks for rate changes. |
| 6 | CUSTOMER_GAP | `cust:u8, start_t10:u16, dur_t10:u16` (5) | A customer silent for at least X: written once, at resume (or at the final flush). Directly answers "paused for at least 10 s", and the near-miss durations (2 s vs 10 s) are in the record. |
| 7 | SCAN_SUSPECT | `src_host:u8, target_cust:u8, distinct:u8, window_t10:u16` (5) | One source touching many distinct destinations in another customer's range inside a window. Needs a distinct-counter (small bitmap per source and window). |
| 8 | TOP_TALKER | `cust:u8, share_pct:u8` (2) | Heavy-hitter summary (Misra-Gries). Optional. |
| 9 | SWITCH_SILENT | `sw:u8` (1) | All of a switch's neighbours stop hearing it. Distinguishes a switch failure from a single link failure. |
| 10 | CUSTOMER_PROFILE | `cust:u8, avg_size:u8 (x8 B), udp_pct:u8, ack_pct:u8, rate_bucket:u8` (5) | A fingerprint, because ranges are reshuffled in every claim. Only the traffic shape tells you who Spotify is. |
| 11 | FLOW_LOSS | `src:u8, dst:u8, entered:u16, left:u16` (6) | Per (src, dst) customer pair over 2 s. A switch writes `entered` for packets it admitted and `left` for packets it delivered, so the detector sums both halves per pair. |
| 12 | BUDGET_STATS | `events_lost:u16, reports_refused:u16` (4) | Written by the logger for origin X (switch nibble = X) each time X's heartbeat arrives: X's estimated punt loss, and X's reports sent minus reports the logger received. Tells the detector how much to trust silence. |
| 13 | RAW_SAMPLE | `src:u32, dst:u32, proto:u8, size:u16, ttl:u8` (12) | **Truth runs only**, to explore. Compiled out of claims. |
| 14 | RAW_KEEPALIVE | `port:u8, peer:u8, delay_100us:u16` (4) | **Truth runs only**, to measure link delays, which the map doesn't give. |
| 15 | ESCAPE | `ext_type:u8, ...` | Room to grow. |

Customer ids are indexes into `input.customers` (address order, known at init),
so a `cust` is one byte.

Not every type is needed per claim. A claim for one egg should write only the 2
or 3 types its detector reads, and a truth run can write nearly all of them.

## 4. Where to put the logging switch

The map is a 3 x 5 grid (id = row * 5 + col). Distance to every other switch, in hops:

| Candidate | Degree | Sum of hops to all | Worst hops |
|---|---|---|---|
| **7** (center) | 4 | **28** | **3** |
| 6 or 8 | 4 | 31 | 4 |
| 2 or 12 (top or bottom middle) | 3 | 33 | 4 |
| 1, 3, 5, 9, 11, 13 | 3 | 36 to 40 | 5 |
| 0, 4, 10, 14 (corners) | 2 | 45 | 6 |

### Center (7, or 6 or 8)

Pros:
- Shortest reports: at most 3 hops, so less delay and fewer chances to drop one in a 4-packet queue.
- Four incoming links, so one link failure doesn't cut it off.
- Its own observations are free, and a central switch sees a lot of through traffic.

Cons:
- The center switch likely carries the most customer traffic, and reports queue behind customers. The reports of far switches may be dropped exactly when the network is busiest.
- Its own controller's 80 events a second are shared between watching traffic and relaying reports.
- Planted failures that hit the logger (a switch or CPU failure) would end the whole log. A central switch is more likely to be a target than a corner. We can't know this ahead of the first truth run.

### Edge or corner (0, 4, 10, 14)

Pros:
- Light through traffic, so reports pass less congestion near it.
- Less likely to be hit by a failure egg that targets the busy middle.

Cons:
- Up to 6 hops, so the most latency and the most dropped reports, and the last reports of a run can fail to arrive before 40 s.
- Degree 2: if one of its two links fails, half its reports have to go around.
- It sees little customer traffic itself.

### Recommendation

- Use **7** for truth run 1. It gives the best coverage while we still don't know the load or where failures land.
- Decide the claim placement from the truth logs: where traffic concentrates, and whether reports from far switches arrive complete.
- The logger is chosen by id in the program: only one switch asks to be the logger. If more than one asks, the later asks are refused.
- It is also worth sending the same data from two different switches in two different truth runs, to compare.

## 5. How much can the logging switch hold, and should it write now or wait?

### What it can hold

- The program is a struct that lives for the whole run, so state persists between events.
- Wasm linear memory is capped at 128 MiB per instance by default (`src/wasm.rs`). The reference module is 116 KB. Memory is not the binding limit. The log's own cap is 24 KB, so even buffering every byte we might ever write is trivial.
- Dedupe keys: at most about 6,000 records fit in 24 KB. At roughly 16 B a key, the whole dedupe set is under 100 KB.
- The limits that actually matter are fuel per call (4 M by default, an assumption for A3), the 4,096 B of output per call, and the event and report budgets above.
- Data-plane state is smaller: 16 counter arrays x 4,096 slots, 16 register arrays x 4,096 slots. That is where per-customer counters and distinct-count bitmaps live.

### Write immediately or hold?

Holding costs:
- Anything held at 40 s is never written.
- A held record that is still around when the log fills is lost.
- Delay blurs time, though the observation time travels in the record.

Holding buys:
- Dedupe: one link failure is noticed by **both ends**, and a flood delivers the same report by several paths.
- Merging of repeated or flapping events into one record.
- Roll-up of a rate into a window summary.

Recommendation: split by kind of record.

| Kind | Policy | Reason |
|---|---|---|
| State changes (LINK_DOWN, LINK_UP, SWITCH_SILENT, CUSTOMER_GAP) | **Write the first report at once**, remember its key `(type, subject, t10 bucket)`, drop duplicates that arrive within a window of about 300 to 500 ms | Rare and cheap, and latency of recording is zero. Duplicates are caught after the fact, so nothing is held back. |
| Flapping | Hold a short window (about 1 s) for one link. If it flaps, write one summary record. | Stops a flapping link from eating the log. |
| Rates (CUSTOMER_RATE, PORT_DROPS, LINK_QUALITY) | Aggregate in memory, flush per window (about 1 s). Write only a window that differs from the last by a threshold. | These are steady-state, so most windows are "same as before". |
| Final flush | Flush everything held at about 39.5 s, and again as late as possible | Anything still in memory at 40 s is lost. Time the flush after the last report can still arrive. |

Rules of thumb:
- A held window must be shorter than the smallest thing a detector has to resolve (2 s pause vs 10 s pause is not a problem, but a "flapping" egg may be).
- Cap the number of held records and write the oldest first when the cap is hit.
- Reserve some log space for the last seconds, for example by rate-limiting writes so the log is never full before 40 s. The practice log filled at 35.6 s because I wrote every event.

## 6. Open questions to settle in the first truth run

1. The real values of `punt_events_per_s`, `export_bytes_per_s` and the report queue size, read from `input.observe` and written once into the first record.
2. Link delays (RAW_KEEPALIVE): the map doesn't give them, and they set how late the farthest reports arrive.
3. How concentrated the traffic is. In the practice run, switches 3 and 4 were offered about 100k punts each and the others about 25 to 35k.
4. Whether each customer's packet shape (size, TCP or UDP share, rate) is stable enough to fingerprint it. Ranges are reshuffled in claims.
5. Whether reports from the farthest switch reach the logger fully, with and without heavy load.

## 7. Implementation status (`a3log/`)

All 15 types are implemented (type 15, the escape, is reserved and never written).
`a3log/src/fmt.rs` is the format, shared by the switch and the `logdump` reader;
`a3log/src/lib.rs` is the switch program. Build and run:

```sh
(cd a3log && cargo build --release --target wasm32-unknown-unknown --features practice)   # logger = switch 1
(cd a3log && cargo build --release --bin logdump)
./target/release/competitive_net_sim run-world worlds/practice/truth/run-a.toml --load-scale 1.0 \
    --program a3log/target/wasm32-unknown-unknown/release/a3log.wasm --team-log run-a.team --log run-a.simlog
a3log/target/release/logdump run-a.team 65536 [--all]
```

Without `--features practice` the logger is switch 7 (the real network).

How it works:
- The data plane counts instead of punting: packets and bytes entered per (src, dst) customer, packets left, acks and UDP, and keepalives per port. Counters cost no event budget.
- It punts only a few cheap samples: every 16th keepalive per port (peer and delay), every 64th customer data packet (raw sample), every small customer data packet under 128 B (scan hunting), and reports.
- Link down and up come from the keepalive counter going quiet or rising, not from punts, so a busy event budget can't fake a dead link.
- Non-logger switches batch records into one report per timer and flood it; the logger writes each record and does the dedupe, the dead-switch inference (two switches report the same peer down) and the budget stats.
- Raw samples and raw keepalives stop once the log holds 40,000 bytes, so the rest keeps room.

Findings that changed the design:
- `input.customers` lists only the 6 sending customers in the practice world, not their peers, and the order is by address. Peers show up as customer "none".
- Registers don't persist across TinyVM stages; values are passed through metadata. A stage may touch one memory resource, and there is no multiply, so the (src, dst) index is built from two table lookups that each set a metadata value.

Practice results (6 switches, 40 s):

| Run | Load | Log bytes | % of 64 KB | Records | Raw samples |
|---|---|---|---|---|---|
| a | 1.0 | 22,460 | 34% | 1,932 | 77% of bytes |
| b | 1.25 | 24,077 | 37% | 2,050 | 78% |
| c | 1.5 | 25,638 | 39% | 2,160 | 80% |

- Without raw records the whole log is about 5 KB, so a claim's 24 KB is not a problem for the structured types.
- Ground truth from the full log for run c: 0 events lost to the budget, 0 reports lost, 0 refused log writes.
- The planted eggs are visible: LINK_DOWN from both S1 and S2 at 17.13 s (the link failed at 17.2 s), a 12.03 s CUSTOMER_GAP for Spotify from 13.56 s, and SCAN_SUSPECT at 16 then 50 distinct addresses from the scanner's host.
- Not yet tested: the 15-switch network (about 2.5x the traffic and customers, so the raw samples could reach the 40 KB guard), a SWITCH_SILENT case, and the link-quality and delay thresholds on a link that degrades.

## 8. The 15-switch stress world (`worlds/practice15/`)

`gen15.py` writes `run-a/b/c.toml` (loads 1.0, 1.25, 1.5): the real 3 x 5 grid, 22 apps (11 customer
pairs: 3 Netflix, 2 Fortnite, 2 Steam, 2 Amazon, Jane Street, Ping), and one plant of every kind the
simulator has: `fail` (7-8, 15 to 21 s), `keepalive_gap` (9 to 14, 21 to 27 s), `pause`, `scan`,
`side_flow`, `link_delay` (2-3), `wire_loss` (11-12, 8%) and `link_capacity` (6-7, 10 to 3 Mbit/s at 18 s).
Build the program without `--features practice` so the logger is switch 7.

What it found, and the fix:
- Flooding reports does not scale. With 15 switches each report was punted by every switch, up to
  4 times, and the event budget (80 a second) was gone: switch 7 lost about 4,000 events, reports and
  heartbeats went missing, and one false `SWITCH_SILENT` appeared.
- Now the logger floods a **beacon** once a second (`[seq u16][sender's distance u8]`), each switch
  remembers which neighbour is closest to the logger, and reports go **hop by hop up that tree**.
  A dead parent port is skipped for the next-closest live one.
- Non-logger switches send at most one report a second (250 ms for link, gap and scan records, and
  at once after 39.3 s), so the logger sees about 15 reports a second.
- Result on all three runs: 0 events lost to the budget, 0 refused log writes.

| World | Run | Log bytes | % of 64 KB |
|---|---|---|---|
| 15 switches | a / b / c | 40,899 / 41,080 / 40,965 | 62% / 63% / 63% |
| 6 switches | a / b / c | 23,457 / 25,128 / 26,802 | 36% / 38% / 41% |

At 15 switches the raw-record guard (40,000 bytes) is what stops the log at about 41 KB; the
structured records are about 12 KB.

## 9. `a3truth/`: the truth-run switch

A copy of `a3log/` with the gaps from section 8 closed. Differences:
- **Counters:** one matrix array per group instead of one per counter (a TinyVM stage may touch only one
  memory resource, and there are 8 stages). Blocks: entered packets, bytes, left, duplicates (a data packet
  with the same `seq` as the one before it, found with a register), acks, UDP, and cumulative TTL below
  60, 30 and 10; a per-port array holds keepalives by ingress port and bytes sent by egress port.
- **PORT_STATS (type 4)** replaces PORT_DROPS: `peer, drops:u16, kbytes_out:u16` every 2 s per switch
  port when it changed by 25% or more, dropped something, or every 10 s.
- **FLOW_LOSS (type 11)** is now every (src, dst) pair, with "not a customer" as 255, every 5 s, with
  `dups:u16` (8-byte body).
- **CUSTOMER_PROFILE (type 10)** every 5 s, with `idle_pct`, burstiness (`cv` of packets per 100 ms,
  times 32) and the share of packets with TTL below 60, 30 and 10 (9-byte body).
- **CUSTOMER_RATE** also reports peers and other non-customers as customer 255.
- Customers are capped at 25 (the matrix has to fit in 4,096 slots).

Results (logger 7 on the 15-switch worlds): 42 to 43 KB of 64 KB, 0 events lost, 0 refused writes.
Structured records are about 12 KB (PORT_STATS 2.3, FLOW_LOSS 3.0, PROFILE 0.9, RATE 4.8); the rest is
raw samples, which stop at 40 KB.

Checked in the extras world (`run-x`): duplicates (a Fortnite sender echoing: `dups` about half of
`entered`), a TTL ladder (profile shows 100% of packets under TTL 10), a split route (both ports out of
switch 0 carry traffic), and every earlier plant (link fail, one-way keepalive gap, leak, delay,
capacity drop, pause, scan).

### Changes after the first truth build
- Every 4th keepalive is punted (was every 16th): more delay samples per port.
- **PORT_STATS (type 4)** is now `peer, drops:u16, kbytes_out:u16, ka_lost:u8` (6-byte body): `ka_lost` is the
  keepalives that did not arrive in the 2 s window (expected minus counted, scaled to the window's real length), and a
  record is written whenever it is above 0. A missing record means 0, so a detector can sum it per link. Windows with
  the link down are skipped. Healthy links show about 1 stray in 240 windows from timing; a 3% leak showed 4 records on the
  leaky link, against 0 elsewhere, so it is a weak signal and should be read next to `drops`.
- **CUSTOMER_PROFILE (type 10)** has a 10th byte, `ack_pct`.
- Raw samples now include acks and unnumbered packets (side flows, probes), not only numbered data.
- Practice results: 15-switch worlds 47 to 48 KB (72%); 6-switch practice worlds about 40 KB (62%, raw samples reach their 40 KB guard); 0 events lost, 0 refused writes.
- **Raw records are rationed per second** at the logger: 800 bytes a second in all and at most 80 from any one switch, so they
  cover the whole run (before, they were used up by about 20 s). A backstop stops them at 50 KB. Small data packets (the scan
  packets) are sampled raw as well as counted. 15-switch worlds: 41 to 44 KB (65%); 6-switch practice worlds about 20 KB.

### The first truth run did not finish (and the fix)
The server reported that the program stopped on a switch. Reproduced with a 23-customer world: `the event did not fit in the
4,096-byte input buffer`. The controller receives a copy of every counter array with each timer, so counter arrays are limited by that
4,096-byte event, not by the 4,096 slots per array. The matrix layout (9 blocks of W x W) overflowed from about 19 customers.
Now: one counter array with per-row blocks (packets, bytes, dups, UDP, acks, TTL below 60, 30, 10), a per-destination block for packets
left, per-pair blocks for packets entered and left only while W <= 24, and 64 slots for keepalives and bytes by port. At most about 1,500
slots at any customer count (customers are capped at 60). `FLOW_LOSS` without pair blocks reports sources and destinations separately
(255 for "not a customer"). `CUSTOMER_RATE` is written only when a rate moved by a fifth, crossed zero, or every 5 s. Lesson: test with
many more customers than the practice world has.
