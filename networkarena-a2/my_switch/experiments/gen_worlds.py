import random, re, sys, collections, tomllib, os
W='/mnt/c/Users/Mason/Documents/Classes/CS356/network/networkarena-a2/worlds/practice/'
S='/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad/'
out=S+'worlds_rand/'; os.makedirs(out,exist_ok=True)
for f in os.listdir(out): os.remove(out+f)
bases=['a2p1-dumb-004','a2p1-grid-009','a2p1-ring-020']
def shortest(adj,a,b):
    prev={a:None};q=[a]
    for u in q:
        for v in adj[u]:
            if v not in prev: prev[v]=u;q.append(v)
    p=[];x=b
    while x is not None: p.append(x);x=prev[x]
    return p[::-1]
n=0
for base in bases:
    txt=open(W+base+'.toml').read(); d=tomllib.loads(txt)
    head=txt[:txt.index('[[app]]')]; tail=txt[txt.index('[workload]'):]
    dc=d['params']['link_capacity_mbps']
    links=[(l['a'],l['b'],l.get('capacity_mbps',dc)) for l in d['link']]
    thin=min(links,key=lambda l:l[2]); adj=collections.defaultdict(list)
    for a,b,c in links: adj[a].append(b);adj[b].append(a)
    apps=[(a['id'],a['switch'],a['prefix']) for a in d['app']]
    for seed in range(20):
        rng=random.Random(hash((base,seed))%10**9)
        for attempt in range(200):
            ids=apps[:]; rng.shuffle(ids); k=rng.randint(3,5); rows=[]; cross=0; used=0
            types=[rng.choice(['netflix','amazon','steam','jane_street','fortnite','netflix','steam','amazon']) for _ in range(k)]
            if 'steam' not in types and rng.random()<.6: types[0]='steam'
            ok=True; rowsd={}
            for ty in types:
                if len(ids)<2: ok=False;break
                a=ids.pop(); b=ids.pop(); law=lambda: rng.choice(['aimd','rate_match'])
                path=shortest(adj,a[1],b[1])
                if any({path[i],path[i+1]}=={thin[0],thin[1]} for i in range(len(path)-1)): cross+=1
                if ty=='fortnite':
                    r=round(rng.uniform(.3,.6),2)
                    rowsd[a[0]]=dict(preset='fortnite',law=law(),rate=r,peer=b[0],size=200)
                    rowsd[b[0]]=dict(preset='fortnite',law=law(),rate=r,peer=a[0],size=200)
                elif ty=='steam': rowsd[a[0]]=dict(preset='steam',law=law(),peer=b[0])
                elif ty=='netflix': rowsd[a[0]]=dict(preset='netflix',law=law(),rate=round(rng.uniform(3,5),2),peer=b[0])
                elif ty=='jane_street': rowsd[a[0]]=dict(preset='jane_street',law=law(),rate=round(rng.uniform(.05,.2),3),peer=b[0],size=200)
                else: rowsd[a[0]]=dict(preset='amazon',law=law(),rate=round(rng.uniform(6,8),2),peer=b[0],on=rng.randint(8,10)*1000,off=rng.randint(8,10)*1000,start=rng.randint(0,3000))
            if ok and cross>=3: break
        else: continue
        body=''
        for (i,sw,pre) in apps:
            body+=f'[[app]]\nid = {i}\nswitch = {sw}\nprefix = "{pre}"\n'
            r=rowsd.get(i)
            if r:
                body+=f'preset = "{r["preset"]}"\nlaw = "{r["law"]}"\n'
                if 'rate' in r: body+=f'rate_mbps = {r["rate"]}\n'
                body+=f'peer = {r["peer"]}\n'
                if 'size' in r: body+=f'size_bytes = {r["size"]}\n'
                if 'on' in r: body+=f'on_ms = {r["on"]}\noff_ms = {r["off"]}\nstart_ms = {r["start"]}\n'
            body+='\n'
        open(out+f'{base[5:9]}-r{seed:02d}.toml','w').write(head+body+tail); n+=1
print(n,'worlds written')
