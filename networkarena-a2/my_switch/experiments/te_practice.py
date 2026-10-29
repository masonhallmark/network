import sys; sys.path.insert(0,'/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad')
from te_model import *
import tomllib, itertools
def load(path, scale):
    d = tomllib.load(open(path,'rb')); dc = d['params']['link_capacity_mbps']
    links = [(l['a'],l['b'],l.get('capacity_mbps',dc)) for l in d['link']]
    sw = {a['id']: a['switch'] for a in d['app']}
    cs = []
    for a in d['app']:
        if 'preset' not in a: continue
        ask = None if a['preset']=='steam' else a['rate_mbps']*scale
        cs.append(dict(name=f"{a['id']}:{a['preset']}", sw=a['switch'], peer_sw=sw[a['peer']], ask=ask, bursty=a['preset']=='amazon'))
    return World(len(d['switch']), links, cs)
W='/mnt/c/Users/Mason/Documents/Classes/CS356/network/networkarena-a2/worlds/practice/'
for name in ['a2p2-dumb-003','a2p2-grid-006']:
    for scale in (1,1.5):
        w = load(W+name+'.toml', scale); b = [i for i,c in enumerate(w.custs) if c['bursty']]
        print(f"== {name} x{scale}: customers", [(c['name'],c['sw'],'->',c['peer_sw'],c['ask']) for c in w.custs])
        print("   menu sizes", [len(m) for m in w.menus], "shortest hops", [len(m[0]) for m in w.menus])
        for on in itertools.chain.from_iterable(itertools.combinations(b,k) for k in range(len(b)+1)):
            act = [i for i in range(len(w.custs)) if not w.custs[i]['bursty'] or i in on]
            idx, ref, ours, base = evaluate(w, act)
            f1,f2 = ok(ref, ours); b1,b2 = ok(ref, base)
            print(f"   bursts on={[w.custs[i]['name'] for i in on]}: all-on plan F1 {f1:.2f} F2 {f2:.2f} | shortest-only F1 {b1:.2f} F2 {b2:.2f}")
