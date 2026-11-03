//! Wire types and TinyVM text parser shared between the simulator host and
//! the `switch_program_sdk`.
//!
//! All on-the-wire structs are postcard-serializable and `no_std`-friendly so
//! a switch program compiled to `wasm32-unknown-unknown` can encode and decode
//! them with the same code the simulator uses.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub mod text;
pub mod bgp;

// ---------- Identity ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortIdW(pub u16);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QueueIdW(pub u16);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TableIdW(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegisterArrayIdW(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CounterArrayIdW(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryIdW(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MetaKeyW(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegW(pub u8);

// ---------- Packet fields ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PacketFieldW {
    IpSrc,
    IpDst,
    IpProto,
    IpTtl,
    IpDscp,
    SrcPort,
    DstPort,
    AppId,
    FlowId,
    Custom(u8),
    Size,
    Kind,
    LabelTop,
    LabelDepth,
    /// The local port the packet arrived on. Read-only; a store is
    /// ignored.
    IngressPort,
    /// In a run where the harness forwards, the port the harness will send
    /// this packet out of, plus one (0 = none). Always 0 otherwise.
    /// Read-only.
    HarnessEgress,
    /// The sender's sequence number for closed-loop traffic, and the
    /// acknowledged one on an ack; 0 when the packet has none. Read-only.
    Seq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PuntReasonW {
    NoRoute,
    TtlExpired,
    Custom(u32),
}

// ---------- TinyVM ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchKindW {
    Exact,
    Lpm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstrW {
    LoadField { dst: RegW, field: PacketFieldW },
    StoreField { field: PacketFieldW, src: RegW },
    LoadMeta { dst: RegW, key: MetaKeyW },
    StoreMeta { key: MetaKeyW, src: RegW },
    Const { dst: RegW, value: u64 },
    Add { dst: RegW, a: RegW, b: RegW },
    Sub { dst: RegW, a: RegW, b: RegW },
    And { dst: RegW, a: RegW, b: RegW },
    Or  { dst: RegW, a: RegW, b: RegW },
    Xor { dst: RegW, a: RegW, b: RegW },
    Eq  { dst: RegW, a: RegW, b: RegW },
    Lt  { dst: RegW, a: RegW, b: RegW },
    TableLookup { table_id: TableIdW, key_reg: RegW, result_meta: MetaKeyW },
    RegisterRead { array: RegisterArrayIdW, index: RegW, dst: RegW },
    RegisterWrite { array: RegisterArrayIdW, index: RegW, src: RegW },
    CounterAdd { counter: CounterArrayIdW, index: RegW, value: RegW },
    BranchIf { cond: RegW, target: u32 },
    Drop,
    Punt { reason: PuntReasonW },
    SetEgress { port: PortIdW },
    SetQueue { queue: QueueIdW },
    Recirculate,
    Noop,
    PushLabel { label: u32 },
    PopLabel,
    SwapLabel { label: u32 },
    /// Mark the packet so that wherever it terminates, a TraceReply is
    /// magically delivered to *this* switch on the egress port the data
    /// plane chose. The reply consumes link bandwidth normally.
    MarkTrace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageProgramW {
    pub instrs: Vec<InstrW>,
    pub max_alu_ops: u32,
    pub max_memory_accesses: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TinyProgramW {
    pub stages: Vec<StageProgramW>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TableActionW {
    SetMeta { key: MetaKeyW, value: u64 },
    SetEgress { port: PortIdW },
    SetQueue { queue: QueueIdW },
    Drop,
    Punt { reason: PuntReasonW },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableEntryW {
    pub id: EntryIdW,
    pub key: u64,
    pub prefix_len: u8,
    pub priority: i32,
    pub action: TableActionW,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDeclW {
    pub id: TableIdW,
    pub kind: MatchKindW,
    pub max_entries: u32,
    pub initial_entries: Vec<TableEntryW>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDeclW {
    pub id: RegisterArrayIdW,
    pub size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterDeclW {
    pub id: CounterArrayIdW,
    pub size: u32,
}

/// What `init` returns: the data plane's program plus state declarations.
/// The simulator takes ownership and installs them on the switch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProgramSetupW {
    pub program: TinyProgramW,
    pub tables: Vec<TableDeclW>,
    pub registers: Vec<RegisterDeclW>,
    pub counters: Vec<CounterDeclW>,
}

// ---------- Controller events / actions ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitInputW {
    pub switch_id: u32,
    /// Local port ids that have a link attached, filled in from the
    /// topology. Which port leads where is not said, and nothing here
    /// implies what a program should do with them.
    pub local_ports: Vec<u16>,
    /// The capacity of the link on each port, in bits per second, in the
    /// same order as `local_ports`. A switch knows how fast its own
    /// cables are; what is on the other end it still has to learn.
    /// Appended after the first release; older decoders read an empty
    /// list.
    #[serde(default)]
    pub port_capacity_bps: Vec<u64>,
    /// The customer contract: one row for every customer in the network,
    /// the same list at every switch. What each customer bought, not
    /// what it is doing now. Empty in a world without customers.
    /// Appended after `port_capacity_bps`; older decoders ignore it.
    #[serde(default)]
    pub customers: Vec<CustomerW>,
    /// The observation limits of this run, when it has them: how many
    /// events per second the switch can hand its controller and how many
    /// bytes per second the controller may send. `None` means no limits
    /// are published. Appended after `customers`.
    #[serde(default)]
    pub observe: Option<ObserveLimitsW>,
    /// Present in a run where the harness forwards (an A3 run): the program
    /// only observes, and its routes and queues carry only the packets its
    /// controllers create. Appended after `observe`.
    #[serde(default)]
    pub harness: Option<HarnessInfoW>,
}

/// What a program is told about a run where the harness forwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessInfoW {
    /// The harness sends a keepalive out of every switch-to-switch port this
    /// often, in each direction. A keepalive reads `PacketFieldW::Kind` as
    /// [`KIND_KEEPALIVE`], `Seq` as its count on that port, and carries a
    /// postcard [`KeepaliveW`] payload.
    pub keepalive_period_ns: u64,
}

/// `PacketFieldW::Kind` of a harness keepalive. (0 is data, 4 an ack.)
pub const KIND_KEEPALIVE: u64 = 5;

/// The payload of a harness keepalive: who sent it, out of which port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepaliveW {
    pub switch: u32,
    pub port: u16,
    pub seq: u64,
    /// When it left the sending switch, in simulation nanoseconds. Arrival
    /// minus this is the one-way delay of the link in that direction.
    pub sent_ns: u64,
}

/// The observation budget of one switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObserveLimitsW {
    /// Punt and mirror events delivered to the controller per second.
    pub punt_events_per_s: u32,
    /// Events that may wait for delivery; one more is lost.
    pub punt_backlog: u32,
    /// Bytes per second the controller may inject, sustained.
    pub export_bytes_per_s: u64,
    /// Bytes the controller may inject at once after a quiet spell.
    pub export_burst_bytes: u64,
}

/// How a customer's sender slows down when the network is congested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CcaW {
    /// Halves its rate on a loss, climbs a step at a time without one.
    Aimd,
    /// Sends at the rate that was acknowledged lately, and a little more.
    RateMatch,
    /// Sends at its own rate whatever happens to its packets. Appended for
    /// A3; only a run with an observation layer lists such a customer.
    Fixed,
    /// Not told. A run with an observation layer withholds the law: telling
    /// the customers apart is part of what it grades.
    Withheld,
}

/// One row of the customer contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerW {
    /// The customer's source prefix: its packets carry an `ip_src`
    /// inside `prefix_addr/prefix_len`.
    pub prefix_addr: u32,
    pub prefix_len: u8,
    pub cca: CcaW,
    /// The rate it asks for while it is sending, in bits per second.
    /// `None` means no limit: it takes whatever it is given.
    pub ask_bps: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PuntEventW {
    pub now_ns: u64,
    pub switch_id: u32,
    pub ingress_port: u16,
    pub reason: PuntReasonW,
    pub packet_size: u64,
    pub ip_src: u32,
    pub ip_dst: u32,
    pub ip_proto: u8,
    pub ip_ttl: u8,
    pub payload: Vec<u8>,
    /// The packet's sequence number, as `PacketFieldW::Seq` reads it.
    #[serde(default)]
    pub seq: u64,
    /// As `PacketFieldW::HarnessEgress`, without the offset: the port the
    /// harness will send the packet out of, if it forwards this run.
    #[serde(default)]
    pub harness_egress: Option<u16>,
    /// When the packet reached this switch, in simulation nanoseconds.
    /// `now_ns` is later: a punt waits its turn in the event budget. For
    /// a one-way delay, use this minus the keepalive's `sent_ns`.
    #[serde(default)]
    pub arrived_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkEventW {
    /// 0 = up, 1 = down
    pub kind: u8,
    pub port: u16,
}

/// Drops one egress port refused since the program's previous timer.
/// Only ports that dropped something are listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortDropsW {
    pub port: u16,
    /// Every refusal: pool full, queue full, or the drop threshold.
    pub dropped: u32,
    /// Of those, refusals by a queue's drop threshold -- the program's own
    /// policy, not a lack of buffer.
    pub policy: u32,
    /// The same refusals split by the queue the packet was headed for,
    /// indexed like the port's `set_queues` list.
    #[serde(default)]
    pub queues: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerEventW {
    pub now_ns: u64,
    /// Appended after the first release; a decoder that only knows
    /// `now_ns` still reads the event, because postcard ignores what
    /// follows the last field it asked for.
    #[serde(default)]
    pub port_drops: Vec<PortDropsW>,
    /// A copy of every counter array the program declared, as it stands
    /// now, when the run publishes counters to the controller. Empty
    /// otherwise. Appended after `port_drops`.
    #[serde(default)]
    pub counters: Vec<CounterSnapshotW>,
}

/// One counter array's values at a timer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterSnapshotW {
    pub id: CounterArrayIdW,
    pub values: Vec<u64>,
}

/// One egress queue as a program asks for it. Everything is counted in
/// packets of the world's data size; the simulator converts to bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSpecW {
    /// Drop-tail cap: admission fails once this many packets wait.
    pub max_packets: u32,
    /// Share of the wire under DRR (0 reads as 1). Ignored otherwise.
    pub weight: u32,
    /// Early-drop line for the drop policy; 0 = drop-tail only.
    pub drop_threshold_packets: u32,
}

impl QueueSpecW {
    /// A drop-tail queue holding at most `max_packets` packets, weight 1.
    pub fn new(max_packets: u32) -> Self {
        Self {
            max_packets,
            weight: 1,
            drop_threshold_packets: 0,
        }
    }

    /// Share of the wire under DRR.
    pub fn weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }

    /// Start dropping arrivals once this many packets wait (drop policy).
    pub fn drop_at(mut self, packets: u32) -> Self {
        self.drop_threshold_packets = packets;
        self
    }
}

/// Scheduler discipline codes for `ActionW::SetQueueConfig`.
pub const DISCIPLINE_FIFO: u8 = 0;
pub const DISCIPLINE_STRICT_PRIORITY: u8 = 1;
pub const DISCIPLINE_DRR: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionW {
    InstallTableEntry { table_id: TableIdW, entry: TableEntryW },
    DeleteTableEntry { table_id: TableIdW, entry_id: EntryIdW },
    /// Replace the queues on `port` and pick how they are served. Queue
    /// `i` of `queues` is what `SetQueue { queue: i }` in the data plane
    /// refers to. An empty `queues` keeps the current queues and only
    /// changes the discipline. Packets already waiting are kept.
    SetQueueConfig {
        port: PortIdW,
        /// One of the `DISCIPLINE_*` codes.
        discipline: u8,
        queues: Vec<QueueSpecW>,
    },
    InjectPacket {
        port: PortIdW,
        ip_src: u32,
        ip_dst: u32,
        ip_proto: u8,
        ip_ttl: u8,
        /// PacketKind discriminant: 0=Data, 1=SimpleBgp, 2=TraceProbe, 3=TraceReply.
        kind: u8,
        size_bytes: u64,
        payload: Vec<u8>,
    },
    /// Schedule a controller-timer callback `delay_ns` after now.
    ScheduleTimer { delay_ns: u64 },
    /// Declare what this switch believes about the link on `port`. Logged
    /// when it lands; changes nothing in the data plane.
    DeclareLink { port: PortIdW, up: bool },
    /// `InjectPacket` that also names the egress queue the packet joins.
    /// Appended so programs built against the older shape still decode.
    InjectPacketQueued {
        port: PortIdW,
        queue: QueueIdW,
        ip_src: u32,
        ip_dst: u32,
        ip_proto: u8,
        ip_ttl: u8,
        kind: u8,
        size_bytes: u64,
        payload: Vec<u8>,
    },
    /// Make this switch the logging switch, in a run where the harness
    /// forwards. The first switch to ask keeps it for the run; any other
    /// switch that asks is refused. Ignored in other runs. Appended after
    /// `InjectPacketQueued`.
    NameLoggingSwitch,
    /// Append `bytes` to the team's log. Only the logging switch's writes
    /// count, and a write that would take the log past its cap is lost
    /// whole. Ignored in runs where the harness does not forward.
    LogWrite { bytes: Vec<u8> },
}

// ---------- Trace reply (delivered to marking switch's controller) ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceHopW {
    pub switch_id: u32,
    pub arrived_at_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceReplyW {
    /// The switch that asked for the trace by running `MarkTrace`.
    pub mark_switch: u32,
    /// The port on `mark_switch` where the reply is arriving (this is
    /// also the egress port the marked packet originally took).
    pub mark_port: u16,
    /// Switches the original packet visited, in order.
    pub hops: Vec<TraceHopW>,
    /// Whether the packet was eventually delivered (true) or dropped.
    pub delivered: bool,
    /// 0 = delivered, 1 = ttl, 2 = no_route, 3 = no_egress_link,
    /// 4 = link_down, 5 = switch_failed, 6 = recirc, 7 = queue_full,
    /// 8 = dropped by the switch program.
    pub drop_reason: u8,
}

// ---------- Decode error ----------

#[derive(Debug)]
pub enum WireError {
    Decode(String),
}

impl WireError {
    pub fn decode(e: impl ToString) -> Self {
        WireError::Decode(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shapes a program built before the A3 fields would decode.
    #[derive(Deserialize)]
    struct InitBeforeObserve {
        switch_id: u32,
        local_ports: Vec<u16>,
        port_capacity_bps: Vec<u64>,
        customers: Vec<CustomerW>,
    }
    #[derive(Deserialize)]
    struct TimerBeforeCounters {
        now_ns: u64,
        port_drops: Vec<PortDropsW>,
    }
    #[derive(Deserialize)]
    struct PuntBeforeSeq {
        now_ns: u64,
        switch_id: u32,
        ingress_port: u16,
        reason: PuntReasonW,
        packet_size: u64,
        ip_src: u32,
        ip_dst: u32,
        ip_proto: u8,
        ip_ttl: u8,
        payload: Vec<u8>,
    }

    #[test]
    fn older_programs_still_decode_the_new_inputs() {
        let init = InitInputW {
            switch_id: 4,
            local_ports: alloc::vec![1, 2],
            port_capacity_bps: alloc::vec![10, 20],
            customers: alloc::vec![CustomerW { prefix_addr: 7, prefix_len: 24, cca: CcaW::Aimd, ask_bps: None }],
            observe: Some(ObserveLimitsW {
                punt_events_per_s: 500,
                punt_backlog: 16,
                export_bytes_per_s: 1000,
                export_burst_bytes: 4000,
            }),
            harness: Some(HarnessInfoW { keepalive_period_ns: 100_000_000 }),
        };
        let old: InitBeforeObserve = postcard::from_bytes(&postcard::to_allocvec(&init).unwrap()).unwrap();
        assert_eq!((old.switch_id, old.local_ports, old.port_capacity_bps), (4, alloc::vec![1, 2], alloc::vec![10, 20]));
        assert_eq!(old.customers, init.customers);

        let timer = TimerEventW {
            now_ns: 9,
            port_drops: alloc::vec![PortDropsW { port: 1, dropped: 3, policy: 1, queues: alloc::vec![3] }],
            counters: alloc::vec![CounterSnapshotW { id: CounterArrayIdW(2), values: alloc::vec![5, 6] }],
        };
        let old: TimerBeforeCounters = postcard::from_bytes(&postcard::to_allocvec(&timer).unwrap()).unwrap();
        assert_eq!((old.now_ns, old.port_drops), (9, timer.port_drops.clone()));

        let punt = PuntEventW {
            now_ns: 1, switch_id: 2, ingress_port: 3, reason: PuntReasonW::Custom(8), packet_size: 100,
            ip_src: 5, ip_dst: 6, ip_proto: 17, ip_ttl: 60, payload: alloc::vec![1, 2, 3],
            seq: 77, harness_egress: Some(2), arrived_ns: 0,
        };
        let old: PuntBeforeSeq = postcard::from_bytes(&postcard::to_allocvec(&punt).unwrap()).unwrap();
        assert_eq!((old.now_ns, old.switch_id, old.ingress_port, old.packet_size), (1, 2, 3, 100));
        assert!(matches!(old.reason, PuntReasonW::Custom(8)));
        assert_eq!((old.ip_src, old.ip_dst, old.ip_proto, old.ip_ttl, old.payload), (5, 6, 17, 60, alloc::vec![1, 2, 3]));
    }

    #[test]
    fn appended_variants_keep_the_old_tags() {
        // postcard writes an enum's variant index first; old variants must
        // keep theirs.
        let tag = |a: &ActionW| postcard::to_allocvec(a).unwrap()[0];
        assert_eq!(tag(&ActionW::DeclareLink { port: PortIdW(1), up: false }), 5);
        assert_eq!(tag(&ActionW::NameLoggingSwitch), 7);
        assert_eq!(tag(&ActionW::LogWrite { bytes: alloc::vec![1] }), 8);
        let itag = |i: &InstrW| postcard::to_allocvec(i).unwrap()[0];
        assert_eq!(itag(&InstrW::MarkTrace), 26);
        let ftag = |f: &PacketFieldW| postcard::to_allocvec(f).unwrap()[0];
        assert_eq!(ftag(&PacketFieldW::LabelDepth), 13);
        assert_eq!(ftag(&PacketFieldW::Seq), 16);
        let ttag = |t: &TableActionW| postcard::to_allocvec(t).unwrap()[0];
        assert_eq!(ttag(&TableActionW::Punt { reason: PuntReasonW::NoRoute }), 4);
    }
}
