//! Read a team log written by a3log: size per type, fill over time, and the records themselves.
//! usage: logdump <team.log> [cap_bytes] [--all]
use a3truth3::fmt::*;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let log = std::fs::read(&a[1]).expect("read log");
    let cap: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(65536);
    let all = a.iter().any(|s| s == "--all");
    let (recs, used) = parse(&log);
    println!("{} bytes of {} cap ({:.1}%), {} records, {} trailing bytes unparsed", log.len(), cap, 100.0 * log.len() as f64 / cap as f64, recs.len(), log.len() - used);
    let mut by_type: BTreeMap<u8, (usize, usize)> = BTreeMap::new();
    let mut by_sec: BTreeMap<u16, usize> = BTreeMap::new();
    for r in &recs {
        let e = by_type.entry(r.ty).or_default();
        e.0 += 1;
        e.1 += HEADER + r.body.len();
        *by_sec.entry(r.t10 / 100).or_default() += HEADER + r.body.len();
    }
    println!("\n{:<18}{:>8}{:>9}{:>7}", "type", "records", "bytes", "%log");
    for (t, (n, b)) in &by_type {
        println!("{:<18}{:>8}{:>9}{:>6.1}%", NAMES[*t as usize], n, b, 100.0 * *b as f64 / log.len().max(1) as f64);
    }
    println!("\nbytes by observation second (cumulative, as a claim would need <= 24576):");
    let mut cum = 0;
    for s in 0..40u16 {
        let b = by_sec.get(&s).copied().unwrap_or(0);
        cum += b;
        print!("{s:>2}s {b:>5} {cum:>6}   ");
        if s % 3 == 2 { println!(); }
    }
    println!();
    println!("\nrecords (non-raw unless --all):");
    for r in &recs {
        if !all && matches!(r.ty, T_RAW_SAMPLE | T_RAW_KEEPALIVE | T_CUSTOMER_RATE | T_FLOW_LOSS | T_PORT_STATS) {
            continue;
        }
        println!("{:>7.2}s S{:<2} {:<17} {}", r.t10 as f64 / 100.0, r.sw, NAMES[r.ty as usize], show(r));
    }
}

fn show(r: &Rec) -> String {
    let b = &r.body;
    match r.ty {
        T_HEARTBEAT => format!("records_originated={} events_lost_est={}", u16_at(b, 0), u16_at(b, 2)),
        T_EXT if b[0] == EXT_OBSERVE => format!("OBSERVE events_per_s={} backlog={} export_B_per_s={} burst_B={}", u16_at(b, 1), u16_at(b, 3), u32::from_le_bytes([b[5], b[6], b[7], b[8]]), u32::from_le_bytes([b[9], b[10], b[11], b[12]])),
        T_EXT => format!("ext {}", b[0]),
        T_LINK_DOWN | T_LINK_UP => format!("peer=S{}", b[0]),
        T_LINK_QUALITY => format!("peer=S{} loss={}% delay={:.1}ms", b[0], b[1], u16_at(b, 2) as f64 / 10.0),
        T_PORT_STATS => format!("peer={} drops={} kB_out={} ka_lost={}", b[0], u16_at(b, 1), u16_at(b, 3), b[5]),
        T_CUSTOMER_RATE => format!("cust={} pkts={} kB={}", b[0], u16_at(b, 1), u16_at(b, 3)),
        T_CUSTOMER_GAP => format!("cust={} start={:.2}s dur={:.2}s", b[0], u16_at(b, 1) as f64 / 100.0, u16_at(b, 3) as f64 / 100.0),
        T_SCAN_SUSPECT => format!("src_host=.{} src_cust={} target_cust={} distinct={} window_start={:.2}s", b[0], b[1], b[2], b[3], u16_at(b, 4) as f64 / 100.0),
        T_TOP_TALKER => format!("cust={} share={}%", b[0], b[1]),
        T_SWITCH_SILENT => format!("switch=S{}", b[0]),
        T_CUSTOMER_PROFILE => format!("cust={} avg_size={}B udp={}% idle={}% rate_bucket={} burst_cv={:.2} ttl<60={}% <30={}% <10={}% ack={}%", b[0], b[1] as u32 * 8, b[2], b[3], b[4], b[5] as f64 / 32.0, b[6], b[7], b[8], b[9]),
        T_FLOW_LOSS => format!("{}->{} entered={} left={} dups={}", b[0], b[1], u16_at(b, 2), u16_at(b, 4), u16_at(b, 6)),
        T_BUDGET_STATS => format!("events_lost={} reports_refused={}", u16_at(b, 0), u16_at(b, 2)),
        T_RAW_SAMPLE => format!("{}.{}.{}.{} -> {}.{}.{}.{} proto={} size={} ttl={}", b[3], b[2], b[1], b[0], b[7], b[6], b[5], b[4], b[8], u16_at(b, 9), b[11]),
        T_RAW_KEEPALIVE => format!("port={} peer=S{} delay={:.1}ms", b[0], b[1], u16_at(b, 2) as f64 / 10.0),
        _ => String::new(),
    }
}
