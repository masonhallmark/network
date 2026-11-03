//! # broken_link — the practice egg we solve for you
//!
//! In Assignment 3 the harness forwards; your switch program watches,
//! reports and logs. This program, with `broken_link_detect`, wins the
//! practice egg "the broken link": partway through a run, link 1-2 goes
//! down and stays down. Read it end to end; it is the whole chain, from a
//! packet a switch notices to a `true` the detector gives.
//!
//! **What a switch sees.** The harness sends a keepalive out of every
//! switch-to-switch port every `keepalive_period_ns` (told at init, in
//! `input.harness`). A keepalive is a packet whose `kind` field reads
//! [`KIND_KEEPALIVE`] and whose payload says who sent it. The data plane
//! here punts every keepalive to the controller: a punt is a sample
//! through the switch's event budget (`punt_events_per_s`), and a switch
//! with a few ports gets most of its keepalives that way. A link that is
//! down carries nothing, so its keepalives stop. Noticing the quiet is the
//! measurement.
//!
//! **What a switch says.** On every timer the controller checks each port
//! it has heard a keepalive on. A port quiet for `QUIET` keepalive periods
//! produces one `SILENT` record (who went quiet, and when); a port that
//! speaks again produces one `HEARD` record. A record this switch makes is
//! written to the log if this is the logging switch, and otherwise sent as
//! a report to every neighbour, who passes it on to theirs: the flood from
//! `load_report`, under the export budget. Only the logging switch's
//! `log_write` counts, and the log has a cap, so every byte here is spent
//! on a change, never on a steady state.
//!
//! **The log.** A sequence of 9-byte records, little-endian:
//!
//! ```text
//! [tag u8: 1 SILENT, 2 HEARD][switch u8][neighbour u8][port u16][at_ms u32]
//! ```
//!
//! `broken_link_detect` reads them back. To go after another egg, keep
//! the shape and change what is measured: a customer's packets instead
//! of keepalives, a count instead of a silence.

use std::collections::{HashMap, HashSet};
use switch_program_sdk::*;

/// The switch that writes the log. Every other switch reports to it.
pub const LOGGER: u32 = 0;
/// The `ip_proto` of our reports; nothing else in the network uses it.
const PROTO_REPORT: u8 = 93;
const REPORT_IP: u32 = 0xe000_0007; // 224.0.0.7
/// A port is silent after this many keepalive periods without one.
const QUIET: u64 = 10;
const TICK_NS: u64 = 100_000_000; // 100 ms

pub const TAG_SILENT: u8 = 1;
pub const TAG_HEARD: u8 = 2;

/// One record: a port of `switch` toward `neighbour` went quiet (`silent`)
/// or spoke again, at `at_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub silent: bool,
    pub switch: u8,
    pub neighbour: u8,
    pub port: u16,
    pub at_ms: u32,
}

impl Record {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![if self.silent { TAG_SILENT } else { TAG_HEARD }, self.switch, self.neighbour];
        b.extend_from_slice(&self.port.to_le_bytes());
        b.extend_from_slice(&self.at_ms.to_le_bytes());
        b
    }

    /// Every whole record in `log`, in order.
    pub fn decode(log: &[u8]) -> Vec<Record> {
        log.chunks_exact(9)
            .filter(|c| c[0] == TAG_SILENT || c[0] == TAG_HEARD)
            .map(|c| Record {
                silent: c[0] == TAG_SILENT,
                switch: c[1],
                neighbour: c[2],
                port: u16::from_le_bytes([c[3], c[4]]),
                at_ms: u32::from_le_bytes([c[5], c[6], c[7], c[8]]),
            })
            .collect()
    }
}

pub struct BrokenLink {
    me: u32,
    local_ports: Vec<u16>,
    period_ns: u64,
    /// port -> (the switch on the other end, when we last heard it).
    heard: HashMap<u16, (u32, u64)>,
    silent: HashSet<u16>,
    started: bool,
    /// Our reports' sequence numbers, and every (origin, seq) seen, so a
    /// flooded report is passed on once.
    seq: u32,
    seen: HashSet<(u32, u32)>,
}

impl BrokenLink {
    /// A record this switch made: logged here, or reported to the logging
    /// switch by way of every neighbour.
    fn originate(&mut self, r: Record, out: &mut Vec<Action>) {
        if self.me == LOGGER {
            out.push(actions::log_write(r.encode()));
            return;
        }
        self.seq += 1;
        let mut p = self.me.to_le_bytes().to_vec();
        p.extend_from_slice(&self.seq.to_le_bytes());
        p.extend_from_slice(&r.encode());
        self.seen.insert((self.me, self.seq));
        self.flood(&p, None, out);
    }

    /// A report out of every port but the one it came in on.
    fn flood(&self, payload: &[u8], skip: Option<u16>, out: &mut Vec<Action>) {
        for port in self.local_ports.iter().filter(|p| Some(**p) != skip) {
            out.push(actions::inject_packet(*port, REPORT_IP, REPORT_IP, PROTO_REPORT, 1, payload.to_vec()));
        }
    }

    /// A report from another switch: `[origin u32][seq u32][record]`.
    fn on_report(&mut self, payload: &[u8], port: u16, out: &mut Vec<Action>) {
        if payload.len() < 8 {
            return;
        }
        let origin = u32::from_le_bytes(payload[0..4].try_into().unwrap());
        let seq = u32::from_le_bytes(payload[4..8].try_into().unwrap());
        if !self.seen.insert((origin, seq)) {
            return;
        }
        if self.me == LOGGER {
            out.push(actions::log_write(payload[8..].to_vec()));
        } else {
            self.flood(payload, Some(port), out);
        }
    }
}

impl SwitchProgram for BrokenLink {
    fn init_with(input: InitInput) -> (Self, ProgramSetup) {
        let (mut me, setup) = Self::init(input.switch_id, input.local_ports);
        if let Some(h) = input.harness {
            me.period_ns = h.keepalive_period_ns;
        }
        (me, setup)
    }

    fn init(switch_id: u32, local_ports: Vec<u16>) -> (Self, ProgramSetup) {
        let mut setup = ProgramSetup::new();
        // One stage: a keepalive is punted with reason 1, a report with
        // reason 2, and anything else goes on its way (the harness forwards
        // it; `bif` on a nonzero register is a jump).
        setup.set_program(
            text::parse_tiny_program(&format!(
                "
                stage alu=12 mem=1
                    load    r0, kind
                    const   r1, {keepalive}
                    eq      r2, r0, r1
                    bif     r2 -> 9
                    load    r3, ip_proto
                    const   r4, {report}
                    eq      r2, r3, r4
                    bif     r2 -> 11
                    bif     r1 -> 12
                    punt    1
                    bif     r1 -> 12
                    punt    2
                    noop
                ",
                keepalive = KIND_KEEPALIVE,
                report = PROTO_REPORT,
            ))
            .expect("broken_link program"),
        );
        let me = Self {
            me: switch_id,
            local_ports,
            period_ns: TICK_NS,
            heard: HashMap::new(),
            silent: HashSet::new(),
            started: false,
            seq: 0,
            seen: HashSet::new(),
        };
        (me, setup)
    }

    fn on_punt(&mut self, ev: PuntEvent) -> Vec<Action> {
        let mut out = Vec::new();
        if ev.ip_proto == PROTO_REPORT {
            self.on_report(&ev.payload, ev.ingress_port, &mut out);
        } else if let Some(k) = keepalive(&ev.payload) {
            // `arrived_ns` is when the packet reached the switch; `now_ns`
            // is later, when the punt came through to us.
            self.heard.insert(ev.ingress_port, (k.switch, ev.arrived_ns));
        }
        out
    }

    fn on_timer(&mut self, ev: TimerEvent) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            if self.me == LOGGER {
                out.push(actions::name_logging_switch());
            }
        }
        let quiet = QUIET * self.period_ns;
        let mut ports: Vec<u16> = self.heard.keys().copied().collect();
        ports.sort_unstable();
        for port in ports {
            let (neighbour, last) = self.heard[&port];
            let is_quiet = ev.now_ns.saturating_sub(last) >= quiet;
            if is_quiet == self.silent.contains(&port) {
                continue;
            }
            if is_quiet {
                self.silent.insert(port);
            } else {
                self.silent.remove(&port);
            }
            let at = if is_quiet { last } else { ev.now_ns };
            let r = Record { silent: is_quiet, switch: self.me as u8, neighbour: neighbour as u8, port, at_ms: (at / 1_000_000) as u32 };
            self.originate(r, &mut out);
        }
        out.push(actions::schedule_timer(TICK_NS));
        out
    }
}

switch_program!(BrokenLink);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_survives_the_log() {
        let r = Record { silent: true, switch: 1, neighbour: 2, port: 3, at_ms: 15_070 };
        let heard = Record { silent: false, ..r };
        let log = [r.encode(), heard.encode()].concat();
        assert_eq!(log.len(), 18);
        assert_eq!(Record::decode(&log), [r, heard]);
    }
}
