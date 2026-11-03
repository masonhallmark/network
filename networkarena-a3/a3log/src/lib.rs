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
pub const LOGGER: u32 = 7;

const APP_PORT: u16 = 100;
const PROTO_REPORT: u8 = 93;
const PROTO_BEACON: u8 = 94;
const REPORT_IP: u32 = 0xe000_0007; // 224.0.0.7
/// Non-logger switches send a report at most this often, or sooner for urgent records.
const SEND_EVERY_NS: u64 = 1_000_000_000;
const URGENT_EVERY_NS: u64 = 250_000_000;
const BEACON_TICKS: u64 = 10;
const PENDING_CAP: usize = 64;
const TICK_NS: u64 = 100_000_000;
const WINDOW_TICKS: u64 = 10; // 1 s
const FLOW_TICKS: u64 = 20; // 2 s
const SLOW_TICKS: u64 = 50; // 5 s
const FINAL_NS: u64 = 39_300_000_000;
const QUIET_PERIODS: u64 = 10;
const GAP_NS: u64 = 1_000_000_000;
const SCAN_WINDOW_NS: u64 = 5_000_000_000;
const SMALL_PKT: u64 = 128;
const KA_MASK: u64 = 15;
const RAW_MASK: u64 = 63;
const MAX_CUSTOMERS: usize = 44;
const REPORT_CHUNK: usize = 300;
/// Raw samples stop once this many bytes are in the log (of 64 KB in a truth run).
const RAW_LIMIT: usize = 40_000;
const PROFILE_TICKS: [u64; 2] = [50, 300];

const C_ENT_PKTS: u32 = 0;
const C_ENT_BYTES: u32 = 1;
const C_LEFT: u32 = 2;
const C_KA: u32 = 3;
const C_CLASS: u32 = 4;

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

fn program_text() -> String {
    let from_switch = "load r3, ingress_port\nconst r4, 100\nlt r5, r3, r4\nconst r6, 1";
    let mut p = String::new();
    p += &stage("stage alu=4 mem=1", "load r0, ip_src\ntable t1, r0 -> m10\nnoop");
    p += &stage("stage alu=4 mem=1", "load r0, ip_dst\ntable t2, r0 -> m11\nnoop");
    // idx = src_row + dst_col, kept in m2; packets entered from a customer, counted by packets.
    p += &stage(
        "stage alu=12 mem=1",
        &format!(
            "loadm r0, m0\nloadm r1, m1\nadd r2, r0, r1\nstorem m2, r2\n{from_switch}\nbif r5 -> @end\ncounter c{C_ENT_PKTS}[r2], r6\n@end:\nnoop"
        ),
    );
    p += &stage(
        "stage alu=12 mem=1",
        &format!("loadm r2, m2\n{from_switch}\nbif r5 -> @end\nload r7, size\ncounter c{C_ENT_BYTES}[r2], r7\n@end:\nnoop"),
    );
    // Left toward a customer port: harness_egress is port + 1, so more than 100 means a customer port.
    p += &stage(
        "stage alu=12 mem=1",
        &format!(
            "loadm r2, m2\nload r9, harness_egress\nconst r10, 100\nlt r11, r10, r9\nconst r12, 1\nsub r13, r12, r11\nbif r13 -> @end\ncounter c{C_LEFT}[r2], r12\n@end:\nnoop"
        ),
    );
    p += &stage(
        "stage alu=12 mem=1",
        &format!(
            "load r0, kind\nconst r1, {KIND_KEEPALIVE}\neq r2, r0, r1\nconst r3, 1\nsub r4, r3, r2\nbif r4 -> @end\nload r5, ingress_port\ncounter c{C_KA}[r5], r3\n@end:\nnoop"
        ),
    );
    p += &stage(
        "stage alu=16 mem=1",
        &format!(
            "loadm r2, m2\n{from_switch}\nbif r5 -> @end\nload r7, kind\nconst r8, 4\neq r9, r7, r8\ncounter c{C_CLASS}[r2], r9\nload r10, ip_proto\nconst r11, 17\neq r12, r10, r11\nloadm r13, m3\nadd r14, r2, r13\ncounter c{C_CLASS}[r14], r12\n@end:\nnoop"
        ),
    );
    p += &stage(
        "stage alu=32 mem=0",
        &format!(
            "load r0, ip_proto
const r1, {PROTO_REPORT}
eq r2, r0, r1
const r3, 1
sub r4, r3, r2
bif r4 -> @beacon
punt 4
@beacon:
load r0, ip_proto
const r1, {PROTO_BEACON}
eq r2, r0, r1
sub r4, r3, r2
bif r4 -> @ka
punt 5
@ka:
load r0, kind
const r1, {KIND_KEEPALIVE}
eq r2, r0, r1
load r5, seq
const r6, {KA_MASK}
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
sub r23, r3, r22
bif r23 -> @raw
punt 3
@raw:
const r24, {RAW_MASK}
and r25, r5, r24
eq r26, r25, r8
and r27, r18, r26
sub r28, r3, r27
bif r28 -> @end
punt 2
@end:
noop"
        ),
    );
    p
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
    win_delay_sum: u64,
    win_delay_n: u64,
    drops: u32,
    last_delay: Option<u16>,
}

#[derive(Default, Clone)]
struct CustState {
    last_active_ns: u64,
    seen: bool,
    gap_open: bool,
    gap_start_ns: u64,
    last_win_pkts: u64,
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
    w: usize,
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
    cust: Vec<CustState>,

    out_records: Vec<Vec<u8>>,
    report_seq: u16,
    reports_sent: u16,
    seen_reports: HashSet<(u8, u16)>,
    rx_from: [u16; 16],
    written: usize,
    /// Records waiting for the next report, and when the last one left.
    pending: Vec<Vec<u8>>,
    last_send_ns: u64,
    urgent: bool,
    /// Tree toward the logger: what each neighbour port last said its distance was, and the newest beacon.
    cand: HashMap<u16, (u8, u64)>,
    last_beacon: Option<u16>,
    beacon_seq: u16,

    ka_total: u64,
    ka_punts: u64,
    scan: HashMap<u32, ScanState>,
    /// Logger only: who said a link toward a peer went down, to tell a dead switch from a dead link.
    down_reports: HashMap<u8, Vec<(u8, u16)>>,
    silent_done: HashSet<u8>,
}

impl A3Log {
    fn rec(&mut self, ty: u8, t_ns: u64, body: &[u8]) {
        let r = Rec { ty, sw: self.me as u8, t10: (t_ns / 10_000_000).min(u16::MAX as u64) as u16, body: body.to_vec() };
        self.accept(r);
    }

    /// A record this switch made or received: queued for the log or for a report.
    fn accept(&mut self, r: Rec) {
        if self.is_logger {
            self.watch(&r);
        }
        self.out_records.push(r.encode());
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

    /// Hand the queued records to the log (logging switch) or queue them for the next report.
    fn flush(&mut self, now: u64, out: &mut Vec<Action>) {
        let recs = std::mem::take(&mut self.out_records);
        if self.is_logger {
            for r in recs {
                // Exploration records give way once the log is mostly full, so the rest keeps its room.
                let raw = matches!(r[0] >> 4, T_RAW_SAMPLE | T_RAW_KEEPALIVE);
                if raw && self.written >= RAW_LIMIT {
                    continue;
                }
                self.written += r.len();
                out.push(actions::log_write(r));
            }
            return;
        }
        for r in recs {
            if matches!(r[0] >> 4, T_LINK_DOWN | T_LINK_UP | T_SCAN_SUSPECT | T_CUSTOMER_GAP) {
                self.urgent = true;
            }
            if self.pending.len() >= PENDING_CAP {
                // Out of room before a parent was known: keep the newest, and never the raw ones first.
                if let Some(i) = self.pending.iter().position(|p| matches!(p[0] >> 4, T_RAW_SAMPLE | T_RAW_KEEPALIVE)) {
                    self.pending.remove(i);
                } else {
                    self.pending.remove(0);
                }
            }
            self.pending.push(r);
        }
        let since = now.saturating_sub(self.last_send_ns);
        let due = since >= SEND_EVERY_NS || (self.urgent && since >= URGENT_EVERY_NS) || self.final_done;
        if self.pending.is_empty() || !due || self.parent(None).is_none() {
            return;
        }
        let recs = std::mem::take(&mut self.pending);
        self.urgent = false;
        self.last_send_ns = now;
        let mut payload: Vec<u8> = Vec::new();
        let mut batches: Vec<Vec<u8>> = Vec::new();
        for r in recs {
            if !payload.is_empty() && payload.len() + r.len() > REPORT_CHUNK {
                batches.push(std::mem::take(&mut payload));
            }
            payload.extend_from_slice(&r);
        }
        if !payload.is_empty() {
            batches.push(payload);
        }
        for body in batches {
            self.report_seq = self.report_seq.wrapping_add(1);
            let mut p = vec![self.me as u8];
            p.extend_from_slice(&self.report_seq.to_le_bytes());
            p.extend_from_slice(&body);
            self.seen_reports.insert((self.me as u8, self.report_seq));
            self.reports_sent = self.reports_sent.wrapping_add(1);
            self.send_up(&p, None, out);
        }
    }

    /// The port toward the logger: the live neighbour that said it is closest, not `avoid`.
    fn parent(&self, avoid: Option<u16>) -> Option<u16> {
        self.cand
            .iter()
            .filter(|(p, _)| Some(**p) != avoid && !self.ports.get(*p).is_some_and(|s| s.down))
            .min_by_key(|(p, (h, _))| (*h, **p))
            .map(|(p, _)| *p)
    }

    fn send_up(&self, payload: &[u8], avoid: Option<u16>, out: &mut Vec<Action>) {
        if let Some(port) = self.parent(avoid) {
            out.push(actions::inject_packet(port, REPORT_IP, REPORT_IP, PROTO_REPORT, 1, payload.to_vec()));
        }
    }

    /// A beacon is `[seq u16][sender's distance from the logger u8]`, flooded once a second from the logger.
    fn on_beacon(&mut self, payload: &[u8], port: u16, now: u64, out: &mut Vec<Action>) {
        if payload.len() < 3 || self.is_logger {
            return;
        }
        let seq = u16::from_le_bytes([payload[0], payload[1]]);
        let hops = payload[2];
        self.cand.insert(port, (hops, now));
        let newer = self.last_beacon.is_none_or(|l| seq.wrapping_sub(l) < 0x8000 && seq != l);
        if newer {
            self.last_beacon = Some(seq);
            let mine = hops.saturating_add(1);
            let mut p = seq.to_le_bytes().to_vec();
            p.push(mine);
            for q in self.switch_ports.iter().filter(|q| **q != port) {
                out.push(actions::inject_packet(*q, REPORT_IP, REPORT_IP, PROTO_BEACON, 1, p.clone()));
            }
        }
    }

    fn on_report(&mut self, payload: &[u8], port: u16, out: &mut Vec<Action>) {
        if payload.len() < 3 {
            return;
        }
        let origin = payload[0];
        let seq = u16::from_le_bytes([payload[1], payload[2]]);
        if !self.seen_reports.insert((origin, seq)) {
            return;
        }
        if self.is_logger {
            let (recs, _) = parse(&payload[3..]);
            for r in recs {
                self.accept(r);
            }
            // Counted after its records, so a heartbeat compares against the reports sent before it.
            self.rx_from[(origin & 15) as usize] = self.rx_from[(origin & 15) as usize].wrapping_add(1);
        } else {
            self.send_up(payload, Some(port), out);
        }
    }

    fn cust_of(&self, ip: u32) -> u8 {
        self.customers
            .iter()
            .take(MAX_CUSTOMERS)
            .position(|c| {
                let bits = c.prefix_len as u32;
                let mask = if bits == 0 { 0 } else if bits >= 32 { u32::MAX } else { !0u32 << (32 - bits) };
                (ip & mask) == (c.prefix_addr & mask)
            })
            .map(|i| i as u8)
            .unwrap_or(255)
    }

    fn snapshot(&self, ev: &TimerEvent) -> HashMap<u32, Vec<u64>> {
        ev.counters.iter().map(|c| (c.id.0, c.values.clone())).collect()
    }

    fn at(map: &HashMap<u32, Vec<u64>>, id: u32, i: usize) -> u64 {
        map.get(&id).and_then(|v| v.get(i)).copied().unwrap_or(0)
    }

    fn delta(cur: &HashMap<u32, Vec<u64>>, old: &HashMap<u32, Vec<u64>>, id: u32, i: usize) -> u64 {
        Self::at(cur, id, i).saturating_sub(Self::at(old, id, i))
    }

    /// Sum over a customer's row (everything it sent) of one counter's change.
    fn row_delta(&self, cur: &HashMap<u32, Vec<u64>>, old: &HashMap<u32, Vec<u64>>, id: u32, c: usize) -> u64 {
        let base = (c + 1) * self.w;
        (0..self.w).map(|d| Self::delta(cur, old, id, base + d)).sum()
    }

    fn ports_tick(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>, ev: &TimerEvent) {
        for d in &ev.port_drops {
            self.ports.entry(d.port).or_default().drops += d.dropped;
        }
        let ports: Vec<u16> = self.switch_ports.clone();
        for port in ports {
            let count = Self::at(cur, C_KA, port as usize);
            let (new, down_now, last_rise, peer) = {
                let ps = self.ports.entry(port).or_default();
                let new = count.saturating_sub(ps.last_count);
                ps.last_count = count;
                ps.win_got += new;
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
            let active = self.row_delta(cur, &self.prev, C_ENT_PKTS, c) > 0;
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
            let (heard, down, got, dsum, dn, last_delay, drops, peer) = {
                let ps = self.ports.entry(port).or_default();
                let r = (ps.heard, ps.down, ps.win_got, ps.win_delay_sum, ps.win_delay_n, ps.last_delay, ps.drops, ps.peer.unwrap_or(255));
                ps.win_got = 0;
                ps.win_delay_sum = 0;
                ps.win_delay_n = 0;
                ps.drops = 0;
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
            if drops > 0 {
                let mut b = vec![peer];
                b.extend_from_slice(&(drops.min(u16::MAX as u32) as u16).to_le_bytes());
                self.rec(T_PORT_DROPS, now, &b);
            }
        }
        // Customer rates.
        for c in 0..self.customers.len().min(MAX_CUSTOMERS) {
            let pkts = self.row_delta(cur, &self.win, C_ENT_PKTS, c);
            let bytes = self.row_delta(cur, &self.win, C_ENT_BYTES, c);
            let prev = self.cust[c].last_win_pkts;
            self.cust[c].last_win_pkts = pkts;
            if pkts > 0 || prev > 0 {
                let mut b = vec![c as u8];
                b.extend_from_slice(&(pkts.min(u16::MAX as u64) as u16).to_le_bytes());
                b.extend_from_slice(&((bytes / 1000).min(u16::MAX as u64) as u16).to_le_bytes());
                self.rec(T_CUSTOMER_RATE, now, &b);
            }
        }
    }

    fn flow_window(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>) {
        let n = self.customers.len().min(MAX_CUSTOMERS);
        for s in 0..n {
            for d in 0..n {
                let idx = (s + 1) * self.w + d + 1;
                let ent = Self::delta(cur, &self.flow, C_ENT_PKTS, idx);
                let left = Self::delta(cur, &self.flow, C_LEFT, idx);
                if ent > 0 || left > 0 {
                    let mut b = vec![s as u8, d as u8];
                    b.extend_from_slice(&(ent.min(u16::MAX as u64) as u16).to_le_bytes());
                    b.extend_from_slice(&(left.min(u16::MAX as u64) as u16).to_le_bytes());
                    self.rec(T_FLOW_LOSS, now, &b);
                }
            }
        }
    }

    fn slow_window(&mut self, now: u64, cur: &HashMap<u32, Vec<u64>>) {
        let n = self.customers.len().min(MAX_CUSTOMERS);
        let mut total_bytes = 0u64;
        let mut best: Option<(u64, usize)> = None;
        let profile = PROFILE_TICKS.contains(&self.tick);
        for c in 0..n {
            let pkts = self.row_delta(cur, &self.slow, C_ENT_PKTS, c);
            let bytes = self.row_delta(cur, &self.slow, C_ENT_BYTES, c);
            total_bytes += bytes;
            if best.is_none_or(|(b, _)| bytes > b) && bytes > 0 {
                best = Some((bytes, c));
            }
            if profile && pkts > 0 {
                let mut acks = 0;
                let mut udp = 0;
                let base = (c + 1) * self.w;
                for d in 0..self.w {
                    acks += Self::delta(cur, &self.slow, C_CLASS, base + d);
                    udp += Self::delta(cur, &self.slow, C_CLASS, self.w * self.w + base + d);
                }
                let secs = (SLOW_TICKS * TICK_NS / 1_000_000_000).max(1);
                let rate = pkts / secs;
                let bucket = (64 - (rate + 1).leading_zeros()) as u8;
                let b = [c as u8, (bytes / pkts / 8).min(255) as u8, (udp * 100 / pkts).min(100) as u8, (acks * 100 / pkts).min(100) as u8, bucket];
                self.rec(T_CUSTOMER_PROFILE, now, &b);
            }
        }
        if let Some((bytes, c)) = best {
            self.rec(T_TOP_TALKER, now, &[c as u8, (bytes * 100 / total_bytes.max(1)) as u8]);
        }
    }

    fn heartbeat(&mut self, now: u64) {
        let expected = self.ka_total / (KA_MASK + 1);
        let lost = expected.saturating_sub(self.ka_punts).min(u16::MAX as u64) as u16;
        let mut b = self.reports_sent.to_le_bytes().to_vec();
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
        let w = customers.len() + 1;
        let ww = (w * w) as u32;
        let mut setup = ProgramSetup::new();
        setup.declare_table(1, MatchKind::Lpm, customers.len().max(1) as u32);
        setup.declare_table(2, MatchKind::Lpm, customers.len().max(1) as u32);
        for (i, c) in customers.iter().enumerate() {
            let entry = |id: u64, key: u32, value: u64| TableEntry {
                id: wire::EntryIdW(id),
                key: c.prefix_addr as u64,
                prefix_len: c.prefix_len,
                priority: 0,
                action: wire::TableActionW::SetMeta { key: wire::MetaKeyW(key), value },
            };
            // Row of the source customer, column of the destination customer; 0 means "no customer".
            setup.add_entry(1, entry(i as u64 + 1, 0, ((i + 1) * w) as u64));
            setup.add_entry(2, entry(i as u64 + 1, 1, (i + 1) as u64));
        }
        setup.declare_counter_array(C_ENT_PKTS, ww);
        setup.declare_counter_array(C_ENT_BYTES, ww);
        setup.declare_counter_array(C_LEFT, ww);
        setup.declare_counter_array(C_KA, 128);
        setup.declare_counter_array(C_CLASS, 2 * ww);
        // m3 carries the offset of the UDP half of c4, set by a table-free constant stage below.
        let mut text = program_text();
        text = text.replacen("stage alu=4 mem=1\nload r0, ip_dst", &format!("stage alu=4 mem=1\nconst r9, {ww}\nstorem m3, r9\nload r0, ip_dst"), 1);
        setup.set_program(text::parse_tiny_program(&text).expect("a3log data plane"));
        let me = Self {
            me: input.switch_id,
            is_logger: input.switch_id == LOGGER,
            switch_ports: input.local_ports.iter().copied().filter(|p| *p < APP_PORT).collect(),
            cust: vec![CustState::default(); customers.len()],
            customers,
            w,
            period_ns: input.harness.map(|h| h.keepalive_period_ns).unwrap_or(TICK_NS),
            started: false,
            final_done: false,
            tick: 0,
            ports: HashMap::new(),
            prev: HashMap::new(),
            win: HashMap::new(),
            flow: HashMap::new(),
            slow: HashMap::new(),
            out_records: Vec::new(),
            report_seq: 0,
            reports_sent: 0,
            seen_reports: HashSet::new(),
            rx_from: [0; 16],
            written: 0,
            pending: Vec::new(),
            last_send_ns: 0,
            urgent: false,
            cand: HashMap::new(),
            last_beacon: None,
            beacon_seq: 0,
            ka_total: 0,
            ka_punts: 0,
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
            PuntReason::Custom(2) => {
                let mut b = ev.ip_src.to_le_bytes().to_vec();
                b.extend_from_slice(&ev.ip_dst.to_le_bytes());
                b.push(ev.ip_proto);
                b.extend_from_slice(&(ev.packet_size.min(u16::MAX as u64) as u16).to_le_bytes());
                b.push(ev.ip_ttl);
                self.rec(T_RAW_SAMPLE, ev.arrived_ns, &b);
            }
            PuntReason::Custom(3) => self.on_scan(&ev),
            PuntReason::Custom(4) => self.on_report(&ev.payload, ev.ingress_port, &mut out),
            PuntReason::Custom(5) => self.on_beacon(&ev.payload, ev.ingress_port, ev.now_ns, &mut out),
            _ => {}
        }
        self.flush(ev.now_ns, &mut out);
        out
    }

    fn on_timer(&mut self, ev: TimerEvent) -> Vec<Action> {
        let mut out = Vec::new();
        let now = ev.now_ns;
        if !self.started {
            self.started = true;
            if self.is_logger {
                out.push(actions::name_logging_switch());
            }
        }
        let cur = self.snapshot(&ev);
        self.ports_tick(now, &cur, &ev);
        self.customers_tick(now, &cur);
        if self.tick % WINDOW_TICKS == 0 && self.tick > 0 {
            self.window(now, &cur);
            self.win = cur.clone();
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
        if now >= FINAL_NS && !self.final_done {
            self.final_done = true;
            self.finish_run(now);
        }
        self.prev = cur;
        if self.is_logger && self.tick % BEACON_TICKS == 0 {
            self.beacon_seq = self.beacon_seq.wrapping_add(1);
            let mut p = self.beacon_seq.to_le_bytes().to_vec();
            p.push(0);
            for q in &self.switch_ports {
                out.push(actions::inject_packet(*q, REPORT_IP, REPORT_IP, PROTO_BEACON, 1, p.clone()));
            }
        }
        self.tick += 1;
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
    fn the_data_plane_assembles() {
        let t = program_text();
        text::parse_tiny_program(&t).expect("parses");
    }
}
