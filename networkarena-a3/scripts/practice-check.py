#!/usr/bin/env python3
"""Score a claim on the A3 practice runs, as the server would.

    scripts/practice-check.py --switch switch.wasm --detect detect.wasm [--egg 21]

Checks that both programs load (the switch program on a 1 ms run, the
detector on an empty log), then runs every practice run of the chosen eggs
(all three without --egg): the switch program writes its log, the detector
reads it, and the answer is right when it is `true` exactly on the runs
whose egg is on. One table per egg: the run, its load, whether the egg is
in it and what the detector said. Then the card the server would show
for this claim, or that there is none: one wrong answer loses an egg.

Exit 0 when every run is right, 1 when one is wrong, 2 when a program does
not load (the server would not count such a claim).
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORLDS = ROOT / "worlds/practice"
SIM = ROOT / "target/release/competitive_net_sim"


def read_toml(text: str) -> dict:
    """The small part of TOML answers.toml uses: `key = value` lines and
    `[[name]]` tables."""
    out: dict = {}
    table = out
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        m = re.fullmatch(r"\[\[(\w+)\]\]", line)
        if m:
            table = {}
            out.setdefault(m.group(1), []).append(table)
            continue
        key, value = (x.strip() for x in line.split("=", 1))
        table[key] = json.loads(value)
    return out


def run(cmd: list) -> subprocess.CompletedProcess:
    return subprocess.run([str(c) for c in cmd], cwd=ROOT, capture_output=True, text=True)


def loads(switch: Path, detect: Path, world: Path, work: Path) -> str | None:
    """Why a program does not load, or None."""
    v = run([SIM, "run-world", world, "--program", switch, "--until-ms", 1])
    if v.returncode != 0:
        return "the switch program does not load:\n" + (v.stderr or v.stdout).strip()[-600:]
    empty = work / "empty.team"
    empty.write_bytes(b"")
    a = run([SIM, "detect", detect, empty]).stdout.strip()
    if a.startswith("failed:"):
        return "the detector does not load: " + a.removeprefix("failed: ")
    return None


def answer(switch: Path, detect: Path, r: dict, work: Path, worlds: Path) -> dict:
    team = work / f"e{r['egg']:02}-r{r['run']:02}.team"
    sim = run([SIM, "run-world", worlds / r["world"], "--load-scale", r["load"], "--program", switch, "--team-log", team])
    out = {**r, "meant": "true" if r["on"] else "false"}
    if sim.returncode != 0 or not team.exists():
        out.update(answer="no run", right=False)
        return out
    a = run([SIM, "detect", detect, team]).stdout.strip()
    out.update(answer=a, right=a == out["meant"])
    team.unlink()
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--switch", type=Path, required=True, help="your switch.wasm")
    ap.add_argument("--detect", type=Path, required=True, help="your detect.wasm")
    ap.add_argument("--egg", type=int, action="append", help="a practice egg to run (repeatable); default all")
    ap.add_argument("--jobs", type=int, default=os.cpu_count())
    ap.add_argument("--worlds", type=Path, default=WORLDS, help="the practice runs (default worlds/practice)")
    args = ap.parse_args()
    worlds = args.worlds
    if not (worlds / "answers.toml").exists():
        sys.exit("no practice worlds: expected worlds/practice/")
    if not SIM.exists():
        sys.exit(f"{SIM}: build the simulator first (cargo build --release --features wasm)")

    answers = read_toml((worlds / "answers.toml").read_text())
    eggs = {e["n"]: e for e in answers["egg"]}
    runs = [r for r in answers["run"] if not args.egg or r["egg"] in args.egg]
    if not runs:
        sys.exit(f"no practice egg {args.egg}; there are {sorted(eggs)}")

    work = Path(tempfile.mkdtemp(prefix="practice-check-"))
    why = loads(args.switch.resolve(), args.detect.resolve(), worlds / runs[0]["world"], work)
    if why:
        print(f"INVALID  {why}")
        return 2
    with cf.ThreadPoolExecutor(args.jobs) as pool:
        got = list(pool.map(lambda r: answer(args.switch.resolve(), args.detect.resolve(), r, work, worlds), runs))

    played = sorted({g["egg"] for g in got})
    won = [n for n in played if all(g["right"] for g in got if g["egg"] == n)]
    yes_no = {"true": "yes", "false": "no", "no run": "none"}
    print(f"practice-check  {args.switch.name} + {args.detect.name}  on "
          + ("egg {0}, {1} ({2})".format(played[0], eggs[played[0]]["name"], eggs[played[0]]["category"])
             if len(played) == 1 else "the practice eggs"))
    for n in played:
        mine = [g for g in got if g["egg"] == n]
        if len(played) > 1:
            print(f"\negg {n}, {eggs[n]['name']} ({eggs[n]['category']})")
        print("\n run  load   egg   said")
        for g in mine:
            print(f"  {g['run']:>2}  {g['load']:<5.2f}  {yes_no[g['meant']]:<5} {yes_no.get(g['answer'], g['answer']):<6} "
                  + ("ok" if g["right"] else "WRONG"))
        right = sum(g["right"] for g in mine)
        print(f"\n{right} of {len(mine)} right.", end="" if len(played) == 1 else "\n")
    lead = "  " if len(played) == 1 else "\n"
    if won:
        print(lead + "Card:  " + "   ".join(f"Egg {n} · {eggs[n]['category']} · {eggs[n]['name']}" for n in won))
    else:
        print(lead + "The server would show no card for this claim: one wrong answer loses the egg.")
    return 0 if len(won) == len(played) else 1


if __name__ == "__main__":
    sys.exit(main())
