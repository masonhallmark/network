//! # a3link16: a switch program that watches link 1-6 and nothing else
//!
//! Switch 6 is the logging switch and one end of link 1-6, so no report ever crosses the network
//! (reports cost export budget and are dropped when it is small). Every other switch installs
//! nothing and stays idle.
//!
//! Data plane (switch 6): a counter array, one slot per ingress port, counts keepalives (free of
//! the event budget). One keepalive in `SAMPLE` is punted so the controller learns who is on
//! each port (the keepalive payload names its sender).
//!
//! Controller (switch 6): on every timer, a port whose keepalive count has not risen for `QUIET`
//! periods is silent; the port whose peer is switch 1 is link 1-6. Each change writes a 5-byte
//! record: `[tag u8][at_ms u32 LE]`, tag 1 = silent (at = last keepalive heard), 2 = heard again
//! (at = when we noticed).
use std::collections::HashMap;
use switch_program_sdk::*;

pub const LOGGER: u32 = 6;
pub const PEER: u8 = 1;
pub const TAG_SILENT: u8 = 1;
pub const TAG_HEARD: u8 = 2;
const QUIET: u64 = 10; // keepalive periods without one
const SAMPLE_MASK: u32 = 15; // punt a keepalive when seq & mask == 0
const TICK_NS: u64 = 100_000_000;
const PORTS: u32 = 64;

pub struct Link16 {
    me: u32,
    period_ns: u64,
    started: bool,
    peer: HashMap<u16, u8>,
    last_count: HashMap<u16, u64>,
    last_rise: HashMap<u16, u64>,
    silent: bool,
}

impl SwitchProgram for Link16 {
    fn init_with(input: InitInput) -> (Self, ProgramSetup) {
        let mut setup = ProgramSetup::new();
        let me = input.switch_id;
        if me == LOGGER {
            setup.declare_counter_array(0, PORTS);
            setup.set_program(
                text::parse_tiny_program(&format!(
                    "stage alu=12 mem=1
                    load r0, kind
                    const r1, {ka}
                    eq r2, r0, r1
                    const r3, 1
                    sub r4, r3, r2
                    bif r4 -> 8
                    load r5, ingress_port
                    counter c0[r5], r3
                    noop
                    stage alu=12 mem=0
                    load r0, kind
                    const r1, {ka}
                    eq r2, r0, r1
                    load r5, seq
                    const r6, {mask}
                    and r7, r5, r6
                    const r8, 0
                    eq r9, r7, r8
                    and r10, r2, r9
                    const r11, 1
                    sub r12, r11, r10
                    bif r12 -> 13
                    punt 1
                    noop",
                    ka = KIND_KEEPALIVE,
                    mask = SAMPLE_MASK
                ))
                .expect("a3link16 program"),
            );
        }
        let period_ns = input.harness.map(|h| h.keepalive_period_ns).unwrap_or(TICK_NS);
        (
            Self { me, period_ns, started: false, peer: HashMap::new(), last_count: HashMap::new(), last_rise: HashMap::new(), silent: false },
            setup,
        )
    }

    fn init(_switch_id: u32, _local_ports: Vec<u16>) -> (Self, ProgramSetup) {
        unreachable!("built with init_with")
    }

    fn on_punt(&mut self, ev: PuntEvent) -> Vec<Action> {
        if let Some(k) = keepalive(&ev.payload) {
            self.peer.insert(ev.ingress_port, k.switch as u8);
        }
        Vec::new()
    }

    fn on_timer(&mut self, ev: TimerEvent) -> Vec<Action> {
        if self.me != LOGGER {
            return Vec::new(); // idle: no timers, no reports
        }
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            out.push(actions::name_logging_switch());
        }
        let now = ev.now_ns;
        let counts: Vec<u64> = ev.counters.iter().find(|c| c.id.0 == 0).map(|c| c.values.clone()).unwrap_or_default();
        for (port, &n) in counts.iter().enumerate() {
            let port = port as u16;
            let prev = self.last_count.insert(port, n).unwrap_or(0);
            if n > prev {
                self.last_rise.insert(port, now);
            }
        }
        if let Some((&port, _)) = self.peer.iter().find(|(_, p)| **p == PEER) {
            if let Some(&last) = self.last_rise.get(&port) {
                let is_quiet = now.saturating_sub(last) >= QUIET * self.period_ns;
                if is_quiet != self.silent {
                    self.silent = is_quiet;
                    let (tag, at) = if is_quiet { (TAG_SILENT, last) } else { (TAG_HEARD, now) };
                    let mut r = vec![tag];
                    r.extend_from_slice(&((at / 1_000_000) as u32).to_le_bytes());
                    out.push(actions::log_write(r));
                }
            }
        }
        out.push(actions::schedule_timer(TICK_NS));
        out
    }
}

switch_program!(Link16);
