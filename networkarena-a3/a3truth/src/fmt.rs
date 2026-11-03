//! The log record format, shared by the switch program and the tools that read the log.
//!
//! ```text
//! byte 0     [ type:4 | switch:4 ]
//! bytes 1-2  t10: u16 LE, observation time in 10 ms ticks
//! bytes 3..  body, its length fixed by the type
//! ```

pub const T_HEARTBEAT: u8 = 0;
pub const T_LINK_DOWN: u8 = 1;
pub const T_LINK_UP: u8 = 2;
pub const T_LINK_QUALITY: u8 = 3;
pub const T_PORT_STATS: u8 = 4;
pub const T_CUSTOMER_RATE: u8 = 5;
pub const T_CUSTOMER_GAP: u8 = 6;
pub const T_SCAN_SUSPECT: u8 = 7;
pub const T_TOP_TALKER: u8 = 8;
pub const T_SWITCH_SILENT: u8 = 9;
pub const T_CUSTOMER_PROFILE: u8 = 10;
pub const T_FLOW_LOSS: u8 = 11;
pub const T_BUDGET_STATS: u8 = 12;
pub const T_RAW_SAMPLE: u8 = 13;
pub const T_RAW_KEEPALIVE: u8 = 14;
pub const T_ESCAPE: u8 = 15;

pub const HEADER: usize = 3;

pub const NAMES: [&str; 16] = [
    "HEARTBEAT", "LINK_DOWN", "LINK_UP", "LINK_QUALITY", "PORT_STATS", "CUSTOMER_RATE", "CUSTOMER_GAP",
    "SCAN_SUSPECT", "TOP_TALKER", "SWITCH_SILENT", "CUSTOMER_PROFILE", "FLOW_LOSS", "BUDGET_STATS",
    "RAW_SAMPLE", "RAW_KEEPALIVE", "ESCAPE",
];

/// Body length of a type; `None` for the escape type, which this version never writes.
pub fn body_len(ty: u8) -> Option<usize> {
    Some(match ty {
        T_HEARTBEAT => 4,        // reports_sent:u16, events_lost_est:u16 (both cumulative)
        T_LINK_DOWN => 1,        // peer
        T_LINK_UP => 1,          // peer
        T_LINK_QUALITY => 4,     // peer, loss_pct, delay_100us:u16
        T_PORT_STATS => 6,       // peer, drops:u16, kbytes_out:u16, ka_lost:u8 (keepalives that did not arrive in this 2 s window)
        T_CUSTOMER_RATE => 5,    // cust (255 = not a customer, e.g. a peer), pkts:u16, kbytes:u16
        T_CUSTOMER_GAP => 5,     // cust, start_t10:u16, dur_t10:u16
        T_SCAN_SUSPECT => 6,     // src_host, src_cust, target_cust, distinct, window_start_t10:u16
        T_TOP_TALKER => 2,       // cust, share_pct
        T_SWITCH_SILENT => 1,    // switch
        T_CUSTOMER_PROFILE => 10, // cust, avg_size/8, udp_pct, idle_pct, rate_bucket, burst_cv32, ttl_lt60_pct, ttl_lt30_pct, ttl_lt10_pct, ack_pct
        T_FLOW_LOSS => 8,        // src_cust, dst_cust (255 = not a customer), entered:u16, left:u16, dups:u16
        T_BUDGET_STATS => 4,     // events_lost:u16, reports_refused:u16
        T_RAW_SAMPLE => 12,      // src:u32, dst:u32, proto, size:u16, ttl
        T_RAW_KEEPALIVE => 4,    // port, peer, delay_100us:u16
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rec {
    pub ty: u8,
    pub sw: u8,
    pub t10: u16,
    pub body: Vec<u8>,
}

impl Rec {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(HEADER + self.body.len());
        b.push((self.ty << 4) | (self.sw & 0x0F));
        b.extend_from_slice(&self.t10.to_le_bytes());
        b.extend_from_slice(&self.body);
        b
    }
}

/// Every whole record at the front of `buf`, and how many bytes they took.
pub fn parse(buf: &[u8]) -> (Vec<Rec>, usize) {
    let mut out = Vec::new();
    let mut at = 0;
    while at + HEADER <= buf.len() {
        let ty = buf[at] >> 4;
        let Some(n) = body_len(ty) else { break };
        if at + HEADER + n > buf.len() {
            break;
        }
        out.push(Rec {
            ty,
            sw: buf[at] & 0x0F,
            t10: u16::from_le_bytes([buf[at + 1], buf[at + 2]]),
            body: buf[at + HEADER..at + HEADER + n].to_vec(),
        });
        at += HEADER + n;
    }
    (out, at)
}

pub fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
