import sys; sys.path.insert(0,'/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad')
from te_model import *
import random, itertools, collections

def topo(rng):
    kind = rng.choice(['ring','grid','dumbbell','random','random'])
    edges=set()
    if kind=='ring':
        n=rng.randint(6,12); edges={tuple(sorted((i,(i+1)%n))) for i in range(n)}
        for _ in range(rng.randint(0,2)):
            a,b=rng.sample(range(n),2); edges.add(tuple(sorted((a,b))))
    elif kind=='grid':
        r,c=rng.choice([(2,3),(2,4),(3,3),(2,5),(3,4)]); n=r*c
        for i in range(r):
            for j in range(c):
                if j+1<c: edges.add((i*c+j,i*c+j+1))
                if i+1<r: edges.add((i*c+j,(i+1)*c+j))
    elif kind=='dumbbell':
        k=rng.randint(3,6); n=2*k
        for side in (0,k):
            for i in range(k): edges.add(tuple(sorted((side+i,side+(i+1)%k))))
        edges.add((rng.randrange(k),k+rng.randrange(k))); 
        if rng.random()<.7: edges.add((rng.randrange(k),k+rng.randrange(k)))
    else:
        n=rng.randint(6,15)
        for i in range(1,n): edges.add(tuple(sorted((i,rng.randrange(i)))))
        for _ in range(rng.randint(2,n//2+2)):
            a,b=rng.sample(range(n),2); edges.add(tuple(sorted((a,b))))
    deg=collections.Counter()
    out=[]
    for a,b in sorted(edges):
        if a==b or deg[a]>=4 or deg[b]>=4: continue
        deg[a]+=1;deg[b]+=1; out.append((a,b))
    # connectivity check
    adj=collections.defaultdict(list)
    for a,b in out: adj[a].append(b);adj[b].append(a)
    seen={0};q=[0]
    for u in q:
        for v in adj[u]:
            if v not in seen: seen.add(v);q.append(v)
    if len(seen)<n: return None
    return n,out

def make(rng, scale):
    t=None
    while t is None: t=topo(rng)
    n,edges=t
    caps={e:10.0 for e in edges}
    thin=rng.sample(edges, min(len(edges), rng.randint(1,3)))
    for e in thin: caps[e]=float(rng.choice([2,3,4,5,6]))
    links=[(a,b,caps[(a,b)]) for a,b in edges]
    free=list(range(n)); rng.shuffle(free); cs=[]
    types=['steam','amazon','netflix','jane','fortnite','netflix','amazon','jane']
    rng.shuffle(types); types=['steam','amazon']+types
    for ty in types:
        need=2
        if len(free)<need: break
        a=free.pop(); b=free.pop()
        if ty=='steam': cs.append(dict(sw=a,peer_sw=b,ask=None,bursty=False,ty=ty))
        elif ty=='amazon': cs.append(dict(sw=a,peer_sw=b,ask=rng.uniform(6,8)*scale,bursty=True,ty=ty))
        elif ty=='netflix': cs.append(dict(sw=a,peer_sw=b,ask=rng.uniform(3,5)*scale,bursty=False,ty=ty))
        elif ty=='jane': cs.append(dict(sw=a,peer_sw=b,ask=rng.uniform(.05,.2)*scale,bursty=False,ty=ty))
        else:
            r=rng.uniform(.3,.6)*scale
            cs.append(dict(sw=a,peer_sw=b,ask=r,bursty=False,ty=ty)); cs.append(dict(sw=b,peer_sw=a,ask=r,bursty=False,ty=ty))
    return World(n,links,cs)

def run(seed0, N, scale):
    stats=collections.Counter(); worst=[]
    for s in range(N):
        rng=random.Random(seed0+s); w=make(rng,scale)
        b=[i for i,c in enumerate(w.custs) if c['bursty']]
        bad_all=bad_off=bad_sp=False; m_all=9;m_off=9;m_sp=9
        for on in itertools.chain.from_iterable(itertools.combinations(b,k) for k in range(len(b)+1)):
            act=[i for i in range(len(w.custs)) if not w.custs[i]['bursty'] or i in on]
            idx,ref,ours,base=evaluate(w,act)                   # plan made with everyone on
            f1,f2=ok(ref,ours)
            # plan made as if bursty customers are OFF (bursty ones get placed after by shortest path)
            off_plan=[i for i in range(len(w.custs)) if not w.custs[i]['bursty']]
            _,_,ours_off,_=evaluate(w,act,off_plan)
            g1,g2=ok(ref,ours_off); h1,h2=ok(ref,base)
            m_all=min(m_all,f1,f2/ .55*.4); m_off=min(m_off,g1,g2/.55*.4); m_sp=min(m_sp,h1,h2/.55*.4)
            if f1<.4 or f2<.55: bad_all=True
            if g1<.4 or g2<.55: bad_off=True
            if h1<.4 or h2<.55: bad_sp=True
        stats['worlds']+=1; stats['all_on_fail']+=bad_all; stats['bursty_off_fail']+=bad_off; stats['shortest_fail']+=bad_sp
        if bad_all: worst.append((s,round(m_all,2),[(c['ty'],c['sw'],c['peer_sw']) for c in w.custs]))
        stats['all_on_margin<0.6']+= (m_all<.6)
    return stats,worst
if __name__=='__main__':
    for scale in (1,1.5):
        st,worst=run(1000,1500,scale)
        print(f"scale {scale}:",dict(st))
        for x in worst[:3]: print("   example failing world",x)
