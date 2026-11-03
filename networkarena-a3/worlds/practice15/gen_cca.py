#!/usr/bin/env python3
"""gen_cca.py <seed> <load> <events_per_s> <backlog> <bytes_per_s> <burst> <out.toml>

gen15's customers plus three senders from switch 0 to switch 14 over the same links, each asking 8, 8 and 3 Mbps on 10 Mbps links:
CC-AIMD (law aimd), CC-RM (law rate_match) and CC-FIXED (a fixed-rate sender). Their congestion-control laws are known, to test
whether the log tells them apart.
"""
import sys, re
seed, load, ev, backlog, bps, burst, out = int(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5], sys.argv[6], sys.argv[7]
src = open(__file__.replace("gen_cca.py", "gen15.py")).read()
src = src[: src.index("head = '''")].replace("random.Random(15)", f"random.Random({seed})")
ns: dict = {}
exec(src, ns)
pair = ns["pair"]
import os
if os.environ.get("CCA_SEP"):   # each on its own path: the rate-match customer is not starved by the others
    a1 = pair(0, 14, "CC-AIMD", "netflix", 8.0, "aimd")
    a2 = pair(5, 9, "CC-RM", "netflix", 6.0, "rate_match")
    a3 = pair(1, 13, "CC-FIXED", "fortnite", 3.0, None, size=1000)
else:
    a1 = pair(0, 14, "CC-AIMD", "netflix", 8.0, "aimd")
    a2 = pair(0, 14, "CC-RM", "netflix", 8.0, "rate_match")
    a3 = pair(0, 14, "CC-FIXED", "fortnite", 3.0, None, size=1000)
apps, links = dict(ns["apps"]), ns["links"]
head = f'''# gen_cca.py seed {seed}: the practice customers plus three with known congestion-control laws (apps {a1}, {a2}, {a3}). Load {load}.
[world]
name = "a3-cca-{seed}"
seed = {seed}
version = 1
family = "harness"

[params]
link_capacity_mbps = 10
link_latency_us = 500
queue_capacity_bytes = 40000
warmup_ms = 3000
duration_ms = 40000
delivery_floor_pct = 0.0
packet_size_bytes = 1000
control_delay_us = 2000

'''
body = "".join(f"[[switch]]\nid = {i}\n\n" for i in range(15)) + "".join(f"[[link]]\na = {a}\nb = {b}\n\n" for a, b in links) + "\n".join(apps[i] for i in sorted(apps))
tail = f'''
[workload]
kind = "mixed"
size_bytes = 1000

[observe]
punt_events_per_s = {ev}
punt_backlog = {backlog}
export_bytes_per_s = {bps}
export_burst_bytes = {burst}

[harness]
keepalive_ms = 100
log_cap_bytes = 65536
'''
open(out, "w").write(head + body + tail)
print(out, a1, a2, a3)
