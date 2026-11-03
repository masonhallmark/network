#!/usr/bin/env python3
"""ccashow.py <world.toml> <logdump --all text> [name-prefix]: the CCA / CCA_TRACE records of the customers whose name starts with the prefix."""
import re,sys,ipaddress
w=open(sys.argv[1]).read(); pre=sys.argv[3] if len(sys.argv)>3 else ""
apps={}
for blk in w.split("[[app]]")[1:]:
    blk=blk.split("\n[[")[0].split("\n[workload]")[0]
    g=lambda k:(re.search(rf'^{k} = "?([^"\n]+)"?',blk,re.M) or [None,None])[1]
    apps[int(g("id"))]=(g("name"),g("prefix"),g("preset"),int(g("switch")))
sn=sorted([a for a in apps if apps[a][2]],key=lambda a:int(ipaddress.ip_network(apps[a][1]).network_address))
name={i:apps[a][0] for i,a in enumerate(sn)}
for l in open(sys.argv[2]):
    m=re.match(r'\s+([\d.]+)s S(\d+)\s+EXT\s+(CCA|CCA_TRACE)\s+cust=(\d+)\s+(.*)',l)
    if m and name[int(m[4])].startswith(pre): print(f"{m[1]:>6}s S{m[2]:<2} {name[int(m[4])]:9s} {m[3]:9s} {m[5][:120]}")
