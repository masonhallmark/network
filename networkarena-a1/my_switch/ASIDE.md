# ASIDE: what's left, and the decisions involved

`src/lib.rs` in this crate already does three things: learns who's on each
port (neighbor discovery), notices when a neighbor stops talking (liveness),
and learns directly-attached customer prefixes. Those are mechanical —
there's basically one reasonable way to do each of them, and the scaffold
does it.

What's left is not mechanical. It's the actual assignment:

1. **Propagation** — getting a prefix you know about into the routing
   tables of switches that aren't directly attached to it.
2. **Recovery** — noticing a link died and re-routing around it inside the
   world's recovery budget (report card criterion C4).

Both come down to picking a routing protocol shape and living with its
consequences. Below are the decisions that shape, roughly in the order
you'll hit them.

## 1. Distance-vector vs. link-state vs. flood-everything

You need every switch to end up with a route to every customer prefix,
using only pairwise HELLO-style messages between neighbors (no switch sees
the global topology — `docs/switch-programs.md` is explicit about this).
Three shapes, roughly in order of "less to build" to "more robust":

- **Flood-and-cache.** When you learn a new prefix (directly, or from a
  neighbor), announce it to every neighbor except the one you heard it
  from. Everyone eventually hears everything. Simple, but you need a way
  to stop packets from flooding forever — e.g. a sequence number per
  origin switch, so a switch drops (doesn't re-flood) an announcement it's
  already seen or a strictly older one.
- **Distance-vector (RIP-like).** Each switch tracks, per prefix, the best
  known distance and which neighbor it heard it from; it announces its
  current table to neighbors periodically and on change. Cheaper to keep
  correct than flooding (no separate dedup mechanism — the DV update rule
  handles it), but classic distance-vector has slow-convergence and
  count-to-infinity failure modes when a route disappears, which is
  exactly the case you're graded on (C4). Split horizon (never tell a
  neighbor about a route whose next hop is that neighbor) or split horizon
  with poison reverse mitigates this and is worth the small extra
  complexity.
- **Link-state (OSPF-like).** Each switch floods *link* information (its
  own neighbor list, not its routes) network-wide; every switch computes
  shortest paths locally once it has a consistent picture. Most robust
  recovery behavior, most work to build (you're computing shortest paths
  inside a fuel-limited `on_punt`/`on_timer` call — see §5 below).

For a topology this small (5-15 switches, `docs/LIMITS.md`), any of the
three fits inside the resource limits. The real trade-off is convergence
speed and correctness-under-failure vs. implementation effort. Distance
vector with split horizon is the usual sweet spot for an assignment this
size.

## 2. What exactly do you announce?

- A **whole table dump** every tick is the simplest to implement and to
  reason about (no partial-state bugs), at the cost of more control
  bandwidth as the topology grows. At this scale it's affordable.
- **Incremental updates** (only what changed since the last announce) are
  cheaper but require your announce and receive paths to agree on when
  something "changed," which is more state to get right.

Whichever you pick, the announce message is your own format — encode it
however you like. Reusing `postcard` (already a transitive dependency via
the SDK) with a `#[derive(Serialize, Deserialize)]` struct is the path of
least resistance instead of hand-rolling a byte layout.

## 3. Loop prevention and staleness

Any propagation scheme needs a way to reject information that's stale or
circular:

- Sequence numbers or timestamps per originating switch, so a switch can
  tell "this announcement is newer than what I have" from "I've already
  processed this."
- Split horizon (distance-vector) or a visited-switch-id list (flooding)
  to keep announcements from circulating forever.
- Note `docs/switch-programs.md`'s per-call limits: this bookkeeping has
  to fit in a bounded amount of state and bounded per-call work — no
  unbounded history of every announcement ever seen.

Getting this wrong shows up as **C3 (loops)** in the report card for
*data* packets, but the underlying cause is usually stale or circular
*routing* state, not the data plane itself misbehaving.

## 4. Failure detection thresholds

`HELLO_TICK_NS` (how often you say hello) and `NEIGHBOR_TIMEOUT_NS` (how
long silence means "dead") trade against each other and against the
world's `recovery_budget_ms` (see `worlds/practice/failures/*.toml` —
commonly 1000 ms):

- Too aggressive (short tick, short timeout) → false positives on ordinary
  jitter, and more control traffic competing with customer packets for
  link bandwidth (bandwidth is not free — see `docs/LIMITS.md`, "not
  limited" is wall-clock and packet *count*, not bandwidth).
- Too conservative → you blow the recovery budget just on *detection*,
  before you've even started re-routing.

Leave yourself a comfortable margin: detection time + propagation time to
find an alternate route both have to fit under the budget.

## 5. Fuel and output-size budgets while doing real work

`docs/LIMITS.md`: 4,000,000 instructions per call, 4,096 bytes of
returned actions per call (roughly 150 table installs). A loop over every
switch or every neighbor you know about is explicitly called out as fine
at this scale. Where this bites:

- If you do link-state shortest-path computation, do it in response to a
  topology change, not from scratch inside every `on_punt`.
- If a neighbor dying invalidates many routes at once, you may need more
  than 150 `DeleteTableEntry`/`InstallTableEntry` actions to fully
  recover — spread the work across more than one return if you have to
  (e.g., react further on the next timer tick rather than trying to do it
  all in one handler call).

## 6. Prefix length: assumed vs. derived

The scaffold hardcodes `prefix_len = 24` for a newly-seen customer,
matching what the practice worlds hand out (`worlds/practice/*.toml`).
That's an assumption, not something the program derives. Alternatives:

- Install a `/32` host route per customer address actually observed.
  Simpler and assumption-free, costs one table entry per customer address
  rather than per customer subnet — fine at these table-size limits
  (1,024 entries/table), and LPM naturally prefers the most specific match
  so this composes fine with prefixes learned from neighbors.
- Keep the fixed assumption but document it, and note it breaks on a
  hand-written world with a different addressing scheme.

## 7. What a dead neighbor invalidates

When `NEIGHBOR_TIMEOUT_NS` fires, at minimum every route whose
`egress_port` is that neighbor's port is now wrong. Cheapest correct
option: delete those `T_ROUTE` entries outright — the next packet for
that destination gets punted again as `NoRoute`, and whatever your
propagation logic has settled on by then installs a fresh route (or
doesn't, if there's genuinely no other path, in which case C1/C2 correctly
reflect that some pairs became unreachable). This is slower than
maintaining a precomputed backup path but far less code, and at this
world size the difference is usually inside the recovery budget.

## 8. Testing loop

`practice-<name>.toml` and `part2-<name>.toml` share a topology — the
README's own advice: get C1/C2 clean on the Part 1 (no-failure) twin
first. If Part 1 is clean and Part 2 fails, the bug is specifically in
detection or re-routing, not in steady-state routing — don't go looking in
`handle_customer_punt` for a `handle_hello`/timer bug.

```sh
cd my_switch
cargo build --release --target wasm32-unknown-unknown
cd ..
./target/release/competitive_net_sim run-world \
  worlds/practice/practice-ring-002.toml \
  --program my_switch/target/wasm32-unknown-unknown/release/my_switch.wasm \
  --score
```
