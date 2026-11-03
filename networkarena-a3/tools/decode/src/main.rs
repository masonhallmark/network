use competitive_net_sim::sim_log::{self, LogEvent, NodeRefW};
use std::collections::BTreeMap;
use std::fmt::Write;

fn ip(a: u32) -> String { format!("{}.{}.{}.{}", a >> 24, (a >> 16) & 255, (a >> 8) & 255, a & 255) }
fn node(n: &NodeRefW) -> String { match n { NodeRefW::Switch(i) => format!("S{i}"), NodeRefW::App(i) => format!("app{i}") } }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).unwrap();
    let frames = sim_log::writer::decode_all(&bytes).unwrap();
    let mut out = String::new();
    let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut per_sec: BTreeMap<(u64, String), u64> = BTreeMap::new();
    let mut drops: BTreeMap<String, u64> = BTreeMap::new();
    let mut sw_samples: BTreeMap<u32, u64> = BTreeMap::new();
    let mut sw_lost: BTreeMap<u32, u64> = BTreeMap::new();
    let mut proto_of: std::collections::HashMap<u64, u8> = Default::default();
    let mut drop_proto: BTreeMap<(String, u8), u64> = BTreeMap::new();
    let mut lost_cap: BTreeMap<(u32, u8), u64> = BTreeMap::new();
    let mut punt_kinds: BTreeMap<(u8, u8), u64> = BTreeMap::new();
    for f in &frames {
        let t = f.at_ns;
        let sec = t / 1_000_000_000;
        let line = match &f.event {
            LogEvent::PacketIngress { switch, port, packet } => {
                proto_of.insert(packet.id, packet.ip_proto);
                *punt_kinds.entry((packet.kind, packet.ip_proto)).or_default() += 1;
                format!("ingress   S{switch} p{port} id={:#x} kind={} proto={} {}->{} {}B ttl={} flow={} seq={:?}", packet.id, packet.kind, packet.ip_proto, ip(packet.ip_src), ip(packet.ip_dst), packet.size_bytes, packet.ip_ttl, packet.flow_id, packet.seq)
            }
            LogEvent::PacketEgress { switch, port, link, packet_id, size_bytes, arrive_at_ns } => format!("egress    S{switch} p{port} L{link} id={packet_id:#x} {size_bytes}B arrives@{arrive_at_ns}"),
            LogEvent::PacketDropped { at, reason, packet_id } => { *drop_proto.entry((format!("{:?}", reason), proto_of.get(packet_id).copied().unwrap_or(255))).or_default() += 1; *drops.entry(format!("{:?}", reason)).or_default() += 1; format!("DROP      at {} {:?} id={packet_id:#x}", node(at), reason) }
            LogEvent::DroppedAtPort { switch, port, packet_id } => format!("dropport  S{switch} p{port} id={packet_id:#x}"),
            LogEvent::PacketDelivered { app, packet_id } => format!("delivered app{app} id={packet_id:#x}"),
            LogEvent::PacketPunted { switch, reason, packet_id } => format!("punted    S{switch} reason={reason} id={packet_id:#x}"),
            LogEvent::PacketSampled { switch, reason, packet_id } => { *sw_samples.entry(*switch).or_default() += 1; format!("SAMPLE    S{switch} reason={reason} id={packet_id:#x}") }
            LogEvent::ObservationDropped { switch, cap, packet_id } => { *lost_cap.entry((*switch, *cap)).or_default() += 1; *sw_lost.entry(*switch).or_default() += 1; format!("LOST      S{switch} cap={cap} id={packet_id:#x}") }
            LogEvent::LinkFailed { id } => format!("LINK L{id} FAILED"),
            LogEvent::LinkRestored { id } => format!("LINK L{id} restored"),
            LogEvent::LoggingSwitchNamed { switch, accepted } => format!("logging switch S{switch} accepted={accepted}"),
            LogEvent::TeamLogWritten { switch, bytes } => format!("logwrite  S{switch} {}B", bytes.len()),
            LogEvent::TeamLogRefused { switch, len, why } => format!("LOGREFUSED S{switch} {len}B why={why}"),
            other => format!("{:?}", other).chars().take(110).collect(),
        };
        let name = line.split_whitespace().next().unwrap().to_string();
        *kinds.entry(name.clone()).or_default() += 1;
        *per_sec.entry((sec, name)).or_default() += 1;
        writeln!(out, "{t:>12} {line}").unwrap();
    }
    std::fs::write(&a[2], &out).unwrap();
    println!("== frame counts ==");
    for (k, v) in &kinds { println!("{k:12} {v}"); }
    println!("== drops by reason ==");
    for (k, v) in &drops { println!("{k:12} {v}"); }
    println!("== samples (events delivered to controller) per switch ==");
    for (k, v) in &sw_samples { println!("S{k}: {v}   lost to budget: {}", sw_lost.get(k).unwrap_or(&0)); }
    println!("== drops by (reason, ip_proto) =="); for (k,v) in &drop_proto { println!("{k:?} {v}"); }
    println!("== observation drops by (switch, cap 0=events 1=export 2=size) =="); for (k,v) in &lost_cap { println!("{k:?} {v}"); }
    println!("== ingress by (packet kind, ip_proto) ==");
    for (k, v) in &punt_kinds { println!("{k:?} {v}"); }
    println!("== per-second: ingress/delivered/SAMPLE/LOST/DROP ==");
    for s in (0..40).step_by(1) {
        let g = |n: &str| per_sec.get(&(s, n.to_string())).copied().unwrap_or(0);
        println!("t={s:>2}s ingress={:>6} deliv={:>5} sample={:>4} lost={:>4} drop={:>4}", g("ingress"), g("delivered"), g("SAMPLE"), g("LOST"), g("DROP"));
    }
}
