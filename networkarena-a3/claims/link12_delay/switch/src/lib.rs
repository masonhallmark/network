//! # delay_switch: the claim switch for "link 1-2 has a slow direction"
//!
//! Truth run 2 showed switch 1 hearing switch 2 at exactly 30.0 ms while switch 2 heard switch 1 at 2.0 ms (every other link 0.5 ms):
//! the simulator's `link_delay` plant, with a different delay each way. Switch 1 is the logging switch and one end of the link; switch 2 is the
//! other end. Every other switch installs nothing and stays idle.
//!
//! Every keepalive carries when it left its sender, so arrival minus that is the one-way delay of the link in that direction.
//! Each of the two switches measures the direction toward itself, per one-second window: how many keepalives, the smallest delay (queueing only adds
//! to it) and the mean. Switch 2 sends its windows to switch 1 in a one-hop report (the last two windows in each, so one lost report costs nothing);
//! switch 1 writes both directions. Log records, 7 bytes: `[tag][second u8][count u8][min u16 LE][mean u16 LE]`, delays in units of 100 us.
//! Tag 1 = delay of switch 2 -> switch 1 (measured at switch 1); tag 2 = delay of switch 1 -> switch 2 (measured at switch 2).
use std::collections::{HashMap, HashSet};
use switch_program_sdk::*;

/// The logging switch, and one end of the link.
pub const LOGGER: u32 = 1;
/// The other end.
pub const OTHER: u32 = 2;
pub const TAG_TO_LOGGER: u8 = 1;
pub const TAG_FROM_LOGGER: u8 = 2;
const PROTO_REPORT: u8 = 93;
const REPORT_IP: u32 = 0xe000_0007; // 224.0.0.7
const TICK_NS: u64 = 100_000_000;
const WINDOW_NS: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub sec: u8,
    pub n: u8,
    pub min: u16,
    pub mean: u16,
}

impl Window {
    pub fn encode(&self, tag: u8) -> Vec<u8> {
        let mut r = vec![tag, self.sec, self.n];
        r.extend_from_slice(&self.min.to_le_bytes());
        r.extend_from_slice(&self.mean.to_le_bytes());
        r
    }
    fn body(&self) -> [u8; 6] {
        let e = self.encode(0);
        [e[1], e[2], e[3], e[4], e[5], e[6]]
    }
    fn from_body(b: &[u8]) -> Window {
        Window { sec: b[0], n: b[1], min: u16::from_le_bytes([b[2], b[3]]), mean: u16::from_le_bytes([b[4], b[5]]) }
    }
}

#[derive(Default)]
struct Acc {
    sec: u64,
    n: u64,
    min: u64,
    sum: u64,
}

pub struct DelaySwitch {
    me: u32,
    started: bool,
    /// Which switch is on each port, from its keepalives.
    peer: HashMap<u16, u32>,
    acc: Option<Acc>,
    /// Finished windows of the direction this switch measures; the last two go in every report.
    recent: Vec<Window>,
    /// Logger: seconds already written for the other direction.
    written: HashSet<u8>,
}

fn program() -> TinyProgram {
    // A keepalive is punted with reason 1, a report with reason 2, anything else goes on its way.
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
    .expect("delay_switch data plane")
}

fn to_100us(ns: u64) -> u16 {
    (ns / 100_000).min(u16::MAX as u64) as u16
}

impl DelaySwitch {
    fn finish(&mut self, out: &mut Vec<Action>) {
        let Some(a) = self.acc.take() else { return };
        if a.n == 0 {
            return;
        }
        let w = Window { sec: a.sec.min(255) as u8, n: a.n.min(255) as u8, min: to_100us(a.min), mean: to_100us(a.sum / a.n) };
        if self.me == LOGGER {
            out.push(actions::log_write(w.encode(TAG_TO_LOGGER)));
        }
        self.recent.push(w);
        if self.recent.len() > 2 {
            self.recent.remove(0);
        }
        if self.me == OTHER {
            if let Some((&port, _)) = self.peer.iter().find(|(_, p)| **p == LOGGER) {
                let mut payload = vec![self.recent.len() as u8];
                for w in &self.recent {
                    payload.extend_from_slice(&w.body());
                }
                out.push(actions::inject_packet(port, REPORT_IP, REPORT_IP, PROTO_REPORT, 1, payload));
            }
        }
    }
}

impl SwitchProgram for DelaySwitch {
    fn init_with(input: InitInput) -> (Self, ProgramSetup) {
        let mut setup = ProgramSetup::new();
        let me = input.switch_id;
        if me == LOGGER || me == OTHER {
            setup.set_program(program());
        }
        (Self { me, started: false, peer: HashMap::new(), acc: None, recent: Vec::new(), written: HashSet::new() }, setup)
    }

    fn init(_switch_id: u32, _local_ports: Vec<u16>) -> (Self, ProgramSetup) {
        unreachable!("built with init_with")
    }

    fn on_punt(&mut self, ev: PuntEvent) -> Vec<Action> {
        let mut out = Vec::new();
        if ev.ip_proto == PROTO_REPORT {
            if self.me == LOGGER && !ev.payload.is_empty() {
                let k = ev.payload[0] as usize;
                for c in ev.payload[1..].chunks_exact(6).take(k) {
                    let w = Window::from_body(c);
                    if self.written.insert(w.sec) {
                        out.push(actions::log_write(w.encode(TAG_FROM_LOGGER)));
                    }
                }
            }
        } else if let Some(k) = keepalive(&ev.payload) {
            self.peer.insert(ev.ingress_port, k.switch);
            let other = if self.me == LOGGER { OTHER } else { LOGGER };
            if k.switch == other {
                let sec = ev.arrived_ns / WINDOW_NS;
                if self.acc.as_ref().is_some_and(|a| a.sec != sec) {
                    self.finish(&mut out);
                }
                let d = ev.arrived_ns.saturating_sub(k.sent_ns);
                let a = self.acc.get_or_insert(Acc { sec, n: 0, min: u64::MAX, sum: 0 });
                a.n += 1;
                a.min = a.min.min(d);
                a.sum += d;
            }
        }
        out
    }

    fn on_timer(&mut self, ev: TimerEvent) -> Vec<Action> {
        if self.me != LOGGER && self.me != OTHER {
            return Vec::new(); // idle: no timers, no reports
        }
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            if self.me == LOGGER {
                out.push(actions::name_logging_switch());
            }
        }
        if self.acc.as_ref().is_some_and(|a| a.sec < ev.now_ns / WINDOW_NS) {
            self.finish(&mut out);
        }
        out.push(actions::schedule_timer(TICK_NS));
        out
    }
}

switch_program!(DelaySwitch);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_round_trips() {
        let w = Window { sec: 7, n: 9, min: 300, mean: 312 };
        let e = w.encode(1);
        assert_eq!(e.len(), 7);
        assert_eq!(Window::from_body(&e[1..]), w);
        assert_eq!(w.body().to_vec(), e[1..].to_vec());
    }
    #[test]
    fn data_plane_assembles_and_init_goes_through_for_every_switch() {
        let _ = program();
        for sw in 0..15u32 {
            let input = InitInput { switch_id: sw, local_ports: vec![0, 1, 2, 100], port_capacity_bps: vec![], customers: vec![], observe: None, harness: None };
            let (mut me, _) = DelaySwitch::init_with(input);
            let _ = me.on_timer(TimerEvent { now_ns: 1_000_000, port_drops: vec![], counters: vec![] });
        }
    }
}
