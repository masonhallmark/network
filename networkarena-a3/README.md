# NetworkArena — Assignment 3 kit: Make It Visible

Same simulator, same one program on every switch. This time the harness
forwards every packet, and your program's forwarding is ignored. What
counts is what it notices, what it reports to one switch, and what that
switch writes down: your log. After the run a second program of yours,
the detector, reads the log and answers one question about the run.

The assignment text, the eggs and the grading rules are in the handout.
They are not in this kit. `docs/a3-logging.md` is the API for the chain
from a packet to a true or false.

## Layout

```
src/  switch_program_types/  switch_program_sdk/   the simulator and the SDK
examples/hello_switch/       the smallest program: runs and sees nothing
examples/learning_switch/    one static route on the first punt: the shape of a program
examples/load_report/        measures each link's load and shares it with every switch (Assignment 2)
examples/broken_link/        the solved practice egg: notices a quiet link, reports it, logs it
examples/broken_link_detect/ its detector: both ends of link 1-2 silent for good
worlds/practice/             the practice network, three practice eggs x 10 runs, their answers
scripts/practice-check.py    scores your two programs on the practice runs as the server would
viewer/                      browser replay of a full log
docs/                        simulator reference, and the logging guide
```

## Setup

```sh
rustup target add wasm32-unknown-unknown
cargo build --release
```

## Two programs

Your switch program is a crate like `examples/broken_link/`; your
detector is a crate like `examples/broken_link_detect/`. Build each to
wasm:

```sh
(cd examples/broken_link && cargo build --release --target wasm32-unknown-unknown)
(cd examples/broken_link_detect && cargo build --release --target wasm32-unknown-unknown)
```

## Run

One practice run, with your log and the full log side by side:

```sh
./target/release/competitive_net_sim run-world worlds/practice/egg21/run02.toml --load-scale 1.25   --program examples/broken_link/target/wasm32-unknown-unknown/release/broken_link.wasm   --team-log run02.team --log run02.simlog
./target/release/competitive_net_sim detect   examples/broken_link_detect/target/wasm32-unknown-unknown/release/broken_link_detect.wasm run02.team
./target/release/simlog_dump run02.simlog | less
```

All ten runs of a practice egg, scored as a claim:

```sh
scripts/practice-check.py --switch YOUR_SWITCH.wasm --detect YOUR_DETECT.wasm --egg 21
```

`worlds/practice/README.md` says what the three practice eggs are and
which runs have them. The server grades you on a different, larger
network, where the ranges are dealt afresh and the full log is not yours
to see.

## Watch a run

Add `--log run.simlog` to any run, then serve the viewer over HTTP
(opening the file directly will not work — it loads wasm as a module):

```sh
python3 -m http.server -d viewer/web 8080
```

Open <http://localhost:8080> and drag `run.simlog` onto the page.

## Reference

| File | What it covers |
|------|----------------|
| `docs/a3-logging.md` | Keepalives, samples, counters, reports and budgets, the logging switch and the cap, `detect.wasm`. |
| `docs/switch-programs.md` | Your API: the SDK, the TinyVM text format, the raw wasm ABI. |
| `docs/LIMITS.md` | Fuel, output size, buffer, queues, control time. |
| `docs/tinyvm.md` | The data-plane instruction set. |
| `docs/sim_log.md` | The `.simlog` format and the viewer. |
| `SUBMITTING.md` | How to submit: published when the submission server opens. |
