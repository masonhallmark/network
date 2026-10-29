use competitive_net_sim::sim_log::{self, LogEvent};
use std::collections::BTreeMap;
fn main(){
    let p=std::env::args().nth(1).unwrap();
    let frames=sim_log::writer::decode_all(&std::fs::read(p).unwrap()).unwrap();
    let mut path:BTreeMap<u64,Vec<(u64,u32)>>=BTreeMap::new();
    let mut info:BTreeMap<u64,(u8,u8,u32,u32)>=BTreeMap::new();
    for f in &frames { if let LogEvent::PacketIngress{switch,packet,..}=&f.event {
        path.entry(packet.id).or_default().push((f.at_ns,*switch));
        info.insert(packet.id,(packet.kind,packet.ip_proto,packet.ip_src,packet.ip_dst)); }}
    let mut n=0;
    for (id,v) in &path { let mut seen=std::collections::HashSet::new(); let mut l=false;
        for (_,s) in v { if !seen.insert(*s){l=true;} }
        if l { n+=1; let (k,pr,s,d)=info[id];
            println!("t={:.3}s pkt{} kind{} proto{} {}.{}.{}.x->{}.{}.{}.x path {:?}", v[0].0 as f64/1e9,id,k,pr,s>>24&255,s>>16&255,s>>8&255,d>>24&255,d>>16&255,d>>8&255, v.iter().map(|x|x.1).collect::<Vec<_>>()); } }
    println!("{n} looping packets");
}
