//! # flap_switch: the claim switch for "link 1-6 flaps"
//!
//! Switch 6 is the logging switch and one end of link 1-6, so no report crosses the network (in truth run 2 most reports never arrived).
//! Every other switch installs nothing and stays idle.
//!
//! Data plane (switch 6): a counter array with one slot per ingress port counts keepalives (free of the event budget). One keepalive in 4
//! is punted so the controller learns which switch is on each port (the keepalive payload names its sender).
//!
//! Controller (switch 6), every 100 ms: a port whose keepalive count has not risen for `QUIET` keepalive periods is silent. Each change
//! writes a 7-byte record `[tag u8][port u8][peer u8, 255 = unknown][at_ms u32 LE]`:
//! tag 1 = port went silent (at = when its last keepalive was heard), tag 2 = heard again (at = when we noticed).
use std::collections::HashMap;
use switch_program_sdk::*;

pub const LOGGER: u32 = 6;
pub const TAG_SILENT: u8 = 1;
pub const TAG_HEARD: u8 = 2;
const QUIET: u64 = 5; // keepalive periods without one (0.5 s at the real 100 ms period)
const SAMPLE_MASK: u32 = 3; // punt a keepalive when seq & mask == 0 (about 10 events a second of the 80 allowed)
const TICK_NS: u64 = 100_000_000;
const PORTS: u32 = 64;

pub struct FlapSwitch {
    me: u32,
    period_ns: u64,
    started: bool,
    peer: HashMap<u16, u8>,
    last_count: Vec<u64>,
    last_rise: Vec<Option<u64>>,
    silent: Vec<bool>,
}

fn program() -> TinyProgram {
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
    .expect("flap_switch data plane")
}

impl SwitchProgram for FlapSwitch {
    fn init_with(input: InitInput) -> (Self, ProgramSetup) {
        let mut setup = ProgramSetup::new();
        let me = input.switch_id;
        if me == LOGGER {
            setup.declare_counter_array(0, PORTS);
            setup.set_program(program());
        }
        let period_ns = input.harness.map(|h| h.keepalive_period_ns).unwrap_or(TICK_NS);
        let n = PORTS as usize;
        (Self { me, period_ns, started: false, peer: HashMap::new(), last_count: vec![0; n], last_rise: vec![None; n], silent: vec![false; n] }, setup)
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
        for port in 0..PORTS as usize {
            let n = counts.get(port).copied().unwrap_or(0);
            let rose = n > self.last_count[port];
            self.last_count[port] = n;
            let peer = self.peer.get(&(port as u16)).copied().unwrap_or(255);
            if rose {
                self.last_rise[port] = Some(now);
                if self.silent[port] {
                    self.silent[port] = false;
                    out.push(actions::log_write(record(TAG_HEARD, port as u8, peer, now)));
                }
            } else if let Some(last) = self.last_rise[port] {
                if !self.silent[port] && now.saturating_sub(last) >= QUIET * self.period_ns {
                    self.silent[port] = true;
                    out.push(actions::log_write(record(TAG_SILENT, port as u8, peer, last)));
                }
            }
        }
        out.push(actions::schedule_timer(TICK_NS));
        out
    }
}

fn record(tag: u8, port: u8, peer: u8, at_ns: u64) -> Vec<u8> {
    let mut r = vec![tag, port, peer];
    r.extend_from_slice(&((at_ns / 1_000_000) as u32).to_le_bytes());
    r
}

switch_program!(FlapSwitch);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_data_plane_assembles_and_init_goes_through_for_every_switch() {
        let _ = program();
        for sw in 0..15u32 {
            let input = InitInput { switch_id: sw, local_ports: vec![0, 1, 2, 100], port_capacity_bps: vec![], customers: vec![], observe: None, harness: None };
            let (mut me, _) = FlapSwitch::init_with(input);
            let _ = me.on_timer(TimerEvent { now_ns: 1_000_000, port_drops: vec![], counters: vec![] });
        }
    }
    #[test]
    fn record_layout() {
        assert_eq!(record(1, 2, 3, 5_910_000_000), vec![1, 2, 3, 0x16, 0x17, 0, 0]);
    }
}
