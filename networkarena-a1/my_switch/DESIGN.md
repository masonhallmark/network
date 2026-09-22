# Switch Design Document — Link-State Routing

**Status:** draft · **Scope:** `src/lib.rs` control + data plane for the competitive net sim
**Target world size:** 5–15 switches (`docs/LIMITS.md`)

---

## 1. Overview and scope

Each switch runs an independent copy of this program. No switch sees the global
topology; every switch learns the network by exchanging messages only with its
direct neighbors. The program is responsible for two things the scaffold does
not do: **propagation** (getting every customer prefix into every switch's
forwarding table) and **recovery** (re-routing around a dead link inside the
world's recovery budget).

The scaffold already provides neighbor discovery (which switch is on each port),
raw liveness plumbing, and learning of directly-attached customer prefixes. This
document covers everything built on top of that.

The chosen approach is a **link-state protocol**: each switch floods a
description of its own links (and the prefixes it serves) to every other switch,
every switch assembles those descriptions into a full topology map, and each
switch independently computes shortest paths over that map with Dijkstra's
algorithm.

---

## 2. Terminology

- **Switch id** — a stable identifier for a switch (learned via neighbor discovery).
- **Port** — a physical interface on this switch; one link per port.
- **Neighbor** — the switch directly attached to a given port.
- **LSP (Link-State Packet)** — the announcement a switch floods describing its
  own adjacencies and originated prefixes.
- **LSDB (Link-State Database)** — the collection of the most recent LSP from
  every known switch; this is the topology map.
- **Control plane** — the routing logic (flooding, LSDB, Dijkstra). Runs in the
  background, never per data packet.
- **Data plane** — the installed forwarding entries (`T_ROUTE`) the hardware
  consults per packet.

---

## 3. Architecture: control plane vs data plane

The two planes are kept strictly separate, matching how real routers work (SPF
runs in the background; the per-packet path is only a table lookup).

- **Control plane** owns the neighbor table, the LSDB, and the computed routing
  table. It reacts to timer ticks and to received control packets.
- **Data plane** is the set of `T_ROUTE` entries installed into the switch. It is
  a *projection* of the control plane's routing table — the control plane is the
  source of truth, and it reconciles the data plane toward its computed result.

The only work done on the customer-packet path is: if a packet punts as
`NoRoute`, hand it to the control plane's already-computed routing table, install
the entry, and forward. No Dijkstra, no flooding, no allocation on this path.

---

## 4. Core data structures

All sizes are bounded by the world limits (≤15 switches, ≤1,024 table entries).
Fixed-capacity collections are preferred over unbounded growth so per-call work
stays bounded (`docs/switch-programs.md`).

```rust
type SwitchId = u32;
type PortId   = u32;

/// A customer prefix. Fixed /24 for now (see §8).
struct Prefix {
    network: u32,   // e.g. 10.0.90.0
    len: u8,        // always 24 for now
}

/// One entry per local port, describing the neighbor on it.
struct NeighborEntry {
    neighbor_id: SwitchId,
    port: PortId,
    link_cost: u32,       // always 1 for now (see §5.2)
    last_hello_ns: u64,   // timestamp of most recent HELLO received
    up: bool,             // false once NEIGHBOR_TIMEOUT_NS elapses
}

/// One LSP per known switch — the flooded description of that switch.
/// The set of these IS the topology map.
struct LinkStateEntry {
    origin: SwitchId,
    seq: u32,                       // monotonic per-origin sequence number
    installed_ns: u64,              // when we last accepted a refresh; drives aging
    adjacencies: Vec<(SwitchId, u32)>,  // (neighbor, cost)
    prefixes: Vec<Prefix>,          // customer prefixes this origin serves
}

type Lsdb = HashMap<SwitchId, LinkStateEntry>;

/// Result of running Dijkstra from this switch. One entry per reachable dest.
struct RouteEntry {
    dest_switch: SwitchId,
    next_hop_port: PortId,
    cost: u32,
}

/// Top-level program state.
struct SwitchState {
    self_id: SwitchId,
    self_seq: u32,                       // our own LSP sequence counter
    neighbors: HashMap<PortId, NeighborEntry>,
    lsdb: Lsdb,
    routes: HashMap<SwitchId, RouteEntry>,   // control-plane routing table
    prefix_routes: HashMap<Prefix, PortId>,  // prefix -> egress port (mirrors data plane)
    dirty_installs: Vec<Prefix>,             // work queued when > budget per call (§9)
}
```

The `prefix_routes` map is what the installed `T_ROUTE` entries are reconciled
against: after each SPF run we diff the newly computed prefix→port mapping
against `prefix_routes` and emit only the `InstallTableEntry` / `DeleteTableEntry`
actions needed to close the gap. This diff-based reconciliation keeps output
actions minimal (§9).

---

## 5. Protocol design

### 5.1 Neighbor discovery and HELLO

Neighbor discovery (learning which switch sits on each port) is provided by the
scaffold. On top of it we run a HELLO heartbeat:

- On every timer tick, send a HELLO out every port.
- On receiving a HELLO, refresh `last_hello_ns` for that neighbor and mark it
  `up`.
- On the timer, any neighbor whose `last_hello_ns` is older than
  `NEIGHBOR_TIMEOUT_NS` is marked `down` — this is the failure signal (§7).

### 5.2 Link-state announcements (LSP contents)

Each switch originates exactly one LSP describing itself. It carries:

1. **Adjacencies** — one `(neighbor_id, cost)` pair per *up* neighbor. This is
   the "links and their cost" that we announce.
2. **Originated prefixes** — the customer prefixes directly attached to this
   switch.

> **Note (addition to stated scope):** announcing only links is not sufficient
> for a link-state protocol to route customer traffic. Switches would learn the
> shape of the network but not *where the customer prefixes live*. Therefore each
> LSP also carries the originating switch's directly-attached prefixes. Dijkstra
> then gives a next-hop toward the *switch* that owns a prefix, and the prefix is
> installed pointing at that next-hop's port.

> **Note (link costs — future work):** for now every link is announced with
> **cost = 1**, so shortest path is fewest hops. In future development we will
> assign real costs derived from measured **time to deliver** across each link
> (e.g. smoothed round-trip delay, as the ARPANET metric evolved to do). The LSP
> format already carries a per-adjacency cost field, so this is a value change,
> not a format change. Real costs should be range-compressed and smoothed to
> avoid the classic instability where a link oscillates between "cheap" and
> "expensive" and drags all traffic back and forth.

### 5.3 Reliable flooding

LSPs propagate by controlled flooding:

- When a switch receives an LSP, it compares the LSP's `seq` against what it has
  stored for that origin.
  - **Higher seq** → accept, replace the LSDB entry, and re-flood to every
    neighbor *except the one it arrived from* (split horizon on the flood path).
  - **Equal or lower seq** → drop, do not re-flood. This is the loop/duplicate
    stopper — without it LSPs circulate forever.
- A switch originates a new LSP (incrementing its own `seq`) whenever its set of
  up-neighbors or originated prefixes changes, and also periodically as a refresh
  (§5.4).

Sequence numbers, not a visited-node list, are what make flooding terminate.

### 5.4 LSDB sequence numbers and aging

- `self_seq` is a monotonic `u32` bumped each time we originate a new LSP.
- Each LSDB entry records `installed_ns`. On the timer, any entry not refreshed
  within `MAX_LSP_AGE_NS` is removed — this is how a switch that has silently
  disappeared (not just a single link) eventually drops out of every topology
  map. Periodic refresh (§5.3) keeps live entries from aging out.
- Sequence-number wraparound is not a concern at this world size and runtime, but
  the comparison should treat the space as large and use "greater than" on `u32`
  with the understanding that worlds do not run long enough to wrap.

---

## 6. Route computation (Dijkstra)

### 6.1 What we compute

On each recompute we run Dijkstra from `self_id` over the LSDB, producing a
shortest-path tree. From that tree we derive, for every reachable destination
switch, the **first-hop port** out of this switch. Then, for every prefix in the
LSDB, we map `prefix → next_hop_port_toward(origin_of_prefix)`. That mapping is
reconciled into the data plane (§4).

Dijkstra uses a deterministic tie-break (lowest destination switch id) so that
two switches with the same topology map always agree on next-hops. This matters
during the brief windows where LSDBs are momentarily inconsistent — deterministic
tie-breaking reduces transient micro-loops.

### 6.2 When we compute — precompute on every timer

> **Design decision:** we run Dijkstra on **every timer tick** (~50 ms), rebuilding
> routes from the current LSDB, rather than only on detected topology change.
>
> Rationale: at 5–15 nodes a full Dijkstra is a few hundred relaxation steps —
> negligible against the ~4,000,000-instruction per-call budget (well under 1%).
> Recomputing unconditionally removes a whole class of bugs where a topology
> change fails to trigger a recompute and routes silently go stale. The cost of
> "wasted" recomputes when nothing changed is trivial, and the diff-based
> reconciliation (§4) means an unchanged result produces zero output actions.

### 6.3 Alternative — lazy path resolution on punt

> **Design decision (available fallback):** instead of (or in addition to)
> precomputing all paths every tick, we can resolve a path **lazily on punt**:
> when a customer packet punts as `NoRoute` for a destination whose next-hop we
> have not yet installed, compute/consult the route for that one destination at
> punt time, install the single `T_ROUTE` entry, and forward.
>
> When this helps: if the prefix count ever grows enough that installing the full
> forwarding table in one recompute approaches the output-action limit (§9),
> lazy resolution spreads installs out over actual traffic demand — you only
> install routes for destinations that are actually being talked to.
>
> Tradeoff: the first packet to an as-yet-uninstalled destination eats a punt (and
> the small resolution cost) instead of hitting an already-installed entry. At
> this world size the precompute-every-tick approach is the default because the
> full table fits comfortably; lazy resolution is documented as the escape hatch
> if that stops being true.

---

## 7. Failure detection and recovery

### 7.1 Detection

Detection is HELLO-based (§5.1). A neighbor is declared `down` when no HELLO has
arrived within `NEIGHBOR_TIMEOUT_NS`. With a ~50 ms timer, a timeout of a few
missed HELLOs gives detection in the low-hundreds of milliseconds — comfortably
inside a ~1000 ms recovery budget, while tolerating one dropped HELLO without a
false positive.

The tradeoff being managed: too short a timeout → false positives on ordinary
jitter and wasted re-floods; too long → detection alone consumes the recovery
budget before re-routing even starts.

### 7.2 Recovery — delete on dead neighbor, let it re-punt

> **Design decision:** when a neighbor goes `down`, delete every `T_ROUTE` whose
> egress port is that neighbor's port. Do **not** attempt to pre-install a backup
> path.
>
> After deletion, the next customer packet for an affected destination punts as
> `NoRoute`. By then the control plane has re-originated its LSP (the dead link
> is gone from our adjacencies), flooded it, and rerun Dijkstra over the updated
> topology, so the punt resolves onto the new path — or correctly finds no path,
> in which case that destination is genuinely unreachable and C1/C2 reflect it.
>
> Tradeoff: this is slower than maintaining a precomputed backup next-hop (a brief
> unreachability window while reconvergence happens) but far less code and state.
> At this world size the window fits inside the recovery budget. Precomputed
> backups are noted as future work if C4 timing ever becomes tight.

On a dead neighbor we also drop it from our adjacency set and originate a fresh
LSP immediately (a triggered update), rather than waiting for the next periodic
refresh, so the rest of the network learns of the failure as fast as possible.

---

## 8. Data-plane forwarding and prefix model

> **Design decision:** every customer prefix is a **/24**, and the customer
> application responds at the **first host address** of that prefix. For example,
> `10.0.90.0/24` is reachable at `10.0.90.1`.

Implications for the forwarding logic:

- A packet's destination address is masked to its /24 network to find the prefix.
- The installed `T_ROUTE` entry maps that /24 prefix to an egress port.
- Longest-prefix match is still used by the hardware, so if a more specific route
  is ever installed it will correctly win — this keeps the door open for the
  future work below without changing the data-plane contract.

> **Note (future work):** the fixed /24 assumption matches the practice worlds but
> is an assumption, not something the program derives. It breaks on a hand-written
> world with a different addressing scheme. A more robust alternative is to install
> a `/32` host route per customer address actually observed — assumption-free, one
> entry per address rather than per subnet, and composes cleanly with LPM. We keep
> the /24 assumption for now and revisit if worlds vary the addressing.

---

## 9. Resource budget management

Per-call limits (`docs/LIMITS.md`): ~4,000,000 instructions and ~4,096 bytes of
returned actions (~150 table installs).

- **Instructions:** the only non-trivial computation is Dijkstra, which is tiny at
  this scale (§6.2). Flooding and liveness are linear scans over ≤15 switches /
  their neighbors, explicitly fine per `docs/switch-programs.md`. No concern.
- **Output actions:** the real pinch point. A single topology change can, in the
  worst case, shift many prefixes onto new next-hops at once, and a full
  reconciliation could exceed ~150 installs. Mitigation: the diff-based
  reconciliation only emits actions for prefixes whose next-hop actually changed,
  and if the diff exceeds the per-call budget, the overflow is queued in
  `dirty_installs` and drained on subsequent timer ticks. Recovery therefore
  spreads across more than one handler call rather than trying to do everything in
  one return — which is also why the delete-and-re-punt recovery (§7.2) is a good
  fit, since re-punts naturally pull in whatever hasn't been installed yet.

---

## 10. Constants

| Constant | Value (initial) | Meaning |
|---|---|---|
| `TIMER_PERIOD_NS` | ~50 ms | How often `on_timer` runs |
| `HELLO_TICK_NS` | ~50 ms (every tick) | How often HELLOs are sent |
| `NEIGHBOR_TIMEOUT_NS` | ~150 ms (≈3 ticks) | Silence before a neighbor is declared down |
| `LSP_REFRESH_NS` | ~200 ms | Periodic re-origination of our own LSP |
| `MAX_LSP_AGE_NS` | ~1 s | LSDB entry expiry if never refreshed |
| `DEFAULT_LINK_COST` | 1 | Cost announced per link (future: measured delay) |
| `PREFIX_LEN` | 24 | Assumed customer prefix length |
| `recovery_budget_ms` | ~1000 (world-set) | Deadline detection + reroute must fit under |

Values are starting points to tune against the practice worlds; the key
invariant is `detection + flooding + SPF + reconvergence < recovery_budget_ms`.

---

## 11. Design decisions and tradeoffs — summary

| Decision | Choice | Chief tradeoff |
|---|---|---|
| Protocol shape | Link-state (flood links, local Dijkstra) | Most robust convergence, no count-to-infinity; more code than distance-vector |
| What we announce | Adjacencies + originated prefixes, cost 1 each | Simple now; cost field ready for measured delays later |
| Loop prevention (flood) | Per-origin sequence numbers + split-horizon flood | Bounded state; rejects stale/duplicate LSPs |
| Loop prevention (compute) | Rerun Dijkstra on full topology each tick | Removes stale-route bugs; wasted recomputes are cheap |
| Compute timing | Precompute every `on_timer` | Simplicity over minimal CPU; lazy-on-punt available as fallback |
| Failure detection | Missed HELLOs over `NEIGHBOR_TIMEOUT_NS` | Fast enough for budget; tolerates one dropped HELLO |
| Dead neighbor | Delete affected routes, let re-punt | Minimal code; brief unreachability vs. precomputed backup |
| Prefix model | Fixed /24, app at first host addr | Matches practice worlds; not derived, revisit with /32 later |
| Output budget | Diff-based reconcile, spill to `dirty_installs` | Recovery may span several ticks under heavy churn |

---

## 12. Future work

- **Real link costs** from measured time-to-deliver, range-compressed and smoothed
  to avoid oscillation (§5.2).
- **Per-`/32` host routes** to drop the fixed-/24 assumption (§8).
- **Precomputed backup next-hops** (loop-free alternates) if C4 timing gets tight
  (§7.2).
- **Lazy path resolution on punt** promoted from fallback to default if prefix
  counts grow enough to stress the output-action budget (§6.3).

---

## 13. Testing and report-card mapping

- **C1/C2 (reachability):** exercised by the no-failure (Part 1) worlds. Get these
  clean *first* on a topology before touching its Part 2 (failure) twin.
- **C3 (no data loops):** driven by correct flooding dedup (sequence numbers) and
  deterministic Dijkstra tie-breaking. If C3 fails, suspect the control-plane LSDB
  / SPF, not the data-plane forwarding.
- **C4 (recovery in budget):** driven by detection timeout + flooding + SPF +
  reconciliation fitting under `recovery_budget_ms`. If Part 1 is clean and Part 2
  fails, the bug is specifically in detection or reconvergence.

Build and score:

```sh
cd my_switch
cargo build --release --target wasm32-unknown-unknown
cd ..
./target/release/competitive_net_sim run-world \
  worlds/practice/practice-ring-002.toml \
  --program my_switch/target/wasm32-unknown-unknown/release/my_switch.wasm \
  --score
```

---

## 14. Assumptions to confirm

- Control packets (HELLO and LSP) arrive via the punt path and are distinguished
  from customer traffic by a well-known marker (port/ethertype/destination). If
  the SDK provides a dedicated control channel instead, §5 slots onto that with no
  logic change.
- Switch ids are stable for the lifetime of a world run (used as Dijkstra
  tie-break and LSDB key).
- `recovery_budget_ms` is ~1000 ms per `worlds/practice/failures/*.toml`; retune
  §10 constants if a target world differs.