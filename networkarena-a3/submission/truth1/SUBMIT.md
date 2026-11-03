# Truth run 1: ready to upload

**File to upload:** `switch.wasm` in this folder, to the Canvas assignment **A3 — Truth run**.
One file, one person for the whole team. Wait for the reply (a few minutes) before sending anything else.

Built from `a3truth/` for the real network (logger = switch 7, no `practice` feature):

    (cd a3truth && cargo build --release --target wasm32-unknown-unknown)
    cp a3truth/target/wasm32-unknown-unknown/release/a3truth.wasm submission/truth1/switch.wasm

sha256 is in `switch.wasm.sha256`.

## Why the first attempt did not finish (fixed in this build)
Truth run A, first try: "your program stopped on at least one switch". Cause, reproduced locally: the controller gets a copy of
every counter array on every timer, and the whole timer event must fit in a **4,096-byte input buffer**. The first build kept
(source, destination) customer matrices in 9 blocks; with about 20 or more customers that overflowed, and every switch stopped at
t=0 ("the event did not fit in the 4,096-byte input buffer"). The practice worlds have 6 to 13 customers, so it never showed.
Now the counters are one small array (per-customer rows, plus a per-pair block only while there are 23 customers or fewer), at most
about 1,500 slots at any customer count.

## Checked before this upload (practice, as the server would run it)
- No `program failed` line on any of these, and 0 events lost, 0 refused log writes: 15-switch worlds at loads 1.0, 1.25 and 1.5
  (39.5 to 40.5 KB of 64 KB, 60%), the extras world (59%), and worlds with 23, 56 and 100 customers (67%, 82%, 80%).
- The three practice truth worlds (6 switches): the submission build runs clean but has no switch 7, so it writes no log there; with the
  `practice` feature (logger = switch 1) it writes about 18 KB.

## What comes back, and what to do with it
You get `team.log` and `customers.txt`. Read the log:

    a3truth/target/release/logdump team.log 65536            # sizes per type, then the records
    a3truth/target/release/logdump team.log 65536 --all      # also rates, flows, port stats, raw samples

Build `logdump` first with `(cd a3truth && cargo build --release --bin logdump)`.
Customer ids in the log are indexes into the contract list (address order); `customers.txt` gives the
address ranges, so map them to names there. Read `LOGGING_DESIGN.md` for what each record means.

## Budget
This spends **1 of 3** truth runs. A program that does not install, or a run that errors, is free for the
first two such errors. Keep the returned log: it is the only look at the hidden network.

Raw samples are rationed (800 B a second in all, 80 B from any one switch) so they cover the whole 40 s; a backstop stops them at 50 KB.
