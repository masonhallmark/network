//! Broad (truth-run) logging switch. See `LOGGING_DESIGN.md` for the record format.
//!
//! **Data plane** (every switch, every packet): counts, never punts, except for a few cheap samples.
//! - c0/c1: packets/bytes that entered from a customer port, by (src customer, dst customer)
//! - c2:    packets that left toward a customer port, by (src customer, dst customer)
//! - c3:    keepalives that arrived, by ingress port
//! - c4:    acks and UDP packets that entered from a customer port, by (src, dst)
//!
//! Punts: 1 = every 16th keepalive on a port (peer id, link delay), 2 = every 64th data packet from a
//! customer (raw sample), 3 = a small data packet from a customer (scan hunting), 4 = a report.
//!
//! **Controller**: turns counters and punts into records. A switch that is not the logging switch
//! batches its records into one report packet per timer and floods it; the logging switch writes them.

use std::collections::{HashMap, HashSet, VecDeque};
use switch_program_sdk::*;

pub mod fmt;
use fmt::*;

#[cfg(feature = "practice")]
pub const LOGGER: u32 = 1;
#[cfg(not(feature = "practice"))]
pub const LOGGER: u32 = match option_env!("A3_LOGGER") {
    Some(s) => {
        let b = s.as_bytes();
        let mut v = 0u32;
        let mut i = 0;
        while i < b.len() {
            v = v * 10 + (b[i] - b'0') as u32;
            i += 1;
        }
        v
    }
    None => 7,
};

const APP_PORT: u16 = 100;

/// The map: it is the same in every run, so each switch can work out the way to the logger once it knows who is on the other
/// end of its ports.
#[cfg(not(feature = "practice"))]
const LINKS: &[(u8, u8)] = &[
    (0, 1), (0, 5), (1, 2), (1, 6), (2, 3), (2, 7), (3, 4), (3, 8), (4, 9), (5, 6), (5, 10), (6, 7), (6, 11), (7, 8), (7, 12),
    (8, 9), (8, 13), (9, 14), (10, 11), (11, 12), (12, 13), (13, 14),
];
#[cfg(feature = "practice")]
const LINKS: &[(u8, u8)] = &[(0, 1), (1, 2), (0, 3), (1, 4), (1, 5), (2, 5), (3, 4), (4, 5)];

/// Hops from every switch to `root`.
fn distances(root: u8) -> HashMap<u8, u8> {
    let mut d = HashMap::new();
    d.insert(root, 0u8);
    let mut queue = VecDeque::from([root]);
    while let Some(x) = queue.pop_front() {
        let dx = d[&x];
        for &(a, b) in LINKS {
            let next = if a == x { b } else if b == x { a } else { continue };
            if !d.contains_key(&next) {
                d.insert(next, dx + 1);
                queue.push_back(next);
            }
        }
    }
    d
}

/// How often the data plane punts, as masks on a counter: a packet is punted when `seq & mask == 0`.
#[derive(Clone, Copy)]
struct Masks {
    /// keepalives (peer id and link delay)
    ka: u64,
    /// customer packets sampled raw
    raw: u64,
}

impl Masks {
    /// Fewer punts when the event budget is small, so keepalive samples and reports are not crowded out.
    fn for_events(per_s: u32) -> Self {
        if per_s < 40 {
            Masks { ka: 15, raw: 1023 }
        } else {
            Masks { ka: 7, raw: 255 }
        }
    }
}

fn priority(ty: u8) -> u8 {
    match ty {
        T_LINK_DOWN | T_LINK_UP | T_SWITCH_SILENT | T_SCAN_SUSPECT | T_CUSTOMER_GAP => 0,
        T_HEARTBEAT | T_LINK_QUALITY | T_PORT_STATS | T_CUSTOMER_PROFILE | T_FLOW_LOSS => 1,
        T_CUSTOMER_RATE | T_TOP_TALKER | T_BUDGET_STATS => 2,
        _ => 3,
    }
}

struct Pending {
    sent: u8,
    prio: u8,
    at_ns: u64,
    rec: Vec<u8>,
}
const PROTO_REPORT: u8 = 93;
const REPORT_IP: u32 = 0xe000_0007; // 224.0.0.7
/// Reports are paced by the export budget (`input.observe`), mirrored here as a token bucket. A report carries up to CHUNK bytes of
/// records, goes when something urgent is waiting (but not twice within URGENT_GAP_NS), when the oldest record has waited
/// SEND_AGE_NS, or when a full chunk is ready; never twice within MIN_GAP_NS.
const SEND_AGE_NS: u64 = 1_500_000_000;
const URGENT_GAP_NS: u64 = 250_000_000;
const MIN_GAP_NS: u64 = 100_000_000;
const PENDING_BYTES: usize = 1500;
const CHUNK: usize = 180;
const REPORT_HEADER: f64 = 40.0; // what a report is charged on top of its payload
const TICK_NS: u64 = 100_000_000;
const WINDOW_TICKS: u64 = 10; // 1 s
const FLOW_TICKS: u64 = 50; // 5 s
const PORT_TICKS: u64 = 20; // 2 s
const SLOW_TICKS: u64 = 50; // 5 s
const FINAL_NS: u64 = 39_300_000_000; // the logger's own last flush
/// Other switches flush their last records earlier, one slot each, so they do not all arrive in the same instant.
const FINAL_BASE_NS: u64 = 37_600_000_000;
const FINAL_STEP_NS: u64 = 100_000_000;
/// Report transport: every switch installs a route for REPORT_IP toward the logger (table REPORT_TABLE), so transit switches
/// forward a report in the data plane (no punt, no new header); only the logger punts it (reason 4).
const REPORT_TABLE: u32 = 4;
const LOGGER_ENTRY: u64 = 1;
const SENDERS: f64 = 14.0;
/// An urgent record (link change, gap, scan) rides in this many reports in all; the logger drops the copies by hash.
const URGENT_COPIES: u8 = 3;
const QUIET_PERIODS: u64 = 10;
const GAP_NS: u64 = 1_000_000_000;
const SCAN_WINDOW_NS: u64 = 5_000_000_000;
const SMALL_PKT: u64 = 128;
const MAX_CUSTOMERS: usize = 60;
/// Raw records are rationed per second so they cover the whole run: this many bytes a second in all, and at most
/// RAW_PER_ORIGIN from any one switch, so a busy switch can't crowd out the rest. RAW_LIMIT is a backstop: once the log
/// holds that many bytes (of 64 KB in a truth run) raw records stop.
const RAW_PER_SEC: usize = 800;
const RAW_PER_ORIGIN: usize = 80;
const RAW_LIMIT: usize = 50_000;

// One counter array, and it has to stay small: the controller gets a copy of it on every timer, and the whole timer
// event must fit in 4,096 bytes (a customer count of about 20 overflowed a layout of full matrices).
// Row blocks have one slot per source row (row 0 = not a customer, row c+1 = customer c); COL_LEFT is per destination
// column. Per-pair blocks (packets entered and left, by source and destination) exist only while W*W stays small.
const ARR: u32 = 0;
const R_PKTS: usize = 0; // packets that entered from a customer port
const R_BYTES: usize = 1;
const R_DUP: usize = 2; // a data packet with the same seq as the one before it
const R_UDP: usize = 3;
const R_ACK: usize = 4;
const R_T60: usize = 5; // entered with ttl below 60, 30, 10
const R_T30: usize = 6;
const R_T10: usize = 7;
const COL_LEFT: usize = 8; // packets that left toward a customer port, by destination column
const ROW_BLOCKS: usize = 9;
const MAX_PAIR_W: usize = 24;
const PORT_REGION: usize = 32; // keepalives by ingress port, then bytes sent by egress (port + 1)

#[derive(Clone, Copy)]
struct Layout {
    w: usize,
    pairs: bool,
}

impl Layout {
    fn new(customers: usize) -> Self {
        let w = customers + 1;
        Self { w, pairs: w <= MAX_PAIR_W }
    }
    fn row(&self, blk: usize) -> usize {
        blk * self.w
    }
    fn pair_pkts(&self) -> usize {
        ROW_BLOCKS * self.w
    }
    fn pair_left(&self) -> usize {
        self.pair_pkts() + self.w * self.w
    }
    fn ports(&self) -> usize {
        ROW_BLOCKS * self.w + if self.pairs { 2 * self.w * self.w } else { 0 }
    }
    fn total(&self) -> usize {
        self.ports() + 2 * PORT_REGION
    }
}

/// Assemble one stage, turning `@label:` lines and `-> @label` targets into instruction indices.
fn stage(header: &str, body: &str) -> String {
    let mut labels: HashMap<&str, usize> = HashMap::new();
    let mut n = 0;
    for l in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(name) = l.strip_prefix('@').and_then(|s| s.strip_suffix(':')) {
            labels.insert(name, n);
        } else {
            n += 1;
        }
    }
    let mut out = format!("{header}\n");
    for l in body.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('@')) {
        match l.split_once("-> @") {
            Some((head, label)) => out.push_str(&format!("{head}-> {}\n", labels[label.trim()])),
            None => {
                out.push_str(l);
                out.push('\n');
            }
        }
    }
    out
}

/// A tiny register allocator and line builder for the counting stage.
struct Asm {
    lines: Vec<String>,
    next: u32,
}

impl Asm {
    fn reg(&mut self) -> String {
        self.next += 1;
        format!("r{}", self.next)
    }
    fn line(&mut self, l: impl Into<String>) {
        self.lines.push(l.into());
    }
    fn konst(&mut self, v: usize) -> String {
        let r = self.reg();
        self.line(format!("const {r}, {v}"));
        r
    }
    /// `base + off` as a register.
    fn offset(&mut self, base: &str, off: usize) -> String {
        if off == 0 {
            return base.to_string();
        }
        let k = self.konst(off);
        let r = self.reg();
        self.line(format!("add {r}, {base}, {k}"));
        r
    }
    fn load(&mut self, field: &str) -> String {
        let r = self.reg();
        self.line(format!("load {r}, {field}"));
        r
    }
    fn loadm(&mut self, m: u32) -> String {
        let r = self.reg();
        self.line(format!("loadm {r}, m{m}"));
        r
    }
    fn bin(&mut self, op: &str, a: &str, b: &str) -> String {
        let r = self.reg();
        self.line(format!("{op} {r}, {a}, {b}"));
        r
    }
}

fn program_text(lay: &Layout, masks: Masks) -> String {
    let mut p = String::new();
    // row index of the source customer (0 = none), its row's start in a pair block, and the destination column.
    p += &stage("stage alu=4 mem=1", "load r0, ip_dst\ntable t4, r0 -> m13\nnoop");
    p += &stage("stage alu=4 mem=1", "load r0, ip_src\ntable t1, r0 -> m10\nnoop");
    p += &stage("stage alu=4 mem=1", "load r0, ip_src\ntable t3, r0 -> m12\nnoop");
    p += &stage("stage alu=4 mem=1", "load r0, ip_dst\ntable t2, r0 -> m11\nnoop");
    // The register stage: a data packet from a customer with the seq of the last one on its (src, dst) pair: m5 = 1.
    p += &stage(
        "stage alu=24 mem=1",
        "loadm r0, m6
loadm r1, m1
add r2, r0, r1
load r3, ingress_port
const r4, 100
lt r5, r3, r4
load r6, kind
const r7, 0
eq r8, r6, r7
const r9, 1
sub r10, r9, r5
and r11, r10, r8
load r12, seq
lt r13, r7, r12
and r14, r11, r13
sub r16, r9, r14
bif r16 -> @end
rread a0[r2] -> r17
eq r18, r17, r12
rwrite a0[r2], r12
storem m5, r18
@end:
noop",
    );
    // A small data packet from a customer to a (source, destination) pair this switch has not seen: m7 = 1. A scan is one
    // small packet to each of many addresses, so every one of its packets is new and is punted; an ordinary flow is punted
    // once. The pair is hashed to 12 bits, so a scan packet that collides with a pair already seen is missed.
    p += &stage(
        "stage alu=32 mem=1",
        &format!(
            "load r0, ip_src
load r1, ip_dst
xor r2, r0, r1
const r3, 4095
and r4, r2, r3
load r5, ingress_port
const r6, 100
lt r7, r5, r6
const r8, 1
sub r9, r8, r7
load r10, kind
const r11, 0
eq r12, r10, r11
and r13, r9, r12
load r14, size
const r15, {SMALL_PKT}
lt r16, r14, r15
and r17, r13, r16
sub r18, r8, r17
bif r18 -> @end
rread a1[r4] -> r19
rwrite a1[r4], r8
sub r20, r8, r19
storem m7, r20
@end:
noop"
        ),
    );
    // The counting stage: one array, so one stage.
    let mut a = Asm { lines: vec![], next: 0 };
    let row = a.loadm(0);
    let col = a.loadm(1);
    let base = a.loadm(6);
    let dup = a.loadm(5);
    let pair = a.bin("add", &base, &col);
    let one = a.konst(1);
    let ing = a.load("ingress_port");
    let c100 = a.konst(100);
    let from_switch = a.bin("lt", &ing, &c100);
    a.line(format!("bif {from_switch} -> @left"));
    // Entered from a customer port.
    for (blk, val) in [(R_PKTS, one.clone())] {
        let idx = a.offset(&row, lay.row(blk));
        a.line(format!("counter c{ARR}[{idx}], {val}"));
    }
    let size = a.load("size");
    let idx = a.offset(&row, lay.row(R_BYTES));
    a.line(format!("counter c{ARR}[{idx}], {size}"));
    let idx = a.offset(&row, lay.row(R_DUP));
    a.line(format!("counter c{ARR}[{idx}], {dup}"));
    let kind = a.load("kind");
    let k4 = a.konst(4);
    let is_ack = a.bin("eq", &kind, &k4);
    let idx = a.offset(&row, lay.row(R_ACK));
    a.line(format!("counter c{ARR}[{idx}], {is_ack}"));
    let proto = a.load("ip_proto");
    let k17 = a.konst(17);
    let is_udp = a.bin("eq", &proto, &k17);
    let idx = a.offset(&row, lay.row(R_UDP));
    a.line(format!("counter c{ARR}[{idx}], {is_udp}"));
    let ttl = a.load("ttl");
    for (blk, lim) in [(R_T60, 60), (R_T30, 30), (R_T10, 10)] {
        let k = a.konst(lim);
        let f = a.bin("lt", &ttl, &k);
        let idx = a.offset(&row, lay.row(blk));
        a.line(format!("counter c{ARR}[{idx}], {f}"));
    }
    if lay.pairs {
        let idx = a.offset(&pair, lay.pair_pkts());
        a.line(format!("counter c{ARR}[{idx}], {one}"));
    }
    a.line("@left:");
    // Left toward a customer port: harness_egress is port + 1, so more than 100 means a customer port.
    let he = a.load("harness_egress");
    let k100 = a.konst(100);
    let is_cust = a.bin("lt", &k100, &he);
    let not_cust = a.bin("sub", &one, &is_cust);
    a.line(format!("bif {not_cust} -> @ka"));
    let idx = a.offset(&col, lay.row(COL_LEFT));
    a.line(format!("counter c{ARR}[{idx}], {one}"));
    if lay.pairs {
        let idx = a.offset(&pair, lay.pair_left());
        a.line(format!("counter c{ARR}[{idx}], {one}"));
    }
    a.line("@ka:");
    // Keepalives by ingress port.
    let kind2 = a.load("kind");
    let k5 = a.konst(KIND_KEEPALIVE as usize);
    let is_ka = a.bin("eq", &kind2, &k5);
    let not_ka = a.bin("sub", &one, &is_ka);
    a.line(format!("bif {not_ka} -> @egr"));
    let idx = a.offset(&ing, lay.ports());
    a.line(format!("counter c{ARR}[{idx}], {one}"));
    a.line("@egr:");
    // Bytes sent out of a switch port, by harness egress (port + 1).
    let he2 = a.load("harness_egress");
    let zero = a.konst(0);
    let no_route = a.bin("eq", &he2, &zero);
    a.line(format!("bif {no_route} -> @end"));
    let size2 = a.load("size");
    let idx = a.offset(&he2, lay.ports() + PORT_REGION);
    a.line(format!("counter c{ARR}[{idx}], {size2}"));
    a.line("@end:");
    a.line("noop");
    p += &stage("stage alu=200 mem=1", &a.lines.join("\n"));
    p += &stage("stage alu=40 mem=0", &punt_block(masks));
    p
}

fn punt_block(masks: Masks) -> String {
    let (ka, raw) = (masks.ka, masks.raw);
    format!(
        "const r3, 1
load r0, kind
const r1, {KIND_KEEPALIVE}
eq r2, r0, r1
load r5, seq
const r6, {ka}
and r7, r5, r6
const r8, 0
eq r9, r7, r8
and r10, r2, r9
sub r11, r3, r10
bif r11 -> @app
punt 1
@app:
load r12, ingress_port
const r13, 100
lt r14, r12, r13
sub r15, r3, r14
load r0, kind
eq r17, r0, r8
and r18, r15, r17
load r19, size
const r20, {SMALL_PKT}
lt r21, r19, r20
and r22, r18, r21
loadm r29, m7
sub r23, r3, r29
bif r23 -> @raw
punt 3
@raw:
const r24, {raw}
and r25, r5, r24
eq r26, r25, r8
and r27, r15, r26
sub r28, r3, r27
bif r28 -> @end
punt 2
@end:
noop"
    )
}

#[derive(Default)]
struct PortState {
    peer: Option<u8>,
    /// The keepalive counter last tick, when it last rose, and whether we call the link down.
    last_count: u64,
    last_rise_ns: u64,
    heard: bool,
    down: bool,
    win_got: u64,
    /// Keepalives over the last 2 s, and whether the link was down at any point in it.
    got2: u64,
    down2: bool,
    win_delay_sum: u64,
    win_delay_n: u64,
    drops: u32,
    last_kb: u64,
    last_delay: Option<u16>,
    /// Drops on this port, decayed (half-life about 3 s): a congested port starves the report queue.
    drop_ema: f64,
}

#[derive(Default, Clone)]
struct CustState {
    /// Per-tick packet counts since the last profile: ticks, idle ticks, sum, sum of squares.
    ticks: u64,
    idle: u64,
    sum: u64,
    sumsq: u64,
    last_active_ns: u64,
    seen: bool,
    gap_open: bool,
    gap_start_ns: u64,
    last_win_pkts: u64,
    rate_written: u64,
    rate_tick: u64,
}

#[derive(Default)]
struct ScanState {
    seen: VecDeque<(u64, u32)>,
    dsts: HashMap<u32, u32>,
    first_flagged: bool,
    second_flagged: bool,
}

pub struct A3Log {
    me: u32,
    is_logger: bool,
    switch_ports: Vec<u16>,
    customers: Vec<Customer>,
    lay: Layout,
    period_ns: u64,
    started: bool,
    final_done: bool,
    tick: u64,

    ports: HashMap<u16, PortState>,
    /// Counter arrays at the last tick, the last window (1 s), the last flow window (2 s) and the last 5 s.
    prev: HashMap<u32, Vec<u64>>,
    win: HashMap<u32, Vec<u64>>,
    flow: HashMap<u32, Vec<u64>>,
    slow: HashMap<u32, Vec<u64>>,
    pwin: HashMap<u32, Vec<u64>>,
    cust: Vec<CustState>,

    out_records: Vec<Vec<u8>>,
    /// Records this switch made (cumulative), and records the logger has taken in from each origin.
    recs_originated: u16,
    seen_recs: HashSet<u64>,
    rx_from: [u16; 16],
    written: usize,
    raw_sec: u64,
    raw_this: usize,
    raw_by: [usize; 16],
    other_last_pkts: u64,
    other_rate: (u64, u64),
    last_port_win_ns: u64,
    now: u64,
    /// Records waiting for a report (own and relayed), most important first when they leave.
    pending: Vec<Pending>,
    pending_bytes: usize,
    last_send_ns: u64,
    /// The export budget, mirrored: bytes now, the cap, the refill per second, and when we last looked.
    tokens: f64,
    burst: f64,
    rate: f64,
    last_tok_ns: u64,
    observe: ObserveLimits,
    dist: HashMap<u8, u8>,
    ka_div: u64,

    ka_total: u64,
    ka_punts: u64,
    /// Report route: the port the installed route points at, its entry id, and whether the logger's punt entry is in.
    route_port: Option<u16>,
    route_gen: u64,
    logger_entry: bool,
    /// Report slots: one send per `slot_ns`, this switch's turn comes at `next_slot_ns`; its last flush is at `final_ns`.
    slot_ns: u64,
    next_slot_ns: u64,
    urgent_gap_ns: u64,
    final_ns: u64,
    scan: HashMap<u32, ScanState>,
    /// Logger only: who said a link toward a peer went down, to tell a dead switch from a dead link.
    down_reports: HashMap<u8, Vec<(u8, u16)>>,
    silent_done: HashSet<u8>,
}

impl A3Log {
    fn rec(&mut self, ty: u8, t_ns: u64, body: &[u8]) {
        let r = Rec { ty, sw: self.me as u8, t10: (t_ns / 10_000_000).min(u16::MAX as u64) as u16, body: body.to_vec() };
        self.recs_originated = self.recs_originated.wrapping_add(1);
        self.accept(r);
    }

    /// A record this switch made or relayed: written (logging switch) or queued for a report.
    fn accept(&mut self, r: Rec) {
        if self.is_logger {
            self.watch(&r);
            let o = (r.sw & 15) as usize;
            self.rx_from[o] = self.rx_from[o].wrapping_add(1);
            self.out_records.push(r.encode());
        } else {
            self.enqueue(r);
        }
    }

    fn enqueue(&mut self, r: Rec) {
        let prio = priority(r.ty);
        let bytes = r.encode();
        // Make room by dropping the least important, oldest record; if the new one is the least important, drop it.
        while self.pending_bytes + bytes.len() > PENDING_BYTES {
            let worst = self.pending.iter().enumerate().max_by_key(|(_, p)| (p.prio, std::cmp::Reverse(p.at_ns))).map(|(i, p)| (i, p.prio));
            match worst {
                Some((i, wp)) if wp >= prio => {
                    let gone = self.pending.swap_remove(i);
                    self.pending_bytes -= gone.rec.len();
                }
                _ => return,
            }
        }
        self.pending_bytes += bytes.len();
        self.pending.push(Pending { sent: 0, prio, at_ns: self.now, rec: bytes });
    }

    /// The logger's own inferences from the stream of records.
    fn watch(&mut self, r: &Rec) {
        match r.ty {
            T_LINK_DOWN => {
                let peer = r.body[0];
                let v = self.down_reports.entry(peer).or_default();
                v.retain(|(_, t)| r.t10.saturating_sub(*t) <= 100);
                if !v.iter().any(|(s, _)| *s == r.sw) {
                    v.push((r.sw, r.t10));
                }
                if v.len() >= 2 && self.silent_done.insert(peer) {
                    let t = v.iter().map(|(_, t)| *t).min().unwrap_or(r.t10);
                    let s = Rec { ty: T_SWITCH_SILENT, sw: self.me as u8, t10: t, body: vec![peer] };
                    self.out_records.push(s.encode());
                }
            }
            T_LINK_UP => {
                self.down_reports.remove(&r.body[0]);
                self.silent_done.remove(&r.body[0]);
            }
            T_HEARTBEAT => {
                let sent = u16_at(&r.body, 0);
                let lost = u16_at(&r.body, 2);
                let got = self.rx_from[(r.sw & 15) as usize];
                let refused = sent.wrapping_sub(got).min(0x7fff);
                let mut b = lost.to_le_bytes().to_vec();
                b.extend_from_slice(&refused.to_le_bytes());
                let s = Rec { ty: T_BUDGET_STATS, sw: r.sw, t10: r.t10, body: b };
                self.out_records.push(s.encode());
            }
            _ => {}
        }
    }

    /// Write what the logging switch holds, or (other switches) send one report if it is time and the budget allows.
    fn flush(&mut self, now: u64, out: &mut Vec<Action>) {
        if self.is_logger {
            for r in std::mem::take(&mut self.out_records) {
                // Exploration records give way once the log is mostly full, so the rest keeps its room.
                let raw = matches!(r[0] >> 4, T_RAW_SAMPLE | T_RAW_KEEPALIVE);
                if raw {
                    let sec = now / 1_000_000_000;
                    if sec != self.raw_sec {
                        self.raw_sec = sec;
                        self.raw_this = 0;
                        self.raw_by = [0; 16];
                    }
                    let o = (r[0] & 15) as usize;
                    if self.written >= RAW_LIMIT || self.raw_this + r.len() > RAW_PER_SEC || self.raw_by[o] + r.len() > RAW_PER_ORIGIN {
                        continue;
                    }
                    self.raw_this += r.len();
                    self.raw_by[o] += r.len();
                }
                self.written += r.len();
                out.push(actions::log_write(r));
            }
            return;
        }
        self.refill(now);
        let fresh = self.pending.iter().any(|p| p.sent == 0);
        if !fresh || now.saturating_sub(self.last_send_ns) < MIN_GAP_NS {
            return;
        }
        let since = now.saturating_sub(self.last_send_ns);
        let urgent = self.pending.iter().any(|p| p.prio == 0 && p.sent == 0);
        let slot_due = now >= self.next_slot_ns;
        // A slot sends when there is enough to be worth a header (or the oldest record has waited two slots).
        let oldest = self.pending.iter().filter(|p| p.sent == 0).map(|p| p.at_ns).min().unwrap_or(now);
        let worth = self.pending_bytes >= 100 || now.saturating_sub(oldest) >= 2 * self.slot_ns;
        let due = (urgent && since >= self.urgent_gap_ns)
            || (slot_due && worth)
            || (self.pending_bytes >= CHUNK && since >= self.slot_ns / 2 && self.observe.punt_events_per_s >= 20)
            // With a rich bucket a full chunk may go at once, so records are not held while the bucket sits full.
            || (self.pending_bytes >= CHUNK && self.tokens >= 4.0 * (CHUNK as f64 + REPORT_HEADER) && since >= 3 * self.urgent_gap_ns / 2)
            || (self.final_done && since >= self.urgent_gap_ns);
        if slot_due {
            while self.next_slot_ns <= now {
                self.next_slot_ns += self.slot_ns;
            }
        }
        if !due {
            return;
        }
        // One byte of the payload names the sender; the packet is charged 40 more. Keep a margin on the bucket.
        let room = (self.tokens * 0.9 - REPORT_HEADER - 1.0).min(CHUNK as f64);
        if room < 12.0 {
            return;
        }
        let Some(port) = self.route_port else { return };
        let mut order: Vec<usize> = (0..self.pending.len()).collect();
        order.sort_by_key(|&i| (self.pending[i].prio, self.pending[i].at_ns));
        let mut taken = vec![false; self.pending.len()];
        let mut payload = vec![self.me as u8];
        for i in order {
            let len = self.pending[i].rec.len();
            if (payload.len() - 1 + len) as f64 <= room {
                payload.extend_from_slice(&self.pending[i].rec);
                taken[i] = true;
            }
        }
        if payload.len() == 1 {
            return;
        }
        // An urgent record stays queued for its repeats; everything else leaves.
        for (i, t) in taken.iter_mut().enumerate() {
            if *t && self.pending[i].prio == 0 {
                self.pending[i].sent += 1;
                if self.pending[i].sent < URGENT_COPIES {
                    *t = false;
                }
            }
        }
        let mut keep = taken.iter();
        self.pending.retain(|_| !*keep.next().unwrap());
        self.pending_bytes = self.pending.iter().map(|p| p.rec.len()).sum();
        self.tokens -= REPORT_HEADER + payload.len() as f64;
        self.last_send_ns = now;
        out.push(actions::inject_packet(port, REPORT_IP, REPORT_IP, PROTO_REPORT, 8, payload));
    }

    fn refill(&mut self, now: u64) {
        let dt = now.saturating_sub(self.last_tok_ns) as f64 / 1e9;
        self.last_tok_ns = now.max(self.last_tok_ns);
        self.tokens = (self.tokens + self.rate * dt).min(self.burst);
    }

    /// The port toward the logger: of the live ports whose far end is known, a neighbour closer than this switch, the one whose
    /// port has dropped least lately (a congested port starves the report queue); else the nearest of the others.
    fn parent(&self) -> Option<u16> {
        let mine = *self.dist.get(&(self.me as u8))?;
        let mut closer: Option<((i64, u8, u16), u16)> = None;
        let mut other: Option<((u8, u16), u16)> = None;
        for &p in &self.switch_ports {
            let Some(ps) = self.ports.get(&p) else { continue };
            let (Some(peer), false) = (ps.peer, ps.down) else { continue };
            let Some(&d) = self.dist.get(&peer) else { continue };
            if d < mine {
                let key = ((ps.drop_ema * 2.0) as i64, d, p);
                if closer.is_none_or(|(k, _)| key < k) {
                    closer = Some((key, p));
                }
            } else if other.is_none_or(|(k, _)| (d, p) < k) {
                other = Some(((d, p), p));
            }
        }
        // A detour (a neighbour that is not closer) is only taken once every port is identified, so no closer one is hiding.
        let all_known = self.switch_ports.iter().all(|p| self.ports.get(p).is_some_and(|s| s.peer.is_some()));
        closer.map(|(_, p)| p).or(if all_known { other.map(|(_, p)| p) } else { None })
    }

    /// Whether the neighbour on `port` is nearer the logger than this switch.
    fn is_closer(&self, port: u16) -> bool {
        let mine = self.dist.get(&(self.me as u8)).copied().unwrap_or(0);
        self.ports.get(&port).and_then(|s| s.peer).and_then(|p| self.dist.get(&p)).is_some_and(|d| *d < mine)
    }

    /// Keep the report route installed: the logger punts reports; every other switch forwards them toward the logger.
    fn route_update(&mut self, out: &mut Vec<Action>) {
        if self.is_logger {
            if !self.logger_entry {
                self.logger_entry = true;
                out.push(actions::install_entry(
                    REPORT_TABLE,
                    wire::TableEntryW {
                        id: wire::EntryIdW(LOGGER_ENTRY),
                        key: REPORT_IP as u64,
                        prefix_len: 32,
                        priority: 0,
                        action: wire::TableActionW::Punt { reason: wire::PuntReasonW::Custom(4) },
                    },
                ));
            }
            return;
        }
        let Some(best) = self.parent() else { return };
        if let Some(cur) = self.route_port {
            if cur == best {
                return;
            }
            // Move only when the current port is down, or the new one is clearly less congested.
            let ps = |p: u16| self.ports.get(&p);
            let cur_down = ps(cur).is_none_or(|s| s.down);
            let better = ps(best).map_or(0.0, |s| s.drop_ema) + 3.0 < ps(cur).map_or(0.0, |s| s.drop_ema);
            let wrong_way = !self.is_closer(cur) && self.is_closer(best);
            if !cur_down && !better && !wrong_way {
                return;
            }
            out.push(actions::delete_entry(REPORT_TABLE, self.route_gen));
        }
        self.route_gen += 1;
        out.push(actions::install_route(REPORT_TABLE, self.route_gen, REPORT_IP as u64, 32, best));
        self.route_port = Some(best);
    }

    /// A report is `[sender u8][records]`. The sender tells us who is on that port; the records are relayed or written.
    fn on_report(&mut self, payload: &[u8], port: u16) {
        if payload.is_empty() {
            return;
        }
        let _ = port;
        let (recs, _) = parse(&payload[1..]);
        for r in recs {
            let h = r.encode().iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100_0000_01b3));
            if self.seen_recs.len() > 30_000 {
                self.seen_recs.clear();
            }
            if self.seen_recs.insert(h) {
                self.accept(r);
            }
        }
    }

    fn cust_of(&self, ip: u32) -> u8 {
        let mut best: Option<(u8, usize)> = None;
        for (i, c) in self.customers.iter().take(MAX_CUSTOMERS).enumerate() {
            let bits = c.prefix_len as u32;
            let mask = if bits == 0 { 0 } else if bits >= 32 { u32::MAX } else { !0u32 << (32 - bits) };
            if (ip & mask) == (c.prefix_addr & mask) && best.is_none_or(|(l, _)| c.prefix_len > l) {
                best = Some((c.prefix_len, i));
            }
        }
        best.map(|(_, i)| i as u8).unwrap_or(255)
    }

    fn snapshot(&self, ev: &TimerEvent) -> HashMap<u32, Vec<u64>> {
        ev.counters.iter().map(|c| (c.id.0, c.values.clone())).collect()
    }

    fn at(map: &HashMap<u32, Vec<u64>>, arr: u32, i: usize) -> u64 {
        map.get(&arr).and_then(|v| v.get(i)).copied().unwrap_or(0)
    }

    fn delta(cur: &HashMap<u32, Vec<u64>>, old: &HashMap<u32, Vec<u64>>, i: usize) -> u64 {
        Self::at(cur, ARR, i).saturating_sub(Self::at(old, ARR, i))
    }

    /// Change of a row block for source row `r` (row 0 is "not a customer").
    fn row_delta(&self, cur: &HashMap<u32, Vec<u64>>, old: &HashMap<u32, Vec<u64>>, blk: usize, r: usize) -> u64 {
        Self::delta(cur, old, self.lay.row(blk) + r)
    }

    fn ports_tick(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>, ev: &TimerEvent) {
        for ps in self.ports.values_mut() {
            ps.drop_ema *= 0.98;
        }
        for d in &ev.port_drops {
            let ps = self.ports.entry(d.port).or_default();
            ps.drops += d.dropped;
            ps.drop_ema += d.dropped as f64;
        }
        let ports: Vec<u16> = self.switch_ports.clone();
        for port in ports {
            let count = Self::at(cur, ARR, self.lay.ports() + port as usize);
            let (new, down_now, last_rise, peer) = {
                let ps = self.ports.entry(port).or_default();
                let new = count.saturating_sub(ps.last_count);
                ps.last_count = count;
                ps.win_got += new;
                ps.got2 += new;
                if ps.down {
                    ps.down2 = true;
                }
                if new > 0 {
                    ps.heard = true;
                }
                (new, ps.down, ps.last_rise_ns, ps.peer)
            };
            self.ka_total += new;
            let peer = peer.unwrap_or(255);
            if new > 0 {
                self.ports.get_mut(&port).unwrap().last_rise_ns = now;
                if down_now {
                    self.ports.get_mut(&port).unwrap().down = false;
                    self.rec(T_LINK_UP, now, &[peer]);
                }
            } else if !down_now && self.ports[&port].heard && now.saturating_sub(last_rise) >= QUIET_PERIODS * self.period_ns {
                self.ports.get_mut(&port).unwrap().down = true;
                self.rec(T_LINK_DOWN, last_rise, &[peer]);
            }
        }
    }

    fn customers_tick(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>) {
        for c in 0..self.customers.len().min(MAX_CUSTOMERS) {
            let pk = self.row_delta(cur, &self.prev, R_PKTS, c + 1);
            let active = pk > 0;
            {
                let st = &mut self.cust[c];
                st.ticks += 1;
                st.sum += pk;
                st.sumsq += pk * pk;
                if !active {
                    st.idle += 1;
                }
            }
            if active {
                let (was_open, start) = {
                    let st = &mut self.cust[c];
                    let r = (st.gap_open, st.gap_start_ns);
                    st.gap_open = false;
                    st.seen = true;
                    st.last_active_ns = now;
                    r
                };
                if was_open {
                    let dur = ((now - start) / 10_000_000).min(u16::MAX as u64) as u16;
                    let mut b = vec![c as u8];
                    b.extend_from_slice(&((start / 10_000_000) as u16).to_le_bytes());
                    b.extend_from_slice(&dur.to_le_bytes());
                    self.rec(T_CUSTOMER_GAP, start, &b);
                }
            } else {
                let st = &mut self.cust[c];
                if st.seen && !st.gap_open && now.saturating_sub(st.last_active_ns) >= GAP_NS {
                    st.gap_open = true;
                    st.gap_start_ns = st.last_active_ns;
                }
            }
        }
    }
    fn window(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>) {
        // Link quality and raw keepalive delay, per port.
        let expected = (WINDOW_TICKS * TICK_NS / self.period_ns.max(1)).max(1);
        let ports: Vec<u16> = self.switch_ports.clone();
        for port in ports {
            let (heard, down, got, dsum, dn, last_delay, peer) = {
                let ps = self.ports.entry(port).or_default();
                let r = (ps.heard, ps.down, ps.win_got, ps.win_delay_sum, ps.win_delay_n, ps.last_delay, ps.peer.unwrap_or(255));
                ps.win_got = 0;
                ps.win_delay_sum = 0;
                ps.win_delay_n = 0;
                r
            };
            if heard && !down {
                let loss = if got >= expected { 0 } else { ((expected - got) * 100 / expected) as u8 };
                let delay = if dn > 0 { Some((dsum / dn / 100_000).min(u16::MAX as u64) as u16) } else { None };
                let shifted = match (delay, last_delay) {
                    (Some(d), Some(l)) => d.abs_diff(l) > (l / 3).max(5),
                    (Some(_), None) => true,
                    _ => false,
                };
                if loss >= 10 || shifted {
                    let d = delay.or(last_delay).unwrap_or(0);
                    let mut b = vec![peer, loss];
                    b.extend_from_slice(&d.to_le_bytes());
                    self.rec(T_LINK_QUALITY, now, &b);
                }
                if shifted {
                    self.ports.get_mut(&port).unwrap().last_delay = delay;
                }
                if let Some(d) = delay {
                    if self.tick % (2 * WINDOW_TICKS) == 0 {
                        let mut b = vec![port as u8, peer];
                        b.extend_from_slice(&d.to_le_bytes());
                        self.rec(T_RAW_KEEPALIVE, now, &b);
                    }
                }
            }
        }
        // Rates of every source: customers by index, everything else (peers) as 255. A rate is written when it moved by a
        // fifth or more since the last one written, when it falls to or rises from zero, or every 5 s.
        for r in 0..=self.customers.len().min(MAX_CUSTOMERS) {
            let pkts = self.row_delta(cur, &self.win, R_PKTS, r);
            let bytes = self.row_delta(cur, &self.win, R_BYTES, r);
            let (prev, last_written, last_tick) = if r == 0 {
                (self.other_last_pkts, self.other_rate.0, self.other_rate.1)
            } else {
                let c = &self.cust[r - 1];
                (c.last_win_pkts, c.rate_written, c.rate_tick)
            };
            let moved = pkts.abs_diff(last_written) * 5 >= last_written.max(10);
            let edge = (pkts == 0) != (prev == 0);
            let due = self.tick.saturating_sub(last_tick) >= 5 * WINDOW_TICKS;
            if r == 0 {
                self.other_last_pkts = pkts;
            } else {
                self.cust[r - 1].last_win_pkts = pkts;
            }
            if (pkts > 0 || prev > 0) && (moved || edge || due) {
                if r == 0 {
                    self.other_rate = (pkts, self.tick);
                } else {
                    self.cust[r - 1].rate_written = pkts;
                    self.cust[r - 1].rate_tick = self.tick;
                }
                let id = if r == 0 { 255 } else { (r - 1) as u8 };
                let mut b = vec![id];
                b.extend_from_slice(&(pkts.min(u16::MAX as u64) as u16).to_le_bytes());
                b.extend_from_slice(&((bytes / 1000).min(u16::MAX as u64) as u16).to_le_bytes());
                self.rec(T_CUSTOMER_RATE, now, &b);
            }
        }
    }

    /// Per switch port, every 2 s: what it dropped and how much it sent, when that changed.
    fn port_window(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>) {
        // The last window can be cut short by the final flush, so expect what the elapsed time allows.
        let expected = (now.saturating_sub(self.last_port_win_ns) / self.period_ns.max(1)).max(1);
        self.last_port_win_ns = now;
        let ports: Vec<u16> = self.switch_ports.clone();
        for port in ports {
            let bytes = Self::delta(cur, &self.pwin, self.lay.ports() + PORT_REGION + port as usize + 1);
            let kb = bytes / 1000;
            let (drops, last, peer, lost) = {
                let ps = self.ports.entry(port).or_default();
                let lost = if ps.heard && !ps.down2 && !ps.down { expected.saturating_sub(ps.got2).min(255) as u8 } else { 0 };
                let r = (ps.drops, ps.last_kb, ps.peer.unwrap_or(255), lost);
                ps.drops = 0;
                ps.got2 = 0;
                ps.down2 = false;
                r
            };
            let changed = kb.abs_diff(last) * 4 >= last.max(8);
            let periodic = self.tick % (5 * PORT_TICKS) == 0 && (kb > 0 || last > 0);
            if drops > 0 || changed || periodic || lost > 0 {
                self.ports.get_mut(&port).unwrap().last_kb = kb;
                let mut b = vec![peer];
                b.extend_from_slice(&(drops.min(u16::MAX as u32) as u16).to_le_bytes());
                b.extend_from_slice(&(kb.min(u16::MAX as u64) as u16).to_le_bytes());
                b.push(lost);
                self.rec(T_PORT_STATS, now, &b);
            }
        }
    }

    fn flow_window(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>) {
        let n = self.customers.len().min(MAX_CUSTOMERS);
        let w = self.lay.w;
        let id = |i: usize| if i == 0 { 255 } else { (i - 1) as u8 };
        let sat = |x: u64| x.min(u16::MAX as u64) as u16;
        let emit = |me: &mut Self, src: u8, dst: u8, ent: u64, left: u64, dups: u64| {
            let mut b = vec![src, dst];
            b.extend_from_slice(&sat(ent).to_le_bytes());
            b.extend_from_slice(&sat(left).to_le_bytes());
            b.extend_from_slice(&sat(dups).to_le_bytes());
            me.rec(T_FLOW_LOSS, now, &b);
        };
        // Duplicates are counted per source; they ride on a record of their own.
        for r in 0..=n {
            let dups = self.row_delta(cur, &self.flow, R_DUP, r);
            let pkts = self.row_delta(cur, &self.flow, R_PKTS, r);
            if self.lay.pairs {
                for c in 0..=n {
                    let ent = Self::delta(cur, &self.flow, self.lay.pair_pkts() + r * w + c);
                    let left = Self::delta(cur, &self.flow, self.lay.pair_left() + r * w + c);
                    if ent > 0 || left > 0 {
                        emit(self, id(r), id(c), ent, left, 0);
                    }
                }
                if dups > 0 {
                    emit(self, id(r), 255, 0, 0, dups);
                }
            } else if pkts > 0 || dups > 0 {
                emit(self, id(r), 255, pkts, 0, dups);
            }
        }
        if !self.lay.pairs {
            for c in 0..=n {
                let left = Self::delta(cur, &self.flow, self.lay.row(COL_LEFT) + c);
                if left > 0 {
                    emit(self, 255, id(c), 0, left, 0);
                }
            }
        }
    }

    fn slow_window(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>) {
        let n = self.customers.len().min(MAX_CUSTOMERS);
        let mut total_bytes = 0u64;
        let mut best: Option<(u64, usize)> = None;
        for c in 0..n {
            let r = c + 1;
            let pkts = self.row_delta(cur, &self.slow, R_PKTS, r);
            let bytes = self.row_delta(cur, &self.slow, R_BYTES, r);
            total_bytes += bytes;
            if best.is_none_or(|(b, _)| bytes > b) && bytes > 0 {
                best = Some((bytes, c));
            }
            let st = std::mem::take(&mut self.cust[c]);
            // Counters of a customer's state that must survive the profile.
            self.cust[c] = CustState {
                last_active_ns: st.last_active_ns,
                seen: st.seen,
                gap_open: st.gap_open,
                gap_start_ns: st.gap_start_ns,
                last_win_pkts: st.last_win_pkts,
                rate_written: st.rate_written,
                rate_tick: st.rate_tick,
                ..Default::default()
            };
            if pkts > 0 {
                let udp = self.row_delta(cur, &self.slow, R_UDP, r);
                let ttl60 = self.row_delta(cur, &self.slow, R_T60, r);
                let ttl30 = self.row_delta(cur, &self.slow, R_T30, r);
                let ttl10 = self.row_delta(cur, &self.slow, R_T10, r);
                let acks = self.row_delta(cur, &self.slow, R_ACK, r);
                let secs = (SLOW_TICKS * TICK_NS / 1_000_000_000).max(1);
                let rate = pkts / secs;
                let bucket = (64 - (rate + 1).leading_zeros()) as u8;
                let ticks = st.ticks.max(1) as f64;
                let mean = st.sum as f64 / ticks;
                let var = (st.sumsq as f64 / ticks - mean * mean).max(0.0);
                let cv32 = if mean > 0.0 { (var.sqrt() / mean * 32.0).min(255.0) as u8 } else { 0 };
                let pct = |x: u64| (x * 100 / pkts).min(100) as u8;
                let b = [c as u8, (bytes / pkts / 8).min(255) as u8, pct(udp), (st.idle * 100 / st.ticks.max(1)).min(100) as u8, bucket, cv32, pct(ttl60), pct(ttl30), pct(ttl10), pct(acks)];
                self.rec(T_CUSTOMER_PROFILE, now, &b);
            }
        }
        if let Some((bytes, c)) = best {
            self.rec(T_TOP_TALKER, now, &[c as u8, (bytes * 100 / total_bytes.max(1)) as u8]);
        }
    }
    fn heartbeat(&mut self, now: u64) {
        let expected = self.ka_total / self.ka_div.max(1);
        let lost = expected.saturating_sub(self.ka_punts).min(u16::MAX as u64) as u16;
        let mut b = self.recs_originated.to_le_bytes().to_vec();
        b.extend_from_slice(&lost.to_le_bytes());
        self.rec(T_HEARTBEAT, now, &b);
    }

    fn finish_run(&mut self, now: u64) {
        for c in 0..self.cust.len() {
            if self.cust[c].gap_open {
                let start = self.cust[c].gap_start_ns;
                let dur = ((now - start) / 10_000_000).min(u16::MAX as u64) as u16;
                let mut b = vec![c as u8];
                b.extend_from_slice(&((start / 10_000_000) as u16).to_le_bytes());
                b.extend_from_slice(&dur.to_le_bytes());
                self.rec(T_CUSTOMER_GAP, start, &b);
            }
        }
        self.heartbeat(now);
    }

    fn raw_sample(&mut self, ev: &PuntEvent) {
        let mut b = ev.ip_src.to_le_bytes().to_vec();
        b.extend_from_slice(&ev.ip_dst.to_le_bytes());
        b.push(ev.ip_proto);
        b.extend_from_slice(&(ev.packet_size.min(u16::MAX as u64) as u16).to_le_bytes());
        b.push(ev.ip_ttl);
        self.rec(T_RAW_SAMPLE, ev.arrived_ns, &b);
    }

    fn on_scan(&mut self, ev: &PuntEvent) {
        if self.scan.len() >= 64 && !self.scan.contains_key(&ev.ip_src) {
            return;
        }
        let now = ev.arrived_ns;
        let st = self.scan.entry(ev.ip_src).or_default();
        while st.seen.front().is_some_and(|(t, _)| now.saturating_sub(*t) > SCAN_WINDOW_NS) {
            let (_, d) = st.seen.pop_front().unwrap();
            if let Some(n) = st.dsts.get_mut(&d) {
                *n -= 1;
                if *n == 0 {
                    st.dsts.remove(&d);
                }
            }
        }
        if st.seen.len() < 1024 {
            st.seen.push_back((now, ev.ip_dst));
            *st.dsts.entry(ev.ip_dst).or_default() += 1;
        }
        let distinct = st.dsts.len();
        let start = st.seen.front().map(|(t, _)| *t).unwrap_or(now);
        let flag = if distinct >= 50 && !st.second_flagged {
            st.second_flagged = true;
            true
        } else if distinct >= 16 && !st.first_flagged {
            st.first_flagged = true;
            true
        } else {
            false
        };
        if flag {
            let target = self.cust_of(ev.ip_dst);
            let src_cust = self.cust_of(ev.ip_src);
            let mut b = vec![(ev.ip_src & 0xff) as u8, src_cust, target, distinct.min(255) as u8];
            b.extend_from_slice(&((start / 10_000_000) as u16).to_le_bytes());
            self.rec(T_SCAN_SUSPECT, now, &b);
        }
    }
}

impl SwitchProgram for A3Log {
    fn init_with(input: InitInput) -> (Self, ProgramSetup) {
        let customers: Vec<Customer> = input.customers.iter().copied().take(MAX_CUSTOMERS).collect();
        let lay = Layout::new(customers.len());
        let w = lay.w;
        let observe = input.observe.unwrap_or(ObserveLimits { punt_events_per_s: 80, punt_backlog: 64, export_bytes_per_s: 16_000, export_burst_bytes: 8_000 });
        // The logger also has to take in every report as an event, so it samples less itself.
        let masks = if input.switch_id == LOGGER { Masks::for_events((observe.punt_events_per_s / 3).max(1)) } else { Masks::for_events(observe.punt_events_per_s) };
        let rank = (input.switch_id as u64) % 15;
        // One report per slot: slow enough that all senders together stay within about half the logger's event budget,
        // and that a report of about 100 bytes fits the byte budget.
        let p_e = SENDERS / (0.3 * observe.punt_events_per_s.max(1) as f64);
        let p_b = 140.0 / (0.9 * observe.export_bytes_per_s.max(1) as f64);
        let slot_ns = (p_e.max(p_b).clamp(1.0, 8.0) * 1e9) as u64;
        let mut setup = ProgramSetup::new();
        for t in [1, 2, 3] {
            setup.declare_table(t, MatchKind::Lpm, customers.len().max(1) as u32);
        }
        // A table answers with the first entry that matches, so the longest prefixes go in first: a customer that is one host
        // inside another customer's range has to be found before the range.
        let mut order: Vec<usize> = (0..customers.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(customers[i].prefix_len));
        for i in order {
            let c = &customers[i];
            let entry = |id: u64, key: u32, value: u64| TableEntry {
                id: wire::EntryIdW(id),
                key: c.prefix_addr as u64,
                prefix_len: c.prefix_len,
                priority: 0,
                action: wire::TableActionW::SetMeta { key: wire::MetaKeyW(key), value },
            };
            // Customer i is row i + 1 and column i + 1; 0 means "not a customer".
            setup.add_entry(1, entry(i as u64 + 1, 0, (i + 1) as u64));
            setup.add_entry(3, entry(i as u64 + 1, 6, ((i + 1) * w) as u64));
            setup.add_entry(2, entry(i as u64 + 1, 1, (i + 1) as u64));
        }
        setup.declare_table(REPORT_TABLE, MatchKind::Exact, 4);
        setup.declare_register_array(0, (w * w) as u32);
        setup.declare_register_array(1, 4096);
        setup.declare_counter_array(ARR, lay.total() as u32);
        setup.set_program(text::parse_tiny_program(&program_text(&lay, masks)).expect("a3truth3 data plane"));
        let burst = observe.export_burst_bytes as f64;
        let me = Self {
            me: input.switch_id,
            is_logger: input.switch_id == LOGGER,
            switch_ports: input.local_ports.iter().copied().filter(|p| *p < APP_PORT).collect(),
            cust: vec![CustState::default(); customers.len()],
            customers,
            lay,
            period_ns: input.harness.map(|h| h.keepalive_period_ns).unwrap_or(TICK_NS),
            started: false,
            final_done: false,
            tick: 0,
            ports: HashMap::new(),
            prev: HashMap::new(),
            win: HashMap::new(),
            flow: HashMap::new(),
            slow: HashMap::new(),
            pwin: HashMap::new(),
            out_records: Vec::new(),
            recs_originated: 0,
            seen_recs: HashSet::new(),
            rx_from: [0; 16],
            written: 0,
            raw_sec: 0,
            raw_this: 0,
            raw_by: [0; 16],
            other_last_pkts: 0,
            other_rate: (0, 0),
            last_port_win_ns: 0,
            now: 0,
            pending: Vec::new(),
            pending_bytes: 0,
            last_send_ns: 0,
            tokens: burst,
            burst,
            rate: observe.export_bytes_per_s as f64,
            last_tok_ns: 0,
            observe,
            dist: distances(LOGGER as u8),
            ka_div: masks.ka + 1,
            ka_total: 0,
            ka_punts: 0,
            route_port: None,
            route_gen: 100,
            logger_entry: false,
            slot_ns,
            next_slot_ns: slot_ns * (rank + 1) / 15,
            urgent_gap_ns: (slot_ns / 4).max(URGENT_GAP_NS),
            final_ns: if input.switch_id == LOGGER { FINAL_NS } else { FINAL_BASE_NS + rank * FINAL_STEP_NS },
            scan: HashMap::new(),
            down_reports: HashMap::new(),
            silent_done: HashSet::new(),
        };
        (me, setup)
    }

    fn init(_switch_id: u32, _local_ports: Vec<u16>) -> (Self, ProgramSetup) {
        unreachable!("a3log is built with init_with")
    }

    fn on_punt(&mut self, ev: PuntEvent) -> Vec<Action> {
        let mut out = Vec::new();
        self.now = ev.now_ns;
        match ev.reason {
            PuntReason::Custom(1) => {
                if let Some(k) = keepalive(&ev.payload) {
                    self.ka_punts += 1;
                    let ps = self.ports.entry(ev.ingress_port).or_default();
                    ps.peer = Some(k.switch as u8);
                    ps.win_delay_sum += ev.arrived_ns.saturating_sub(k.sent_ns);
                    ps.win_delay_n += 1;
                }
            }
            PuntReason::Custom(2) => self.raw_sample(&ev),
            PuntReason::Custom(3) => {
                self.on_scan(&ev);
                // Small packets are rare and worth seeing whole, so they are raw samples too.
                self.raw_sample(&ev);
            }
            PuntReason::Custom(4) => self.on_report(&ev.payload, ev.ingress_port),
            _ => {}
        }
        self.flush(ev.now_ns, &mut out);
        out
    }

    fn on_timer(&mut self, ev: TimerEvent) -> Vec<Action> {
        let mut out = Vec::new();
        let now = ev.now_ns;
        self.now = now;
        if !self.started {
            self.started = true;
            if self.is_logger {
                out.push(actions::name_logging_switch());
                // What the real budgets are, since the practice world's are only a guess.
                let o = self.observe;
                let mut b = vec![EXT_OBSERVE];
                b.extend_from_slice(&(o.punt_events_per_s.min(u16::MAX as u32) as u16).to_le_bytes());
                b.extend_from_slice(&(o.punt_backlog.min(u16::MAX as u32) as u16).to_le_bytes());
                b.extend_from_slice(&(o.export_bytes_per_s.min(u32::MAX as u64) as u32).to_le_bytes());
                b.extend_from_slice(&(o.export_burst_bytes.min(u32::MAX as u64) as u32).to_le_bytes());
                self.out_records.push(Rec { ty: T_EXT, sw: self.me as u8, t10: 0, body: b }.encode());
            }
        }
        let cur = self.snapshot(&ev);
        self.ports_tick(now, &cur, &ev);
        self.customers_tick(now, &cur);
        if self.tick % WINDOW_TICKS == 0 && self.tick > 0 {
            self.window(now, &cur);
            self.win = cur.clone();
        }
        if self.tick % PORT_TICKS == 0 && self.tick > 0 {
            self.port_window(now, &cur);
            self.pwin = cur.clone();
        }
        if self.tick % FLOW_TICKS == 0 && self.tick > 0 {
            self.flow_window(now, &cur);
            self.flow = cur.clone();
        }
        if self.tick % SLOW_TICKS == 0 && self.tick > 0 {
            self.slow_window(now, &cur);
            self.slow = cur.clone();
        }
        if self.tick % SLOW_TICKS == 0 {
            self.heartbeat(now);
        }
        if now >= self.final_ns && !self.final_done {
            self.final_done = true;
            self.port_window(now, &cur);
            self.flow_window(now, &cur);
            self.finish_run(now);
        }
        self.prev = cur;
        self.tick += 1;
        self.route_update(&mut out);
        self.flush(now, &mut out);
        out.push(actions::schedule_timer(TICK_NS));
        out
    }
}

switch_program!(A3Log);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let r = Rec { ty: T_LINK_QUALITY, sw: 7, t10: 1720, body: vec![3, 20, 0x10, 0x00] };
        let mut log = r.encode();
        log.extend(Rec { ty: T_LINK_DOWN, sw: 2, t10: 5, body: vec![9] }.encode());
        let (recs, used) = parse(&log);
        assert_eq!(used, log.len());
        assert_eq!(recs[0], r);
        assert_eq!(recs[1].ty, T_LINK_DOWN);
    }

    #[test]
    fn every_layout_assembles_and_stays_small() {
        for n in [0, 1, 13, 23, 24, 40, 60] {
            let lay = Layout::new(n);
            text::parse_tiny_program(&program_text(&lay, Masks::for_events(80))).expect("parses");
            text::parse_tiny_program(&program_text(&lay, Masks::for_events(10))).expect("parses");
            // Most slots are zero (one byte each); a timer event has to fit in 4,096 bytes with room for the busy ones.
            assert!(lay.total() <= 1500, "{n} customers: {} slots", lay.total());
            assert!(lay.total() <= 4096);
        }
    }

    #[test]
    fn the_data_plane_assembles() {
        let t = program_text(&Layout::new(13), Masks::for_events(80));
        text::parse_tiny_program(&t).expect("parses");
    }
}
