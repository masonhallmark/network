use competitive_net_sim::sim_log::{self, LogEvent};
use std::collections::BTreeMap;
fn main(){
    let a:Vec<String>=std::env::args().collect();
    let frames=sim_log::writer::decode_all(&std::fs::read(&a[1]).unwrap()).unwrap();
    let pre:u32=u32::from_str_radix(&a[2],16).unwrap(); // src prefix e.g. 0a00xx00
    let bin:u64=a[3].parse::<u64>().unwrap()*1_000_000; let t1:u64=a[4].parse::<u64>().unwrap()*1_000_000;
    let mut seen=std::collections::HashSet::new();
    let mut m:BTreeMap<u64,u64>=BTreeMap::new();
    for f in &frames { if f.at_ns>t1 {break;} if let LogEvent::PacketIngress{packet,..}=&f.event { if packet.kind==0 && packet.size_bytes>0 && packet.ip_src&0xffffff00==pre && seen.insert(packet.id) { *m.entry(f.at_ns/bin).or_default()+=1; }}}
    for (k,v) in m { println!("{:>6} ms: {} pkts sent", k*bin/1_000_000, v); }
}
