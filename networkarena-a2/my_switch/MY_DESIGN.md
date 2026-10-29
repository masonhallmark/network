# My A2 Design — decisions we have picked, and what follows from them

Companion to `DESIGN_A2.md` (the menu of options and the experiment findings).
This file records only what we have **chosen**, why, and what it forces on the rest of the design.
Status tags: **DECIDED** (user picked), **PROPOSED** (recommended, not yet confirmed), **OPEN**.

---

## D1. Control-plane protection — DECIDED: folded into the class scheme as class 0
Control is simply **class 0**: its own queue, index 0, on every port (injected with
`in_queue(0, ..)`). It is not a separate mechanism. Rules that go with it:
- **Never call `link_down`** (no link fails in A2, so F3 cannot fail).
- **Pool headroom rule**: a control queue does not reserve any pool bytes (the pool is charged only for bytes
  actually queued, see `egress.rs` `admit`). Control is protected only if the *other* queues cannot fill the pool.
  So the data-class caps must satisfy `ctrl_reserve + k x (sum of data caps on one port) <= 40` for the number of
  simultaneously congested ports `k` we expect (>= 2 in Part 2). A priority/high-weight control queue alone is not
  enough (a cap-40 data queue still produced 62 loops in the experiment).
- **Neighbour timeout raised from 150 ms to 200 ms (4 hello ticks)** so a couple of dropped control packets do not look like a
  dead link; A1 withdraws routes on silence. Verified no regression (96/98).

## D2. Queue structure — REVISED: one queue per **customer**, classes choose its profile (see D2e). Original decision was one shared queue per class
Instead of one queue per customer, customers are grouped into a small number of classes, and each class has one
queue on every port.

Implications (the user's questions answered):

1. **Does each port have its own per-class queues?** Yes. Queues are per egress port (`set_queues(port, ..)`).
   Every port holds its own copy of every class queue, with its own backlog and its own scheduler. Class "AIMD-bulk"
   on port 2 and on port 3 are independent queues that only share the switch's buffer *pool*.
2. **The class -> queue-index mapping must be the same on every port.** The data plane picks the queue number once
   per packet (`SetQueue`), before the egress port is chosen, so index k has to mean the same class on every port.
   What may differ per port is the *spec* (cap, weight, drop line) of queue k.
3. **The pool is switch-wide, caps are per queue.** There is no per-port cap and no per-class-across-ports cap.
   The only knob is each queue's own cap, so a switch with P ports and C class queues has P x C caps competing for
   one 40-packet pool. Aggregate limits can only be enforced by choosing the caps so their relevant sum is small.
4. **Classification is by contract**: source-prefix LPM table -> `SetQueue(class)`; misses (acks from non-customer
   receivers) fall to the default queue.
5. **Same-class customers share fate.** Two customers in one class share one queue: one can crowd the other out
   (this is the exact failure that kills shared queues). Classes must be defined so the members are alike.
6. **Evidence so far**: in the 3 Part-1 practice worlds, "class" and "customer" coincide almost everywhere, so
   per-class (4 classes: small / AIMD / RateMatch / unlimited) gave identical results to per-customer at the same
   cap (pass at cap 8; fail grid@1.5 at caps 12 and in a mixed 6/10/10/6 split because of the RateMatch startup
   trap). The *per-law only* split (2 data queues) failed. So the test of per-class vs per-customer is **not**
   decided by the practice worlds; hidden worlds with two same-class customers sharing a link will decide it.
   Risk to check: two Netflix-like AIMD customers, or Steam + another unlimited customer, in one class.

### D2a. How to partition customers into classes — REVISED after testing: queue per customer, class = sizing profile (see D2e). P2 (shared queue per class) was decided first and then failed on random worlds
Classes: **small** (ask < 1 Mbit/s), **bulk-AIMD**, **bulk-RateMatch**, **unlimited** (`ask = None`). Plus q0 control and q1 misc/acks.
Fallback if same-class customers interfere in tests: P6 (per customer). Options table kept below for the record.
Options, with what each protects and costs (number of data queues in brackets):

| Option | Classes | Notes |
|---|---|---|
| P1. By sender law only (2) | AIMD, RateMatch | Tested: **fails** (a small customer sits behind a bulk one of the same law). Do not use. |
| P2. By size x law (4) | small, bulk-AIMD, bulk-RateMatch, unlimited | What was tested as `mode=3`. Separates Jane Street/Fortnite from bulk and Steam from the rest. Uses `ask_bps` threshold (<1 Mbit/s = small, `None` = unlimited). |
| P3. Add a latency class (5) | priority-small, small, bulk-AIMD, bulk-RM, unlimited | Small latency-sensitive customers in a strict-first queue. Costs one more queue per port; needs a policer substitute since SP can starve. |
| P4. Per-port-aware (varies) | only classes that actually cross this port get a queue | Needs path knowledge; changes when routes move (Part 2), reconfigure risk. |
| P5. Hybrid | per-customer queues for unlimited/big, one shared queue for all small | Isolates the hogs, keeps small customers cheap. More logic; small customers still share. |
| P6. Per-customer | one per customer row | Strongest isolation, passes all Part-1 runs; costs the most pool space (<= 5-6 queues/port). Kept as the fallback if per-class fails on same-class pairs. |

(Superseded by D2e: P6 became the choice.) Open sub-questions: the size threshold, and where an
unlimited-but-small-weight customer or a customer that is both small *and* RateMatch (Fortnite) belongs.

### D2e. Revision: per-customer queues, classes only choose the queue's profile — SUPERSEDES the shared-queue part of P2
Trigger: 40 randomly generated Part-1 worlds (`experiments/gen_worlds.py`, 3-5 customers with random presets, laws and
peers, so same-class pairs occur; x1 and x1.5 = 80 runs; real simulator).

| Queue structure | Passes (of 80) |
|---|---|
| one shared data queue | 24 |
| two queues, split by law | 57 |
| P2: one queue per class (small / AIMD / RM / unlimited), any threshold rule | 70 |
| one queue per customer, uniform cap 3 | 78 |
| one queue per customer, class profile caps (cap 3 small/RM, 8 AIMD) | 79 |

All three threshold rules for "small" (absolute 1 Mbit, 10% of the largest ask, no small class) gave **identical** results,
so the small threshold was never the problem; sharing a queue was. Typical P2 failures: two RateMatch customers in one
queue (one took 184-189% of its fair share while the other got 1-2%: RateMatch senders do not back off on drops, so
whichever has more in flight wins), two unlimited RateMatch customers (one 189%, one 2%), two AIMD customers (39%).

New rule: **every customer gets its own queue on every port** (queue index = customer's position in the contract, same on all
switches because the contract is identical). Classes (law x {small, other}, unlimited split by law) now only select the
queue's *profile*: cap, weight, drop line. Consequences:
- Item 1 (class threshold) becomes low stakes: a wrong class changes a cap by a few packets, not who shares a queue.
  Keep a relative rule so it survives load scaling: small = ask < 10% of the largest finite ask (identical results to
  the absolute rule here, and independent of --load-scale).
- Queues per port = 2 + number of customers (contract rows). A row count over 14 would exceed the 16-queue limit; the
  excess would need to share a queue (state this assumption; practice worlds have <= 8 rows).
- Profile caps that passed (both sets, 98 runs at x1/x1.5, 97-98 pass): small 3, RM 3, AIMD 8, unlimited-AIMD 8,
  unlimited-RM 3. AIMD wants more depth than RateMatch (it needs roughly a BDP to keep the pipe full after a halving);
  RateMatch wants it shallow (deep RateMatch queues trigger the startup trap).

`ring-r17` (a world **we generated**, `gen_worlds.py` from the provided `a2p1-ring-020` topology; not a provided world) fails F2 at
46-47% in exactly 2 of ~32 graded windows (seconds 10 and 28), 50-56% pass elsewhere. Diagnosis:
- Both bad windows are the ones where Amazon's burst **ends**. The grader leaves Amazon out of that window (it was
  sending for only part of it) but still counts the AIMD "Steam" customer, whose fair share in that window assumes the link
  is all its own (5.7 Mbit).
- Steam was sharing the 6 Mbit link with Amazon until mid-window and AIMD only climbs by +10% of its rate every 100 ms, so its
  average over the window is ~45% of 5.7. The switch cannot speed up a sender's own recovery.
- Deeper AIMD queues raise steady utilisation (link 89% -> 97% for cap 8 -> 30) but F2 still failed at caps 6-20 and passed at
  x1 only for cap 30 (and failed at x1.5): a depth fix is not reliable.
- It needs: a bursty customer + an AIMD bulk customer sharing a thin link where the bulk customer dominates the judged
  customers. Hidden worlds can contain this, so it is a real risk, not a quirk of one file. Possible levers (not chosen): a mild
  weight tilt toward unlimited customers during a burst, or deeper AIMD queues. Both trade fairness or delay for F2; to decide.

### D2f. Scheduler (item 3) — DECIDED: DRR, equal weights for customers
Same 98 runs, per-customer queues, caps as above:
| Scheduler variant | Passes | Small-customer p99 delay |
|---|---|---|
| DRR, control weight 4 (baseline) | 97/98 | 2.8 ms mean, 7.5 ms max |
| control weight 1 or 16 | 97/98 | identical |
| small-customer weight 2 or 4 | 97/98 | identical (7.4 ms max) |
| **strict priority** (control, acks, then customers by class) | **47/98** | no better |
Conclusions: strict priority is out (a higher-priority queue starves the rest; passes collapsed to 47). Weights did not matter
(control traffic is tiny; small customers already wait ~3 ms), so use **equal DRR weights** and a modest control weight
(4). DRR equal weights already realise max-min among backlogged customers. Revisit weights only if a case shows a small
customer's delay or share suffering.

### D2b. How to size the class queues per port — DECIDED: S2 (capacity-proportional); parameters provisional, see results
We can only set per-queue caps, so "partitioning the buffer" means choosing those caps. Options:

| Option | Rule | Pro | Con |
|---|---|---|---|
| S1. Uniform | every port gets the same caps for the same class | trivial; consistent | worst-case sum = P x C x cap, oversubscribed; wastes depth on idle ports |
| S2. By link capacity | cap proportional to the port's link speed (thin 2 Mbit/s ports get shallow queues, 10 Mbit/s deeper), roughly a BDP share | matches what a queue can usefully hold; thin links are where delay explodes | ports that *become* congested in Part 2 (a detour) were sized for idle |
| S3. Fixed total per port, split by class weight | T per port divided across classes | each port bounded; simple rule | T must be guessed; ports with few classes waste share |
| S4. Reserve + oversubscribe | data caps oversubscribed against 40, control has a small reserved cap, rest relies on few ports being busy at once | maximises burst absorption | worst case still fills the pool; control reserve must be checked for 2-3 busy ports |
| S5. Dynamic | start small, resize with `set_queues` on drop reports | adapts | 2 ms + timer lag, mis-steps look like loss, more state |


How the S options relate to the original menu (`DESIGN_A2.md` section C, buffer allocation). Section C chose the
*switch-wide* policy for the 40-packet pool; the S options choose the *per-queue caps* that realise it.

| S option (this file) | Section C option (`DESIGN_A2.md`) | Relation |
|---|---|---|
| S1 uniform caps | C2 full oversubscription / C3 partial | S1 is how C3 is implemented when every port is treated alike. If the caps happen to sum to <= 40 it degenerates to C1 (strict partition). |
| S2 by link capacity | C4 per-port budget (by capacity) + the BDP rule of thumb in C | Gives each port a share of the pool in proportion to its link speed. |
| S3 fixed total per port | C4 per-port budget (equal split) + C1 inside a port | Equal budget per port, then partitioned strictly among the classes on it. |
| S4 reserve + oversubscribe | C3 + C6 reserve for control | What D1's headroom rule already requires. |
| S5 dynamic | C5 dynamic threshold | Approximates the DT scheme with periodic `set_queues`; slow and risky. |
C1 (strict partition, caps sum <= 40 over all queues on the switch) is the S3 limit with tiny caps; it was not
chosen because it leaves ~1-2 packets per queue and RateMatch needs more than that to survive its opening burst.

Tension to resolve: S2 would make thin (2 Mbit/s) links shallow because their bandwidth-delay product is only 1-2
packets, but the RateMatch startup trap happens on exactly those thin links and wants a few packets of depth
(a RateMatch sender keeps at least 4 packets in flight). So "scale caps down with capacity" may be backwards for
RateMatch classes. Needs an experiment before choosing.

#### D2b experiment results (S2 sweep, P2 classes, per-port caps)
Scenario set (45 runs per configuration, `experiments/robust.sh`): the 3 Part-1 worlds plus 6 variants with the thin
link changed to 3 or 4 Mbit/s, each at load x0.75, 1, 1.25, 1.5, 2. Part 1 only: Part 2 cannot be scored for sizing
until routing exists. "pass" = all three floors and no loops.

| Rule | Parameter | Result | Resulting caps (per data class) |
|---|---|---|---|
| uniform (S1) | cap 2 / 3 / 4 / 8 | 45/45 | 2-8 everywhere |
| uniform (S1) | cap 6 / 12 | 43/45 (grid x1.5, x2) | |
| uniform (S1) | cap 20 | 33/45 | |
| proportional to total link capacity (budget T x port/sum, split over 4 classes, min 2) | T = 16, 24, 32, 40, 60, 100 | **45/45 for every T** | thin 2-3 Mbit port: 2 (the floor binds); 10 Mbit port: up to ~8 at T=100 |
| linear in the port's speed, g = cap at 10 Mbit | g = 10, 20, 40 | 45/45 | thin 2-3 Mbit: 2-6 |
| linear | g = 30 | 37/45 | thin 3 Mbit: 9 |
| linear | g = 60 | 30/45 | thin 3 Mbit: 18 |
| inverse (thin deeper): K at 2 Mbit | K = 4, 8 | 45/45 | 2 Mbit: 4-8, 10 Mbit: 2 |
| inverse | K = 12, 16, 24 | 43, 42, 40 of 45 | 2 Mbit: 12-24 |

Reading it:
- **Shallow on thin links is safe; deep on thin links is not.** Every configuration with at most ~4-8 packets per
  class on the 2-4 Mbit link passed all 45 runs; every configuration with 9 or more on a thin link failed some run,
  and all failures are a RateMatch customer stuck near 0.1 Mbit/s (the startup trap) on grid or ring. This reverses
  my earlier worry that thin links need *more* depth for RateMatch.
- **The result is not monotonic** (g=40 passes between g=30 and g=60 failing), so the safe region has to be chosen
  with margin, not at its edge.
- **The proportional-to-total rule passes at every T** because the thin port's share falls below the floor, so the
  floor of 2 is what protects thin links. T only changes the cap on fast ports, which Part 1 never congests.
  Part 1 therefore says nothing about the right cap for 6-10 Mbit ports; Part 2 will congest them.
- Every passing configuration has the same worst case (Fortnite, `grid` at x2, 45% of fair share, floor 40%), which
  the cap does not change.
- Rule chosen for now: per-class cap = clamp(round(T x port_capacity / sum_of_switch_port_capacities / n_classes), 2,
  hi), T near 36 so the headroom rule of D1 holds with two congested ports; thin (<= 3 Mbit) ports sit at the floor.
  To revisit once Part 2 routing exists, because that is where fast-port depth matters.

Constraint that must hold under any of them (from D1): with `k` ports congested at once, `ctrl_reserve + k x
(sum of data caps on one port) <= 40` for the largest `k` we expect (>= 2 in Part 2).

### D2c. Queue layout per port and when queues come into existence — DECIDED: (a) all queues on every port, created at init
Indices fixed: 0 control, 1 misc/acks, 2 small, 3 bulk-AIMD, 4 bulk-RateMatch, 5 unlimited.
Full layout on every port: q0 control, q1 misc/acks, then the data classes (e.g. small, AIMD, RateMatch, unlimited).

What an unused queue costs (from `egress.rs`): **nothing in pool space** (the pool is charged by queued bytes, not by
caps) and nothing in the data plane. Real costs of having every queue on every port:
- Caps still add up: P ports x C queues x cap is the worst case, so caps must stay small or the headroom rule breaks.
- A small cap on a class that does carry traffic means early drops (and the RateMatch startup trap).
- DRR visits every backlogged queue each round; more backlogged queues = longer wait for each (matters for small/latency class).
- A bigger `set_queues` action (a few dozen bytes per port; 4 KB limit, fine for <= 5 ports).

Options: (a) create everything everywhere at init and size by rule (simple, no pool cost in this sim);
(b) create only the classes a port can carry, from the static model + chosen paths (needs path knowledge; changes
when routes move); (c) lazy: create a queue on the first packet of that class (**bad**, see below).

Why (c) is risky: `set_queues` lands >= 2 ms (+ timer delay) after the trigger; until then packets for an undefined
queue index are **clamped into the last existing queue**, so a customer's opening burst (the dangerous moment for
RateMatch) would share the wrong queue; replacing the queue list also re-indexes carried-over packets. If queues are
ever added later, order classes so that new ones never shift existing indices.

## D3. Destination learning, path set, path assignment — OPEN (options being weighed)

Facts established while preparing this (from the code and logs):
- The contract gives only each customer's *source* prefix. Each app has exactly one `peer` in the practice worlds
  (a Fortnite pair is two rows). The size-0 announcement each app sends at ~0.5 ms has `dst` = its **own** address, so
  it does not reveal the peer.
- A punted packet is **consumed** (`handle_punt_arrive` hands it to `on_punt`; nothing forwards it). A `Punt` action
  therefore costs the packet; the sender retransmits it.
- The grader's menu is directional, per (customer's switch, peer's switch): every simple path up to shortest+2 hops,
  and the greedy placement is run **per window on that window's demands**, so the reference path can change when an
  Amazon burst turns on or off.

### D3a. Destination learning (Q11) — DECIDED: L3 (learn from the first ack)
Caveat: the single ack punted per customer is consumed, so the sender sees one lost ack (one spurious retransmission /
halving for AIMD at start-up). Accepted: it is a one-time cost, tens of ms in.
Options table kept for the record:
| Option | Mechanism | Cost |
|---|---|---|
| L1. Punt every packet of an unseen source | `Punt` entry keyed on `ip_src` until replaced | Entry replacement lands >= 2 ms later, so several packets (incl. RateMatch's opening burst) are lost. Worst for the startup trap. |
| L2. **Punt-once latch** | per-customer register flag in the data plane: first packet reads flag==0, sets it to 1 immediately and punts | One packet lost per customer (retransmitted); the latch is instant, not subject to the 2 ms control delay. Needs a register-array stage (read + write = 2 memory accesses, one resource) |
| L3. **Learn from the first ack** | latch on acks arriving at the customer's own switch (`ip_dst` = customer, `ip_src` = receiver = peer) | No data packet is lost; costs one ack; reveals the peer only after one round trip (tens of ms). Needs the customer's attach switch (known from LSP) |
| L4. Fixed/static guess | none available | The contract has no peers; not viable |
Either L2 or L3 must then **flood** "customer prefix X talks to prefix Y" (a few bytes in the LSP) so every switch can run
the same placement.

### D3b. Path set — DECIDED: I3 (the grader's menu: every simple path up to shortest+2 hops, per direction)
| Option | Meaning |
|---|---|
| I1 shortest only | what A1 does; cannot pass Part 2 |
| I2 ECMP | only equal-cost paths; detours are longer, so insufficient |
| I3 **grader's menu** | every simple path up to shortest+2 hops per (src switch, dst switch), by hop count |
| I4 capacity-weighted shortest | prefers fat links; one fixed answer, no load awareness |
| I5 load-aware weights | adapts, but moved traffic can make the new path look full and bounce |

### D3c. Path assignment — DECIDED: J1 with the all-customers-active assumption (J2 per-state plans dropped; see model check)
| Option | Meaning |
|---|---|
| J1 **replicate the grader's greedy placement on every switch** | same inputs (topology, capacities, demands, learned pairs) -> same plan everywhere; no coordination traffic |
| J2 two plans (burst-on / burst-off) | the grader re-places per window, so precompute both and switch on a burst signal |
| J3 reactive move of the biggest offender | uses drop reports; adaptive but may oscillate |
| J4 coordinator | one elected switch decides and floods the plan; consistent but a flood delay and a single point |

#### Model check of the placement options (ideal queues; `experiments/te_*.py`)
A Python port of the grader's menu, greedy placement and max-min filling, with an idealised data plane (our queues
deliver max-min shares on the chosen paths). It scores a placement against the grader's per-state reference with
F1 = min(ours/ref) >= 0.4 and F2 = sum(ours)/sum(ref) >= 0.55 for every combination of bursty customers being on or off.
- Practice Part-2 worlds: placing **once, assuming every customer is active** keeps F1 >= 0.83 and F2 >= 0.85 in every
  burst state at load x1 and x1.5; shortest-path-only gets F1 0.23-0.56 and F2 as low as 0.47 (consistent with what we
  measured).
- 1,500 random worlds per load (rings, grids, dumbbells, random sparse graphs, 6-15 switches, degree <= 4, 1-3 thin
  links, random customer mixes with 1-4 bursty customers; **not** the real generator): all-on placement violates a floor
  in some burst state in 0.4% (x1) / 1.1% (x1.5) of worlds; placing as if bursty customers were absent fails 7.5% / 11%;
  shortest-path-only fails 21% / 25%. Failing all-on worlds have 3+ bursty customers or several unlimited ones.
- Takeaway: J1 with the all-active assumption is a robust baseline; per-state plans (J2) only buy the last ~1%.
  Per-state plans need an activity signal we do not have cleanly (see D3c discussion): the contract does not mark which
  customers are bursty, and a latch/punt probe per period would cost a lost packet each time (a spurious AIMD halving).

How a plan reaches the data plane: either hop-by-hop `(customer, dst)` entries at every switch on the path (J7;
needs every switch to agree) or source routing with a label stack pushed at the ingress (J6; transit switches only
follow labels, so no disagreement loops by construction). Acks have no customer id (receivers have no contract
row) and follow the default destination route.

### D3d. How a plan is delivered to the data plane — DECIDED: H2 (hop-by-hop entries keyed by plan epoch, two-phase update); H3 (path in the packet) is the fallback if H2 shows loops
Facts (from `packet.rs`, `tinyvm.rs`): `push_label`/`swap_label` take **immediates**, so a table cannot choose what to push;
there is no action that replaces the TinyVM program after `init`; the 4 `custom` packet fields are writable u64s that
travel with the packet; LPM compares only the top bits of the low 32 key bits; there is no shift but doubling (`add r,r,r`)
shifts left; each pipeline stage costs only 100 ns, so stage count is not a performance concern.

Why it matters: the grader counts *any* packet that revisits a switch, including during start-up. A plan that switches
see at slightly different times (pairs are learned and flooded one customer at a time) can make two switches point at each
other for a few milliseconds.

| Option | Mechanism | Loop safety | Cost |
|---|---|---|---|
| H1. Hop-by-hop `(customer, dst)` entries | every switch on the path installs an Exact entry; default dst route elsewhere | Not by itself: a switch that has not yet installed its entry sends the packet by default route, which can come back | Simplest; entries are plain table entries |
| H2. H1 + plan epoch (two-phase update) | entries are keyed by `(epoch, customer, dst)`; transit switches install epoch N+1 entries first, then after a settle delay the *ingress* switch starts stamping epoch N+1 into `custom0` | Per-packet consistent: a packet carries one epoch end to end, and its entries exist everywhere before anyone stamps it | A little more data-plane work and controller state; one fixed delay to pick |
| H3. Source route in the packet | ingress writes the whole path as 4-bit switch ids into `custom0`; every switch reads the top nibble (LPM, prefix 4), forwards to that neighbour, shifts the word left by 4 | Cannot loop: the path is finite and carried by the packet; transit switches need no plan knowledge, only their own neighbour table | Most data-plane logic; <= 16 switch ids and 8 hops per 32-bit word (more needs a second field); ids >= 16 unrepresentable |
Not viable: label stacks pushed from a table (immediates only).

## D4. Decisions still open (ordered by risk to passing)

Status update: D3 is implemented (see `DESIGN_A2.md` Implementation status) and verified: official practice worlds all pass, Part 1 set 98/98,
random Part 2 worlds 157/160. Items 4-6 below were implemented with fixed values (neighbour timeout 200 ms, settle 300 ms, drain 1 s,
deterministic plan, epoch = hash of plan); the remaining items are tuning and validation.


Tier 1 — can break correctness or robustness
1. ~~Class thresholds.~~ RESOLVED by D2e (low stakes; relative rule). Original concern: "Small" = ask < 1 Mbit/s is a hard-coded guess: Fortnite at x1.5 asks 0.9 but at x2 asks 1.2 and would flip
   class. Need a rule that survives load scaling and unusual contracts (e.g. relative to the link/fair share, or by packet
   size observed, or by rank among customers).
2. **RateMatch startup-trap protection.** Shallow caps avoided it in our scenarios, but the mechanism (opening burst loses
   nearly everything) is still there; a moved customer (Part 2) restarts its probing. Decide whether caps alone are enough or
   the RateMatch class needs a specific rule (cap, weight, or burst allowance), validated beyond the practice worlds.
3. ~~Scheduler: DRR vs strict priority, and weights.~~ RESOLVED by D2f. Original: One discipline per port: control must get priority via a high DRR weight
   (no SP+DRR mix). Decide quantum/weights for control, small/latency class, and equal vs ask-proportional weights for bulk.
4. **Control timing and reserve.** Neighbour timeout (A1: 150 ms), hello/LSP periods, control-queue cap, and the *minimum* pool
   headroom that keeps control alive with k congested ports (measured only at a generous 12 packets so far).
5. **Plan inputs and determinism.** What exactly feeds the placement: contract asks x load-scale, unlimited as infinite, capacities
   and attachments from the LSP, learned pairs from L3. Deterministic tie-breaking so every switch computes the identical plan;
   behaviour while pairs are still unknown (default shortest paths); customers whose peer never becomes known; Fortnite pairs
   (two rows, both directions).
6. **Epoch/two-phase mechanics.** Settle delay value; when a new epoch is triggered (each learned pair vs batched); how old-epoch
   entries are garbage-collected; table sizing; spreading installs across calls under the 4 KB action limit; entry-id management.

Tier 2 — tuning and secondary behaviour
7. **Ack and reverse-direction handling.** Acks have no customer id and follow the default destination route; decide whether that
   is acceptable on thin links, and what queue acks use (misc q1, or the Fortnite customer's queue when the receiver is a row).
8. **Drop policy.** Leaning: no `drop_at` (it acted like a second cap); confirm and decide the misc/ack queue cap.
9. **Adaptive correction.** Only if testing shows the static plan failing: drop-report trigger, hysteresis, hold-down. Currently deferred.
10. **Load information.** With the static model we do not need `load_report`; confirm we drop it (saves control traffic, removes its
    5 s memory problem) and what, if anything, drop counts are still used for.
11. **Fuel/size budget.** Menu enumeration + greedy placement + max-min inside one call (4M fuel, 4 KB output); measure on a
    15-switch world.

Tier 3 — validation and deliverables
12. **Test plan beyond the practice worlds.** Turn the random-world generator into real `.toml` worlds and run the real simulator
    (not just the model), including load x0.5-2, many topologies, thin-link variants, and same-class customer pairs.
13. **Known assumptions to state or remove:** one peer per customer, switch ids < 16 (only if H3), at most 5 ports, packet sizes.
14. **Design card / quiz material** (format not constrained).

## Known hazards carried by these decisions
- **RateMatch startup trap**: an unlimited RateMatch customer can lose nearly its whole opening burst and stick at
  ~0.1 Mbit/s forever. Outcomes are non-monotonic in queue depth. Any chosen caps must be checked across all worlds,
  both load scales, and a few neighbouring cap values, not just one.
- **Same-class interference** (D2 point 5) is invisible in the practice worlds.
- **Part 2 moves traffic**, so port sizing based on today's busy ports can be wrong tomorrow.
