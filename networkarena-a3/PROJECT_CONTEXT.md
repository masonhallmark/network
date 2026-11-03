# CS356 Assignment 3 ("Make It Visible"): project context and handoff

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
