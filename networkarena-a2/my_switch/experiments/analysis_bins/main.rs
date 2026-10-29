use competitive_net_sim::sim_log::{self, LogEvent};
use std::collections::BTreeMap;
fn main(){
    let p=std::env::args().nth(1).unwrap();
    let frames=sim_log::writer::decode_all(&std::fs::read(p).unwrap()).unwrap();
    // (kind, proto, size, src/24) -> count at ingress
    let mut m:BTreeMap<(u8,u8,u64,String),u64>=BTreeMap::new();
    for f in &frames { if let LogEvent::PacketIngress{packet,..}=&f.event {
        let s=packet.ip_src; let src=format!("{}.{}.{}.x",s>>24,(s>>16)&255,(s>>8)&255);
        *m.entry((packet.kind,packet.ip_proto,packet.size_bytes,src)).or_default()+=1; }}
    println!("kind proto size src  count");
    for ((k,pr,sz,src),c) in m { println!("{k} {pr} {sz} {src} {c}"); }
}
