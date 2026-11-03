#!/usr/bin/env python3
"""parselog.py TEAM.LOG [--type NAME] [--switch N] -> one line per record (no Rust needed); format in LOG_FORMATS.md and a3truth7/src/fmt.rs.

Record: byte0 = type<<4 | switch, bytes 1-2 = time in 10 ms ticks (u16 LE), then a body whose length depends on the type.
Works for both truth logs (submission/truth1/result/team.log, submission/truth7/result/team.log). Import `parse(bytes)` to use it from Python.
"""
import struct, sys
NAMES = ["HEARTBEAT", "LINK_DOWN", "LINK_UP", "LINK_QUALITY", "PORT_STATS", "CUSTOMER_RATE", "CUSTOMER_GAP", "SCAN_SUSPECT", "TOP_TALKER",
         "SWITCH_SILENT", "CUSTOMER_PROFILE", "FLOW_LOSS", "BUDGET_STATS", "RAW_SAMPLE", "RAW_KEEPALIVE", "EXT"]
LEN = [4, 1, 1, 4, 6, 5, 5, 6, 2, 1, 10, 8, 4, 12, 4, 13]
EXT = {0: "OBSERVE", 3: "CCA", 4: "CCA_TRACE", 5: "REPORT_QDROP", 6: "DELIVERY"}

def ip(v):  # the switch wrote the numeric address as a little-endian u32, so v is the usual a.b.c.d number
    return ".".join(str(b) for b in struct.pack(">I", v))

def fields(name, b):
    u16 = lambda i: struct.unpack_from("<H", b, i)[0]
    if name == "HEARTBEAT": return dict(records_originated=u16(0), events_lost_est=u16(2))
    if name in ("LINK_DOWN", "LINK_UP"): return dict(peer=b[0])
    if name == "LINK_QUALITY": return dict(peer=b[0], loss_pct=b[1], delay_ms=u16(2) / 10)
    if name == "PORT_STATS": return dict(peer=b[0], drops=u16(1), kbytes_out=u16(3), ka_lost=b[5])
    if name == "CUSTOMER_RATE": return dict(cust=b[0], pkts=u16(1), kbytes=u16(3))
    if name == "CUSTOMER_GAP": return dict(cust=b[0], start_s=u16(1) / 100, dur_s=u16(3) / 100)
    if name == "SCAN_SUSPECT": return dict(src_host=b[0], src_cust=b[1], target_cust=b[2], distinct=b[3], window_start_s=u16(4) / 100)
    if name == "TOP_TALKER": return dict(cust=b[0], share_pct=b[1])
    if name == "SWITCH_SILENT": return dict(switch=b[0])
    if name == "CUSTOMER_PROFILE":
        return dict(cust=b[0], avg_size=b[1] * 8, udp_pct=b[2], idle_pct=b[3], rate_bucket=b[4], burst_cv=b[5] / 32, ttl_lt60=b[6], ttl_lt30=b[7], ttl_lt10=b[8], ack_pct=b[9])
    if name == "FLOW_LOSS": return dict(src=b[0], dst=b[1], entered=u16(2), left=u16(4), dups=u16(6))
    if name == "BUDGET_STATS": return dict(events_lost=u16(0), reports_refused=u16(2))
    if name == "RAW_SAMPLE": return dict(src=ip(struct.unpack_from("<I", b, 0)[0]), dst=ip(struct.unpack_from("<I", b, 4)[0]), proto=b[8], size=u16(9), ttl=b[11])
    if name == "RAW_KEEPALIVE": return dict(port=b[0], peer=b[1], delay_ms=u16(2) / 10)
    e = b[0]
    if e == 0: return dict(ext="OBSERVE", events_per_s=u16(1), backlog=u16(3), export_bytes_per_s=struct.unpack_from("<I", b, 5)[0], burst=struct.unpack_from("<I", b, 9)[0])
    if e == 6: return dict(ext="DELIVERY", origin=b[1], reports=u16(2), records=u16(4), duplicate_copies=u16(6))
    return dict(ext=EXT.get(e, e), raw=b[1:].hex())

def parse(buf):
    out, at = [], 0
    while at + 3 <= len(buf):
        ty, sw = buf[at] >> 4, buf[at] & 15
        t = struct.unpack_from("<H", buf, at + 1)[0] / 100
        n = LEN[ty]
        if at + 3 + n > len(buf): break
        name = NAMES[ty]
        out.append((t, sw, name, fields(name, buf[at + 3: at + 3 + n])))
        at += 3 + n
    return out

if __name__ == "__main__":
    args = sys.argv[1:]
    want_type = args[args.index("--type") + 1] if "--type" in args else None
    want_sw = int(args[args.index("--switch") + 1]) if "--switch" in args else None
    for t, sw, name, f in parse(open(args[0], "rb").read()):
        if (want_type is None or name == want_type) and (want_sw is None or sw == want_sw):
            print(f"{t:6.2f}s S{sw:<2} {name:<16} " + " ".join(f"{k}={v}" for k, v in f.items()))
