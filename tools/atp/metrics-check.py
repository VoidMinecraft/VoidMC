#!/usr/bin/env python3
"""Scrapes VoidMC /metrics and checks it against the documented families (VMC-ATP-032).

The documented families are listed in void-docs/docs/reference/server/observability.md
("/metrics"). The tool checks the Prometheus text format: every family has # HELP and
# TYPE lines, every sample line is "name{labels} value", counters end in _total.

Example:
    python3 tools/atp/metrics-check.py --address 127.0.0.1:9464

Prints the content type, "missing families:", "undocumented families:", "format errors:",
the values of the player and tick metrics, then "result: PASS" or "result: FAIL".
"""

from __future__ import annotations

import argparse
import re
import socket
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atplib import parse_address  # noqa: E402

DOCUMENTED = {
    "voidmc_ticks_total": "counter",
    "voidmc_tick_duration_seconds": "histogram",
    "voidmc_target_ticks_per_second": "gauge",
    "voidmc_ticks_per_second": "gauge",
    "voidmc_seconds_since_last_tick": "gauge",
    "voidmc_players_online": "gauge",
    "voidmc_players_max": "gauge",
    "voidmc_players_joined_total": "counter",
    "voidmc_connections_accepted_total": "counter",
    "voidmc_connection_errors_total": "counter",
    "voidmc_clients_kicked_total": "counter",
    "voidmc_packets_unrecognized_total": "counter",
    "voidmc_packet_errors_total": "counter",
    "voidmc_chunk_load_errors_total": "counter",
}
SAMPLE = re.compile(r'^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{[^}]*\})? (-?[0-9.eE+-]+|NaN|\+Inf|-Inf)$')
SHOWN = ["voidmc_players_online", "voidmc_players_max", "voidmc_ticks_total",
         "voidmc_target_ticks_per_second", "voidmc_ticks_per_second"]


def scrape(address) -> tuple[str, str, str]:
    with socket.create_connection(address, timeout=5) as sock:
        sock.sendall(b"GET /metrics HTTP/1.1\r\nHost: atp\r\nConnection: close\r\n\r\n")
        data = b""
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            data += chunk
    head, _, body = data.partition(b"\r\n\r\n")
    lines = head.decode(errors="replace").split("\r\n")
    content_type = next((l.split(":", 1)[1].strip() for l in lines if l.lower().startswith("content-type:")), "")
    return lines[0], content_type, body.decode(errors="replace")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--address", default="127.0.0.1:9464", help="observability address (default 127.0.0.1:9464)")
    args = parser.parse_args()
    try:
        status_line, content_type, body = scrape(parse_address(args.address))
    except OSError as error:
        print(f"no answer from {args.address}: {error}")
        return 1
    helps, types, errors, values = set(), {}, [], {}
    sampled = set()
    for number, line in enumerate(body.splitlines(), 1):
        if not line:
            continue
        if line.startswith("# HELP "):
            helps.add(line.split()[2])
        elif line.startswith("# TYPE "):
            parts = line.split()
            types[parts[2]] = parts[3] if len(parts) > 3 else ""
        elif line.startswith("#"):
            continue
        else:
            match = SAMPLE.match(line)
            if not match:
                errors.append(f"line {number}: {line}")
                continue
            name = match.group(1)
            family = re.sub(r"_(bucket|sum|count)$", "", name) if re.sub(r"_(bucket|sum|count)$", "", name) in types else name
            if family not in types:
                errors.append(f"line {number}: sample {name} has no # TYPE line")
            sampled.add(family)
            if not match.group(2):
                values[name] = match.group(3)
    for family, kind in types.items():
        if family not in helps:
            errors.append(f"{family}: no # HELP line")
        if kind == "counter" and not family.endswith("_total"):
            errors.append(f"{family}: counter name does not end in _total")
        if family in DOCUMENTED and DOCUMENTED[family] != kind:
            errors.append(f"{family}: type {kind}, documented {DOCUMENTED[family]}")
    missing = sorted(set(DOCUMENTED) - set(types))
    undocumented = sorted(set(types) - set(DOCUMENTED))
    print(f"status: {status_line}")
    print(f"content type: {content_type}")
    print(f"families: {len(types)} (documented: {len(DOCUMENTED)})")
    print(f"missing families: {', '.join(missing) or 'none'}")
    print(f"undocumented families: {', '.join(undocumented) or 'none'}")
    print(f"format errors: {len(errors)}")
    for error in errors[:10]:
        print(f"  {error}")
    for name in SHOWN:
        print(f"{name}: {values.get(name, 'no sample')}")
    passed = (status_line.split(" ")[1:2] == ["200"] and content_type.startswith("text/plain; version=0.0.4")
              and not missing and not undocumented and not errors)
    print("result:", "PASS" if passed else "FAIL")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
