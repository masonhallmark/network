#!/bin/bash
# one.sh <S> <build 5|6> <world name>
S=$1; b=$2; n=$3
if [ $b = 6 ]; then W=submission/truth6/switch.wasm; L=a3truth6/target/release/logdump; else W=submission/truth7/switch.wasm; L=a3truth7/target/release/logdump; fi
d=$S/ba/${n}_t$b
./target/release/competitive_net_sim run-world $S/ba/$n.toml --load-scale 1.25 --program $W --team-log $d.team --log $d.simlog > $d.out 2>&1
$L $d.team 65536 --all > $d.txt 2>/dev/null
tools/decode/target/release/decode $d.simlog $d.dec >/dev/null 2>&1
python3 - $S $d $n $b <<'PY'
import re,sys,collections,os
S,d,n,b=sys.argv[1:5]
fail=open(d+'.out').read().count('program failed')
ids={};adv=0;punt=set();drop=set()
for l in open(d+'.dec'):
    m=re.search(r'ingress\s+S\d+ p\d+ id=(0x8\w+) kind=0 proto=(93|94) \S+ (\d+)B',l)
    if m:
        if m[1] not in ids: ids[m[1]]=(int(m[3]),m[2])
    m=re.search(r'punted\s+S7 reason=4 id=(0x8\w+)',l)
    if m: punt.add(m[1])
rep=sum(s for s,p in ids.values() if p=='93'); advb=sum(s for s,p in ids.values() if p=='94'); nrep=sum(1 for s,p in ids.values() if p=='93')
txt=open(d+'.txt').read()
cca=len(re.findall(r'EXT\s+(?:CCA|CCA_TRACE|PORT_SATURATION|DELIVERY)\s',txt)); catr=len(re.findall(r'EXT\s+CCA_TRACE\s',txt))
# events audit
sent={};ref={}
for l in txt.splitlines():
    m=re.match(r'\s+[\d.]+s S(\d+)\s+HEARTBEAT\s+records_originated=(\d+)',l)
    if m: sent[int(m[1])]=int(m[2])
    m=re.match(r'\s+[\d.]+s S(\d+)\s+BUDGET_STATS\s+events_lost=\d+ reports_refused=(\d+)',l)
    if m: ref[int(m[1])]=int(m[2])
tot=sum(sent.values()); lost=sum(min(ref.get(k,0),sent[k]) for k in sent)
orig=len({m[1] for m in re.finditer(r'^\s+[\d.]+s S(\d+) ',txt,re.M)})
extra=''
if n.startswith('eggs'):
    import subprocess
    r=subprocess.run(['python3','tools/eggcheck.py',f'{S}/ba/{n}.toml',d+'.txt'],capture_output=True,text=True).stdout
    extra=' '+re.search(r'found (\d+)/20',r)[0]+' | '+re.search(r'consistency: (\d+)/15',r)[0]
print(f"{n:11s} truth{b}: fail={fail} log={os.path.getsize(d+'.team'):6d}B report_wire={rep:6d}B in {nrep:3d} pkts, adverts={advb:4d}B, reached={100*len(punt)//max(1,nrep)}%, events={100*(tot-lost)//max(1,tot)}%, switches_heard={orig}, cca_rec={cca}(trace {catr}){extra}")
PY
