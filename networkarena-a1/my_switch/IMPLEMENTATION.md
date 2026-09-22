# Implementation notes — mapping `DESIGN.md` to `src/lib.rs`

This maps every numbered section of `DESIGN.md` to the code that implements
it, notes the handful of places the code had to go beyond or slightly adjust
the literal design text, and records test results against the practice
worlds. Line numbers are current as of this writing; symbol names (function
and struct names) are the stable reference if the file drifts.

---

## 1–3. Overview, terminology, control/data plane split

No dedicated code — this is the shape of the whole file. The split is:

- **Control plane** = `MySwitch`'s fields (`neighbors`, `lsdb`, `routes`,
  `prefix_routes`, ...) plus every method on it, driven only by
  `on_punt`/`on_timer` (`src/lib.rs:529`, `:538`).
- **Data plane** = the two-stage TinyVM pipeline and the two tables declared
  in `init` (`src/lib.rs:468`). `T_PROTO` (exact match) catches HELLO and
  route-announce control packets and punts them; `T_ROUTE` (LPM) is the real
  forwarding table the control plane reconciles toward.

The "no Dijkstra/no flooding on the customer-packet path" rule (§3) holds:
`handle_customer_punt` (`:274`) never calls `run_dijkstra`; it only ever
installs a trivial direct route (egress = the port the packet arrived on)
and fires off one LSP flood (network I/O, not computation).

## 4. Core data structures

| Design type | Code | Notes |
|---|---|---|
| `Prefix` | `struct Prefix` (`:63`) | `network: u32, len: u8`, exactly as designed. Derives `Eq + Hash` (needed as a `HashMap` key; the design text omitted the derive but requires it structurally). |
| `NeighborEntry` | `struct NeighborEntry` (`:68`) | Same fields. Kept even when `up == false` (matches the design's `up: bool` field) rather than removed from the map, unlike the original scaffold which deleted the entry outright — this is what lets a *returning* neighbor be recognized as "was down, now up" and re-trigger origination (see §7 below). |
| `LinkStateEntry` | `struct LinkStateEntry` (`:88`) | Same fields, minus `origin` (redundant — it's already the `HashMap<u32, LinkStateEntry>` key, called `lsdb` at `:103`). |
| `RouteEntry` | `struct RouteEntry` (`:97`) | Same, minus `dest_switch` (same reasoning — it's the `HashMap<u32, RouteEntry>` key, `routes`). |
| `SwitchState` | `struct MySwitch` (`:103`) | One addition beyond the design: `prefix_routes` maps to `(egress_port, entry_id)`, not just a port. See the entry-id note under §6/§9 below for why. |

## 5. Protocol design

- **§5.1 HELLO** — `on_timer` (`:538`) injects a HELLO on every local port
  every tick, exactly as the scaffold already did. `handle_hello` (`:202`)
  refreshes `last_hello_ns`/`up` and, new here, detects the
  down→up *or* absent→up transition (`was_up`) as "new topology
  information" and immediately calls `originate_new_lsp` — so a repaired
  link doesn't wait for the next periodic refresh to be re-announced.
- **§5.2 LSP contents** — `LspMessage` (`:79`) is the wire struct:
  `origin`, `seq`, `adjacencies: Vec<(u32, u32)>`, `prefixes: Vec<Prefix>`.
  `self_lsp` (`:138`) builds it from the current up-neighbor set and
  `local_prefixes`. Encoded with `postcard::to_allocvec` / decoded with
  `postcard::from_bytes` (`flood_lsp` at `:177`, `handle_route_announce` at
  `:236`) — the `#[derive(Serialize, Deserialize)]` + postcard approach
  `ASIDE.md` §2 recommends.
- **§5.3 Reliable flooding** — `handle_route_announce` (`:236`) implements
  the accept rule (`lsp.seq > existing.seq`, or no existing entry) and,
  on accept, re-floods via `flood_lsp(&lsp, Some(ev.ingress_port))` — split
  horizon on the flood path, never back out the arrival port.
  `originate_new_lsp` (`:156`) is the "flood to everyone" case
  (`except_port: None`), used when *we* are the origin.
- **§5.4 Sequence numbers and aging** — `self_seq` bumps in
  `originate_new_lsp`. Aging is `lsdb.retain(...)` in `reconcile_routes`
  (`:407`), dropping any entry not refreshed within `MAX_LSP_AGE_NS`.
  Per the design's own note, wraparound is not handled (`seq > existing`
  is a plain `u32` comparison) — worlds don't run long enough to hit it.

## 6. Route computation

- **§6.1 Dijkstra** — `run_dijkstra` (`:344`). Standard Dijkstra over
  `lsdb`, tracking `first_hop` (the port out of *this* switch) alongside
  `dist`. The priority queue is `BinaryHeap<Reverse<(cost, switch_id)>>`,
  which is a min-heap ordered by `(cost, switch_id)` — so among nodes
  tied for the same tentative distance, the lowest switch id is *settled*
  first. That's my reading of the design's "deterministic tie-break
  (lowest destination switch id)": every node visited during the
  traversal is a candidate "destination" at the moment it's popped, so
  the rule is about pop order among ties, not about comparing two already
  -fixed destinations against each other (which wouldn't make sense — a
  given route's destination isn't a variable to tie-break on). Flagging
  this interpretation explicitly in case the intent was different.
- **§6.2 Precompute every tick** — `reconcile_routes` runs unconditionally
  from `on_timer` (`:538`) every tick, exactly as designed (no
  change-detection gate).
- **§6.3 Lazy-on-punt** — not implemented; it's documented in the design
  itself as an available fallback, not the default, and isn't needed at
  this world size (confirmed: 20/20 practice-world runs stay well inside
  both the fuel and output-action budgets — see Testing below).

## 7. Failure detection and recovery

- **§7.1 Detection** — the dead-neighbor scan in `on_timer` (`:538`),
  unchanged in shape from the scaffold: any `up` neighbor silent for more
  than `NEIGHBOR_TIMEOUT_NS` is marked `up = false`.
- **§7.2 Recovery** — for each newly-dead port, every `prefix_routes` entry
  whose egress is that port gets withdrawn via `withdraw_route` (`:327`),
  and a fresh LSP is originated immediately (`topology_changed` branch,
  `:538`) with the dead neighbor already excluded from `self_lsp`'s
  adjacency list (`self_lsp` filters on `n.up`). No backup path is
  precomputed, matching the design's "delete and let it re-punt" choice.

## 8. Data-plane forwarding and prefix model

`PREFIX_LEN: u8 = 24` (`:48`) and the masking in `handle_customer_punt`
(`:274`) implement the fixed-/24 assumption directly (`network: ev.ip_src &
mask`). Simplified slightly from the scaffold: since `PREFIX_LEN` is now a
compile-time constant rather than a variable, the scaffold's `if
prefix_len == 0 { 0 } else { ... }` guard against a degenerate shift is
dead code and was dropped — the shift amount (`32 - 24`) is always valid.

## 9. Resource budget management

- **Instructions** — `run_dijkstra` is O(V + E log V) over ≤15 nodes;
  negligible against the 4,000,000-instruction budget (confirmed no
  fuel-exhaustion errors across 20 practice-world runs).
- **Output actions** — `reconcile_routes` (`:407`) diffs the freshly
  computed `desired` prefix→port map against `prefix_routes` and emits
  only the changed prefixes, capped at `MAX_TABLE_ACTIONS_PER_CALL = 60`
  per call (`:56`); anything over that is pushed to `dirty_prefixes` and
  retried next tick. At this world's scale (≤15 switches, ≤15 prefixes)
  this cap is never approached — it's a safety margin, not a tuned value.

  **One gap in the literal design text, fixed here:** `MatchActionTable::
  install` (`src/tinyvm.rs`) has no upsert semantics — installing a second
  entry for a prefix that already has one leaves *both* in the table, and
  because LPM ties sort stably, the *older* (possibly now-wrong) entry
  keeps winning lookups forever. `DESIGN.md`'s `prefix_routes: HashMap<
  Prefix, PortId>` doesn't carry an entry id, which isn't enough to safely
  replace a route. This implementation's `prefix_routes` stores
  `(egress_port, entry_id)` instead, and every route change goes through
  `install_or_move_route` (`:307`), which explicitly deletes the old
  entry id before installing the new one. Without this, a route that
  needed to move (e.g. after a failure) would silently keep forwarding
  out the dead port forever instead of moving to the new one.

## 10. Constants

All at the top of the file (`:20`–`:56`), values taken directly from
`DESIGN.md` §10's table (not the scaffold's original, slightly different
defaults — e.g. `NEIGHBOR_TIMEOUT_NS` is 150 ms here, matching the design
doc, rather than the scaffold's original 200 ms).

## 13/14. Testing and assumptions

Verified against every practice world:

- **Part 1 (no failures), all 5 topologies:** C1/C2/C3/C4 all PASS,
  100.00% delivery.
- **Part 2 (with failures), all 15 world/failure-schedule combinations:**
  OVERALL PASS on all 15. C1 delivery 99.0–99.4% (floor 98%), C2 30/30–
  210/210 pairs reachable, C4 12/12 recovery events inside the 1000 ms
  budget (worst case observed: 235 ms). C3 shows `note` (not a failing
  criterion in A1) on about half the runs: a handful of packets (3–64 out
  of tens of thousands) bounce between two adjacent switches for a few
  hops before dying to TTL. This is the transient micro-loop the design
  doc itself calls out in §6.1 as reduced-but-not-eliminated by the
  tie-break — it happens because each switch's `reconcile_routes` only
  runs on its own independent 50 ms timer, so two adjacent switches can
  briefly disagree about which of them is closer to a destination in the
  ~tens-of-ms window right after a failure, before both have re-run SPF
  against the post-failure topology. The report card is explicit that
  this doesn't fail A1 ("Loops do not fail you in A1 ... A2 grades
  capacity").

The three items `DESIGN.md` §14 asked to have confirmed all check out
against the actual code/world files:
- Control packets do arrive via the punt path, distinguished by
  `ip_proto` (89/90) through `T_PROTO`'s exact-match table — this is how
  the pre-existing scaffold already worked, unchanged here.
- Switch ids are stable for a run (nothing in the simulator suggests
  otherwise; `switch_id` is a fixed `init` parameter).
- `recovery_budget_ms` is 1000 ms in every `worlds/practice/failures/*.toml`
  checked.

## Not implemented (per `DESIGN.md` §12, explicitly future work)

Real link costs from measured delay, `/32` host routes, precomputed
backup next-hops, and lazy path resolution — all documented in the design
as future work, not required for this world size, and not implemented.
