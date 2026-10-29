import tomllib, random, itertools, sys, collections, math

def path_menu(adj, src, dst, slack):
    if src == dst: return [[]]
    dist = {dst: 0}; q = collections.deque([dst])
    while q:
        v = q.popleft()
        for u in adj[v]:
            if u not in dist: dist[u] = dist[v] + 1; q.append(u)
    if src not in dist: return []
    limit = dist[src] + slack; out = []; path = []; on = {src}
    def walk(at):
        if at == dst: out.append(list(path)); return
        for u in sorted(adj[at]):
            if u in on or u not in dist: continue
            if len(path) + 1 + dist[u] > limit: continue
            on.add(u); path.append((at, u)); walk(u); path.pop(); on.discard(u)
    walk(src)
    out.sort(key=lambda p: (len(p), p)); return out

def max_min(demands, paths, cap):
    n = len(demands); share = [0.0]*n; active = [True]*n
    remaining = dict(cap); eps = 1e-9
    for _ in range(10000):
        if not any(active): break
        count = collections.Counter()
        for i in range(n):
            if active[i]:
                for l in paths[i]: count[l] += 1
        inc = math.inf
        for l, c in count.items():
            if c: inc = min(inc, remaining[l] / c)
        for i in range(n):
            if active[i] and demands[i] is not None: inc = min(inc, demands[i] - share[i])
        if inc == math.inf: break  # unbounded flow with no links
        if not inc > eps:
            for l, c in count.items():
                if c and remaining[l] <= eps:
                    for i in range(n):
                        if active[i] and l in paths[i]: active[i] = False
            for i in range(n):
                if active[i] and demands[i] is not None and demands[i]-share[i] <= eps: active[i] = False
            continue
        for i in range(n):
            if active[i]: share[i] += inc
        for l, c in count.items(): remaining[l] -= inc * c
        for i in range(n):
            if active[i] and demands[i] is not None and share[i] >= demands[i]-eps: active[i] = False
        for l, c in count.items():
            if c and remaining[l] <= eps:
                for i in range(n):
                    if active[i] and l in paths[i]: active[i] = False
    return share

def assign(demands, menus, cap):
    n = len(demands)
    order = sorted(range(n), key=lambda i: (max(len(menus[i]),1), -(demands[i] if demands[i] is not None else math.inf), i))
    on_link = collections.Counter(); chosen = [[] for _ in range(n)]
    for i in order:
        if not menus[i]: continue
        best = None
        for pi, p in enumerate(menus[i]):
            room = min((cap[l]/(on_link[l]+1) for l in p), default=math.inf)
            if best is None or room > best[0] + 1e-9: best = (room, pi)
        for l in menus[i][best[1]]: on_link[l] += 1
        chosen[i] = menus[i][best[1]]
    return chosen

class World:
    def __init__(self, nsw, links, custs):
        # links: (a,b,cap_mbps) ; custs: dict(sw, peer_sw, ask(None=unlimited), bursty)
        self.adj = collections.defaultdict(list); self.cap = {}
        for a, b, c in links:
            self.adj[a].append(b); self.adj[b].append(a); self.cap[(a,b)] = c; self.cap[(b,a)] = c
        self.custs = custs; self.menus = [path_menu(self.adj, c['sw'], c['peer_sw'], 2) for c in custs]

def evaluate(w, active_idx, plan_active_idx=None):
    """returns (ref_shares, our_shares) over customers in active_idx. our plan made with plan_active_idx (default=all)."""
    cs = w.custs
    def dem(i): return cs[i]['ask']
    # reference: greedy over this state's active customers
    idx = list(active_idx)
    menus = [w.menus[i] for i in idx]; d = [dem(i) for i in idx]
    ch = assign(d, menus, w.cap); ref = max_min(d, ch, w.cap)
    # our plan: placement from plan set, then evaluated on active
    pidx = list(range(len(cs))) if plan_active_idx is None else list(plan_active_idx)
    pm = [w.menus[i] for i in pidx]; pd = [dem(i) for i in pidx]
    pch = dict(zip(pidx, assign(pd, pm, w.cap)))
    ours_paths = [pch.get(i, (w.menus[i][0] if w.menus[i] else [])) for i in idx]
    ours = max_min(d, ours_paths, w.cap)
    # baseline: everyone on first (shortest) path
    sp = [(m[0] if m else []) for m in menus]; base = max_min(d, sp, w.cap)
    return idx, ref, ours, base

def ok(ref, got):
    f1 = min((g/r if r > 1e-9 else 1.0) for r, g in zip(ref, got))
    f2 = sum(got)/sum(ref) if sum(ref) > 0 else 1.0
    return f1, f2
