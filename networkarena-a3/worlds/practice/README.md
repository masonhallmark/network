# The practice network

Six switches, every link 10 Mbit/s:

```
0 ---- 1 ---- 2
|      | \    |
3 ---- 4 ---- 5
```

Five customers. **Spotify** streams from 0 to 5 at 2 Mbit/s. **Fortnite**'s
players sit at 0 and 2. **Steam** downloads from 3 to 4 as fast as it can.
**Amazon**'s shoppers burst from 4 to 2, eight seconds on, eight off.
**Jane Street** trades from 2 to 3 from three addresses. `practice.toml`
names them and gives their usual rates. The map never changes, but every
run is dealt afresh: each customer gets a new address range and new host
numbers in it, its own rate for the run, and a new place in the order the
customers start, and the egg happens at a different moment. So find a
customer by how it behaves, not by its addresses, its size or when it
first spoke.

## The three practice eggs

- **Egg 21, The broken link** (Failure). Partway through, link 1-2 goes down and stays down. *Off:* no link goes down. *Near misses:* another link goes down; 1-2 goes down and comes back.
- **Egg 22, The long pause** (Customer). Partway through, Spotify stops sending for at least 10 s, then carries on. *Off:* Spotify never pauses. *Near misses:* Spotify pauses for only 2 s; another customer pauses for 10 s.
- **Egg 23, The scanner** (Traffic). Partway through, one host sends a single small packet to each of at least 50 addresses in another customer's range, all within 5 s. *Off:* no host does. *Near misses:* one host sends many packets to just a few addresses; one host reaches 50 addresses, but spread over the whole run.

## The runs

Each egg has ten runs, `eggNN/run01.toml` to `run10.toml`, and
`answers.toml` says which have the egg (`on = true`) and at what load. Run
one yourself:

```
competitive_net_sim run-world worlds/practice/egg21/run01.toml --load-scale 1.25 \
    --program switch.wasm --team-log run01.team --log run01.simlog
competitive_net_sim detect detect.wasm run01.team
simlog_dump run01.simlog | less
```

Or all of them, scored as the server would score a claim:

```
scripts/practice-check.py --switch switch.wasm --detect detect.wasm --egg 21
```

## The practice truth runs

`truth/run-a.toml`, `run-b.toml` and `run-c.toml` are what a truth run on
the server is like: all three eggs are on, the three runs share one deal
and differ in load (1.0, 1.25, 1.5), and the log cap is the truth runs'
bigger one (64 KB) rather than the claims' 16 KB. Use them the way you
will use your three real ones: run your logging switch, then read your
team log and ask what it tells you about each egg.

```
competitive_net_sim run-world worlds/practice/truth/run-a.toml --load-scale 1.0 \
    --program switch.wasm --team-log run-a.team --log run-a.simlog
```

On the server you get back your team log and `customers.txt`, which
names each address range, and nothing else. Here you also have the full
log, so you can see what your instruments missed. That is the practice.
