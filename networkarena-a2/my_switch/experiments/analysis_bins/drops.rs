use competitive_net_sim::sim_log::{self, LogEvent};
use std::collections::BTreeMap;
fn main(){
    let p=std::env::args().nth(1).unwrap();
    let t0:f64=std::env::args().nth(2).map(|s|s.parse().unwrap()).unwrap_or(0.0);
    let t1:f64=std::env::args().nth(3).map(|s|s.parse().unwrap()).unwrap_or(1e9);
    let frames=sim_log::writer::decode_all(&std::fs::read(p).unwrap()).unwrap();
    let mut info:BTreeMap<u64,(u8,u8,u64)>=BTreeMap::new();
    let mut cnt:BTreeMap<String,u64>=BTreeMap::new();
    let mut first:BTreeMap<String,f64>=BTreeMap::new();
    for f in &frames {
        let t=f.at_ns as f64/1e9;
        match &f.event {
            LogEvent::PacketIngress{packet,..}=>{info.insert(packet.id,(packet.kind,packet.ip_proto,packet.size_bytes));}
            LogEvent::PacketDropped{at,reason,packet_id}=>{ if t<t0||t>t1 {continue;}
                let (k,pr,sz)=info.get(packet_id).copied().unwrap_or((255,255,0));
                let key=format!("kind{k} proto{pr} size{sz} {:?} at {:?}",reason,at);
                *cnt.entry(key.clone()).or_default()+=1; first.entry(key).or_insert(t);}
            _=>{}
        }}
    for (k,c) in cnt { println!("{c:6}  first@{:.3}s  {k}",first[&k]); }
}
