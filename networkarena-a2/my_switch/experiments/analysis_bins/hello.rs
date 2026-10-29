use competitive_net_sim::sim_log::{self, LogEvent};
use std::collections::BTreeMap;
fn main(){
    let p=std::env::args().nth(1).unwrap();
    let frames=sim_log::writer::decode_all(&std::fs::read(p).unwrap()).unwrap();
    let mut last:BTreeMap<(u32,u16),u64>=BTreeMap::new();
    let mut maxgap:BTreeMap<(u32,u16),(u64,u64)>=BTreeMap::new();
    let mut n:BTreeMap<(u32,u16),u64>=BTreeMap::new();
    for f in &frames { if let LogEvent::PacketIngress{switch,port,packet}=&f.event { if packet.ip_proto==89 {
        let k=(*switch,*port); *n.entry(k).or_default()+=1;
        if let Some(l)=last.get(&k){ let g=f.at_ns-l; let e=maxgap.entry(k).or_insert((0,0)); if g>e.0 {*e=(g,f.at_ns);} }
        last.insert(k,f.at_ns);}}}
    for (k,(g,t)) in maxgap { println!("switch {} port {}: hellos {} max gap {:.1} ms at t={:.3}s",k.0,k.1,n[&k],g as f64/1e6,t as f64/1e9); }
}
