#!/usr/bin/env python3
"""Compares a SingleThreaded and a MultiThreaded run of the region-parallel fixture (VMC-ATP-028).

Each run is the output prefix given to the patched example with ATP_TRACE_OUT; the run
wrote PREFIX.trace, PREFIX.ticks and PREFIX.invocations
(see tools/atp/patches/region-parallel.patch).

Example:
    python3 tools/atp/region-trace-analyze.py --serial atp-028-serial --multi atp-028-multi

For each run it prints the ticks recorded, whether the tick numbers are contiguous, the
ticks where the entity count is not 120, the ticks where a region ran twice, the ticks
that used more than one thread and the ticks where two regions ran at the same time.
Then it prints "traces identical: yes|no" and "result: PASS" or "result: FAIL".
PASS means: both runs complete and clean, the serial run never overlaps, the multi run
overlaps on at least half of its ticks, and the per-tick traces are byte-identical.
"""

from __future__ import annotations

import argparse
import collections
import re
import sys
from pathlib import Path

LINE = re.compile(r"tick=(\d+) region=\((-?\d+),(-?\d+)\) entities=(\d+) thread=ThreadId\((\d+)\) enter_ns=(\d+) exit_ns=(\d+)")
TICKS = 120
PIGS = 120


def summarise(prefix: str) -> dict:
    calls = collections.defaultdict(list)
    for line in Path(f"{prefix}.invocations").read_text().splitlines():
        match = LINE.match(line)
        if match:
            tick, rx, rz, entities, thread, enter, leave = map(int, match.groups())
            calls[tick].append((rx, rz, entities, thread, enter, leave))
    tick_numbers = [int(l.split()[0][5:]) for l in Path(f"{prefix}.ticks").read_text().splitlines() if l]
    ticks = sorted(calls)
    wrong_count = [t for t in ticks if sum(c[2] for c in calls[t]) != PIGS]
    twice = [t for t in ticks if len({(c[0], c[1]) for c in calls[t]}) != len(calls[t])]
    several_threads = [t for t in ticks if len({c[3] for c in calls[t]}) > 1]
    overlapping = [t for t in ticks if any(a[4] < b[5] and b[4] < a[5]
                                            for i, a in enumerate(calls[t]) for b in calls[t][i + 1:])]
    return {
        "ticks": len(ticks),
        "contiguous": tick_numbers == list(range(1, TICKS + 1)),
        "wrong_count": len(wrong_count),
        "twice": len(twice),
        "several_threads": len(several_threads),
        "overlapping": len(overlapping),
        "trace": Path(f"{prefix}.trace").read_bytes(),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--serial", required=True, help="output prefix of the SingleThreaded run")
    parser.add_argument("--multi", required=True, help="output prefix of the MultiThreaded run")
    args = parser.parse_args()
    runs = {"serial": summarise(args.serial), "multi": summarise(args.multi)}
    for name, r in runs.items():
        print(f"{name}: ticks={r['ticks']} contiguous={'yes' if r['contiguous'] else 'no'} "
              f"wrong_entity_count={r['wrong_count']} region_twice={r['twice']} "
              f"ticks_with_several_threads={r['several_threads']} ticks_with_overlap={r['overlapping']}")
    identical = runs["serial"]["trace"] == runs["multi"]["trace"]
    rows = runs["serial"]["trace"].count(b"\n")
    print(f"trace rows: {rows}")
    print(f"traces identical: {'yes' if identical else 'no'}")
    clean = all(r["ticks"] == TICKS and r["contiguous"] and not r["wrong_count"] and not r["twice"] for r in runs.values())
    passed = (clean and identical and runs["serial"]["overlapping"] == 0 and runs["serial"]["several_threads"] == 0
              and runs["multi"]["overlapping"] >= TICKS // 2)
    print("result:", "PASS" if passed else "FAIL")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
