# A2 Design Menu — every decision, with options and trade-offs

**Status:** menu only. Nothing here is chosen yet.
**Goal of this doc:** list every decision that A2 forces on us, the realistic
options for each, what each option costs, and which floor (F1/F2/F3) it
helps or hurts. Pick later.

Sources are marked: **[kit]** = read from the A2 kit/handout/source,
**[book]** = Peterson & Davie, *Computer Networks: A Systems Approach*
(book.systemsapproach.org, Ch. 6), **[industry]** = common production practice
(from web search results plus general knowledge; flagged where it is only that).
**[verify]** = something I believe but have not confirmed in the simulator;
test it before relying on it.

---

## 0. Hard facts that constrain every choice

| Fact | Source | Why it matters |
|---|---|---|
| One shared **40,000-byte pool per switch**, all ports draw from it | [kit] LIMITS.md | Queue caps are per-queue, but the pool binds first. More ports/queues = less per queue. |
| ≤ 5 ports per switch (practice worlds: max degree 4 + 1 app port) | [kit] handout, worlds | Worst case 5 ports × several queues competing for 40 packets. |
| Up to **16 queues per port**; disciplines `Fifo`, `StrictPriority`, `Drr` | [kit] | Fifo with several queues behaves like StrictPriority (egress.rs). So effectively two real choices: SP or DRR. |
| `QueueSpec`: `cap` (hard), `weight` (DRR only), `drop_at` (early-drop line, packets) | [kit] | Threshold drop is **deterministic tail-drop at a lower line**, not RED. No probabilistic drop, no ECN. |
| Drop check order: pool full → queue cap → drop_at | [kit] egress.rs | A quiet queue can be starved of pool space by a busy neighbour if caps oversubscribe 40. |
| Switch reports **only drop counts** per port per queue each timer (since last timer). No occupancy, no rates, no delay | [kit] | Any load/queue estimate is inferred from drops. |
| `set_queues` on a port replaces queues; waiting packets carry over by index | [kit] egress.rs | Reconfiguring live is safe but not free of side effects. |
| Actions land `control_delay` = 2 ms after being returned | [kit] | Route changes create a 2 ms mixed-state window (loop risk, §K). |
| Per call: 4M fuel, 4096 B of returned actions (~150 installs) | [kit] LIMITS.md | Bounds how many route/queue changes per call. |
| Link capacities known at `init_with`; neighbours must still be discovered | [kit] | Per-port capacity is free; per-link far-end id is not. |
| Customer contract: prefix, `Cca::{Aimd,RateMatch}`, `ask_bps` (already × load-scale); same list on every switch | [kit] | Classification and weights can be computed statically at init. |
| Customer rows are **senders**; a pure receiver has no row. Fortnite = two rows | [kit] README | Ack/return traffic may carry a non-customer source address **[verify]**. |
| Acks are 40 B, `PacketKind::Ack`; your control packets are 20 B + payload; Jane Street/Fortnite are 200 B | [kit] | Byte accounting: small packets use little pool but many are needed to carry a rate. |
| AIMD: halves on loss (once per congestion event), +10% of rate per 100 ms loss-free | [kit] flow.rs | Reacts to **any** drop → punishes loss heavily. |
| RateMatch: rate = 1.25 × acked rate over max(2·minRTT, 100 ms); in-flight cap 2 BDP, min 4 packets; RTT probe every 2 s | [kit] flow.rs | Barely reacts to drops; reacts to **service rate**. Holds a standing queue ≈ 1 BDP. |
| An Amazon burst restarts its law at its initial rate (full strength) | [kit] flow.rs | The moment of impact is the first 2 s (ungraded) — but window 3+ is graded. |
| Thin links in practice worlds: 2–6 Mbit/s vs 10 default; bottleneck set is named in `floors.bottlenecks` (graded worlds are hidden) | [kit] worlds | Fair shares there are small (~0.3–2 Mbit/s); 40% floor is a low bar *if no one is starved*. |
| F1 checks **every** customer in **every** settled 1 s window; F2 ≥ 55% of max-min total; F3 = zero `link_down` | [kit] floors | Floors catch disasters. Starvation of one customer in one window fails the run. |
| Graded at load ×1 and ×1.5; hidden worlds | [kit] handout | Design must hold at 1.5× demand and on different topologies. |
| Part 2 fair share is over **all routes ≤ 2 hops longer than shortest**, with a greedy fixed placement | [kit] floors | Funnelling everyone down one path while a detour is idle fails F1/F2. |

---

## A. Traffic classification (who goes in which queue)

How the data plane decides a packet's queue. TinyVM can read `ip_src`, `ip_dst`,
`ip_proto`, `dscp`, `size`, `kind`, `flow_id`, `app_id`, and table-match on
them (`SetQueue`).

| Option | Description | Pros | Cons |
|---|---|---|---|
| A1. Per-customer queue | LPM table on `ip_src` → `SetQueue`, one queue per contract row | Full isolation; simplest to reason about; matches "a small customer must not lose packets because a large one filled the buffer" | Queue count × ports eats the 40-packet pool (5 customers × 4 ports = 20 queues); per-queue depth tiny |
| A2. Per-class queue | Group customers into few classes (e.g. small-packet/latency, AIMD, RateMatch, control) | Few queues → deep enough to absorb bursts; cheap | Members of a class interfere with each other (a big AIMD sender hurts a small AIMD sender) |
| A3. Hybrid | Isolated queue for each *large/unlimited* customer, shared queue for all small ones, separate control queue | Small customers (Jane Street 0.05–0.2, Fortnite 0.3–0.6 Mbit/s) rarely overflow a shared queue if the big ones are fenced off | More configuration logic |
| A4. Per-AIMD / per-RateMatch split | Two data queues by sender law, plus control | Directly encodes the handout's hint ("do not put the two kinds in the same queue") | Doesn't isolate two AIMD customers from each other (e.g. Amazon vs Steam) |
| A5. Per-port-and-link dependent | Classification differs by egress port (only customers actually routed over that port get queues) | Fewer queues per port → deeper each | Needs path knowledge (§I); reconfigure when paths move (§K) |
| A6. Dynamic: class by observed drops/behaviour | Re-classify from `port_drops` | Prepares for A3 | Out of scope for A2 (contract is given); more complexity for nothing |

Sub-decisions:
- **Unknown-source traffic** (acks from non-customer receivers, anything
  unmatched) needs a deliberate default queue **[verify who the ack's
  `ip_src` is]**. If it falls into queue 0 by accident it may land behind or
  ahead of control traffic.
- **Reverse direction.** Acks travel the reverse path through *other*
  queues/links; a congested reverse link drops acks, which both sender laws
  read as loss/low rate. Decide whether acks get priority (they are 40 B, so
  cheap).
- **Match key.** Source prefix for customer data; `ip_proto` / `kind` for
  control and acks. Classification must happen before the egress queue
  decision on **every** switch, including transit switches (each hop re-queues).
- Two-stage lookup (proto first, then src) costs TinyVM stages (max 8; pipeline
  depth 4 is free).

## B. Queue structure per port

- **How many queues per port:** 2 (control + data) up to 1 per customer + control.
- **Same layout on all ports, or per-port layout?** Same is simpler and safer
  for a hidden topology; per-port wastes less buffer but needs path knowledge.
- **Queue-id ↔ customer mapping must be consistent across ports** (the TinyVM
  `queue N` is chosen once per packet *before* egress port is known, so the
  index must mean the same thing on whichever port it exits) [kit: queue
  selected in same pipeline pass as egress].
- **Queue index 0 reserved for control** (load_report even hints at this).
- **Re-configure at runtime or set once at init?** Init-only is robust;
  runtime lets queues shrink on idle ports and grow on busy ones, but
  `set_queues` replaces state and lands 2 ms late.
- **Ports not carrying any customer** (e.g. a link used only by a detour)
  still need a layout in case traffic moves there.

## C. Buffer allocation (the 40-packet pool)

The core trade-off the handout names. Options:

| Option | Description | Pros | Cons |
|---|---|---|---|
| C1. Strict partition | Σ queue caps (over all ports) ≤ 40 | Each queue's space is guaranteed | Idle space wasted; each queue ≈ 2 packets with 5 ports × 4 queues; hurts bursts and F2 |
| C2. Full oversubscription | Caps large (e.g. each = 40) | Statistical multiplexing; absorbs bursts | One busy queue can eat the pool; small customer's packet hits `PoolFull` — exactly the failure to avoid |
| C3. Partial oversubscription | Reserve a floor per queue, allow burst up to a higher cap | Middle ground | Cap + `drop_at` numbers must be tuned; pool contention still possible |
| C4. Per-port budget | Split 40 by port (equal, or by link capacity, or by expected load) | Switch with an idle port can give space to the busy one | Static split guesses the wrong port if paths move (§K) |
| C5. Dynamic threshold (industry) | Each queue's limit = α × (free pool) — the classic Choudhury–Hahne DT scheme used in shared-buffer ASICs [industry] | Self-balancing, no tuning per-port | Not offered natively: we only have static caps. Approximating means periodic `set_queues` driven by drop counts (§H, slow: 2 ms + timer period) |
| C6. Reserve for control | Small dedicated (cap ≈ 2–4) budget for control queue | Protects HELLO/LSP/report (F3) | Subtracts from data |

Related numbers to compute (do this before choosing):
- **BDP per link:** capacity × path RTT. A 10 Mbit/s link with ~4–8 ms RTT is
  ~5–10 full packets; a 2 Mbit/s link is 1–2 packets. RateMatch senders hold
  ~1 BDP queued *and* keep ≥4 packets in flight (min inflight 4 [kit]), so on
  thin links even one RateMatch customer wants ≥4 packets of buffer.
- **Serialisation time:** 1000 B at 2 Mbit/s = 4 ms; a 4-packet queue on that
  link is 16 ms of delay. Latency-sensitive (Jane Street/Fortnite) queues must
  not sit behind that.
- **Queue depth rule of thumb [industry]:** BDP/√N for N flows (Appenzeller),
  or ≈ 1 BDP for few flows. With 3–5 customers that means ≈ 1 BDP.

## D. Scheduler (which queue sends next)

| Option | Description | Pros | Cons |
|---|---|---|---|
| D1. FIFO (one queue) | Baseline | Trivial; what A1 effectively was | No isolation — fails F1 under load [book: no protection from aggressive flows] |
| D2. Strict priority for all | Order customers by priority | Gives latency class zero queueing delay | Starves lower classes → F1 fail for starved customer; Steam (unlimited) will eat anything not above it |
| D3. DRR with weights | Per-queue quantum; weights = fair share proportions | Work-conserving, simple, isolates; approximates max-min [book: FQ/WFQ] | Quantum granularity vs 200 B packets; a small-packet queue gets bytes-fair but waits a full round |
| D4. SP + DRR hybrid | Control (and maybe latency class) strict-first, rest DRR | What production gear does: PQ + WRR/DRR [industry; Huawei/Cisco docs surfaced in search] | Need a policer on the SP class or it can starve DRR (not available → must rely on the class being small by contract) |
| D5. Weighted by `ask_bps` | Weights ∝ contract ask, capped | Roughly max-min when links are partially loaded | Unlimited customers need an artificial weight; weight 0 reads as 1 |
| D6. Weighted by "fair share" calculation | Compute max-min per link from contract + routing, set weights = share | Matches the grader's own definition | Needs to know who uses which link (§I/§H) and to recompute when load moves |
| D7. Equal weights | One weight for everyone | Zero computation; max-min among backlogged flows is *exactly* equal split | Small customers just use less (work-conserving), so equal weights already gives the max-min allocation for backlogged flows — often enough |

Sub-decisions:
- **Is DRR "unit" (quantum) in packets or bytes?** Weight = packets of credit
  [kit]; a 200 B packet uses 1/5 of a credit. Check whether small-packet queues
  need a larger weight to keep latency low. **[verify in egress.rs / by experiment]**
- **Priority for small/latency customers** (Jane Street, Fortnite: "all it asks
  for, with low delay"): own SP queue vs DRR with high weight. Their rates are
  ≤ 0.6 Mbit/s so an SP queue costs little, but it is an extra queue to place
  in the pool.
- **Work conservation:** all three disciplines are work-conserving; unused
  share goes to others (good for F2).

## E. Drop policy / active queue management

Tools available: tail drop at `cap`, deterministic early drop at `drop_at`.
No RED/ECN. Options:

| Option | Description | Pros | Cons |
|---|---|---|---|
| E1. Pure tail drop | `drop_at = 0` | Simple | Deep queues → long standing delay; synchronised AIMD halving |
| E2. Early-drop line below cap | `drop_at` < `cap`, per queue | Keeps queue short (delay); signals AIMD sooner; leaves headroom for burst/other customers | One line, deterministic: all senders hitting it see loss together; wrong value either under-uses the link (F2) or lets queue overflow |
| E3. Different `drop_at` per sender law | **AIMD**: line close to cap/BDP (sharp reaction to a single drop, needs ≥ 1 BDP of buffer to keep the pipe full after halving). **RateMatch**: mostly ignores drops, reacts to service rate → its standing queue is set by *its own in-flight cap*, not by your drop line | Encodes the handout's central insight | Needs experiments to calibrate (the handout tells us to) |
| E4. Throttle by dropping | Deliberately hold an over-share customer down by dropping its packets (AIMD obeys) | Cheap policing for AIMD hogs | RateMatch does **not** back off on drop (loss up to ~20% only) — dropping it just wastes capacity and creates duplicates (counts as nothing). For RateMatch, control by **service rate** (weights), not drops |
| E5. Approximate RED | Emulate probabilistic early detection by periodically changing `drop_at` based on drop counters | Desynchronises AIMD senders [book] | Control loop is ≥ timer period + 2 ms slow; likely not worth it with ≤ 5 customers |
| E6. Per-customer admission cap | A cap on each customer's queue ≈ its fair share of buffer | Prevents one customer taking the pool | Same as per-queue caps (C) |

Things the sender code implies (from `flow.rs`):
- AIMD: one halving per congestion event; losses within one RTO after a
  halving are ignored. Bursty drop clusters are fine; a steady trickle of
  drops keeps rate low.
- RateMatch: loss is retransmitted; **a duplicate delivery counts as nothing**
  for goodput. A drop of a packet that was already delivered by a retransmit
  would waste link capacity — avoid dropping late in a path (drop at the
  first congested hop where possible; see §G).
- Standing queue for a RateMatch flow: ~1 BDP queued + 1 on the wire. If the
  queue cap < its in-flight, it drops instead of queues; it throttles to
  service rate regardless.

## F. Protecting control traffic (F3)

Hello/LSP/load-report packets share links and the pool with data. A drop
streak makes the neighbour timeout fire (A1: 150 ms) → false `link_down`
→ F3 fail. Options (can combine):

1. **Dedicated priority queue** (queue 0, SP) for `ip_proto` 89/90/…; small cap
   (~2–4 packets); control packets are 20 B + payload so 4 packets ≈ tiny bytes.
2. **Never call `actions::link_down`.** Zero risk for F3, since no link ever
   fails. A1's control plane used hello timeouts for *routing*, not
   necessarily for the declaration; the SDK's `link_down` is a separate
   voice [kit]. Simply don't emit it. Cost: gives up failure detection (needed
   again in A3 — plan for the later cost, but A2 is graded without it).
3. **Loosen the neighbour timeout** (e.g. 150 ms → 500 ms+) so a few drops don't
   mark neighbours down in the routing logic (A1's `NEIGHBOR_TIMEOUT_NS`).
   Dead-neighbour handling in A1 also **withdraws routes** (`dead_ports`) —
   a false neighbour-down under load would rip out working routes → collapse.
4. **Reduce control volume:** hello every 50 ms on each port + LSP refresh
   every 200 ms flood is non-trivial on 2 Mbit/s links; slower periods leave
   room for data.
5. **Reserve pool bytes** for control (C6) so a full data pool can't drop it.
6. **Use `actions::in_queue(q, inject_packet(...))`** to push *our own*
   injected control packets into queue 0 [kit]; transit control packets need
   the proto match in TinyVM → queue 0.

Decision needed: do we keep any liveness logic in A2? (Recommendation of this
menu: the cheapest safe policy is options 1 + 2 + 3; but it is a choice.)

## G. Per-customer-type treatment (the decisions the card must justify)

Contract tells us `cca` and `ask_bps` but not application name. Customers by
behaviour (use `ask_bps`, `cca`, and packet size as observed at runtime if
needed):

| Type (inferred) | Ask | Law (varies by world!) | Needs | Treatment options |
|---|---|---|---|---|
| Netflix | 3–5 Mbit/s steady | either | its fair share | Own DRR queue, depth ≥ ~1 BDP of its share |
| Amazon | 6–8 Mbit/s, bursty 8–10 s on/off | either | fair share *soon* after burst start | Don't starve its queue when idle but don't waste buffer on it (C3: shared reserve); weight set for the on-state |
| Steam | unlimited | either | leftovers | Lowest weight / or equal weight — it will always fill whatever it is given; give it the *shallowest* queue/lowest drop line so its standing queue doesn't eat the pool |
| Jane Street | 0.05–0.2 Mbit/s, 200 B | either | all it asks, low delay | Priority or high-weight shallow queue |
| Fortnite | 0.3–0.6 Mbit/s each way, 200 B | either | all it asks both directions, low delay | Same; remember **both directions** — return path queues matter |

Notes:
- Identification by contract: `ask_bps = None` ⇒ unlimited (Steam-like);
  small `ask` (< ~1 Mbit/s) ⇒ latency class; but the *application name is not
  given*, so classify by numbers, not names. Thresholds are a design choice.
- The law differs per customer *per world*, e.g. Steam is AIMD in two worlds and
  RateMatch in two others. Never hard-code "Steam = AIMD".
- **Same-law grouping rule** from the handout: AIMD and RateMatch customers
  must not share a queue.
- **Where to drop:** a drop at a later hop wastes the capacity earlier hops
  spent. An alternative is policing at the *ingress* switch. We do not know
  which switch a customer's first hop is until learned (A1 learns from punts).
- **Hold-back:** "you may hold some customers back" [kit]. A hog (Steam) is
  held back by weight (service), not by dropping RateMatch senders.

## H. Load information (Part 2 input)

What the program sees: per-port drop counts, per queue, per timer; static
capacity; the contract. Options:

| Option | Description | Pros | Cons |
|---|---|---|---|
| H1. Local-only | Each switch acts on its own ports' drops | Zero control traffic; instant | Sees only its links; may move traffic onto a detour that's full 2 hops away |
| H2. `load_report` as provided | 500 ms evaluation, memory of the worst of 10 evals (5 s), flooded; picture `(from,to) → cap, level 0–3, #customers` | Given, works, any switch can copy it | 500 ms + memory means up to 5 s of stale "busy" after a burst ends → slow to reuse an emptied path; drop-based so an *idle* link and a *just-fits* link look the same (both zero drops) |
| H3. Static model from the contract | Everyone knows every customer's ask and every link capacity (after learning topology) → compute expected load per link for any assignment, no measurements | Deterministic, **identical on every switch** → no inconsistency, no control traffic, no oscillation. Part-2 grader also uses the contract | Doesn't know *when* an Amazon burst is on (contract doesn't say) — plan for worst case, or detect burst from drop counts; doesn't know where each customer attaches until learned |
| H4. Hybrid | H3 as the model + H2 drops as the trigger to re-solve | Robust to model error | Both complexities |
| H5. Faster/finer reports | Shorter eval period, shorter memory, add a "customers dropping" mask instead of a count | Faster reaction | More control traffic on thin links; risk of flapping |
| H6. Add own topology to the picture | A1's LSDB already floods adjacency; extend LSP with `capacity_kbps` per adjacency and the customer prefixes it serves | One protocol for topology + capacity + attachment | Larger LSPs; need to re-flood on change |

Open sub-decisions: how often to flood; whether to flood only on change; level
thresholds (`level_of`: 1–4, 5–24, ≥25 per 500 ms) vs proportional-to-capacity
thresholds; whether the *direction* of the link is tracked (capacity and load
are per direction; reverse direction carries acks and Fortnite's other side).

## I. Path set (what routes exist)

| Option | Description | Pros | Cons |
|---|---|---|---|
| I1. Shortest path only (A1) | Dijkstra, hop cost | Already works | Fails Part 2 by construction [kit] |
| I2. ECMP | Split among *equal-cost* shortest paths | Industry default [industry] | Not enough: detours are *longer* than shortest |
| I3. k-shortest simple paths / "within +2 hops" | Enumerate every route ≤ (shortest + 2); matches the grader's own definition [kit] | Exactly the candidate set the fair-share is computed over | Needs the full topology (LSDB; we have it); path enumeration is bounded for ≤ 15 switches |
| I4. Capacity-aware weights | Dijkstra with cost = 1/capacity (or hops + penalty for thin links) | Avoids thin links by default | Doesn't know the load; one fixed answer |
| I5. Load-aware weights | Cost grows with reported level | Adaptive | Oscillation (§K); weight scale tuning |
| I6. Constrained / residual-capacity path | Route each customer on the path with max bottleneck of *residual* capacity (capacity − already-placed demand) | **Mirrors the grader's greedy placement** (fewest routes first, unlimited next, biggest demands; take the route whose thinnest shared link leaves most; shortest on ties) — copying it should make our assignment ≈ the reference | Needs the same inputs on every switch |

## J. Path assignment (which customer on which path)

Per-customer (not per-packet) so a sender's packets don't reorder (reordering
looks like loss) [kit].

| Option | Description | Pros | Cons |
|---|---|---|---|
| J1. Static at init, computed from contract + topology | Every switch runs the same deterministic algorithm on the same inputs | Consistent; no oscillation; no control-plane load | Ignores Amazon on/off unless modelled for both states; topology only known after discovery |
| J2. Replicate the grader's greedy order | fewest-routes-first, unlimited next, bigger demands next; thinnest-residual route; ties → shortest | Likely matches the share the grader credits | Grader's own "routes" definition must be reproduced exactly; Amazon's burst state changes who is "present" |
| J3. Worst-case provisioning | Assume every Amazon burst is on | Never congested by surprise | May leave a detour under-used when Amazon is off → F2 |
| J4. Two plans (burst-on / burst-off), switch on detected burst | Detect via drops or traffic | Best use of capacity | Detection is slow (500 ms+, memory) and the first 2 s of a burst are ungraded, so a 2 s lag is *free* — but not 3 s+ |
| J5. Reactive move of the largest offender | When a link level ≥ threshold, move the customer with largest ask (or Steam) to the best detour | Reacts to real conditions | Moving a big customer onto the detour makes the detour look full → bounce back (§K) |
| J6. Source-routed per customer | Encode path at ingress switch (MPLS label stack / dscp / custom field), transit switches follow the label | Immune to hop-by-hop disagreement → **no loops by construction** during transitions | More TinyVM work; labels are available (`push_label`, `label_top`); only the ingress switch decides |
| J7. Hop-by-hop per-(src-prefix, dst-prefix) entries | Each switch installs (src, dst) → egress | No labels; direct | Switches may disagree briefly → loops (§L). Needs a table key combining `ip_src` and `ip_dst` (2-stage: src→meta, then (meta,dst) match) **[verify TinyVM capability]** |

Where the packet *enters* decides everything: A1 learns attachments from
`NoRoute` punts on non-neighbour ports. The reverse (ack) direction also needs
a path per customer; asymmetric is allowed, symmetric is simpler.

## K. When to change routes (stability)

| Decision | Options | Trade-off |
|---|---|---|
| Trigger | Fixed schedule; on drop-report change; only when level ≥ N for M evals | Reacting to every report causes ping-pong |
| Hysteresis | Move only if the improvement ≥ δ; don't move the same customer again for T seconds (hold-down); require two consecutive reports | Costs reaction time |
| Timing budget | Grading starts at 3 s (Part 2); burst start gives 2 s ungraded + settling; bursts every 8–10 s [kit] | An ≤ ~2 s reaction is free after a burst begins; do it *after* it settles and before the next burst |
| Damping for the shifted-load illusion | After moving customer X to path P, add X's ask to P's expected load immediately (don't wait for measurement) | Needs the demand model (H3/H4) |
| Who decides | Everyone (deterministic, same inputs) vs one elected coordinator (lowest id) that floods the assignment | Deterministic-everywhere needs identical views; coordinator adds a single point and flood latency but guarantees consistency |
| Order of updates | Install new route first, delete old after delay > 2 ms; or make-before-break by label | Avoids black-holes in the transition |

## L. Loop and black-hole safety

- A packet that revisits a switch = hard fail [kit]. Hop-by-hop per-customer
  routing with *different switches reading different loads* can loop during the
  2 ms (+ flood) window.
- Mitigations: (a) path-vector or source-routed paths (J6) — only the
  ingress picks, so no disagreement; (b) all switches use a deterministic
  function of the same static data (H3) so they never disagree; (c) only ever
  route *forward along a precomputed loop-free DAG* (e.g. next hop must be
  strictly closer to the destination by hop count, + at most 2 hops of
  slack tracked in a label/TTL-like field); (d) TTL — set low by the
  injector? the packet is what loops, not us; (e) delete-then-install ordering
  already used in A1's `install_or_move_route` (there is a gap with *no* route
  → `NoRoute` punt → handled as customer punt; check this doesn't re-learn a
  bogus local prefix; A1's `handle_customer_punt` ignores packets from
  neighbor ports, good).
- The punt path itself: packets that hit `Punt` during table gaps are *lost*
  (or delayed?) **[verify]** — avoid gaps by installing before deleting.

## M. Declaring link state (F3)

Covered in §F. Options: (1) never call `link_down`; (2) call only on a very
conservative rule (e.g. neighbour silent ≥ 2–3 s). Since no link fails in A2,
(1) is the zero-risk choice for the floors. Re-evaluate for A3.

## N. Computational / protocol budgets

- Per call: 4M fuel, 4096 B of actions. A `set_queues` action per port ×
  ≤ 5 ports + route installs. Plan to spread large updates over ticks.
- Timer: A1 uses a 50 ms tick doing hello + reconcile every tick; fine.
- Control overhead on thin links: hello (every 50 ms on every port) + LSP
  refresh (200 ms flood × N switches) + load reports. On a 2 Mbit/s link this
  is a measurable fraction; quantify before adding more.
- Memory is irrelevant (1 MiB vs 128 MiB limit).

## O. Mechanical decisions in the existing code that interact

From reading `networkarena-a1/my_switch/src/lib.rs`:

1. **Routing is per-destination-prefix (LPM on `ip_dst`)** with one entry per
   prefix. Per-customer paths need a second dimension (source). Decide the
   TinyVM program shape and table layout (§J6/J7) *before* anything else — it
   determines everything about path TE.
2. **Cost is hop-count** (`DEFAULT_LINK_COST = 1`) and LSPs carry no capacity.
   Part 2 needs capacity (at least `capacity_kbps` per adjacency) in the LSDB.
3. **Neighbor liveness removes routes** on 150 ms of hello silence — with
   hellos in the same queues as data this can cause self-inflicted route
   collapse. Priority queue for control and/or longer timeout (§F).
4. **LSP age-out 1 s with refresh 200 ms** — an LSP drop streak (5 in a row)
   would expire a healthy switch from the LSDB and then routes withdraw.
5. **Customer prefix learned from first punt on a non-neighbor port**, assumed
   `/24`. In A2 the contract gives prefix lengths (and all the sender prefixes)
   up front, but *where* each attaches is still learned. Receivers have no row.
6. **Ties settled by lowest switch id** — good for determinism across switches.
7. **Action budget**: `MAX_TABLE_ACTIONS_PER_CALL = 60` (A1) — adding queue
   configuration + per-customer routes must still fit in 4096 B.

---

## What production networks actually do [industry + book]

Summary so we can say "what companies deploy" on the card.

**Queueing / scheduling at one link**
- The Internet default is still **FIFO with tail drop** [book].
- Datacenter and enterprise switches use **a handful of traffic-class queues
  (typically 4–8) per port, strict priority for the small latency/control
  class and (D)WRR/DRR among the rest** (PQ+WDRR) — this appears in vendor
  QoS guides for Huawei/Cisco found in search [industry]. Per-flow fair
  queueing is rare in hardware (state cost); **fq_codel / FQ + CoDel** is the
  common software/Linux (and home-router) choice **[industry, from general
  knowledge — not read this session]**.
- **DiffServ** [book]: *EF* = strict priority (or high-weight WFQ) with rate
  limited at the edge; *AF* = WRED with different drop curves per class.
  Practical implementation = WRED or WFQ selected by DSCP.
- **WFQ/DRR are the production approximation of max-min fairness** [book:
  weights 2:1 → 2× bandwidth; work-conserving, so idle shares redistribute].

**Buffer management**
- Shared-buffer switch ASICs use **dynamic thresholds** (α × free buffer) per
  queue so busy ports borrow from idle ones, plus small static reservations
  per queue [industry; Cisco's "Intelligent Buffer Management" white paper
  surfaced in the search describes dynamic buffer protection and approximate
  fair drop (AFD)]. Our static caps are the closest knob we have (§C5).
- **AQM:** RED/WRED to desynchronise TCP and keep queues short; **ECN**
  marking instead of dropping where hosts cooperate; **DCTCP** in datacenters
  with a single marking threshold K ≈ RTT·C/7 [book]. We have a single
  deterministic threshold (`drop_at`) — structurally the same as DCTCP's step
  marking but with drop instead of mark.
- Host-side: AIMD (Reno/CUBIC) vs model-based (BBR) — the two sender laws in
  the kit. The book notes BBR can crowd out loss-based flows in a shared
  queue [book] — the reason the handout says to separate them.

**Traffic engineering**
- **ECMP** (hash flows over equal-cost paths) is the near-universal default;
  **WCMP** adds weights for asymmetry; flowlet switching (CONGA / LetFlow)
  rebalances at gaps in a flow to avoid reordering [industry, search results].
- **WAN:** Google **B4** (centralised TE controller, multipath, up to near-100% link
  use, but ~1 s to recompute; slow to react) and Microsoft **SWAN**
  (priority classes, max-min fairness across flows, centralised) [industry,
  search results; the book's TE page 404'd]. Both **assign whole flows
  (groups) to paths** and **update in a careful make-before-break order**
  to avoid transient loops/overload — same constraints as §K/§L.
- Takeaway for our design space: *central controller with a global view* ≈
  our "shared load picture / deterministic same-inputs" option (H3/J2);
  *local reactive* ≈ H1; production practice combines a **slow global
  optimiser** with **fast local fallback**.

**Congestion control:** the book's evaluation lens — fairness (Jain index,
max-min), utilisation vs delay ("power" = throughput/delay), router-centric vs
host-centric — is exactly the vocabulary for the card.

---

## Experiment findings (answers to the open questions)

All numbers are from running the real simulator on the 5 practice worlds. Tooling is in `experiments/`.
"Part 1 sweep" = the 3 Part-1 worlds at load x1 and x1.5. Treat single-world results as evidence, not proof;
hidden worlds differ.

### Q1. What do acks look like? (answered)
`kind=4`, `ip_proto=0`, 40 B, `ip_src` = the **receiver's** address. Most receivers have no contract row, so a
source-prefix lookup misses and the ack takes the default queue. A Fortnite receiver *is* a contract row, so its
acks land in that customer's queue (same direction as its data). `ip_proto` cannot separate acks from data; the
`kind` field can. Control packets: proto 89/90, src 169.254.x.x.

### Q2. Can TinyVM match (src, dst)? (answered: yes)
2 stages: LPM table on `ip_src` -> `SetMeta(customer_id << 32)`; then `loadm`, `load ip_dst`, `and 0xFFFFFF00`, `or`,
Exact table (full 64-bit key). Probe: a Drop rule on (Amazon, dst of its peer) zeroed Amazon only; the same rule
with a wrong dst or wrong src changed nothing. LPM compares only the low 32 key bits, so the combined key must be
an Exact table. Queue and egress come from different table hits.

### Q10. Why does the A1 switch loop under load? (answered; root cause found)
**Not** tie-breaking, **not** the routing algorithm. Chain of events on `a2p1-dumb-004`:
1. The 2 Mbit/s link 4->7 is saturated; its queue fills **switch 4's whole 40-packet pool**.
2. A switch's pool is shared by *all* its ports, so hellos/LSPs that switch 4 sends on its *other* ports
   (to switches 5 and 8) are refused. Hello gaps seen: S8<-S4 up to 208 ms, S5<-S4 156 ms, S7<-S4 162 ms
   (everyone else's max gap is ~52 ms = one tick). The three victims are exactly S4's neighbours.
3. A1's liveness (150 ms silence = neighbour dead) fires, withdraws every route through that port, then re-learns
   them one tick later. S8's routes flipped between ports 1 and 2 over and over (99 delete actions on S8).
4. Neighbours disagree for a few ticks -> packets ping-pong (1<->7, 8<->3). The grader counts any revisit: 14 packets.
5. Also seen: hello/LSP drops on injection toward the *app* ports (irrelevant) and 3 TTL-expired data packets.

What fixes it: a separate control queue with strict priority **and** data queue caps that sum to < 40 so the pool
cannot be exhausted. Test: control queue cap 4 + data queue cap 28 -> 0 loops, hello gaps back to ~52 ms, and even
a plain shared data queue passes dumb-004. Control queue + data cap 40 (pool can still fill) -> **62 loops, FAIL**:
priority scheduling does not help when the *pool* is full. => Reserving pool space for control is the fix, not
just prioritising it. (Also `link_down` was never called, so F3 passed all along; the damage came from route
withdrawal, which is A1's own reaction to a silent neighbour.)

### Q7. How much control traffic is there? (answered: negligible)
~23-27 kb/s per switch-to-switch link direction (hello + LSP + routing): 1.2% of a 2 Mbit/s link, 0.2-0.3% of a
10 Mbit/s link. Control volume is not the problem; *loss* of control packets is (see Q10). Hello gaps >150 ms occur
only on ports of a switch whose pool is exhausted.

### Q3 / Q4. Queue structure, depth, drop line (answered for Part 1; Part 2 is a routing problem)
Setup: A1 routing + control queue (q0, cap 4, high weight) + misc queue for acks (q1, cap 4) + DRR.
- **One shared data queue (cap 28): FAIL** on 5 of 6 Part-1 runs (F1 worst 0-7% on grid, 6% on ring@1.5).
  This is the handout's warning made concrete.
- **Two data queues split by sender law (cap 14 each): still FAIL** grid@1 (23%), grid@1.5 (12%): a small
  customer behind a big customer of the same law is starved.
- **One queue per customer, DRR equal weights: passes everything** at caps 2-4 with large margins
  (F1 worst 75-90%, F2 worst 86-100%).
- **Depth sweep (per-customer cap)**: cap 2/3/4/8 pass all six runs; cap 6, 12, 20 fail grid@1.5.
  The cause is *not* "too deep or too shallow"; see the RateMatch trap below. Deeper helps AIMD
  customers keep the pipe full (dumb@1 worst share: 79% at cap 2, 97% at cap 8, 99% at cap 20). Delay is not graded, so the real
  constraints are the shared 40-packet pool, the control reserve, and the RateMatch trap below; this sweep did not
  measure what deep queues cost in delay or in other worlds.
- **Early-drop line**: `drop_at=N` behaved the same as lowering the cap to N (cap=12,drop=3 ~ cap 3;
  cap 8,drop 4 ~ cap 4). It is a second cap, not a different mechanism. Its only value is keeping the hard cap
  high (pool headroom accounting) while the effective depth is low.

#### The RateMatch startup trap (new, important)
In grid@1.5 the unlimited RateMatch customer (Steam) got 9% of its fair share *for the whole run* at some depths.
It starts by sending a 10 Mbit/s burst (124 packets in the first 100 ms), loses ~120 of them on the first thin
link, and its `rate = 1.25 x acked-in-last-100ms` collapses to 1 packet per window. At exactly one ack per window
the estimate is quantised and 1.25x of it is again about one packet, so it **never climbs back** (steady ~0.1
Mbit/s, 475 packets in 38 s). At cap 8 two or more packets survived the burst and it climbed 25% per window to full
rate. So whether a RateMatch customer escapes depends on how many of its *first-burst* packets survive. Outcomes
are non-monotonic in queue depth (cap 6 traps, 8 does not, 12 and 20 do). Implications for the design:
- A RateMatch customer must not lose almost its entire opening burst. It needs enough dedicated buffer and/or a
  service share at the first congested hop to keep more than one packet alive per RTT.
- Check every candidate design across all worlds and both load scales for a silent ~0.1 Mbit/s customer. Passing
  five of six runs is not a pass: a hidden world will find the sixth.
- Part 2 amplifies this: a customer *moved* to a new path restarts its RTT probing and may hit the trap again.

### Q9. Exactly what does the grader compare against in Part 2? (answered from `src/floors.rs`)
- Candidate routes = every **simple path** (no repeated switch) from the customer's switch to its peer's switch
  with at most `shortest + detour_hops` hops (hop count, ignoring capacity). Order: shortest first, then by node id.
- Assignment: greedy, customers sorted by (fewest candidate paths, unlimited first then biggest demand, then
  index). Each takes the path maximising `min over links of capacity / (already_placed_on_link + 1)`; ties go to the
  first (shortest) path. Then plain max-min water-filling over those chosen paths.
- Capacities are per **direction**. A customer with no peer/menu is bounded only by demand.
- So an exact replica of this algorithm (J2) is implementable from: topology + link capacities + customer demands +
  each customer's (src switch, dst switch). Our program needs all four to mimic it.

### Q8. How much time to react to a burst? (answered from `floors.rs`)
Window k = [lo, hi). It is unsettled if any burst start b satisfies `b < hi` and `lo < b + 2 s`. So every window that
overlaps [b, b+2 s) is skipped, and the first graded window is the one starting at or after b + 2 s, i.e.
**2 to 3 seconds** of free reaction time after a burst starts (depends on where b falls in the second).
Example: b = 0.8 s -> windows [0,1), [1,2), [2,3) skipped; [3,4) is graded. Nothing is skipped when a burst *ends*;
other customers must reclaim the freed capacity quickly (AIMD adds only 10%/100 ms). Part 2 additionally starts
grading at 3 s. `load_report`'s clock (500 ms evaluation + flood + 2 ms control) fits in the free window; its 5 s
memory does not suit burst-end reaction.

### Q6. Does `load_report` mistake loaded for idle? (partly answered by reasoning + card evidence)
Its level is 0 whenever a link dropped nothing in the last 5 s of evaluations. In `dumb-004` the links 9->5 and
3->9 run at 70% of capacity (Amazon at 7 Mbit/s on 10 Mbit/s) with zero drops, so they report level 0 exactly like an
empty link. A link at 99% with no drops is also level 0. It also reports a link as busy for up to 5 s after the load
leaves (MEMORY = 10 evaluations). I did not run `load_report` itself (it installs no routes so no traffic flows);
the above is from its source plus link-load tables. Design consequence: a drop-based picture only tells you where
the link is *already overflowing*; a static demand model from the contract (H3) is the only way to know headroom.

### Q5. Can switches learn where every customer attaches, and how fast? (answered)
Yes and quickly: routes for every prefix are installed network-wide by ~54 ms (A1 learns attachment from each app's
single size-0 packet, then floods the prefix in the LSP); loops during initial convergence were gone by ~260 ms.
That is well inside the 1-3 s warmup. **But the contract does not say who each customer talks to.** Per-customer
path choice needs the customer's *destination switch*, and nothing hands us that (only the source prefix is in the
contract). See Q11.

### Q11. NEW: how does a switch learn each customer's destination? (open)
Options: (a) route by source only (one path per customer regardless of destination; fine because each customer
has a single peer in these worlds [verify from worlds: one `peer` per app]); (b) install a `Punt` entry keyed on
`ip_src` so the first packet of each (src, dst) pair reaches `on_punt` (`PuntEvent` carries `ip_src`/`ip_dst`),
then replace it with the real rule; (c) infer the destination from the reverse direction (acks carry the
receiver as `ip_src`). Not yet tested.

### Baseline numbers to beat (my A1 switch + trivial control queue, no real policy; `experiments/sweep.sh`)
Part 2 worlds fail badly without traffic engineering: F2 19% (dumb-003) and 60-67% (grid-006) with a single shared
data queue; with per-customer DRR queues F2 is still 40-59% on those worlds. Part 1 is solved at the queue level
(see Q3/Q4) once the pool and control queue are handled.

## Implementation status (what's in `my_switch/src/lib.rs` and `src/plan.rs`)

**Part 1 queueing** (MY_DESIGN D1, D2e, D2f): control queue 0, misc queue 1, one DRR queue per customer (caps by profile, per-port
cap budget 19), source-prefix table to select the queue, control packets injected into queue 0, neighbour timeout 200 ms.

**Part 2 traffic engineering** (MY_DESIGN D3):
- **Destination learning (L3):** each customer's attach switch watches its first ack with a per-customer register latch
  (`T_DSTID` + `R_LATCH`; only kind=Ack, only once); the ack's source /24 is the peer. It is flooded in the LSP (`pairs`).
- **LSP** now carries per-adjacency capacity (kbit/s) and the learned pairs; the LSDB therefore holds topology, capacities,
  attachments and peers on every switch.
- **Plan (`plan.rs`):** every switch runs the same pure computation: grader-style menu (simple paths up to shortest+2 hops) and
  greedy placement (fewest choices, unlimited, biggest demand; thinnest-link room; shortest wins ties), all customers assumed
  active. The plan's 15-bit epoch is a hash of the plan, so equal plans share an epoch.
- **Delivery (H2):** each switch installs its hop of every customer's path as `T_PLAN` entries keyed `(epoch, customer, dst /24)`;
  after a 300 ms settle the ingress writes the epoch into `custom0` through `T_STAMP`; transit switches match
  `(custom0, customer, dst)` and override the default route. Old epochs' entries are deleted 1 s after nothing needs them.
- **Data-plane program:** 8 stages (proto, route, src->queue, src->customer id, stamp, plan lookup, dst->ack-watch id, ack latch).

**Verified (real simulator):**
- the five official practice worlds at x1 and x1.5: all pass all floors.
- Part 1 set (practice + thin-link variants + 40 random worlds, x1 and x1.5): 98/98 (`ring-r17` now passes too).
- Part 2: 80 randomly generated worlds (6-15 switches, 1-3 thin links, 1-3 Amazon-like bursty customers; not the real generator)
  at x1 and x1.5: 157/160 with traffic engineering vs 112/160 for the queueing-only switch. No program failures (fuel/traps).
- The 3 misses are all x1.5 in worlds with three bursty customers, at 39-40% F1 or 51% F2 in a window next to a burst boundary
  (same AIMD recovery behaviour as the old `ring-r17`).
- `experiments/` holds the tooling: `gen_worlds.py` (Part 1 variants), `gen_part2.py` (Part 2 random worlds), `evalp.py` (batch
  runner), the parameterised experimental queue switch, the placement model (`te_*.py`), and the log analysers.

**Known gaps / assumptions:** /24 customer prefixes (A1's assumption); one peer per customer; at most 14 customers get their own
queue; settle (300 ms) and drain (1 s) are fixed constants; the plan assumes all customers are active (no burst detection);
`link_down` is never called.
