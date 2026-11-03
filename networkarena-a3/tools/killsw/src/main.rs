//! usage: killsw <world> <program.wasm> <load> <switch> <at_ms> <cpu|switch> <out.team>
use competitive_net_sim::sim::Simulator;
use competitive_net_sim::types::{OwnerId, SwitchId};
use competitive_net_sim::wasm::WasmLimits;
use std::time::Duration;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let world = competitive_net_sim::world::World::load(std::path::Path::new(&a[1])).unwrap();
    let load: f64 = a[3].parse().unwrap();
    let sw: u32 = a[4].parse().unwrap();
    let at: u64 = a[5].parse().unwrap();
    let mut sim = Simulator::new();
    sim.observe = world.observe.as_ref().map(|o| o.limits());
    let handles = world.build_with_load(&mut sim, load).unwrap();
    let bytes = std::fs::read(&a[2]).unwrap();
    for as_id in &handles.as_ids {
        sim.install_program(OwnerId::new(*as_id), &bytes, WasmLimits::default()).unwrap();
    }
    let mut ids: Vec<u32> = world.switches.iter().map(|s| s.id).collect();
    ids.sort_unstable();
    for id in ids {
        sim.schedule_controller_timer(SwitchId::new(id), Duration::ZERO);
    }
    sim.run_until(Duration::from_millis(at));
    if a[6] == "cpu" { sim.fail_cpu(SwitchId::new(sw)); } else { sim.fail_switch(SwitchId::new(sw)); }
    sim.run_until(world.params.duration());
    std::fs::write(&a[7], sim.team_log().unwrap()).unwrap();
    println!("done, team log {} bytes", sim.team_log().unwrap().len());
}
