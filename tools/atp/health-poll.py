#!/usr/bin/env python3
"""Polls the VoidMC /health endpoint and prints every change of state (VMC-ATP-032).

The `stopping` state can last only milliseconds before the port closes, so a person
with curl cannot see it. This tool polls every 5 ms and prints one line per change:
    HH:MM:SS.mmm  HTTP 503 starting
    HH:MM:SS.mmm  HTTP 200 ok
    HH:MM:SS.mmm  refused
A refused connection means the port is closed (the server has stopped or not started).

Example (start it before the server, stop it with Ctrl+C or wait for --seconds):
    python3 tools/atp/health-poll.py --address 127.0.0.1:9464 --seconds 120

At the end it prints "states seen:" with each state in the order it first appeared.
"""

from __future__ import annotations

import argparse
import re
import socket
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atplib import parse_address  # noqa: E402


def query(address) -> str:
    try:
        with socket.create_connection(address, timeout=1) as sock:
            sock.sendall(b"GET /health HTTP/1.1\r\nHost: atp\r\nConnection: close\r\n\r\n")
            data = b""
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                data += chunk
    except ConnectionRefusedError:
        return "refused"
    except OSError as error:
        return f"error {type(error).__name__}"
    head, _, body = data.partition(b"\r\n\r\n")
    parts = head.split(b" ", 2)
    code = parts[1].decode() if len(parts) > 1 else "?"
    status = re.search(rb'"status":"(\w+)"', body)
    return f"HTTP {code} {status.group(1).decode() if status else '?'}"


def stamp() -> str:
    now = time.time()
    return time.strftime("%H:%M:%S", time.localtime(now)) + f".{int(now * 1000) % 1000:03d}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--address", default="127.0.0.1:9464", help="observability address (default 127.0.0.1:9464)")
    parser.add_argument("--seconds", type=float, default=300.0, help="stop after this many seconds (default 300)")
    parser.add_argument("--interval-ms", type=float, default=5.0, help="time between polls (default 5)")
    args = parser.parse_args()
    address = parse_address(args.address)
    seen: list[str] = []
    last = None
    deadline = time.time() + args.seconds
    try:
        while time.time() < deadline:
            state = query(address)
            if state != last:
                print(f"{stamp()}  {state}", flush=True)
                if state not in seen:
                    seen.append(state)
                last = state
            time.sleep(args.interval_ms / 1000)
    except KeyboardInterrupt:
        pass
    print("states seen: " + ", ".join(seen))
    return 0


if __name__ == "__main__":
    sys.exit(main())
