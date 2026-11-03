# Watching, reporting and logging: the Assignment 3 SDK

In Assignment 3 the harness forwards every packet. Your switch program
still runs on every switch, but what it does to a packet changes nothing:
its routes, queues and egress choices are ignored. What counts is what it
notices, what it reports to one switch, and what that switch writes down.
After the run your detector reads that log and nothing else.

This note is the API for that chain. `docs/switch-programs.md` is still
the reference for the data plane, the controller and the wasm build.

## The run

A world with a `[harness]` section is one where the harness forwards.
`run-world` on such a world takes two more flags:

```sh
competitive_net_sim run-world worlds/practice/egg21/run02.toml --load-scale 1.25 \
    --program switch.wasm --team-log run02.team --log run02.simlog
```

`--team-log` writes your log: the bytes your logging switch wrote, back
to back, nothing added. `--log` writes the full event log, which on the
practice network you may read (`simlog_dump run02.simlog`, or the
viewer). On the server you never see it.

Every switch is told at start, in `init_with(input)`:

- `input.harness`: the keepalive period (`keepalive_period_ns`), when the
  harness forwards; `None` otherwise;
- `input.observe`: the switch's budgets (below);
- `input.customers`: every customer's source range, in address order.
  Ranges are dealt afresh every run, so the order says nothing about who
  a customer is, and `cca` reads `Withheld`: how a customer slows down is
  yours to find out.

## What a switch sees

Your data plane runs on every packet, as before; what it may do now is
count and punt. A `counter` array you declare is copied to you on every
timer (`ev.counters`), so the data plane can count what it likes, per
customer, per port, per anything the TinyVM can compute from the packet,
and the controller reads the totals at its own pace. That is how to see
every packet without touching one.

A `punt` is a sample. The packet goes on; a copy comes to `on_punt`,
through the switch's event budget: `punt_events_per_s` events a second,
`punt_backlog` waiting, one more lost. Only sample what you must.
`ev.arrived_ns` is when the packet reached the switch; `ev.now_ns` is
later, when its turn in the budget came.

The harness sends a **keepalive** out of every switch-to-switch port
every `keepalive_period_ns`. It is a packet whose `kind` field reads
`KIND_KEEPALIVE` (5), 64 bytes, with a payload `keepalive(&ev.payload)`
decodes into `Keepalive { switch, port, seq, sent_ns }`: who sent it, out
of which port, when. From one you learn who is on the other end of a
port; from `arrived_ns - sent_ns` the link's one-way delay toward you;
from its absence that the link is down, or full. Keepalives queue with
customer packets, so a saturated link loses some. A link that is down
loses all of them, at both ends.

Every timer still carries `ev.port_drops`, what each of the switch's
ports refused since your last timer, by queue. A customer's packets that
entered the network and never left it were dropped somewhere or lost on
a wire; the counts say which.

## What a switch says

A switch's observations reach the log in one of two ways. The logging
switch writes them down itself. Every other switch sends them there as
**reports**: injected packets, `actions::inject_packet(port, src, dst,
proto, ttl, payload)`, which the next switch punts to its own
controller and passes on. `examples/load_report` floods a report to
every switch; `examples/broken_link` floods one to the logging switch
the same way. Pick an `ip_proto` nothing else uses and punt it.

Reports cost. Each switch's exports draw on a token bucket of
`export_burst_bytes`, refilled at `export_bytes_per_s`; a packet the
bucket cannot cover is not sent. Reports ride behind customers, in a
small second queue on every port (`report_queue_packets`, 4 by default),
so they never delay a customer packet and a burst of them is dropped. A
report larger than a customer packet is refused where it is made. Send
little, send on change, and say in the record when it happened.

## The log

One switch is the **logging switch**. Return `actions::name_logging_switch()`
from its first timer: every switch gets one at time zero, in switch-id
order, and the first to ask keeps it for the run. Pick it by id in your
program (`examples/broken_link` uses switch 0) and make every other
switch report to it.

`actions::log_write(bytes)` appends to the log. Only the logging switch's
writes count. The log has a cap (`log_cap_bytes`; 16 KB on the practice
network, and the claim runs' cap is published with the submission
server); a write that would take it past the cap is lost whole, and the
rest of the log is kept. `simlog_dump` shows each write and each loss.
The log is your bytes and only your bytes: framing, tags and timestamps
are yours to design, and your detector has to parse what you wrote.

The run ends at its `duration_ms` and no timer fires after that. If your
switch holds observations back until a tidy boundary, say the end of a
window, whatever it holds at the end is never written: an event in the
last seconds of a run is visible only to a switch that writes as it
goes, or writes early.

## The detector

`detect.wasm` is a second crate (`crate-type = ["cdylib"]`, depending on
`switch_program_sdk`) with one function:

```rust
fn detect(log: &[u8]) -> bool { ... }
switch_program_sdk::detect_program!(detect);
```

The simulator calls it once, after the run, with exactly the bytes your
logging switch wrote. It answers one bit: is the egg in this run? It
knows nothing about the run but the log. It has 100 million units of
fuel and 64 MB of memory; a trap or a wrong export is a failed answer,
which is never right.

```sh
competitive_net_sim detect detect.wasm run02.team      # prints true or false
```

## The practice network

`worlds/practice/` has the network, each practice egg's ten runs and
their answers, and three practice truth runs with every egg on and the
truth runs' bigger log cap (`README.md` there says what the eggs are and
how to use the truth runs). Score a pair of programs on an egg's runs as
the server would:

```sh
scripts/practice-check.py --switch switch.wasm --detect detect.wasm --egg 21
```

It checks both programs load, runs every run, and prints the card plus
every run's answer. `examples/broken_link` and `examples/broken_link_detect`
win egg 21; read them end to end before writing your own.
