import sys, random, os
sys.path.insert(0,'/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad')
from te_random import make
S='/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad/'
out=S+'worlds_p2/'; os.makedirs(out,exist_ok=True)
for f in os.listdir(out): os.remove(out+f)
N=int(sys.argv[1]) if len(sys.argv)>1 else 60
n=0
for seed in range(N):
    rng=random.Random(5000+seed)
    w=make(rng,1.0)
    sw=set(); 
    for c in w.custs: sw.add(c['sw']); sw.add(c['peer_sw'])
    nsw=max(max(a,b) for (a,b) in w.cap)+1
    links=sorted({tuple(sorted(k)) for k in w.cap})
    caps={k:w.cap[k] for k in w.cap}
    thin=[l for l in links if caps[l]<10]
    t=f'[world]\nname = "p2rand-{seed:03d}"\nseed = {seed}\nversion = 1\nfamily = "random"\ndiameter = 5\ngenerator = 2\n\n[params]\nlink_capacity_mbps = 10\nlink_latency_us = 500\nqueue_capacity_bytes = 40000\nwarmup_ms = 3000\nduration_ms = 40000\ndelivery_floor_pct = 95.0\nbuffer_budget_packets = 40\npacket_size_bytes = 1000\ncontrol_delay_us = 2000\n\n'
    for i in range(nsw): t+=f'[[switch]]\nid = {i}\nas_id = 0\n\n'
    for a,b in links:
        t+=f'[[link]]\na = {a}\nb = {b}\ncapacity_mbps = {int(caps[(a,b)])}\n\n'
    # apps: one per used switch
    apps={}; pref=set()
    def prefix():
        while True:
            p=rng.randint(1,250)
            if p not in pref: pref.add(p); return f'10.0.{p}.0/24'
    ids={}
    nid=100
    for s_ in sorted(sw):
        ids[s_]=nid; apps[s_]=dict(id=nid,sw=s_,prefix=prefix()); nid+=1
    for c in w.custs:
        a=apps[c['sw']]; a['peer']=ids[c['peer_sw']]; ty=c['ty']
        a['preset']={'jane':'jane_street'}.get(ty,ty); a['law']=rng.choice(['aimd','rate_match'])
        if ty!='steam': a['rate']=round(c['ask'],3)
        if ty in ('jane','fortnite'): a['size']=200
        if ty=='amazon': a['on']=rng.randint(8,10)*1000; a['off']=rng.randint(8,10)*1000; a['start']=rng.randint(0,3000)
    for s_ in sorted(apps):
        a=apps[s_]; t+=f'[[app]]\nid = {a["id"]}\nswitch = {a["sw"]}\nprefix = "{a["prefix"]}"\n'
        if 'preset' in a:
            t+=f'preset = "{a["preset"]}"\nlaw = "{a["law"]}"\n'
            if 'rate' in a: t+=f'rate_mbps = {a["rate"]}\n'
            t+=f'peer = {a["peer"]}\n'
            if 'size' in a: t+=f'size_bytes = {a["size"]}\n'
            if 'on' in a: t+=f'on_ms = {a["on"]}\noff_ms = {a["off"]}\nstart_ms = {a["start"]}\n'
        t+='\n'
    bl=''.join(f'    [{a}, {b}],\n' for a,b in thin)
    t+=f'[workload]\nkind = "mixed"\nsize_bytes = 1000\n\n[floors]\ngoodput_pct = 50.0\nutilization_pct = 55.0\nshare_pct = 40.0\nwindow_ms = 1000\nsettling_ms = 2000\nbottlenecks = [\n{bl}]\ndetour_hops = 2\n'
    open(out+f'p2r{seed:03d}.toml','w').write(t); n+=1
print(n,'part-2 worlds')
