//! `my_switch` — link-state routing.
//!
//! Implements the protocol in `DESIGN.md`: each switch floods a
//! description of its own links and directly-attached customer prefixes
//! (an LSP) to every other switch, assembles those into an LSDB (the
//! topology map), and runs Dijkstra locally to derive next-hop ports for
//! the data plane. See `IMPLEMENTATION.md` for where each design
//! decision lives in this file.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use switch_program_sdk::*;

// ---------------------------------------------------------------------
// Table ids and protocol numbers.
// ---------------------------------------------------------------------

const T_PROTO: u32 = 1;
const T_ROUTE: u32 = 2;
const T_SRC: u32 = 3;

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
const NEIGHBOR_TIMEOUT_NS: u64 = 150_000_000; // 150 ms (~3 ticks)
const LSP_REFRESH_NS: u64 = 200_000_000; // 200 ms
const MAX_LSP_AGE_NS: u64 = 1_000_000_000; // 1 s
const DEFAULT_LINK_COST: u32 = 1;
const PREFIX_LEN: u8 = 24;

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
    adjacencies: Vec<(u32, u32)>, // (neighbor_id, cost)
    prefixes: Vec<Prefix>,
}

/// One LSP per known switch, as stored in the LSDB — the set of these
/// *is* the topology map (DESIGN.md §4).
struct LinkStateEntry {
    seq: u32,
    installed_ns: u64,
    adjacencies: Vec<(u32, u32)>,
    prefixes: Vec<Prefix>,
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
    configured: bool,
    specs: Vec<QueueSpec>,
    port_specs: HashMap<u16, Vec<QueueSpec>>,
    disc: Discipline,

    /// A2: port -> link capacity in bits per second, told once at init.
    port_capacity: HashMap<u16, u64>,
    /// A2: the customer contract (same list on every switch).
    customers: Vec<Customer>,
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
                .map(|n| (n.neighbor_id, n.link_cost))
                .collect(),
            prefixes: self.local_prefixes.clone(),
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
                ctl(
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
        out.extend(self.originate_new_lsp(ev.now_ns));
        out
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
            for &(neighbor, cost) in &entry.adjacencies {
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
        let cfg = Cfg::parse();
        me.disc = if cfg.get("disc", 0) == 1 { Discipline::StrictPriority } else { Discipline::Drr };
        let mut setup = setup;
        // q0 control, q1 misc (acks / unknown), q2.. customers ordered by class rank
        let mut specs = vec![
            QueueSpec::new(cfg.get("ctrl", 4)).weight(cfg.get("w_ctrl", 4)),
            QueueSpec::new(cfg.get("misc", 4)).weight(cfg.get("w_misc", 1)),
        ];
        let mut order: Vec<(u32, usize)> = Vec::new();
        let max_finite = me.customers.iter().filter_map(|c| c.ask_bps).max().unwrap_or(0);
        let cls = cfg.get("cls", 0);
        let small_cut: u64 = match cls {
            0 => cfg.get("thr_kbps", 1000) as u64 * 1000,                 // absolute
            1 => max_finite * cfg.get("relpct", 10) as u64 / 100,         // fraction of largest finite ask
            _ => 0,                                                       // no small class
        };
        for (i, c) in me.customers.iter().enumerate() {
            let class = match (c.ask_bps, c.cca) {
                (Some(b), _) if b < small_cut => 0, // s
                (None, Cca::Aimd) => 3,              // ua
                (None, Cca::RateMatch) => 4,         // ur
                (_, Cca::Aimd) => 1,                 // a
                (_, Cca::RateMatch) => 2,            // r
            };
            order.push((class, i));
        }
        order.sort();
        let names = ["s", "a", "r", "ua", "ur"];
        let mode = cfg.get("mode", 0);
        let mut qmap: Vec<(usize, u16)> = Vec::new();
        if mode == 3 {
            for n in names.iter() {
                specs.push(QueueSpec::new(cfg.get(&format!("cap_{n}"), cfg.get("cap", 6)))
                    .weight(cfg.get(&format!("w_{n}"), 1))
                    .drop_at(cfg.get(&format!("drop_{n}"), cfg.get("drop", 0))));
            }
        }
        for (k, (class, i)) in order.iter().enumerate() {
            let n = names[*class as usize];
            let c = &me.customers[*i];
            let q: u16 = match mode {
                1 => 2,
                3 => 2 + *class as u16,
                2 => if c.cca == Cca::Aimd { 2 } else { 3 },
                _ => 2 + k as u16,
            };
            if mode != 3 && (q as usize) >= specs.len() {
                let (cap, w, dr) = match mode {
                    0 => (cfg.get(&format!("cap_{n}"), cfg.get("cap", 6)), cfg.get(&format!("w_{n}"), 1), cfg.get(&format!("drop_{n}"), cfg.get("drop", 0))),
                    1 => (cfg.get("cap_all", 28), 1, cfg.get("drop_all", 0)),
                    _ => if q == 2 { (cfg.get("cap_aimd", 14), 1, cfg.get("drop_aimd", 0)) } else { (cfg.get("cap_rm", 14), 1, cfg.get("drop_rm", 0)) },
                };
                specs.push(QueueSpec::new(cap).weight(w).drop_at(dr));
            }
            qmap.push((*i, q));
        }
        for (i, q) in qmap {
            let c = &me.customers[i];
            setup.add_entry(T_SRC, TableEntry { id: wire::EntryIdW(2000 + i as u64), key: c.prefix_addr as u64,
                prefix_len: c.prefix_len, priority: 0,
                action: TableAction::SetQueue { queue: wire::QueueIdW(q) } });
        }
        let sz = cfg.get("sz", 0);
        let total: u64 = me.local_ports.iter().map(|p| me.port_capacity.get(p).copied().unwrap_or(10_000_000)).sum();
        let minc = cfg.get("min", 2);
        for p in me.local_ports.clone() {
            let bps = me.port_capacity.get(&p).copied().unwrap_or(10_000_000);
            let mbps = bps as f64 / 1e6;
            let mut sp = specs.clone();
            for q in 2..sp.len() {
                let base = sp[q].max_packets;
                let cap = match sz {
                    1 => { let budget = cfg.get("T", 36) as f64 * bps as f64 / total as f64; (budget / 4.0).round() as u32 }
                    2 => (cfg.get("g", 10) as f64 * mbps / 10.0).round() as u32,
                    3 => (cfg.get("K", 8) as f64 * 2.0 / mbps).round().min(cfg.get("hi", 12) as f64) as u32,
                    _ => base,
                };
                let cap = cap.max(minc);
                sp[q] = QueueSpec::new(cap).weight(sp[q].weight).drop_at(sp[q].drop_threshold_packets);
            }
            me.port_specs.insert(p, sp);
        }
        me.specs = specs;
        (me, setup)
    }

    fn init(switch_id: u32, local_ports: Vec<u16>) -> (Self, ProgramSetup) {
        let mut setup = ProgramSetup::new();

        setup.declare_table(T_PROTO, MatchKind::Exact, 8);
        setup.declare_table(T_ROUTE, MatchKind::Lpm, 256);
        setup.declare_table(T_SRC, MatchKind::Lpm, 64);

        setup.set_program(
            text::parse_tiny_program(
                "
                stage alu=4 mem=1
                    load    r0, ip_proto
                    table   t1, r0 -> m0
                stage alu=4 mem=1
                    load    r1, ip_dst
                    table   t2, r1 -> m1
                stage alu=4 mem=1
                    queue   1
                    load    r2, ip_src
                    table   t3, r2 -> m2
                ",
            )
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
            configured: false,
            specs: Vec::new(),
            port_specs: HashMap::new(),
            disc: Discipline::Drr,
            port_capacity: HashMap::new(),
            customers: Vec::new(),
        };
        (me, setup)
    }

    fn on_punt(&mut self, ev: PuntEvent) -> Vec<Action> {
        match ev.ip_proto {
            PROTO_HELLO => self.handle_hello(&ev),
            PROTO_ROUTE_ANNOUNCE => self.handle_route_announce(&ev),
            PROTO_CUSTOMER_DATA => self.handle_customer_punt(&ev),
            _ => Vec::new(),
        }
    }

    fn on_timer(&mut self, ev: TimerEvent) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.configured {
            self.configured = true;
            for p in &self.local_ports {
                let sp = self.port_specs.get(p).cloned().unwrap_or_else(|| self.specs.clone());
                out.push(actions::set_queues(*p, self.disc, sp));
            }
        }

        // HELLO heartbeat. Doubles as our liveness signal (DESIGN.md
        // §5.1): if a neighbor's hellos stop, the timeout check below is
        // how we find out.
        let payload = self.switch_id.to_le_bytes().to_vec();
        for port in &self.local_ports {
            out.push(ctl(
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

        out.push(actions::schedule_timer(HELLO_TICK_NS));
        out
    }
}

switch_program!(MySwitch);

fn ctl(port: u16, src: u32, dst: u32, proto: u8, ttl: u8, payload: Vec<u8>) -> Action {
    actions::in_queue(0, actions::inject_packet(port, src, dst, proto, ttl, payload))
}

struct Cfg(Vec<(String, u32)>);
impl Cfg {
    fn parse() -> Self {
        let raw = option_env!("CFG").unwrap_or("");
        Cfg(raw.split(';').filter_map(|kv| { let (k, v) = kv.split_once('=')?; Some((k.to_string(), v.parse().ok()?)) }).collect())
    }
    fn get(&self, k: &str, d: u32) -> u32 { self.0.iter().find(|(n, _)| n == k).map(|x| x.1).unwrap_or(d) }
}
