//! `my_switch` — link-state routing.
//!
//! Implements the protocol in `DESIGN.md`: each switch floods a
//! description of its own links and directly-attached customer prefixes
//! (an LSP) to every other switch, assembles those into an LSDB (the
//! topology map), and runs Dijkstra locally to derive next-hop ports for
//! the data plane. See `IMPLEMENTATION.md` for where each design
//! decision lives in this file.

mod plan;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use switch_program_sdk::*;

// ---------------------------------------------------------------------
// Table ids and protocol numbers.
// ---------------------------------------------------------------------

const T_PROTO: u32 = 1;
const T_ROUTE: u32 = 2;
/// A2: source prefix -> queue (one entry per customer in the contract).
const T_SRC: u32 = 3;
/// A2 part 2 (MY_DESIGN.md D3): source prefix -> customer id (metadata).
const T_SRCID: u32 = 4;
/// ingress switch: (customer id, destination /24) -> the epoch to stamp.
const T_STAMP: u32 = 5;
/// every switch: (epoch, customer id, destination /24) -> egress port.
const T_PLAN: u32 = 6;
/// attach switch: customer prefix -> customer id, to catch the first ack.
const T_DSTID: u32 = 7;
/// Register array: one flag per customer, set when its first ack was punted.
const R_LATCH: u32 = 1;
/// Metadata keys carrying values between stages.
const M_CUST: u32 = 5;
const M_EPOCH: u32 = 6;
const M_ACKID: u32 = 7;
/// `PuntReason::Custom` code for "first ack of a customer" (learns its peer).
const PUNT_ACK_LATCH: u32 = 0x4C33;
/// How long the ingress waits after its plan last changed before it starts
/// stamping packets with the new epoch, so every switch on the paths has
/// installed that epoch's entries first (two-phase update).
const SETTLE_NS: u64 = 300_000_000;
/// How long an epoch's entries are kept after nothing needs them, so
/// packets already stamped with it can drain.
const DRAIN_NS: u64 = 1_000_000_000;

/// Neighbor discovery + liveness. Rides on every local port, forever.
const PROTO_HELLO: u8 = 89;

/// Our link-state protocol's control messages: a postcard-encoded
/// `LspMessage` (DESIGN.md §5.2).
const PROTO_ROUTE_ANNOUNCE: u8 = 90;

/// Customer traffic uses proto 0 (see `worlds/practice/*.toml` and
/// `docs/switch-programs.md`).
const PROTO_CUSTOMER_DATA: u8 = 0;

/// Address nothing routes. Used so a HELLO/announce packet reliably
/// reaches T_PROTO's exact-match punt instead of falling through to the
/// route table.
const CTRL_IP: u32 = 0xe000_0005; // 224.0.0.5

// ---------------------------------------------------------------------
// Constants (DESIGN.md §10).
// ---------------------------------------------------------------------

const HELLO_TICK_NS: u64 = 50_000_000; // 50 ms
const NEIGHBOR_TIMEOUT_NS: u64 = 200_000_000; // 200 ms (~4 ticks)
const LSP_REFRESH_NS: u64 = 200_000_000; // 200 ms
const MAX_LSP_AGE_NS: u64 = 1_000_000_000; // 1 s
const DEFAULT_LINK_COST: u32 = 1;
const PREFIX_LEN: u8 = 24;

// ---------------------------------------------------------------------
// A2 queue layout (MY_DESIGN.md D1, D2e, D2f). Every port gets the same
// list: queue 0 control, queue 1 everything unclassified (acks from
// non-customer receivers), then one queue per customer in contract order.
// The contract is identical on every switch, so the index of a customer's
// queue is the same everywhere.
// ---------------------------------------------------------------------

const Q_CTRL: u16 = 0;
const Q_MISC: u16 = 1;
const FIRST_CUSTOMER_Q: usize = 2;
/// The simulator truncates a port's queue list at 16.
const MAX_QUEUES: usize = 16;
const CTRL_CAP: u32 = 4;
const CTRL_WEIGHT: u32 = 4;
const MISC_CAP: u32 = 4;
/// A customer asking for under this percentage of the largest finite ask
/// in the contract is "small". Relative, so it survives --load-scale.
const SMALL_PCT: u64 = 10;
/// Queue caps in full-size packets by customer profile (MY_DESIGN.md D2e):
/// RateMatch senders are hurt by deep queues, AIMD senders need about a
/// bandwidth-delay product to keep the pipe full after a halving.
const CAP_SMALL: u32 = 3;
const CAP_RATE_MATCH: u32 = 3;
const CAP_AIMD: u32 = 8;
/// Upper bound on the sum of customer-queue caps on one port, so a couple
/// of congested ports cannot exhaust the 40-packet pool and starve control
/// packets (MY_DESIGN.md D1 headroom rule).
const PORT_CAP_BUDGET: u32 = 19;

/// Safety cap on table-mutating actions emitted from a single call, so a
/// pathological topology change can never push us over the 4096-byte
/// output-action budget (`docs/LIMITS.md`, ~150 installs). At this
/// world's scale (≤15 switches) a full reconciliation never gets close
/// to this; overflow, if it ever happened, is queued in `dirty_prefixes`
/// and retried on the next tick (DESIGN.md §9).
const MAX_TABLE_ACTIONS_PER_CALL: usize = 60;

// ---------------------------------------------------------------------
// Data structures (DESIGN.md §4).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Prefix {
    network: u32,
    len: u8,
}

struct NeighborEntry {
    neighbor_id: u32,
    port: u16,
    link_cost: u32,
    last_hello_ns: u64,
    up: bool,
}

/// The LSP as it travels the wire: one origin's adjacencies and
/// originated prefixes (DESIGN.md §5.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LspMessage {
    origin: u32,
    seq: u32,
    adjacencies: Vec<(u32, u32, u32)>, // (neighbor_id, cost, capacity kbit/s)
    prefixes: Vec<Prefix>,
    /// customer prefix -> peer prefix, learned at this switch from acks.
    pairs: Vec<(Prefix, Prefix)>,
}

/// One LSP per known switch, as stored in the LSDB — the set of these
/// *is* the topology map (DESIGN.md §4).
struct LinkStateEntry {
    seq: u32,
    installed_ns: u64,
    adjacencies: Vec<(u32, u32, u32)>,
    prefixes: Vec<Prefix>,
    pairs: Vec<(Prefix, Prefix)>,
}

/// Result of Dijkstra for one destination switch. `dest_switch` itself is
/// omitted here since it is already the key in `routes` below.
struct RouteEntry {
    next_hop_port: u16,
    #[allow(dead_code)]
    cost: u32,
}

pub struct MySwitch {
    switch_id: u32,
    local_ports: Vec<u16>,

    self_seq: u32,
    neighbors: HashMap<u16, NeighborEntry>, // port -> neighbor
    local_prefixes: Vec<Prefix>,            // our own directly-attached customers

    lsdb: HashMap<u32, LinkStateEntry>, // origin switch id -> LSP
    routes: HashMap<u32, RouteEntry>,   // dest switch -> control-plane route

    /// prefix -> (egress port, installed T_ROUTE entry id). Mirrors the
    /// data plane so a fresh SPF result can be diffed against it and only
    /// the actions needed to close the gap get emitted (DESIGN.md §4).
    prefix_routes: HashMap<Prefix, (u16, u64)>,

    /// Prefixes whose reconciliation didn't fit in a call's action
    /// budget; retried on the next timer tick (DESIGN.md §9).
    dirty_prefixes: Vec<Prefix>,

    next_entry_id: u64,
    last_lsp_refresh_ns: u64,

    /// A2: port -> link capacity in bits per second, told once at init.
    port_capacity: HashMap<u16, u64>,
    /// A2: the customer contract (same list on every switch).
    customers: Vec<Customer>,
    /// A2: the queue list every port is given, and whether it was sent.
    queue_specs: Vec<QueueSpec>,
    queues_configured: bool,

    // --- A2 part 2: destination learning, planning, epoch delivery ---
    /// customer prefix -> peer prefix, learned here from the first ack.
    pairs_local: BTreeMap<u32, Prefix>,
    /// Signature of the inputs the current plan was computed from.
    plan_sig: u64,
    /// Per customer (contract index): destination /24 and switch path.
    plan: Vec<Option<(u32, Vec<u32>)>>,
    /// The epoch of the latest plan, when it became the target, and the
    /// epoch this switch's ingress is stamping packets with (0 = none).
    target_epoch: u16,
    target_set_ns: u64,
    active_epoch: u16,
    /// T_PLAN entries installed here: (epoch, entry id).
    plan_entries: Vec<(u16, u64)>,
    /// customer index -> T_STAMP entry id currently installed here.
    stamp_entries: HashMap<usize, u64>,
    /// epoch -> when nothing needed it any more.
    retired: HashMap<u16, u64>,
}

impl MySwitch {
    /// The contract row a source address belongs to, or `None` for
    /// non-customer sources (acks from receivers, control packets).
    #[allow(dead_code)]
    fn customer(&self, ip_src: u32) -> Option<&Customer> {
        self.customers.iter().find(|c| {
            let mask = if c.prefix_len == 0 { 0 } else { u32::MAX << (32 - c.prefix_len) };
            ip_src & mask == c.prefix_addr & mask
        })
    }

    fn fresh_entry_id(&mut self) -> u64 {
        self.next_entry_id += 1;
        self.next_entry_id
    }

    fn is_neighbor_port(&self, port: u16) -> bool {
        self.neighbors.contains_key(&port)
    }

    /// The LSP describing this switch right now.
    fn self_lsp(&self) -> LspMessage {
        LspMessage {
            origin: self.switch_id,
            seq: self.self_seq,
            adjacencies: self
                .neighbors
                .values()
                .filter(|n| n.up)
                .map(|n| {
                    let kbps = self.port_capacity.get(&n.port).copied().unwrap_or(0) / 1000;
                    (n.neighbor_id, n.link_cost, kbps.min(u32::MAX as u64) as u32)
                })
                .collect(),
            prefixes: self.local_prefixes.clone(),
            pairs: self
                .pairs_local
                .iter()
                .map(|(c, p)| (Prefix { network: *c, len: PREFIX_LEN }, *p))
                .collect(),
        }
    }

    /// Originate a fresh LSP: bump `self_seq`, refresh our own LSDB entry,
    /// and flood it to every neighbor. Called whenever our up-neighbor
    /// set or our originated prefixes change, and periodically as a
    /// refresh (DESIGN.md §5.3 / §5.4).
    fn originate_new_lsp(&mut self, now_ns: u64) -> Vec<Action> {
        self.self_seq = self.self_seq.wrapping_add(1);
        let lsp = self.self_lsp();
        self.lsdb.insert(
            self.switch_id,
            LinkStateEntry {
                seq: lsp.seq,
                installed_ns: now_ns,
                adjacencies: lsp.adjacencies.clone(),
                prefixes: lsp.prefixes.clone(),
                pairs: lsp.pairs.clone(),
            },
        );
        self.last_lsp_refresh_ns = now_ns;
        self.flood_lsp(&lsp, None)
    }

    /// Encode `lsp` and inject it out every neighbor port except
    /// `except_port`. `None` means "every neighbor" (used when we
    /// originate); `Some(port)` implements the split-horizon re-flood
    /// rule of DESIGN.md §5.3 (never send an LSP back out the port it
    /// arrived on).
    fn flood_lsp(&self, lsp: &LspMessage, except_port: Option<u16>) -> Vec<Action> {
        let payload = match postcard::to_allocvec(lsp) {
            Ok(bytes) => bytes,
            Err(_) => return Vec::new(),
        };
        self.neighbors
            .keys()
            .filter(|port| Some(**port) != except_port)
            .map(|port| {
                ctl_inject(
                    *port,
                    0xa9fe_0000 | (self.switch_id & 0xffff),
                    CTRL_IP,
                    PROTO_ROUTE_ANNOUNCE,
                    1,
                    payload.clone(),
                )
            })
            .collect()
    }

    /// A HELLO arrived. Learn (or refresh) the neighbor on this port
    /// (DESIGN.md §5.1). A brand-new neighbor, or one transitioning from
    /// down back to up, is new topology information — reoriginate right
    /// away instead of waiting for the next periodic refresh.
    fn handle_hello(&mut self, ev: &PuntEvent) -> Vec<Action> {
        let their_id = ev
            .payload
            .get(0..4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(u32::MAX);

        let was_up = self
            .neighbors
            .get(&ev.ingress_port)
            .map(|n| n.up)
            .unwrap_or(false);

        self.neighbors.insert(
            ev.ingress_port,
            NeighborEntry {
                neighbor_id: their_id,
                port: ev.ingress_port,
                link_cost: DEFAULT_LINK_COST,
                last_hello_ns: ev.now_ns,
                up: true,
            },
        );

        if !was_up {
            return self.originate_new_lsp(ev.now_ns);
        }
        Vec::new()
    }

    /// An LSP arrived from a neighbor. Accept it only if it is strictly
    /// newer than what we have for that origin (DESIGN.md §5.3's
    /// loop/duplicate stopper), then re-flood it to every other neighbor.
    fn handle_route_announce(&mut self, ev: &PuntEvent) -> Vec<Action> {
        let lsp: LspMessage = match postcard::from_bytes(&ev.payload) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        // Never re-learn ourselves off the wire.
        if lsp.origin == self.switch_id {
            return Vec::new();
        }

        let accept = match self.lsdb.get(&lsp.origin) {
            Some(existing) => lsp.seq > existing.seq,
            None => true,
        };
        if !accept {
            return Vec::new();
        }

        self.lsdb.insert(
            lsp.origin,
            LinkStateEntry {
                seq: lsp.seq,
                installed_ns: ev.now_ns,
                adjacencies: lsp.adjacencies.clone(),
                prefixes: lsp.prefixes.clone(),
                pairs: lsp.pairs.clone(),
            },
        );

        self.flood_lsp(&lsp, Some(ev.ingress_port))
    }

    /// A customer data packet with no matching route arrived. If it came
    /// in on a port we don't recognize as a neighbor, it's a
    /// directly-attached customer we haven't met yet: install the route
    /// immediately (so this and later packets on this call's path start
    /// forwarding without another round trip) and announce it to the
    /// network so reachability propagates past this one hop.
    fn handle_customer_punt(&mut self, ev: &PuntEvent) -> Vec<Action> {
        if self.is_neighbor_port(ev.ingress_port) {
            // A neighbor sent us a customer packet we have no route for.
            // That means *we* don't know about the destination yet (our
            // propagation hasn't converged), not that the sender erred.
            return Vec::new();
        }

        let mask: u32 = !0u32 << (32 - PREFIX_LEN as u32);
        let prefix = Prefix {
            network: ev.ip_src & mask,
            len: PREFIX_LEN,
        };

        if self.local_prefixes.contains(&prefix) {
            return Vec::new();
        }
        self.local_prefixes.push(prefix);

        let mut out = self.install_or_move_route(prefix, ev.ingress_port);
        // A contract customer attached here: watch for its first ack (D3a).
        if let Some(idx) = self.customer_index(prefix.network) {
            out.push(actions::install_entry(
                T_DSTID,
                TableEntry {
                    id: wire::EntryIdW(3000 + idx as u64),
                    key: prefix.network as u64,
                    prefix_len: PREFIX_LEN,
                    priority: 0,
                    action: TableAction::SetMeta {
                        key: wire::MetaKeyW(M_ACKID),
                        value: (idx as u64) + 1,
                    },
                },
            ));
        }
        out.extend(self.originate_new_lsp(ev.now_ns));
        out
    }

    /// Index in the contract of the customer whose /24 is `network`.
    fn customer_index(&self, network: u32) -> Option<usize> {
        let mask: u32 = !0u32 << (32 - PREFIX_LEN as u32);
        self.customers.iter().position(|c| c.prefix_addr & mask == network)
    }

    /// The first ack to reach a customer's attach switch names its peer:
    /// the ack's source is the receiver. Remember it and flood it (L3).
    fn handle_ack_latch(&mut self, ev: &PuntEvent) -> Vec<Action> {
        let mask: u32 = !0u32 << (32 - PREFIX_LEN as u32);
        let customer = ev.ip_dst & mask;
        if self.customer_index(customer).is_none() || self.pairs_local.contains_key(&customer) {
            return Vec::new();
        }
        self.pairs_local.insert(
            customer,
            Prefix {
                network: ev.ip_src & mask,
                len: PREFIX_LEN,
            },
        );
        self.originate_new_lsp(ev.now_ns)
    }

    /// Install (or move) the T_ROUTE entry for `prefix` to `egress_port`.
    ///
    /// `MatchActionTable::install` (`src/tinyvm.rs`) has no upsert
    /// semantics: installing a second entry for a prefix that already has
    /// one leaves *both* in the table, and because ties sort stably, the
    /// older (possibly stale) entry keeps winning lookups. So every
    /// reconciliation here is a delete-then-install pair keyed by a
    /// stable per-prefix entry id, never a bare install over an existing
    /// route.
    fn install_or_move_route(&mut self, prefix: Prefix, egress_port: u16) -> Vec<Action> {
        let mut out = Vec::new();
        if let Some(&(old_port, old_id)) = self.prefix_routes.get(&prefix) {
            if old_port == egress_port {
                return out; // already correct; nothing to do
            }
            out.push(actions::delete_entry(T_ROUTE, old_id));
        }
        let entry_id = self.fresh_entry_id();
        out.push(actions::install_route(
            T_ROUTE,
            entry_id,
            prefix.network as u64,
            prefix.len,
            egress_port,
        ));
        self.prefix_routes.insert(prefix, (egress_port, entry_id));
        out
    }

    fn withdraw_route(&mut self, prefix: &Prefix) -> Vec<Action> {
        match self.prefix_routes.remove(prefix) {
            Some((_, id)) => vec![actions::delete_entry(T_ROUTE, id)],
            None => Vec::new(),
        }
    }

    /// Dijkstra from `self_id` over the LSDB (DESIGN.md §6.1). Returns,
    /// for every reachable destination switch, its cost and the
    /// first-hop port out of this switch.
    ///
    /// Ties in tentative distance are settled lowest-switch-id-first (the
    /// `Reverse((cost, node))` ordering below) so that, given the same
    /// LSDB, every switch's local computation makes the same choice on an
    /// equal-cost tie — reducing (not eliminating; LSDBs can be
    /// momentarily inconsistent mid-flood) transient micro-loops on ties,
    /// per DESIGN.md §6.1.
    fn run_dijkstra(&self) -> HashMap<u32, RouteEntry> {
        let direct_port: HashMap<u32, u16> = self
            .neighbors
            .values()
            .filter(|n| n.up)
            .map(|n| (n.neighbor_id, n.port))
            .collect();

        let mut dist: HashMap<u32, u32> = HashMap::new();
        let mut first_hop: HashMap<u32, u16> = HashMap::new();
        let mut settled: HashSet<u32> = HashSet::new();

        dist.insert(self.switch_id, 0);
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        heap.push(Reverse((0, self.switch_id)));

        while let Some(Reverse((d, node))) = heap.pop() {
            if !settled.insert(node) {
                continue;
            }
            let Some(entry) = self.lsdb.get(&node) else {
                continue;
            };
            for &(neighbor, cost, _cap) in &entry.adjacencies {
                if settled.contains(&neighbor) {
                    continue;
                }
                let candidate = d.saturating_add(cost);
                let is_better = dist.get(&neighbor).map_or(true, |&cur| candidate < cur);
                if !is_better {
                    continue;
                }
                let hop = if node == self.switch_id {
                    direct_port.get(&neighbor).copied()
                } else {
                    first_hop.get(&node).copied()
                };
                let Some(hop) = hop else {
                    // Shouldn't happen: `node` was reached, so it has a
                    // first hop, unless `node` is unreachable garbage in
                    // someone's LSP. Skip rather than panic.
                    continue;
                };
                dist.insert(neighbor, candidate);
                first_hop.insert(neighbor, hop);
                heap.push(Reverse((candidate, neighbor)));
            }
        }

        dist.into_iter()
            .filter(|&(node, _)| node != self.switch_id)
            .filter_map(|(node, cost)| {
                first_hop
                    .get(&node)
                    .map(|&port| (node, RouteEntry { next_hop_port: port, cost }))
            })
            .collect()
    }

    /// Age out LSDB entries nobody has refreshed recently (DESIGN.md
    /// §5.4), recompute routes (§6.1–§6.2), and diff the result against
    /// the installed data plane, emitting only the actions needed to
    /// close the gap (§4, §9).
    fn reconcile_routes(&mut self, now_ns: u64) -> Vec<Action> {
        self.lsdb
            .retain(|_, e| now_ns.saturating_sub(e.installed_ns) <= MAX_LSP_AGE_NS);

        self.routes = self.run_dijkstra();

        // Desired prefix -> egress-port mapping: our own customers keep
        // whatever port `handle_customer_punt` already assigned them;
        // everyone else's prefixes go out the first-hop port toward
        // whichever switch originates them.
        let mut desired: HashMap<Prefix, u16> = HashMap::new();
        for prefix in &self.local_prefixes {
            if let Some(&(port, _)) = self.prefix_routes.get(prefix) {
                desired.insert(*prefix, port);
            }
        }
        for (&origin, entry) in &self.lsdb {
            if origin == self.switch_id {
                continue;
            }
            if let Some(route) = self.routes.get(&origin) {
                for prefix in &entry.prefixes {
                    desired.insert(*prefix, route.next_hop_port);
                }
            }
            // No route to `origin`: leave its prefixes out of `desired`,
            // which withdraws them below if we'd previously installed one.
        }

        let mut changed: Vec<Prefix> = Vec::new();
        for (&prefix, &port) in &desired {
            match self.prefix_routes.get(&prefix) {
                Some(&(cur_port, _)) if cur_port == port => {}
                _ => changed.push(prefix),
            }
        }
        for &prefix in self.prefix_routes.keys() {
            if !desired.contains_key(&prefix) && !self.local_prefixes.contains(&prefix) {
                changed.push(prefix);
            }
        }
        changed.extend(self.dirty_prefixes.drain(..));
        changed.sort_by_key(|p| (p.network, p.len));
        changed.dedup();

        let mut out = Vec::new();
        for prefix in changed {
            if out.len() >= MAX_TABLE_ACTIONS_PER_CALL {
                self.dirty_prefixes.push(prefix);
                continue;
            }
            match desired.get(&prefix) {
                Some(&port) => out.extend(self.install_or_move_route(prefix, port)),
                None => out.extend(self.withdraw_route(&prefix)),
            }
        }
        out
    }
}

// ---------------------------------------------------------------------
// A2 part 2: plan computation and epoch-keyed delivery (MY_DESIGN.md D3).
// ---------------------------------------------------------------------

fn fnv(h: &mut u64, x: u64) {
    for b in x.to_le_bytes() {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

impl MySwitch {
    /// The egress port of the up neighbour `next`, if known.
    fn port_to(&self, next: u32) -> Option<u16> {
        self.neighbors.values().find(|n| n.up && n.neighbor_id == next).map(|n| n.port)
    }

    /// Everything the placement depends on, read from the LSDB: topology
    /// with directed capacities, which switch each /24 hangs off, and the
    /// peer each customer's first ack revealed.
    fn plan_inputs(&self) -> (plan::Adj, plan::Caps, BTreeMap<u32, u32>, BTreeMap<u32, u32>) {
        let mut adj = plan::Adj::new();
        let mut caps = plan::Caps::new();
        let mut attach: BTreeMap<u32, u32> = BTreeMap::new();
        let mut pairs: BTreeMap<u32, u32> = BTreeMap::new();
        for (&origin, e) in &self.lsdb {
            for &(n, _cost, kbps) in &e.adjacencies {
                adj.entry(origin).or_default().push(n);
                adj.entry(n).or_default().push(origin);
                caps.insert((origin, n), kbps as u64);
            }
            for p in &e.prefixes {
                attach.insert(p.network, origin);
            }
            for (c, p) in &e.pairs {
                pairs.insert(c.network, p.network);
            }
        }
        for v in adj.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        let reverse: Vec<((u32, u32), u64)> = caps.iter().map(|(&(a, b), &c)| ((b, a), c)).collect();
        for (k, c) in reverse {
            caps.entry(k).or_insert(c);
        }
        (adj, caps, attach, pairs)
    }

    /// Recompute the plan when its inputs changed, install its entries,
    /// start stamping it once the network has had time to install it, and
    /// retire entries nothing needs any more.
    fn plan_tick(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        if self.customers.is_empty() {
            return out;
        }
        let mask: u32 = !0u32 << (32 - PREFIX_LEN as u32);
        let (adj, caps, attach, pairs) = self.plan_inputs();

        let mut flows: Vec<plan::Flow> = Vec::new();
        let mut meta: Vec<(usize, u32)> = Vec::new(); // (customer index, peer /24)
        for (idx, c) in self.customers.iter().enumerate() {
            let net = c.prefix_addr & mask;
            let (Some(&src), Some(&peer)) = (attach.get(&net), pairs.get(&net)) else {
                continue;
            };
            let Some(&dst) = attach.get(&peer) else { continue };
            flows.push(plan::Flow {
                from: src,
                to: dst,
                demand_kbps: c.ask_bps.map(|b| b / 1000),
            });
            meta.push((idx, peer));
        }

        let mut sig: u64 = 0xcbf2_9ce4_8422_2325;
        for (a, v) in &adj {
            fnv(&mut sig, *a as u64);
            for n in v {
                fnv(&mut sig, ((*n as u64) << 32) | caps.get(&(*a, *n)).copied().unwrap_or(0));
            }
        }
        for (f, (idx, peer)) in flows.iter().zip(&meta) {
            fnv(&mut sig, *idx as u64);
            fnv(&mut sig, ((*peer as u64) << 32) | ((f.from as u64) << 16) | f.to as u64);
            fnv(&mut sig, f.demand_kbps.unwrap_or(u64::MAX));
        }

        if sig != self.plan_sig {
            self.plan_sig = sig;
            let paths = plan::assign_paths(&flows, &adj, &caps);
            let mut fresh: Vec<Option<(u32, Vec<u32>)>> = vec![None; self.customers.len()];
            for (k, (idx, peer)) in meta.iter().enumerate() {
                if let Some(p) = &paths[k] {
                    if p.len() > 1 {
                        fresh[*idx] = Some((*peer, p.clone()));
                    }
                }
            }
            let parts: Vec<(usize, u32, &[u32])> = fresh
                .iter()
                .enumerate()
                .filter_map(|(i, x)| x.as_ref().map(|(d, p)| (i, *d, p.as_slice())))
                .collect();
            let epoch = if parts.is_empty() { 0 } else { plan::plan_epoch(&parts) };
            self.plan = fresh;
            if epoch != self.target_epoch {
                self.target_epoch = epoch;
                self.target_set_ns = now;
                out.extend(self.install_plan_entries());
            }
        }

        // Phase 2: stamp the new epoch at the ingress, once every switch had
        // time to install its entries.
        if self.target_epoch != 0
            && self.active_epoch != self.target_epoch
            && now.saturating_sub(self.target_set_ns) >= SETTLE_NS
        {
            let epoch = self.target_epoch;
            let plan = self.plan.clone();
            for (idx, ent) in plan.iter().enumerate() {
                let Some((dst, path)) = ent else { continue };
                if path[0] != self.switch_id {
                    continue;
                }
                if let Some(old) = self.stamp_entries.remove(&idx) {
                    out.push(actions::delete_entry(T_STAMP, old));
                }
                let id = self.fresh_entry_id();
                out.push(actions::install_entry(
                    T_STAMP,
                    TableEntry {
                        id: wire::EntryIdW(id),
                        key: (((idx as u64) + 1) << 32) | *dst as u64,
                        prefix_len: 32,
                        priority: 0,
                        action: TableAction::SetMeta {
                            key: wire::MetaKeyW(M_EPOCH),
                            value: (epoch as u64) << 48,
                        },
                    },
                ));
                self.stamp_entries.insert(idx, id);
            }
            self.active_epoch = epoch;
        }

        // Phase 3: drop entries of epochs nothing needs, after a drain.
        let (target, active) = (self.target_epoch, self.active_epoch);
        let mut epochs: Vec<u16> = self.plan_entries.iter().map(|(e, _)| *e).collect();
        epochs.sort_unstable();
        epochs.dedup();
        for e in epochs {
            if e == target || e == active {
                self.retired.remove(&e);
                continue;
            }
            let since = *self.retired.entry(e).or_insert(now);
            if now.saturating_sub(since) >= DRAIN_NS {
                for (ep, id) in self.plan_entries.iter().filter(|(ep, _)| *ep == e) {
                    let _ = ep;
                    out.push(actions::delete_entry(T_PLAN, *id));
                }
                self.plan_entries.retain(|(ep, _)| *ep != e);
                self.retired.remove(&e);
            }
        }
        out
    }

    /// Install this switch's hop of every customer's path for the target
    /// epoch (phase 1). Entries for an epoch that is already installed are
    /// not duplicated.
    fn install_plan_entries(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        let epoch = self.target_epoch;
        if epoch == 0 || self.plan_entries.iter().any(|(e, _)| *e == epoch) {
            return out;
        }
        self.retired.remove(&epoch);
        let plan = self.plan.clone();
        for (idx, ent) in plan.iter().enumerate() {
            let Some((dst, path)) = ent else { continue };
            for w in path.windows(2) {
                if w[0] != self.switch_id {
                    continue;
                }
                let Some(port) = self.port_to(w[1]) else { continue };
                let id = self.fresh_entry_id();
                out.push(actions::install_entry(
                    T_PLAN,
                    TableEntry {
                        id: wire::EntryIdW(id),
                        key: ((epoch as u64) << 48) | (((idx as u64) + 1) << 32) | *dst as u64,
                        prefix_len: 32,
                        priority: 0,
                        action: TableAction::SetEgress {
                            port: wire::PortIdW(port),
                        },
                    },
                ));
                self.plan_entries.push((epoch, id));
            }
        }
        out
    }
}

impl SwitchProgram for MySwitch {
    /// A2: `init` plus link speeds and the customer contract.
    fn init_with(input: InitInput) -> (Self, ProgramSetup) {
        let port_capacity = input
            .local_ports
            .iter()
            .copied()
            .zip(input.port_capacity_bps.iter().copied())
            .collect();
        let (mut me, setup) = Self::init(input.switch_id, input.local_ports);
        me.port_capacity = port_capacity;
        me.customers = input.customers;

        let mut setup = setup;
        let caps = customer_caps(&me.customers);
        me.queue_specs = vec![
            QueueSpec::new(CTRL_CAP).weight(CTRL_WEIGHT),
            QueueSpec::new(MISC_CAP),
        ];
        for cap in &caps {
            if me.queue_specs.len() < MAX_QUEUES {
                me.queue_specs.push(QueueSpec::new(*cap));
            }
        }
        for (i, c) in me.customers.iter().enumerate().take(60) {
            let q = (FIRST_CUSTOMER_Q + i).min(MAX_QUEUES - 1) as u16;
            setup.add_entry(
                T_SRC,
                TableEntry {
                    id: wire::EntryIdW(2000 + i as u64),
                    key: c.prefix_addr as u64,
                    prefix_len: c.prefix_len,
                    priority: 0,
                    action: TableAction::SetQueue {
                        queue: wire::QueueIdW(q),
                    },
                },
            );
        }
        for (i, c) in me.customers.iter().enumerate().take(60) {
            setup.add_entry(
                T_SRCID,
                TableEntry {
                    id: wire::EntryIdW(2100 + i as u64),
                    key: c.prefix_addr as u64,
                    prefix_len: c.prefix_len,
                    priority: 0,
                    action: TableAction::SetMeta {
                        key: wire::MetaKeyW(M_CUST),
                        value: ((i as u64) + 1) << 32,
                    },
                },
            );
        }
        (me, setup)
    }

    fn init(switch_id: u32, local_ports: Vec<u16>) -> (Self, ProgramSetup) {
        let mut setup = ProgramSetup::new();

        setup.declare_table(T_PROTO, MatchKind::Exact, 8);
        setup.declare_table(T_ROUTE, MatchKind::Lpm, 256);
        setup.declare_table(T_SRC, MatchKind::Lpm, 64);
        setup.declare_table(T_SRCID, MatchKind::Lpm, 64);
        setup.declare_table(T_STAMP, MatchKind::Exact, 64);
        setup.declare_table(T_PLAN, MatchKind::Exact, 256);
        setup.declare_table(T_DSTID, MatchKind::Lpm, 64);
        setup.declare_register_array(R_LATCH, 64);

        setup.set_program(
            text::parse_tiny_program(&format!(
                "
                stage alu=4 mem=1
                    load    r0, ip_proto
                    table   t{T_PROTO}, r0 -> m0
                stage alu=4 mem=1
                    load    r1, ip_dst
                    table   t{T_ROUTE}, r1 -> m1
                stage alu=4 mem=1
                    queue   {Q_MISC}
                    load    r2, ip_src
                    table   t{T_SRC}, r2 -> m2
                stage alu=4 mem=1
                    load    r3, ip_src
                    table   t{T_SRCID}, r3 -> m3
                stage alu=16 mem=1
                    load    r0, custom0
                    const   r1, 0
                    eq      r2, r0, r1
                    const   r3, 1
                    xor     r4, r2, r3
                    bif     r4 -> 14
                    loadm   r5, m{M_CUST}
                    load    r6, ip_dst
                    const   r7, 0xFFFFFF00
                    and     r8, r6, r7
                    or      r9, r5, r8
                    table   t{T_STAMP}, r9 -> m4
                    loadm   r10, m{M_EPOCH}
                    store   custom0, r10
                    noop
                stage alu=16 mem=1
                    load    r0, custom0
                    loadm   r1, m{M_CUST}
                    load    r2, ip_dst
                    const   r3, 0xFFFFFF00
                    and     r4, r2, r3
                    or      r5, r0, r1
                    or      r6, r5, r4
                    table   t{T_PLAN}, r6 -> m4
                stage alu=4 mem=1
                    load    r0, ip_dst
                    table   t{T_DSTID}, r0 -> m3
                stage alu=16 mem=2
                    loadm   r0, m{M_ACKID}
                    const   r1, 0
                    eq      r2, r0, r1
                    bif     r2 -> 18
                    load    r3, kind
                    const   r4, 4
                    eq      r5, r3, r4
                    const   r6, 1
                    xor     r7, r5, r6
                    bif     r7 -> 18
                    rread   a{R_LATCH}[r0] -> r8
                    const   r9, 0
                    eq      r10, r8, r9
                    const   r11, 1
                    xor     r12, r10, r11
                    bif     r12 -> 18
                    rwrite  a{R_LATCH}[r0], r11
                    punt    {PUNT_ACK_LATCH}
                    noop
                "
            ))
            .unwrap(),
        );

        setup.add_entry(
            T_PROTO,
            TableEntry {
                id: wire::EntryIdW(1),
                key: PROTO_HELLO as u64,
                prefix_len: 8,
                priority: 0,
                action: TableAction::Punt {
                    reason: PuntReason::Custom(PROTO_HELLO as u32),
                },
            },
        );
        setup.add_entry(
            T_PROTO,
            TableEntry {
                id: wire::EntryIdW(2),
                key: PROTO_ROUTE_ANNOUNCE as u64,
                prefix_len: 8,
                priority: 0,
                action: TableAction::Punt {
                    reason: PuntReason::Custom(PROTO_ROUTE_ANNOUNCE as u32),
                },
            },
        );

        let me = Self {
            switch_id,
            local_ports,
            self_seq: 0,
            neighbors: HashMap::new(),
            local_prefixes: Vec::new(),
            lsdb: HashMap::new(),
            routes: HashMap::new(),
            prefix_routes: HashMap::new(),
            dirty_prefixes: Vec::new(),
            next_entry_id: 100,
            last_lsp_refresh_ns: 0,
            port_capacity: HashMap::new(),
            customers: Vec::new(),
            queue_specs: Vec::new(),
            queues_configured: false,
            pairs_local: BTreeMap::new(),
            plan_sig: 0,
            plan: Vec::new(),
            target_epoch: 0,
            target_set_ns: 0,
            active_epoch: 0,
            plan_entries: Vec::new(),
            stamp_entries: HashMap::new(),
            retired: HashMap::new(),
        };
        (me, setup)
    }

    fn on_punt(&mut self, ev: PuntEvent) -> Vec<Action> {
        if ev.reason == PuntReason::Custom(PUNT_ACK_LATCH) {
            return self.handle_ack_latch(&ev);
        }
        match ev.ip_proto {
            PROTO_HELLO => self.handle_hello(&ev),
            PROTO_ROUTE_ANNOUNCE => self.handle_route_announce(&ev),
            PROTO_CUSTOMER_DATA => self.handle_customer_punt(&ev),
            _ => Vec::new(),
        }
    }

    fn on_timer(&mut self, ev: TimerEvent) -> Vec<Action> {
        let mut out = Vec::new();

        if !self.queues_configured {
            self.queues_configured = true;
            for port in &self.local_ports {
                out.push(actions::set_queues(
                    *port,
                    Discipline::Drr,
                    self.queue_specs.clone(),
                ));
            }
        }

        // HELLO heartbeat. Doubles as our liveness signal (DESIGN.md
        // §5.1): if a neighbor's hellos stop, the timeout check below is
        // how we find out.
        let payload = self.switch_id.to_le_bytes().to_vec();
        for port in &self.local_ports {
            out.push(ctl_inject(
                *port,
                0xa9fe_0000 | (self.switch_id & 0xffff),
                CTRL_IP,
                PROTO_HELLO,
                1,
                payload.clone(),
            ));
        }

        // Notice neighbors that have gone quiet and recover (DESIGN.md
        // §7.1 detection, §7.2 recovery): delete every T_ROUTE entry whose
        // egress port is the dead neighbor's port, and let the next
        // customer packet toward those destinations re-punt onto
        // whatever path SPF finds once the topology change has flooded.
        let dead_ports: Vec<u16> = self
            .neighbors
            .iter()
            .filter(|(_, n)| n.up && ev.now_ns.saturating_sub(n.last_hello_ns) > NEIGHBOR_TIMEOUT_NS)
            .map(|(port, _)| *port)
            .collect();

        let mut topology_changed = false;
        for port in &dead_ports {
            if let Some(n) = self.neighbors.get_mut(port) {
                n.up = false;
            }
            topology_changed = true;

            let stale: Vec<Prefix> = self
                .prefix_routes
                .iter()
                .filter(|&(_, &(p, _))| p == *port)
                .map(|(prefix, _)| *prefix)
                .collect();
            for prefix in stale {
                out.extend(self.withdraw_route(&prefix));
            }
        }

        // Triggered update on topology change, otherwise periodic refresh
        // (DESIGN.md §5.3 / §5.4).
        if topology_changed {
            out.extend(self.originate_new_lsp(ev.now_ns));
        } else if ev.now_ns.saturating_sub(self.last_lsp_refresh_ns) >= LSP_REFRESH_NS {
            out.extend(self.originate_new_lsp(ev.now_ns));
        }

        // Precompute routes every tick and reconcile the data plane
        // toward the result (DESIGN.md §6.2).
        out.extend(self.reconcile_routes(ev.now_ns));
        out.extend(self.plan_tick(ev.now_ns));

        out.push(actions::schedule_timer(HELLO_TICK_NS));
        out
    }
}

/// Our own control packets ride queue 0 so data cannot crowd them out.
fn ctl_inject(port: u16, src: u32, dst: u32, proto: u8, ttl: u8, payload: Vec<u8>) -> Action {
    actions::in_queue(Q_CTRL, actions::inject_packet(port, src, dst, proto, ttl, payload))
}

/// Cap, in full-size packets, of each customer's queue, in contract order.
fn customer_caps(customers: &[Customer]) -> Vec<u32> {
    let max_finite = customers.iter().filter_map(|c| c.ask_bps).max().unwrap_or(0);
    let small_below = max_finite * SMALL_PCT / 100;
    let mut caps: Vec<u32> = customers
        .iter()
        .take(MAX_QUEUES - FIRST_CUSTOMER_Q)
        .map(|c| match (c.ask_bps, c.cca) {
            (Some(b), _) if b < small_below => CAP_SMALL,
            (_, Cca::RateMatch) => CAP_RATE_MATCH,
            (_, Cca::Aimd) => CAP_AIMD,
        })
        .collect();
    let sum: u32 = caps.iter().sum();
    if sum > PORT_CAP_BUDGET {
        for c in caps.iter_mut() {
            *c = (*c * PORT_CAP_BUDGET / sum).max(2);
        }
    }
    caps
}

switch_program!(MySwitch);
