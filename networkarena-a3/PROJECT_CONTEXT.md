# CS356 Assignment 3 ("Make It Visible"): project context and handoff


## >>> READ THIS FIRST: current state, rewritten 2026-11-03 (afternoon). Sections 1-13 further down are history; where they disagree with this block, this block wins. <<<

### 1. What the project is
CS356 (UT Austin, Fall 2026) Assignment 3, "Make It Visible", team of 2. A simulated 15-switch network (a 3 x 5 grid) runs for 40 simulated seconds with about 30 customers. **20 hidden "eggs"** (yes/no facts about what happened)
are planted in it: links that go down or work badly, customers that pause, unannounced traffic such as scans. We do not run the network; we write two WebAssembly programs:
- `switch.wasm`: runs on every switch. It can count and sample packets, send reports to one chosen **logging switch**, and that switch writes a small byte log in a format we design.
- `detect.wasm`: reads that log after a run and answers true/false for ONE egg.

Two kinds of upload (Canvas, one at a time): a **truth run** (3 per team; upload only `switch.wasm`; we get back `team.log` of up to 64 KB and `customers.txt`) to *look at* the hidden network, and an **egg claim** (30 per team;
`switch.wasm` + `detect.wasm`) to *win* eggs. A claim does not say which egg it targets; the server tests it on every egg not yet won (10 fixed runs per egg: egg on, egg off, or a "near miss" where something similar happens) and the
card lists the eggs won. One wrong answer on an egg loses it. A claim wins at most one egg. The claim log cap is 24,576 B. In claim runs customer address ranges are reshuffled (recognise customers by behaviour), but the map is the same.
Grading: 20 eggs, 200 points (easy 4, medium 8, hard 16); 150 points = 100%. Handout: `~/Downloads/CS356-A3-Handout-2026-10-23.pdf` (read with `pypdf`; no `pdftotext`). **Deadline: end of Tuesday 2026-11-03, Central.**

### 2. Status (as of this writing)
- **Eggs won: 2.** Egg 7 (Traffic, "The traceroute") with `claims/ttl_ladder`; Egg 14 (Link, "The one-way slow link") with `claims/link12_delay`. Card categories seen so far: Traffic, Link.
- **Claims used: 4 of 30.** Claim 1 ttl_ladder WON; claim 2 `claims/link16_flap` (link 1-6 flaps, 2 or more) won nothing; claim 3 link12_delay WON; claim 4 `claims/link16_flap3` (3 or more flaps) won nothing.
- **Built, tested, NOT submitted:** `claims/link05_outage` (link 0-5 goes down a few seconds and comes back; logger = switch 0).
- **Truth runs used: 2 of 3** (the third is unspent; see section 6). Truth run 1 = `submission/truth1`, truth run 2 = `submission/truth7` (the folder name is the build number, not the run number).
- Do not spend a truth run or a claim without asking the user: both are limited and irreversible. Resending identical files is useless.

### 3. Where the truth-run data is, and how to read it
| What | Where |
|---|---|
| Run 1 log, 8,246 B, 705 records (build `a3truth`) | `submission/truth1/result/team.log` |
| Run 1 customer map | `submission/truth1/result/customers.txt` |
| Run 1 decoded text (`logdump --all`) | `submission/truth1/result/logdump-all.txt` |
| Run 2 log, 8,822 B, 783 records (build `a3truth7`) | `submission/truth7/result/team.log` |
| Run 2 customer map / decoded text | `submission/truth7/result/customers.txt`, `logdump-all.txt` |
| The uploaded programs | `submission/truth1/switch.wasm`, `submission/truth7/switch.wasm` (sha256 beside each); sources `a3truth/`, `a3truth7/` |

`customers.txt` is identical in both runs (51 lines: `address-range  name`; names like "Mapper", "Netflix live" and "...'s peer"; some /32 hosts sit inside a /24). Customer ids in the log are positions in the *sending* customers in address order,
not line numbers in `customers.txt`; match raw samples to ranges to name one (ids 0-29: 4=Ping, 9=Mapper and 29=Offsite are confirmed by raw samples, 3=Backup is likely; the rest are a guess). See `LOG_FORMATS.md` "Customer ids".

**Log format** (little-endian; full table with meanings in `LOG_FORMATS.md`, code in `a3truth7/src/fmt.rs`): each record is `byte0 = type<<4 | switch`, `u16 time in 10 ms ticks`, then a fixed-length body by type.
Types 0-15: HEARTBEAT, LINK_DOWN, LINK_UP, LINK_QUALITY, PORT_STATS, CUSTOMER_RATE, CUSTOMER_GAP, SCAN_SUSPECT, TOP_TALKER, SWITCH_SILENT, CUSTOMER_PROFILE, FLOW_LOSS, BUDGET_STATS, RAW_SAMPLE, RAW_KEEPALIVE, EXT.
Body lengths: 4,1,1,4,6,5,5,6,2,1,10,8,4,12,4,13. Run 2 adds EXT subtypes (first body byte): 0 OBSERVE (budgets), 3 CCA, 4 CCA_TRACE, 5 REPORT_QDROP, 6 DELIVERY (per-switch count of reports that reached the logger).
`peer` 255 = not learned. `LINK_QUALITY` and `RAW_KEEPALIVE` delays are in units of 100 us. Raw-sample `proto` 0 = an ack (40 B), 6 TCP, 17 UDP.

**Three ways to read a log** (run from `networkarena-a3/`):
```sh
python3 tools/parselog.py submission/truth7/result/team.log                     # pure Python, one line per record
python3 tools/parselog.py submission/truth7/result/team.log --type LINK_UP --switch 0
a3truth7/target/release/logdump submission/truth7/result/team.log 65536 --all   # Rust reader (add --all for the high-volume types)
less submission/truth7/result/logdump-all.txt                                    # already decoded; truth1's: a3truth/target/release/logdump
```
`import parselog; parselog.parse(open(path,'rb').read())` returns `(seconds, switch, TYPE, {field: value})` tuples. Build the Rust readers with `cargo build --release --bin logdump` in `a3truth/` or `a3truth7/`.

**Read these caveats before drawing conclusions:**
- **Coverage is thin.** Reports from other switches mostly never reached the logger: only 46 report packets in run 2; switches 3 and 12 sent nothing and switch 8 stopped about 17-20 s. About 70% of the records are switch 7's own (it was the logger).
  Absence of a record is not evidence that nothing happened. A `LINK_DOWN` for a link can be missing while its `LINK_UP` arrived (link 0-5). Use `HEARTBEAT`, `BUDGET_STATS` and `EXT DELIVERY` records to see which switches delivered.
- `FLOW_LOSS.entered/left` are sampled by the 80 events/s cap; do not read start times or exact counts from them. `CUSTOMER_GAP start=0.10` means "silent from the start", not "paused at 0.1 s".
- Records named `peer=255` at switch 7 (`LINK_QUALITY ... loss=10%`) are most likely an artifact of switch 7's own overloaded event queue, not a leaky link.
- Real observation budgets (first log record): 80 events/s, backlog 64, 16,000 B/s export, 8,000 B burst. Every timer input and every output must fit 4,096 bytes; 4M fuel per call.

### 4. What the truth logs show: egg candidates (full evidence and caveats in `EGG_CANDIDATES.md`, "Current status" at the bottom)
| Candidate | Status |
|---|---|
| Traceroute at switch 4 (customer "Mapper", TTL ladder 3-7) | WON (egg 7) |
| Link 1-2 one-way delay (30.0 ms from switch 2 to 1, 2.0 ms the other way) | WON (egg 14) |
| Link 13-14 one-way 25 ms (S14 hears S13) | probably the same egg as 14, so cannot be won again; low priority |
| Link 1-6 down twice for 1.63 s (5.91 and 17.95 s, same in both runs) | real; the 2-flap and 3-flap rules both won nothing. Untried: one flap, shorter minimum duration, other definitions |
| Link 0-5 outage (S0 logs LINK_UP at 36.00 s, no down record; link up at 28.56 s) | claim built, not submitted |
| Link 12-13 lossy / congested (40-50% keepalive loss, delays up to 69 ms, 172 drops) | not built; may be ordinary congestion from the Backup customer |
| Sync pair silent 0-20 s at S1; 2 s gap at S3; Home users UDP side stream; Cust 15 surge at S7 | low confidence, not built |
| Scans, duplicates, switch failure, congestion-control sawtooth, customer pauses of 10 s or more | not seen (reports mostly lost); only a better truth run could show them |

### 5. How we build a claim (reuse this)
Lesson learned the hard way: **put the logging switch at the switch that sees the egg and log locally**; do not rely on reports crossing the network (they failed in run 2). Both wins did this.
Each claim is a folder `claims/<name>/{switch,detect}/` (two small Rust crates, `switch_program_sdk`, built for `wasm32-unknown-unknown`) plus `switch.wasm`, `detect.wasm`, `hashes.sha256`, `README.md` (the bet, rule, tests, what it depends on).
- `claims/ttl_ladder`: logger = switch 4; data-plane counters by TTL; detector asks for a ladder of small TTLs.
- `claims/link16_flap`, `link16_flap3`, `link05_outage`: logger = a switch at one end of the link; data plane counts keepalives per ingress port, 1 in 4 punted to learn each port's peer; controller writes a 7-byte record when a port goes silent 0.5 s and when it is heard again.
  Detector counts recovered silences of at least 1-2 s on the port whose peer is the target switch (`MIN_FLAPS`, `MIN_DOWN_MS` constants). To retarget a link change `LOGGER` in the switch and `PEER` in the detector.
- `claims/link12_delay`: logger = switch 1; both ends measure delay toward themselves from keepalive send times in 1 s windows; the far end sends a one-hop report. Detector: one direction steadily at least 10 ms and the other at most a quarter of it.
- **Practice worlds and checks.** `worlds/practice15/gen_*.py` write 10-run practice claim sets (and 40-run stress sets) on the real 15-switch map with the other truth-log anomalies switched on; `scripts/practice-check.py --switch S.wasm --detect D.wasm --worlds DIR --egg N` scores them like the server.
  Every claim passed both its 10-run and 40-run sets locally, yet claim 2 and 4 still won nothing: **local tests only check our own guess of the egg's definition and near misses**, they cannot confirm the grader's.
- The simulator source gives the egg vocabulary: `src/sim/plant.rs` (plants: `fail`, `link_delay` with per-direction delays, `wire_loss`, `link_capacity` steps, `pause`, `side_flow`, `scan`) and `src/app.rs` (`echo` = duplicates, `ttl_ladder`, `hosts`, `proto`).
  Unusually named customers in `customers.txt` (Mapper was one) are likely egg vehicles: Netflix live + viewers, Fortnite player, Prime member, Dropbox-2, Cam-A / Cam-B, Ping.

### 6. Suggested next steps
1. Decide whether to submit `claims/link05_outage`.
2. More flap definitions, e.g. a single down-and-back of 0.5 s or more, or shorter outages. Each is a one-constant change to `detect/src/lib.rs`, rebuild, retest.
3. Build a link 12-13 loss detector (logger switch 13, keepalive loss in 1 s windows), lower confidence.
4. The last truth run is the only way to see customer, scan and duplicate eggs. If spent: logger away from busy switch 7, log per-customer silences, packets to many addresses and repeated packets, and test with many customers first (see Lessons).
   The handout says each truth run has a heavier load, and the log cap is 64 KB (we used about 9 KB).

### 7. Tools and commands (all under `networkarena-a3/`)
```sh
cargo build --release                       # simulator -> target/release/{competitive_net_sim,simlog_dump}; rustup target add wasm32-unknown-unknown once
(cd claims/link05_outage/switch && cargo build --release --target wasm32-unknown-unknown)   # likewise detect/
python3 scripts/practice-check.py --switch claims/link05_outage/switch.wasm --detect claims/link05_outage/detect.wasm --worlds worlds/practice15/claim_outage --egg 34
./target/release/competitive_net_sim run-world WORLD.toml --load-scale 1.5 --program switch.wasm --team-log x.team --log x.simlog
./target/release/competitive_net_sim detect detect.wasm x.team
A3_STATS=1 ./target/release/competitive_net_sim run-world ...     # prints max call input/output/fuel (our patch in src/wasm.rs; behaviour unchanged)
```
Other helpers: `tools/eggcheck.py` (does a log hold a signature for each planted egg), `tools/ccashow.py`, `tools/decode` (ground-truth drops from a simulator trace), `tools/preflight/` (limit/fuzz scripts), `EGG_CANDIDATES.md`, `LOG_FORMATS.md`, `LOGGING_DESIGN.md`, `submission/*/SUBMIT.md`.
Nothing here is committed to git (`networkarena-a3/` is mostly untracked); build output `target/` is large and ignored.

### 8. Lessons
1. A controller call's input and output must fit 4,096 bytes; each timer carries every declared counter array. This broke our first upload. Test with 100 customers, see memory note `a3-controller-limits`.
2. Test with realistic scale and budgets; the practice network is small and generous.
3. Reports across the network are unreliable under real load; log locally at the switch that sees the egg.
4. Customer ids are per-run positions; do not name customers from ids alone.
5. A win needs the grader's exact definition. After a miss, reason about which definitions fit the loss (`claims/link16_flap3/README.md` shows how) rather than just retrying.

---

## HISTORY (stale in places): the original hand-off written 2026-11-02, with later updates in sections 8-13

Written 2026-11-02 (late evening) so a new session can pick up without the conversation. Read this first, then
`LOG_FORMATS.md` (how our logs are made and parsed), `LOGGING_DESIGN.md` (design history and results) and `submission/*/SUBMIT.md`.

Repo root: `/mnt/c/Users/Mason/Documents/Classes/CS356/network/` (git repo, branch `main`; `networkarena-a3/` is **untracked**, nothing
of this work is committed). The kit is `networkarena-a3/`. Handout PDF: `~/Downloads/CS356-A3-Handout-2026-10-23.pdf` (a duplicate
`(1)` copy is identical); extract text with `python3 -c "import pypdf"` (no `pdftotext`). Contact: Ritul (ritul@utexas.edu).

## 1. The assignment

We write two WebAssembly programs and upload them to Canvas:

- **`switch.wasm`** runs on every switch of a hidden 15-switch network. The network forwards everything itself; the program only
  *watches*: it counts, samples, reports, and one switch (the **logging switch**) writes a log of bytes of our own design.
- **`detect.wasm`** (`fn detect(log: &[u8]) -> bool`, `switch_program_sdk::detect_program!`) reads that log after a run and answers one yes/no
  question about one **egg**. It gets 100 M fuel and 64 MB.

**Eggs** are yes/no questions about the network ("did a customer go quiet for 10 s?", "did one host scan 50 addresses?"). There are
**20 hidden eggs** (4 easy x 4 points, 9 medium x 8, 7 hard x 16 = 200 points; **150 points = 100%**; points above 150 count toward A4's
bonus levels at 100, 150, 200). They are in three skill families: *Failure* (links/switches down or working badly), *Customer*
(traffic of named customers), *Traffic* (unannounced trouble). Every egg is present in all three truth runs. For each egg 10 fixed claim
runs exist: some have the egg (true), some don't (off) and some are **near misses** (something close happens instead). A detector must be right
on all 10 to win the egg; always-true loses.

**Two kinds of upload** (Canvas, one person for the team, one upload at a time, wait for the reply):
- **A3 — Truth run** (3 per team): upload only `switch.wasm`. The server runs the hidden network (loads get heavier each run) and returns
  `team.log` (what our logging switch wrote, cap **64 KB**) and `customers.txt` (address range to customer name). Nothing else.
  A run that does not finish (program crashes or a call is too slow, or run too long) is free for the first 2 such errors; later ones use a run.
- **A3 — Claim** (30 per team): upload `switch.wasm` and `detect.wasm`. Not told which egg; the server tries it on every egg not yet won.
  A claim wins **at most one egg**, so send one claim per egg. Log cap in a claim is **24 KB** (24,576 B). The reply lists eggs won and points.
  Resending identical files is free and useless. A claim running over 1 hour is stopped and counts.
- A2 hidden practice: 10 ungraded extra A2 attempts (not our focus).

**Facts about the network and runs** (SUBMITTING.md): the map is the same in every run: switches 0-14 in a 3 x 5 grid, id = row*5 + col; links
0-1, 0-5, 1-2, 1-6, 2-3, 2-7, 3-4, 3-8, 4-9, 5-6, 5-10, 6-7, 6-11, 7-8, 7-12, 8-9, 8-13, 9-14, 10-11, 11-12, 12-13, 13-14. Speeds and delays
not given. Customer address ranges are **reshuffled every run** (so detectors must recognise customers by behaviour); a run lasts 40 simulated
seconds. Claims run on the same network as truth runs with all other eggs on.
**Deadline: truth runs and claims close at the end of Tuesday, November 3 (Central).** Claims share one queue with the whole class.

**Hard limits found in the simulator** (not in the handout; they matter):
- A controller call's **input must fit 4,096 bytes** (`src/wasm.rs` `call_export`), and its output is also capped at 4,096. Each timer carries a
  copy of **every declared counter array**, so counter slots count against 4,096 bytes. This broke our first truth upload.
- A TinyVM stage may touch only one memory resource (one table, register array or counter array); max 8 stages; registers do not persist
  across stages (use metadata `m0..`); there is no multiply or shift; `table` returns the first matching entry, not the longest.
- Reports (`actions::inject_packet`) are charged `40 + payload` bytes against a per-switch token bucket (`export_bytes_per_s`, `export_burst_bytes`),
  full at the start; an unaffordable report is silently dropped. They also wait behind customer packets in a 4-packet queue per port.
  Each punt (`punt N`) uses one of `punt_events_per_s` events a second (backlog `punt_backlog`). **We do not know the real values**: the
  practice world uses 80 events/s, backlog 64, 16,000 B/s, burst 8,000 B; the real ones are far smaller (see section 4).
- `input.customers` lists only the sending customers (not their peers), in ascending address order. The real network has at least 29.
- The simulator rejects customer ranges that overlap, yet the real `customers.txt` lists /32 hosts inside /24 customers.

## 2. State at hand-off (what has been accomplished)

**Truth runs: 1 of 3 used (assumed; check the Canvas reply).** Upload 1 did not finish (free error 1 of 2); the fixed upload returned an 8,246-byte log.
**Claims used: 0 of 30. No detector has been written.** Not yet spent: truth run 2 and 3.

Deliverables, all in `networkarena-a3/`:

| Path | What it is |
|---|---|
| `a3log/` | First, 6-switch-friendly logging switch (flood transport). History only. |
| `a3truth/` | **Truth-run-1 build** (what was uploaded): counters, 15 record types, tree transport, raw rationing. Has `logdump`. |
| `a3truth2/` | **Budget-aware build** for truth run 2 (see section 5). Has `logdump`. Cargo feature `practice` = logger switch 1 and the 6-switch map; default = switch 7 and the real map. |
| `submission/truth1/` | `switch.wasm` as uploaded (sha256 in `switch.wasm.sha256`), `SUBMIT.md`, and `result/` with the returned `team.log`, `customers.txt` and `logdump-all.txt`. |
| `submission/truth2/` | `switch.wasm` (173,358 B, built from `a3truth2/`, logger 7), hash, `SUBMIT.md` with test table. **Not uploaded.** |
| `LOG_FORMATS.md` | How the logs are generated and parsed (truth1 and truth2): the record table, transport, reading, customer ids. |
| `LOGGING_DESIGN.md` | Design history: record format, event types, logger placement, hold-vs-write, results of each iteration, lessons. |
| `worlds/practice15/` | 15-switch stress worlds: `run-a/b/c.toml` (loads 1.0/1.25/1.5), `run-x.toml` (extras), `gen15.py`; `extra/` has many-customer worlds (`many` 23, `huge` 56, `h100` 100 customers), budget worlds (`*_mid/_tight/_tiny`: 40/20/10 events per second and 1000/200/60 B/s), `leak3` (3% leak), `healthy`. |
| `tools/` | `decode/` (full-simlog decoder: drops, events lost by cap), `killsw/` (run a world and fail switch/CPU N at t ms: the CLI cannot), `eval.sh` (one line per run: failures, origins, ground-truth losses, delivery %), `rawfill.py`. Build each with `cargo build --release` in its dir. |
| `PROJECT_CONTEXT.md` | This file. |

Test discipline we settled on (do all of this before any upload): run the exact `.wasm` on the 3 practice truth worlds
(`worlds/practice/truth/run-{a,b,c}.toml`, 6 switches, so use the `practice` build for a log), the 15-switch worlds, the many-customer worlds
(23/56/100 customers) and the tight/tiny budget worlds, loads up to 2.0, and grep the simulator output for `program failed`. Compare with
`simlog_dump`/`tools/decode` for events lost and refused reports (ground truth).

## 3. What the logs capture (summary; full table in LOG_FORMATS.md)

16 record types: HEARTBEAT, LINK_DOWN/UP, LINK_QUALITY (loss, delay), PORT_STATS (drops, bytes out, lost keepalives), CUSTOMER_RATE,
CUSTOMER_GAP, SCAN_SUSPECT, TOP_TALKER, SWITCH_SILENT (logger inference), CUSTOMER_PROFILE (size, UDP%, idle%, burstiness, TTL shares, ack%),
FLOW_LOSS (entered/left/duplicates), BUDGET_STATS (logger's loss accounting), RAW_SAMPLE, RAW_KEEPALIVE, EXT (OBSERVE budgets, truth2 only).
Records are `[type<<4 | switch][t10 u16 LE][body]`, t10 = 10 ms ticks.

Eggs we believe the logs can show (inferred from `src/sim/plant.rs` plant kinds and world options, not from the handout): permanent or flapping
link failure, one-way keepalive gap, slow link (delay), leaky link (loss), capacity drop, customer pause, scan, duplicate packets (`echo`), TTL
ladder, split/backup routes (per-port bytes), small-queue drops. Known gaps: reordering, per-host behaviour inside a customer, customer
congestion-control behaviour (`cca` is withheld; rate at 1 s resolution is too coarse), small leaks (keepalive loss resolution is 10%/s),
flows to or from a failed switch, anything shorter than 1 s, DSCP/custom fields, loss of the logger (no failover).

## 4. What truth run 1 told us (analysis in `submission/truth1/result/`)

- The run finished but the log was only 8 KB, and 494 of 705 records were switch 7's own. `BUDGET_STATS` showed nearly every other switch's reports were lost
  (switch 2: 136 sent, 0 received at 35.7 s; switch 4: 49 of 50 lost; switch 13: 31 of 33; switch 11: 10 of 10 before 10.2 s). Switch 7 lost most
  keepalive samples (`events_lost_est` 289 over 40 s). Conclusion (inferred, the budgets were never logged): the **real export-byte and event budgets are far smaller
  than the practice world's**, and truth1 ignored them.
- `customers.txt` has 52 rows (customers, their peers and targets): Jane Street, Dropbox (several, "Dropbox-2"), Amazon (shoppers, Prime member), Sync (several), Fastly,
  Cloudflare, Akamai, Ledger, Mapper (+ "Mapper's target"), Netflix and **Netflix live**, Zoom, Fortnite and **Fortnite player**, Ping, Login, Backup, Offsite, Vault,
  Cam-A, Cam-B, Steam, Home users. Three rows are /32 hosts inside another customer's /24 (Netflix live 10.17.72.200, Fortnite player 10.42.152.200,
  Amazon Prime member 10.29.156.200): probably eggs about one host in a range. Customer ids in the log reached 28.
- Observed in the log: link 1-6 down 5.91 s to 7.54 s (seen from switch 6); switch 6 had 168 drops toward switch 7 and 30% keepalive loss on 1-6 around 14 s; switch 7
  shows a steady ~10% keepalive loss on one lightly used port all run (peer unknown): probably a leaky link at switch 7; switch 7's customers were Cloudflare, Login,
  Fastly (raw samples), with customers 16 and 21 pausing in 8 s patterns.
- Later truth runs are **busier** (practice pattern: loads 1.0, 1.25, 1.5), so the design has to hold headroom.

## 5. truth2 (the build to upload next) and what to do

`a3truth2/` fixes the diagnosed problems: reads `input.observe`, logs it (`EXT OBSERVE`), mirrors the export bucket, prioritises records, relays in batches,
routes by the fixed map (no beacons), scales punt rates to the event budget, punts only the first small packet of each new (src, dst) pair for scan hunting, and
inserts longest prefixes first. Verified: 0 program failures, 0 export-refused reports and 95-97% of records delivered at practice budgets up to load 2.0 with up to 100
customers; 50-75% delivered at tight/tiny budgets (rest dropped locally by priority); the truth1 build on the same tight/tiny worlds lost 1,952-4,849 events and had 550-691
reports refused. Switch 7 killed at 20 s: run finishes, log ends at 20 s. Details in `submission/truth2/SUBMIT.md`.

**Immediate next steps**
1. Decide whether to upload `submission/truth2/switch.wasm` as truth run 2 (A3 — Truth run). Read the reply files with `a3truth2/target/release/logdump team.log 65536 --all`; the first
   record gives the real budgets. Save the result under `submission/truth2/result/`.
2. From the truth logs, learn: real budgets, link roles and delays, which customers sit on which switch (attach switch is probably stable across runs), each customer's profile
   (size, UDP/TCP, rate, idle%, burstiness), and which events the eggs correspond to. Map customer index to name with raw samples and `customers.txt` (see LOG_FORMATS.md).
3. Build one detector per egg (each a `detect.wasm` crate like `examples/broken_link_detect/`) plus a **trimmed** switch program per claim that writes only what that detector
   reads, within 24 KB. Test on the practice eggs first (`scripts/practice-check.py --switch s.wasm --detect d.wasm --egg 21`; the kit's solved egg is 21, `examples/broken_link*`).
   Test the false side: near misses (another link, the egg shorter than required, another customer).
4. Spend claims one egg at a time (30 available). Do not exceed the deadline (end of Tue Nov 3, Central). Keep one truth run in reserve if possible.

## 6. Commands

```sh
cd /mnt/c/Users/Mason/Documents/Classes/CS356/network/networkarena-a3
cargo build --release                                   # simulator: target/release/{competitive_net_sim,simlog_dump}
rustup target add wasm32-unknown-unknown                # once
# switch program (real network, logger 7); add --features practice for the 6-switch practice worlds (logger 1)
(cd a3truth2 && cargo build --release --target wasm32-unknown-unknown)    # -> a3truth2/target/wasm32-unknown-unknown/release/a3truth2.wasm
(cd a3truth2 && cargo build --release --bin logdump)
./target/release/competitive_net_sim run-world worlds/practice15/run-c.toml --load-scale 1.5 \
    --program a3truth2/target/wasm32-unknown-unknown/release/a3truth2.wasm --team-log c.team --log c.simlog
a3truth2/target/release/logdump c.team 65536 --all
(cd tools/decode && cargo build --release); tools/decode/target/release/decode c.simlog events.txt   # ground-truth drops and losses
tools/eval.sh worlds/practice15/extra/x_tight.toml 1.5 PROGRAM.wasm a3truth2/target/release/logdump
```

Notes: the working tree uses `/tmp/...scratchpad` only for throwaways; everything needed is in the repo. A `memory/` entry (`a3-controller-limits`) records the 4,096-byte lesson.
Build artifacts (`target/`, ~770 MB) are ignored by git. Nothing has been committed; `.gitignore` shows modified from before this work.

## 7. Lessons (so we do not repeat them)

1. Test with realistic scale: the practice world has 6-13 customers and generous budgets; the real one has 29+ customers and tiny budgets. Both failures came from that gap.
2. Always log what the run gives you about itself (`input.observe`) in a truth run: it is the only look at the hidden network.
3. A run that finishes with a bad log still costs a truth run; only "did not finish" errors are free (first two).
4. Inference about hidden eggs came from the simulator source (plant kinds, world options). Treat it as a guess, and check the handout's wording.
5. Reports are expensive (40-byte header each, per hop); aggregate, prioritise, and keep raw samples last.

## 8. Update (later on 2026-11-02/03): truth3 and truth4 builds
`a3truth3/` (route-based transport) and `a3truth4/` (basic broad log, events sent 3x, load-aware route adverts, delivery audit) were built and tested
(see `submission/truth3/SUBMIT.md`, `submission/truth4/SUBMIT.md`). Neither is uploaded. truth4 delivered 100% of audited events in all practice worlds
down to 10 events/s and 60 B/s; the real budgets are probably near that (truth-1 calibration). Report loss before this came from reports starving on
saturated links (6->7) and from loops around failures, not from the export bucket. Recommended next upload: `submission/truth4/switch.wasm`.

## 9. Update 2026-11-03: truth5 pre-upload tests
`submission/truth5/switch.wasm` (sha256 a45eba6b...) is the recommended truth run 2 (not uploaded). Tests, scripts and results: `submission/truth5/SUBMIT.md`,
`tools/preflight/`, `worlds/practice15/gen20.py` (20 planted things) and `tools/eggcheck.py`. The simulator got a small env-gated patch (A3_STATS=1) to print the
maximum call input, output and fuel; the original is not saved in the repo. Key limits: 4,096 B in/out per call, 4M fuel.

## 10. Update 2026-11-03: truth6 (truth5 + congestion-control records)
`submission/truth6/switch.wasm` (sha256 ca41d71b...) adds EXT CCA / CCA_TRACE records (how each customer reacts to loss; AIMD, RateMatch and Fixed are the only three laws, see `src/flow.rs`).
Before/after numbers: `tools/preflight/before_after.txt`. 117-run limit suite passed (0 failures; max in 860, out 2,493 of 4,096 bytes; fuel 1.74M of 4M).
Test world: `worlds/practice15/gen_cca.py` (CCA_SEP=1 puts each law on its own path); reader: `tools/ccashow.py`.

## 11. Update 2026-11-03: truth7 (truth6 + PORT_SATURATION and DELIVERY records)
`submission/truth7/switch.wasm` (sha256 f7e36e97...) is the newest candidate and the one tested last: 123 limit runs and the 20-planted-things suite pass. Not uploaded.
Starved reports are not dropped; they wait in a ~4,000 byte queue behind customer traffic (so queue-drop counts stay near zero). `tools/preflight/before_after_6to7.txt` has the cost of the two new records.

## 12. Update 2026-11-03 01:55: truth run 2 result (truth7 was uploaded)
Result files: `submission/truth7/result/` (team.log 8,822 B, customers.txt identical to run 1's, logdump-all.txt). **Truth runs used: 2 of 3.**
- Real budgets from the log's first record: 80 events/s, backlog 64, 16,000 B/s, burst 8,000 B (the practice values, not the low ones assumed).
- Reports mostly did not arrive: 46 report packets in 40 s (about 430 in our practice worlds with these budgets); only 33 heartbeats of about 120; switches 3 and 12 sent nothing; switch 8 delivered
  until about 20 s and then stopped. Only 2 duplicate copies arrived, so copies did not help. Cause not determined (candidates: reports starved on saturated links, the logger's event queue overflowing,
  a switch program stopping). Switch 7's own records (529 of 783) are complete.
- Findings on the eggs: link 1-6 down twice for 1.6 s (5.91 and 17.95 s); link 12-13 lossy and slow (40-50% loss, delay up to 28.9 ms); link 6->7 saturated (228 drops); one customer pause of only 1.93 s (switch 3).

## 13. Update 2026-11-03: first claim pair built (not submitted)
`claims/ttl_ladder/` (switch.wasm, detect.wasm, README.md): the TTL-ladder egg at switch 4 (Mapper), logger = switch 4, nothing crosses the network. Practice: `worlds/practice15/gen_claim_ttl.py`
makes `worlds/practice15/claim_ttl/`; 10 of 10 right (and 36 of 36 on a harder set). Egg candidates: `EGG_CANDIDATES.md`. Run: `python3 scripts/practice-check.py --worlds worlds/practice15/claim_ttl --egg 31 ...`.
