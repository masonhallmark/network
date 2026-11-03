#!/usr/bin/env python3
"""eggcheck.py <world.toml> <logdump --all text>  -> one line per planted egg: found / missed, plus per-switch report consistency.

"Found" means the log holds a signature an analyst could read as that egg (the check is mine, written from what each plant does);
it is not a detector. Customer ids in the log are the sending customers in address order, mapped here from the world file.
"""
import re, sys, ipaddress, collections
world, logtxt = open(sys.argv[1]).read(), open(sys.argv[2]).read()
apps = {}
for blk in world.split("[[app]]")[1:]:
    blk = blk.split("\n[[")[0].split("\n[workload]")[0]
    g = lambda k: (re.search(rf'^{k} = "?([^"\n]+)"?', blk, re.M) or [None, None])[1]
    apps[int(g("id"))] = dict(sw=int(g("switch")), prefix=g("prefix"), sender=g("preset") is not None)
senders = sorted((a for a in apps if apps[a]["sender"]), key=lambda a: int(ipaddress.ip_network(apps[a]["prefix"]).network_address))
cust = {a: i for i, a in enumerate(senders)}

ev = []   # (t, sw, type, text)
for l in logtxt.splitlines():
    m = re.match(r"\s+([\d.]+)s S(\d+)\s+([A-Z_]+)\s*(.*)", l)
    if m: ev.append((float(m[1]), int(m[2]), m[3], m[4]))
def of(ty, sw=None): return [e for e in ev if e[2] == ty and (sw is None or e[1] == sw)]
def kv(txt, k, cast=float):
    m = re.search(rf"{re.escape(k)}=([-\d.]+)", txt); return cast(m[1]) if m else None
def downs(a, b): return [e for e in ev if e[2] == "LINK_DOWN" and ((e[1], kv(e[3].replace("S", ""), "peer", int)) in ((a, b), (b, a)))]
def ups(a, b):   return [e for e in ev if e[2] == "LINK_UP" and ((e[1], kv(e[3].replace("S", ""), "peer", int)) in ((a, b), (b, a)))]
def gaps(sw, lo, hi, mind):
    return [e for e in of("CUSTOMER_GAP", sw) if lo <= kv(e[3], "start") <= hi and kv(e[3], "dur") >= mind]
def rate_series(a):
    c = cust[a]; sw = apps[a]["sw"]; pts = sorted((e[0], kv(e[3], "pkts")) for e in of("CUSTOMER_RATE", sw) if kv(e[3], "cust") == c)
    return pts
def window(pts, lo, hi):
    return [v for t, v in pts if lo <= t <= hi]
def ratio(a, base, after):
    p = rate_series(a); x, y = window(p, *base), window(p, *after)
    return (max(y) / max(1, max(x))) if x and y else 0.0

R = {}
R[1] = ("link 1-6 down for good", bool(downs(1, 6)) and not [u for u in ups(1, 6) if u[0] > 6.5])
R[2] = ("link 3-8 down 1.6 s", bool(downs(3, 8)) and bool(ups(3, 8)))
R[3] = ("one-way keepalive gap 9->14", any(e[1] == 14 and "peer=S9" in e[3] for e in of("LINK_DOWN")) or bool(downs(9, 14)))
R[4] = ("link 12-13 flaps", len({round(e[0]) for e in downs(12, 13)}) >= 2)
R[5] = ("slow link 2-3", any(kv(e[3], "delay") and kv(e[3], "delay") >= 3 for e in of("LINK_QUALITY") if ((e[1], kv(e[3].replace("S", ""), "peer", int)) in ((2, 3), (3, 2)))))
R[6] = ("leaky link 11-12", any((kv(e[3], "loss") or 0) >= 5 for e in of("LINK_QUALITY") if ((e[1], kv(e[3].replace("S", ""), "peer", int)) in ((11, 12), (12, 11)))))
R[7] = ("capacity drop 6-7", any(e[0] >= 18 and (kv(e[3], "drops") or 0) > 0 and kv(e[3], "peer", int) == 7 for e in of("PORT_STATS", 6)))
R[8] = ("link 4-9 down late, for good", bool(downs(4, 9)) and not [u for u in ups(4, 9) if u[0] > 25])
R[9] = ("10 s pause at switch 4", bool(gaps(4, 13.5, 15, 9)))
R[10] = ("customer quiet for good at 3", bool(gaps(3, 29.5, 31, 7)))
R[11] = ("12 s pause at switch 1", bool(gaps(1, 7.5, 9, 11)))
R[12] = ("extra flow on a customer at 2", ratio(104, (3, 9), (11, 29)) >= 1.15)
R[13] = ("burst on Amazon at 0", ratio(114, (3, 24), (26, 34)) >= 1.3)
R[14] = ("duplicated packets at 7", any((kv(e[3], "dups") or 0) > 0 for e in of("FLOW_LOSS", 7)))
R[15] = ("TTL ladder at 6", any((kv(e[3], "<10", float) or 0) >= 50 for e in of("CUSTOMER_PROFILE", 6) if kv(e[3], "cust", int) == cust[112]))
R[16] = ("scan, 50 addresses in 4 s, at 14", any((kv(e[3], "distinct") or 0) >= 50 for e in of("SCAN_SUSPECT", 14)))
R[17] = ("30-address scan at 5", any((kv(e[3], "distinct") or 0) >= 16 for e in of("SCAN_SUSPECT", 5)))
R[18] = ("many packets to two addresses at 12", ratio(120, (3, 15), (17, 21)) >= 2.0)
R[19] = ("split route at 7", len({kv(e[3], "peer", int) for e in of("PORT_STATS", 7) if (kv(e[3], "kB_out") or 0) > 0} & {8, 12}) == 2)
R[20] = ("small queue 0->1 drops", any((kv(e[3], "drops") or 0) > 0 and kv(e[3], "peer", int) == 1 for e in of("PORT_STATS", 0)))
found = [n for n, (_, ok) in R.items() if ok]
print("found %d/20: %s | missed: %s" % (len(found), ",".join(map(str, found)), ",".join(f"{n}({R[n][0]})" for n in R if n not in found) or "none"))
# consistency: every other switch must be heard through the run (heartbeats every 5 s, plus a final one)
hb = collections.defaultdict(list)
for t, sw, ty, tx in ev:
    if ty == "HEARTBEAT": hb[sw].append(t)
bad = []
for sw in range(15):
    ts = sorted(hb[sw]); gap = max([b - a for a, b in zip(ts, ts[1:])] + [ts[0] if ts else 99, 40 - ts[-1] if ts else 99])
    if len(ts) < 6 or gap > 11: bad.append(f"S{sw}(n={len(ts)},gap={gap:.1f}s)")
print("consistency: %d/15 switches heard throughout%s" % (15 - len(bad), (" | gaps: " + ", ".join(bad)) if bad else ""))
