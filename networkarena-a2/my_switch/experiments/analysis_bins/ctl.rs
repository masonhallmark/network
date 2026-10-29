use competitive_net_sim::sim_log::{self, LogEvent};
use std::collections::BTreeMap;
fn main(){
    let p=std::env::args().nth(1).unwrap();
    let frames=sim_log::writer::decode_all(&std::fs::read(p).unwrap()).unwrap();
    let mut proto:BTreeMap<u64,u8>=BTreeMap::new();
    for f in &frames { if let LogEvent::PacketIngress{packet,..}=&f.event { proto.insert(packet.id,packet.ip_proto); } }
    let mut cap:BTreeMap<u32,u64>=BTreeMap::new();
    let mut ctl:BTreeMap<u32,u64>=BTreeMap::new(); let mut all:BTreeMap<u32,u64>=BTreeMap::new();
    let mut end=0u64;
    for f in &frames { end=f.at_ns; match &f.event {
        LogEvent::LinkAdded{id,config,..}=>{cap.insert(*id,config.bandwidth_bps);}
        LogEvent::PacketEgress{link,packet_id,size_bytes,..}=>{ *all.entry(*link).or_default()+=size_bytes; if matches!(proto.get(packet_id),Some(89)|Some(90)) {*ctl.entry(*link).or_default()+=size_bytes;} }
        _=>{}}}
    let secs=end as f64/1e9;
    for (l,c) in &cap { let cb=*ctl.get(l).unwrap_or(&0) as f64*8.0/secs; println!("link {l}: cap {:.1} Mb/s  control {:.1} kb/s = {:.1}% of capacity  (all traffic {:.0}%)",*c as f64/1e6,cb/1e3,100.0*cb/ *c as f64, 100.0*(*all.get(l).unwrap_or(&0) as f64*8.0/secs)/ *c as f64); }
}
