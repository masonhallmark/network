#!/usr/bin/env python3
"""gen20.py <seed> <load> <events_per_s> <backlog> <bytes_per_s> <burst> <out.toml>

The 15-switch practice network (same customers as gen15, prefixes drawn from <seed>) with twenty things planted in one run,
standing in for what the hidden eggs might be. EGGS below lists them: `n`, a name, the switch that should see it, what to look for.
"""
import random, re, sys
seed, load, ev, backlog, bps, burst, out = int(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5], sys.argv[6], sys.argv[7]
src = open(__file__.replace("gen20.py", "gen15.py")).read()
src = src[: src.index("head = '''")].replace("random.Random(15)", f"random.Random({seed})")
ns: dict = {}
exec(src, ns)
apps, links = dict(ns["apps"]), ns["links"]
extra = {118: "echo = true\n", 112: "ttl_ladder = 8\n"}          # customer options, appended to that app's table
def app_text(i):
    t = apps[i]
    return t + extra.get(i, "")
prefix_of = lambda i: re.search(r'prefix = "([^"]+)"', apps[i]).group(1)
plants = []
def plant(**kw):
    plants.append("[[plant]]\n" + "".join(f'{k} = {v if not isinstance(v, str) else chr(34)+v+chr(34)}\n' for k, v in kw.items()))
plant(kind="fail", a=1, b=6, at_ms=6000)                                   # 1  link 1-6 down for good
plant(kind="fail", a=3, b=8, at_ms=9000, for_ms=1600)                      # 2  link 3-8 down 1.6 s
# 3  one-way keepalive gap 9 -> 14, 21-27 s (harness section below)
for at in (12000, 15000, 18000):
    plant(kind="fail", a=12, b=13, at_ms=at, for_ms=1000)                  # 4  link 12-13 flaps three times
plant(kind="link_delay", a=2, b=3, a_to_b_us=6000, b_to_a_us=6000)         # 5  slow link
plant(kind="wire_loss", a=11, b=12, loss_pct=8.0, start_ms=12000, stop_ms=32000)   # 6  leaky link
plants.append('[[plant]]\nkind = "link_capacity"\na = 6\nb = 7\nsteps = [[0, 10], [18000, 1]]\n')  # 7  capacity drop
plant(kind="fail", a=4, b=9, at_ms=25000)                                  # 8  late permanent failure
plant(kind="pause", app=102, at_ms=14000, for_ms=10000)                    # 9  10 s pause (Netflix at 4)
plant(kind="pause", app=110, at_ms=30000, for_ms=30000)                    # 10 customer goes quiet for good (Steam at 3)
plant(kind="pause", app=106, at_ms=8000, for_ms=12000)                     # 11 12 s pause (Fortnite at 1)
plant(kind="side_flow", app=104, rate_pps=30.0, size_bytes=300, start_ms=10000, stop_ms=30000)   # 12 extra flow (Netflix at 2)
plant(kind="side_flow", app=114, rate_pps=80.0, size_bytes=1000, start_ms=25000, stop_ms=35000)  # 13 burst (Amazon at 0)
# 14 echo (Jane Street at 7), 15 ttl ladder (Steam at 6): app options above
plant(kind="scan", app=116, start_ms=22000, for_ms=4000, addresses=50, size_bytes=60)   # 16 scan, 50 addresses in 4 s (host at 14)
plant(kind="scan", app=108, start_ms=28000, for_ms=3000, addresses=30, size_bytes=60)   # 17 smaller scan (host at 5)
plant(kind="scan", app=120, start_ms=16000, for_ms=5000, addresses=2, each=400, size_bytes=80)  # 18 many packets to two addresses (host at 12)
head = f'''# Twenty planted things on the 15-switch practice network (gen20.py, seed {seed}). Load {load}.
[world]
name = "a3-eggs20-{seed}"
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
body = "".join(f"[[switch]]\nid = {i}\n\n" for i in range(15)) + "".join(f"[[link]]\na = {a}\nb = {b}\n\n" for a, b in links) + "\n".join(app_text(i) for i in sorted(apps))
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

[[harness.keepalive_gap]]
switch = 9
toward = 14
start_ms = 21000
end_ms = 27000

[[harness.route]]
switch = 7
prefix = "{prefix_of(101)}"
via = 8
split = 12

[[harness.queue]]
switch = 0
toward = 1
packets = 1

'''
open(out, "w").write(head.replace("{load}", load) + body + tail + "\n".join(plants))
print(out, len(apps), "apps,", len(plants), "plant rows")
