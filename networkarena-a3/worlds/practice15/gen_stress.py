#!/usr/bin/env python3
"""gen_stress.py <mode> <seed> <load> <out.toml>   (budgets: the real ones, 80 events/s, 64, 16000 B/s, 8000 B)

gen15's customers plus one stress, to test why reports did not get through in truth run 2:
  base    nothing added
  udp7    an 8 Mbps UDP-like sender (no sequence numbers) at the logger, switch 7
  udpall  4 Mbps UDP-like senders at switches 1, 2, 3, 6, 8, 12
  tcp7    an unlimited TCP-like (aimd) sender at each of switches 2, 6, 8, 12 (the logger's neighbours), toward far switches
"""
import sys, os
mode, seed, load, out = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
src = open(__file__.replace("gen_stress.py", "gen15.py")).read()
src = src[: src.index("head = '''")].replace("random.Random(15)", f"random.Random({seed})")
ns: dict = {}
exec(src, ns)
pair = ns["pair"]
if mode == "udp7":
    pair(7, 14, "FLOOD-UDP", "fortnite", 8.0, None, size=200)
elif mode == "udpall":
    for sw, dst in ((1, 13), (2, 12), (3, 11), (6, 14), (8, 0), (12, 4)):
        pair(sw, dst, f"FLOOD-UDP-{sw}", "fortnite", 4.0, None, size=200)
elif mode == "tcp7":
    for sw, dst in ((2, 12), (6, 14), (8, 0), (12, 4)):
        pair(sw, dst, f"HEAVY-TCP-{sw}", "steam", None, "aimd")
apps, links = dict(ns["apps"]), ns["links"]
head = f'''# gen_stress.py {mode} seed {seed}. Load {load}.
[world]
name = "a3-stress-{mode}"
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
tail = '''
[workload]
kind = "mixed"
size_bytes = 1000

[observe]
punt_events_per_s = 80
punt_backlog = 64
export_bytes_per_s = 16000
export_burst_bytes = 8000

[harness]
keepalive_ms = 100
log_cap_bytes = 65536
'''
open(out, "w").write(head + body + tail)
