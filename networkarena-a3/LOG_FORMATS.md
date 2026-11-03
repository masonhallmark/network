# How the truth logs are made and read (truth1 and truth2)

Two switch programs, one log format. `a3truth/` made the log of truth run 1 (`submission/truth1/`, with the result in
`submission/truth1/result/`). `a3truth2/` is the budget-aware build for the next run (`submission/truth2/`). Both write the same
records; what differs is how records travel to the logging switch (always switch 7) and a few record meanings. Each directory has its
own `logdump` reader. Use the one that matches the build.

## 1. How a log is generated

```
packet --> data plane (every switch)         counters, and a few punts
                |
                v
        controller (every switch)            every 100 ms: read counters, turn them into records
                |   own records + relayed records
                v
        report packets  --(up the map)-->  switch 7        a report is [sender][records]
                                              |
                                              v
                                       actions::log_write  --> team.log  (only switch 7's writes count)
```

**Data plane.** One TinyVM program counts every packet and punts only samples. A punt costs one event of the switch's budget
(`punt_events_per_s`); the counters cost nothing. The counters live in one array that is copied to the controller on every timer. The
whole timer event must fit in 4,096 bytes, so the array is small:

- per source row (row 0 = not a customer, row c+1 = customer c): packets entered, bytes, duplicates, UDP packets, acks, and packets with TTL
  below 60, 30 and 10;
- per destination column: packets that left toward a customer port;
- per (source, destination) pair: packets entered and left, only while there are 23 customers or fewer;
- per port: keepalives that arrived by ingress port, and bytes sent by egress port.

Punts: 1 = a sample of keepalives (who is on the port, link delay), 2 = a raw sample of a customer packet, 3 = scan hunting, 4 = a report.
Truth1 punts every 4th keepalive, every 64th customer packet and every small data packet from a customer (this flooded the event budget).
Truth2 sets the rates from the event budget (every 8th keepalive and 256th packet, or 16th and 1024th below 40 events per second) and
punts the first small packet of each new (source, destination) pair.

**Controller.** On every timer it works out windows and writes records. The window lengths are the record's cadence in section 3.
Link down needs 1 s of keepalive silence; a customer gap needs 1 s without packets.

**Transport (truth1).** Each non-logger switch put its records in a report of up to 300 bytes about once a second (250 ms when urgent) and
forwarded it hop by hop up a tree built from a beacon flood (`[seq][distance]`). A report was `[origin u8][seq u16][records]`. It did not
look at the export budget, so on the real network nearly every report was refused (not sent) and the log was almost only switch 7's own
records.

**Transport (truth2).** The switch reads `input.observe` and keeps a mirror of its export token bucket (full at the start, refilled at
`export_bytes_per_s`, capped at the burst). A report costs 40 bytes plus its payload. Records wait in a queue of at most 1,500 bytes, most
important first (link changes, gaps and scans; then heartbeats, link quality, port, profile and flow records; then rates; then raw samples),
and the least important, oldest record is dropped when the queue is full. A report is `[sender u8][records]`, up to 180 bytes of records,
and goes when something urgent is waiting, when the oldest record is 1.5 s old, or when a chunk is full, and only if the bucket can pay
for it. A switch relays by adding the records it receives to its own queue, so one header carries many records. The route is fixed from
the map: each switch sends to the live neighbour nearest switch 7 once it knows who is on its ports (from keepalive samples and from the
sender byte of reports). Duplicate records are dropped by a hash.

**Logger.** Switch 7 writes each record it makes or receives with its own `log_write`. It also: adds `SWITCH_SILENT` when two switches report
the same peer down within 1 s; adds `BUDGET_STATS` when a `HEARTBEAT` arrives; and rations raw records so they cover the whole run (800
bytes a second in all, at most 80 from one switch, stopping at 50 KB written). Truth2 also writes one `EXT OBSERVE` record first.

## 2. Record format

```
byte 0      type << 4 | switch      switch = the switch that saw it (0-14); type 0-15
bytes 1-2   t10  u16 little-endian  observation time in 10 ms ticks (divide by 100 for seconds)
bytes 3..   body, its length fixed by the type
```

All integers are little-endian. A record is 3 + body bytes. The parser stops at the first byte whose type has no length (it never
happens in the current builds).

| Type | Name | Body | Meaning |
|---|---|---|---|
| 0 | HEARTBEAT | `records_originated u16, events_lost_est u16` | Every 5 s from every switch, and once more at about 39.3 s. Cumulative. `events_lost_est` is keepalive samples the switch should have had and did not. In truth1 the first field counts report packets sent (not records). |
| 1 | LINK_DOWN | `peer u8` | No keepalive on a port for 1 s. Time = the last keepalive heard. `peer` 255 = unknown. |
| 2 | LINK_UP | `peer u8` | Keepalives came back. |
| 3 | LINK_QUALITY | `peer, loss_pct u8, delay u16` | Written when 10 or more percent of a second's keepalives were missing, or the delay moved. Delay is in units of 100 µs. |
| 4 | PORT_STATS | `peer u8, drops u16, kbytes_out u16, ka_lost u8` | Per switch port every 2 s when it changed by 25%, dropped something, lost keepalives, or every 10 s. `ka_lost` = keepalives missing in the 2 s window; no record means 0. |
| 5 | CUSTOMER_RATE | `cust u8, pkts u16, kbytes u16` | Packets and kilobytes a source sent in the last second. `cust` 255 = not a customer (a peer). Written when it moved by a fifth, crossed zero, or every 5 s. |
| 6 | CUSTOMER_GAP | `cust u8, start u16, dur u16` | A customer sent nothing for 1 s or more. Written when it resumes, or at 39.3 s if still silent. `start` and `dur` are in ticks. |
| 7 | SCAN_SUSPECT | `src_host u8, src_cust u8, target_cust u8, distinct u8, window_start u16` | One source sent small packets to 16 (and later 50) distinct addresses inside 5 s. `target_cust` 255 = not a customer. |
| 8 | TOP_TALKER | `cust u8, share_pct u8` | Every 5 s, the customer with the most bytes at that switch and its share. |
| 9 | SWITCH_SILENT | `switch u8` | Written by the logger when two switches say the same peer went down. |
| 10 | CUSTOMER_PROFILE | `cust, avg_size/8, udp_pct, idle_pct, rate_bucket, burst_cv32, ttl_lt60_pct, ttl_lt30_pct, ttl_lt10_pct, ack_pct` (10 B) | Every 5 s per active customer. `idle_pct` = share of 100 ms ticks with no packet; `burst_cv32` = 32 x (standard deviation / mean packets per tick); `rate_bucket` = log2(packets per second + 1). |
| 11 | FLOW_LOSS | `src u8, dst u8, entered u16, left u16, dups u16` | Every 5 s. With the pair array (23 customers or fewer): per pair, `entered` is written by the source's switch and `left` by the destination's. Otherwise: a source row (`dst` = 255) and a destination column (`src` = 255). `dups` is on a source row record. Sum records across switches before comparing. |
| 12 | BUDGET_STATS | `events_lost u16, reports_refused u16` | Written by the logger for the switch named in the header when its heartbeat arrives. `reports_refused` = records (truth2) or report packets (truth1) the switch made minus those the logger had received. Truth2 counts records dropped locally by priority too. |
| 13 | RAW_SAMPLE | `src u32, dst u32, proto u8, size u16, ttl u8` | A customer packet seen at its first switch (addresses are in memory order: src bytes read as a little-endian u32). `proto` 0 = ack. |
| 14 | RAW_KEEPALIVE | `port u8, peer u8, delay u16` | Link delay samples (100 µs units) every 2 s per port. |
| 15 | EXT | `ext u8, 12 bytes` | Truth2 only. Ext 0 = OBSERVE: `events_per_s u16, backlog u16, export_bytes_per_s u32, burst u32`, first record, switch 7, time 0. |

## 3. Reading it

```
logdump team.log 65536            # size per type, bytes by second, then the records except the high-volume types
logdump team.log 65536 --all      # also CUSTOMER_RATE, FLOW_LOSS, PORT_STATS, RAW_SAMPLE, RAW_KEEPALIVE
```

Use `a3truth/target/release/logdump` for truth1 logs and `a3truth2/target/release/logdump` for truth2 logs. They differ only in the
HEARTBEAT label and the EXT record. To parse by hand, loop: read the type from the high nibble of byte 0, look up the body length in the
table, read the record, advance by 3 + body; stop on a short record.

**Customer ids.** `cust` is a position in `input.customers`, which lists only the sending customers and is in ascending address order. It is
not in `customers.txt`, which lists every range, peers and targets included. To name a customer, match raw samples to it: a `RAW_SAMPLE`'s
source address falls in one range of `customers.txt`, and the record's switch is the customer's attach switch. In truth run 1, customer
ids reached 28 (29 or more customers). Switch 7's customers were Cloudflare, Login and Fastly. Ids are only meaningful within one run: in
a claim the ranges are reshuffled, so identify a customer by its profile and attach switch.

**Things to know when reading.**
- A missing record means "nothing to report", not "no data" for `PORT_STATS`, `CUSTOMER_RATE` and `CUSTOMER_GAP`. Check the `HEARTBEAT` and
  `BUDGET_STATS` records to see whether a switch's records reached the logger at all.
- Times are observation times at the originating switch; a record may be written seconds later (truth2 holds records up to 1.5 s, more when
  the budget is tight).
- Truth run 1's log came almost entirely from switch 7 (494 of 705 records), so it shows switch 7's links and customers well and the rest
  of the network only by a handful of records.
- `LINK_DOWN` at 10 or more ticks of silence can also be a keepalive gap with the link up; the other end telling you it still hears
  the peer separates the two.
