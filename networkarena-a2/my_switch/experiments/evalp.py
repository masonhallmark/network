import subprocess, json, glob, sys, os, statistics, tempfile
from concurrent.futures import ThreadPoolExecutor
S='/tmp/claude-1000/-mnt-c-Users-Mason-Documents-Classes-CS356-network/eedd3f4c-ade8-496a-b456-9a6d59a4c051/scratchpad/'
A='/mnt/c/Users/Mason/Documents/Classes/CS356/network/networkarena-a2/'
def one(args):
    wasm,world,x=args
    fd,path=tempfile.mkstemp(suffix='.json',dir=S); os.close(fd)
    r=subprocess.run([A+'target/release/competitive_net_sim','run-world',world,'--program',wasm,'--score','--load-scale',str(x),'--json',path],capture_output=True,text=True,cwd=A)
    try: d=json.load(open(path))
    except Exception: d=None
    os.remove(path)
    return world,x,d
def evaluate(wasm,loads=(1,1.5)):
    dirs=os.environ.get('WDIRS','worlds,worlds_rand').split(',')
    worlds=sorted(sum([glob.glob(S+d+'/*.toml') for d in dirs],[]))
    jobs=[(wasm,w,x) for w in worlds for x in loads]
    with ThreadPoolExecutor(24) as ex: res=list(ex.map(one,jobs))
    n=len(res); ok=0; p99s=[]; fails=[]; f1s=[]
    for w,x,d in res:
        if d is None: fails.append(os.path.basename(w)+'?'); continue
        if d['passed']: ok+=1
        else: fails.append(os.path.basename(w)[:-5]+f'@{x}')
        for c in d['customers']:
            if c['preset'] in ('jane_street','fortnite'): p99s.append(c['worst_p99_ms'])
    return ok,n,(statistics.mean(p99s) if p99s else 0),(max(p99s) if p99s else 0),fails
if __name__=='__main__':
    for w in sys.argv[1:]:
        ok,n,m,mx,f=evaluate(S+'w_'+w+'.wasm')
        print(f"{w}: pass {ok}/{n}  small-customer p99 mean {m:.1f} ms max {mx:.1f} ms  fails: {' '.join(f[:6])}")
