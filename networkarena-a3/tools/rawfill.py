import sys
sys.path.insert(0,'.')
LEN={0:4,1:1,2:1,3:4,4:6,5:5,6:5,7:6,8:2,9:1,10:10,11:8,12:4,13:12,14:4}
b=open(sys.argv[1],'rb').read(); i=0; recs=[]
while i+3<=len(b):
    t=b[i]>>4; n=LEN.get(t)
    if n is None or i+3+n>len(b): break
    recs.append((t,int.from_bytes(b[i+1:i+3],'little')/100,3+n)); i+=3+n
cum=0; first_raw_stop=None; raw=[(t,s) for t,s,_ in recs if t in(13,14)]
last_raw=max(s for _,s in raw)
tot=sum(n for *_,n in recs)
# log order = write order; find cumulative bytes when the last raw record was written
cum=0; at_last=None
for idx,(t,s,n) in enumerate(recs):
    cum+=n
    if t in (13,14): at_last=(cum,s)
print(f"total {tot} B | last raw record written at cumulative {at_last[0]} B, observed at {at_last[1]:.1f}s | raw bytes {sum(n for t,s,n in recs if t in (13,14))}")
# raw bytes by second
import collections
by=collections.Counter(); 
for t,s,n in recs:
    if t in (13,14): by[int(s)]+=n
print("raw B per second:", " ".join(f"{k}:{by[k]}" for k in range(0,40,4)))
