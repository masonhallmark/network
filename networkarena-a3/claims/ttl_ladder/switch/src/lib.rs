//! # ttl_switch: the claim switch for the TTL-ladder egg
//!
//! Truth runs 1 and 2 both showed one customer at switch 4 (its range is named Mapper) sending 64-byte UDP packets whose TTL cycles through
//! small values (raw samples 3, 4, 5, 6, 7; its profile says every packet has TTL under 10): a traceroute-like ladder. That switch is the logging
//! switch here, so nothing crosses the network (in truth run 2 most reports never arrived): its data plane counts every data packet that
//! enters from a customer port by TTL (31 or more share one slot), and its controller writes what it counted.
//!
//! Log, little-endian:
//! - tag 1, once a second in which some packet arrived with TTL 0-9: `[1][second u8][count of TTL 0..16 in that second, u16 each]` = 34 bytes.
//! - tag 2, once at about 39.3 s: `[2][packets with TTL 0-9 u32][all counted packets u32]` = 9 bytes.
use switch_program_sdk::*;

/// The logging switch: the one that sees the ladder. Every other switch installs nothing and stays idle.
pub const LOGGER: u32 = 4;
pub const TAG_SECOND: u8 = 1;
pub const TAG_TOTAL: u8 = 2;
const SLOTS: u32 = 32;
const TICK_NS: u64 = 100_000_000;
const FINAL_NS: u64 = 39_300_000_000;

pub struct TtlSwitch {
    me: u32,
    started: bool,
    final_done: bool,
    tick: u64,
    prev: Vec<u64>,
}

/// The data plane: from a customer port (port id 100 or more), a data packet adds one to the slot of its TTL, `min(ttl, 31)`.
fn program() -> TinyProgram {
    text::parse_tiny_program(
        "stage alu=20 mem=1
        load r0, ingress_port
        const r1, 100
        lt r2, r0, r1
        bif r2 -> 16
        load r3, kind
        const r4, 0
        eq r5, r3, r4
        const r6, 1
        sub r7, r6, r5
        bif r7 -> 16
        load r8, ip_ttl
        const r9, 31
        lt r10, r8, r9
        bif r10 -> 15
        const r8, 31
        counter c0[r8], r6
        noop",
    )
    .expect("ttl_switch data plane")
}

impl SwitchProgram for TtlSwitch {
    fn init_with(input: InitInput) -> (Self, ProgramSetup) {
        let mut setup = ProgramSetup::new();
        if input.switch_id == LOGGER {
            setup.declare_counter_array(0, SLOTS);
            setup.set_program(program());
        }
        (Self { me: input.switch_id, started: false, final_done: false, tick: 0, prev: vec![0; SLOTS as usize] }, setup)
    }

    fn init(_switch_id: u32, _local_ports: Vec<u16>) -> (Self, ProgramSetup) {
        unreachable!("built with init_with")
    }

    fn on_timer(&mut self, ev: TimerEvent) -> Vec<Action> {
        if self.me != LOGGER {
            return Vec::new();
        }
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            out.push(actions::name_logging_switch());
        }
        self.tick += 1;
        let cur: Vec<u64> = ev.counters.iter().find(|c| c.id.0 == 0).map(|c| c.values.clone()).unwrap_or_default();
        let at = |v: &Vec<u64>, i: usize| v.get(i).copied().unwrap_or(0);
        if self.tick % 10 == 0 {
            let delta: Vec<u64> = (0..SLOTS as usize).map(|i| at(&cur, i).saturating_sub(self.prev[i])).collect();
            if delta[..10].iter().any(|d| *d > 0) {
                let mut r = vec![TAG_SECOND, (ev.now_ns / 1_000_000_000).min(255) as u8];
                for i in 0..16 {
                    r.extend_from_slice(&(delta[i].min(u16::MAX as u64) as u16).to_le_bytes());
                }
                out.push(actions::log_write(r));
            }
            self.prev = (0..SLOTS as usize).map(|i| at(&cur, i)).collect();
        }
        if ev.now_ns >= FINAL_NS && !self.final_done {
            self.final_done = true;
            let low: u64 = (0..10).map(|i| at(&cur, i)).sum();
            let all: u64 = (0..SLOTS as usize).map(|i| at(&cur, i)).sum();
            let mut r = vec![TAG_TOTAL];
            r.extend_from_slice(&(low.min(u32::MAX as u64) as u32).to_le_bytes());
            r.extend_from_slice(&(all.min(u32::MAX as u64) as u32).to_le_bytes());
            out.push(actions::log_write(r));
        }
        out.push(actions::schedule_timer(TICK_NS));
        out
    }
}

switch_program!(TtlSwitch);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_plane_assembles_and_init_goes_through_for_every_switch() {
        let _ = program();
        for sw in 0..15u32 {
            let input = InitInput { switch_id: sw, local_ports: vec![0, 1, 2, 100], port_capacity_bps: vec![], customers: vec![], observe: None, harness: None };
            let (mut me, _) = TtlSwitch::init_with(input);
            let ev = TimerEvent { now_ns: 1_000_000, port_drops: vec![], counters: vec![] };
            let _ = me.on_timer(ev);
        }
    }
}
