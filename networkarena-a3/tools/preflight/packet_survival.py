import re,sys,collections
ids=set();punt=set();drop=collections.Counter();dropped=set();first={}
for l in open(sys.argv[1]):
    m=re.search(r'^\s*(\d+) ingress\s+S\d+ p\d+ id=(0x8\w+) kind=0 proto=93',l)
    if m: ids.add(m[2]); first.setdefault(m[2],int(m[1]))
    m=re.search(r'punted\s+S7 reason=4 id=(0x8\w+)',l)
    if m: punt.add(m[1])
    m=re.search(r'DROP\s+at (S\d+|app\d+) (\w+) id=(0x8\w+)',l)
    if m: dropped.add(m[3]); drop[(m[1],m[2])]+=1
print("packets on wire:",len(ids),"punted at logger:",len(punt),"dropped:",len(dropped),dict(drop))
