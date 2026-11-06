#!/usr/bin/env python3
"""Lines up what a client and the server say about the same moments.

Each spike of `client-perf.log` is matched, by timestamp, with the pauses of a Java
Flight Recorder file and the holes and late ticks of the server's log, and gets a
probable cause.

    stutters.py --perf client-perf.log --server server.log --jfr client.jfr

All three clocks are the same machine's, in UTC. Any of `--server` and `--jfr` can be
left out. `jfr` comes with a JDK: `--jfr-tool` says where if it is not on the PATH.
"""

import argparse
import bisect
import collections
import datetime
import json
import re
import subprocess
import sys

# A frame is drawn after what held it up: anything this close before it counts.
SLACK_S = 0.05
EVENTS = (
    "jdk.GarbageCollection,jdk.SafepointBegin,jdk.Compilation,"
    "jdk.ExecutionSample,jdk.JavaMonitorEnter,jdk.ThreadSleep"
)
# The game names it, but a recording taken with -XstartOnFirstThread may say `main`.
RENDER_THREADS = ("Render thread", "main")
ANSI = re.compile(r"\x1b\[[0-9;]*m")
SERVER_LINE = re.compile(r"^(\S+Z)\s.*map (report hole|late tick).*gap_ms=(\d+)")
PERF_SPIKE = re.compile(r"^(\S+Z) spike frame_ms=(\d+)(.*)$")
PERF_SECOND = re.compile(r"^(\S+Z) second fps=(\d+)")


def seconds(stamp):
    """Epoch seconds of an ISO stamp, however many digits it has."""
    text = stamp.replace("Z", "+00:00")
    head, dot, tail = text.partition(".")
    if dot:
        digits, sign, zone = re.match(r"(\d+)([+-])(.*)", tail).groups()
        text = f"{head}.{digits[:6].ljust(6, '0')}{sign}{zone}"
    return datetime.datetime.fromisoformat(text).timestamp()


def duration(value):
    """Seconds of a duration as `jfr print --json` writes it, such as `PT0.012S`."""
    if isinstance(value, (int, float)):
        return value / 1e9
    match = re.fullmatch(r"PT(?:(\d+)H)?(?:(\d+)M)?(?:([\d.]+)S)?", value or "")
    if not match:
        return 0.0
    hours, minutes, secs = match.groups()
    return int(hours or 0) * 3600 + int(minutes or 0) * 60 + float(secs or 0)


def read_perf(path):
    spikes, seconds_seen = [], []
    with open(path, errors="replace") as lines:
        for line in lines:
            if match := PERF_SPIKE.match(line):
                at, ms, where = match.groups()
                spikes.append((seconds(at), int(ms), where.strip()))
            elif match := PERF_SECOND.match(line):
                seconds_seen.append(int(match.group(2)))
    return spikes, seconds_seen


def read_server(path):
    events = []
    with open(path, errors="replace") as lines:
        for line in lines:
            if match := SERVER_LINE.match(ANSI.sub("", line)):
                at, kind, gap = match.groups()
                events.append((seconds(at), kind, int(gap)))
    return sorted(events)


def read_jfr(path, tool):
    printed = subprocess.run(
        [tool, "print", "--json", "--events", EVENTS, path],
        capture_output=True,
        text=True,
        check=True,
    )
    events = json.loads(printed.stdout)["recording"]["events"]
    pauses, samples = [], []
    for event in events:
        kind = event["type"].removeprefix("jdk.")
        values = event["values"]
        start = seconds(values["startTime"])
        if kind == "ExecutionSample":
            thread = (values.get("sampledThread") or {}).get("javaName")
            frames = (values.get("stackTrace") or {}).get("frames") or []
            if thread in RENDER_THREADS and frames:
                method = frames[0]["method"]
                samples.append((start, f"{method['type']['name']}.{method['name']}"))
            continue
        length = duration(values.get("sumOfPauses") or values.get("duration"))
        if kind == "Compilation" or length > 0.002:
            pauses.append((start, start + length, kind, length))
    return sorted(pauses), sorted(samples)


def overlapping(pauses, start, end):
    return [pause for pause in pauses if pause[0] < end and pause[1] > start]


def cause(spike_ms, gc, safepoints, compiled, monitors, server):
    held = spike_ms / 1000
    if sum(pause[3] for pause in gc) > held * 0.4:
        return "garbage collection"
    if sum(pause[3] for pause in safepoints) > held * 0.4:
        return "safepoint"
    if sum(pause[3] for pause in monitors) > held * 0.4:
        return "lock contention"
    if server:
        return "server or network"
    if compiled:
        return "JIT compilation, perhaps"
    return "the render thread itself"


def report(spikes, fps, server, pauses, samples):
    print(f"{len(spikes)} spikes", end="")
    if fps:
        print(f", {min(fps)} to {max(fps)} fps over {len(fps)} seconds", end="")
    print()
    causes = collections.Counter()
    moments = [sample[0] for sample in samples]
    for at, ms, where in sorted(spikes, key=lambda spike: -spike[1]):
        start, end = at - ms / 1000 - SLACK_S, at + SLACK_S
        near = overlapping(pauses, start, end)
        by_kind = lambda kind: [pause for pause in near if pause[2] == kind]
        heard = [event for event in server if start <= event[0] <= end]
        why = cause(
            ms,
            by_kind("GarbageCollection"),
            by_kind("SafepointBegin"),
            by_kind("Compilation"),
            by_kind("JavaMonitorEnter"),
            heard,
        )
        causes[why] += 1
        low, high = bisect.bisect(moments, start), bisect.bisect(moments, end)
        top = collections.Counter(name for _, name in samples[low:high]).most_common(2)
        stamp = datetime.datetime.fromtimestamp(at, datetime.UTC).strftime("%H:%M:%S.%f")[:-3]
        line = f"{stamp}  {ms:5d} ms  {why}"
        if heard:
            line += "  | server: " + ", ".join(f"{e[1]} {e[2]} ms" for e in heard[:3])
        if top:
            line += "  | in: " + ", ".join(name for name, _ in top)
        print(line + (f"  | {where}" if where else ""))
    print("\nprobable causes:")
    for why, count in causes.most_common():
        print(f"{count:5d}  {why}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--perf", required=True)
    parser.add_argument("--server")
    parser.add_argument("--jfr")
    parser.add_argument("--jfr-tool", default="jfr")
    args = parser.parse_args()
    spikes, fps = read_perf(args.perf)
    server = read_server(args.server) if args.server else []
    pauses, samples = read_jfr(args.jfr, args.jfr_tool) if args.jfr else ([], [])
    report(spikes, fps, server, pauses, samples)


if __name__ == "__main__":
    sys.exit(main())
