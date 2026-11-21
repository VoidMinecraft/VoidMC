#!/usr/bin/env python3
"""Transparent TCP forwarder that records every connection, for the VoidMC acceptance tests.

The client joins the proxy address instead of the server. Each accepted connection N
is written to OUT/conn-N-server-to-client.bin and OUT/conn-N-client-to-server.bin,
byte for byte (compressed frames stay compressed; the analysers decompress them).

Example:
    python3 tools/atp/capture-proxy.py --listen 127.0.0.1:25566 \
        --upstream 127.0.0.1:25565 --out atp-019

Stop it with Ctrl+C. Each connection prints one line when it opens and one when it closes.
"""

from __future__ import annotations

import argparse
import itertools
import socket
import sys
import threading
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atplib import parse_address  # noqa: E402


def pump(source: socket.socket, target: socket.socket, path: Path) -> None:
    with path.open("wb") as out:
        try:
            while True:
                data = source.recv(65536)
                if not data:
                    break
                out.write(data)
                out.flush()
                target.sendall(data)
        except OSError:
            pass
        finally:
            try:
                target.shutdown(socket.SHUT_WR)
            except OSError:
                pass


def serve(client: socket.socket, upstream: tuple[str, int], out: Path, number: int) -> None:
    try:
        server = socket.create_connection(upstream)
    except OSError as error:
        print(f"conn-{number}: cannot reach upstream {upstream[0]}:{upstream[1]}: {error}", flush=True)
        client.close()
        return
    print(f"conn-{number}: open", flush=True)
    threads = [
        threading.Thread(target=pump, args=(client, server, out / f"conn-{number}-client-to-server.bin")),
        threading.Thread(target=pump, args=(server, client, out / f"conn-{number}-server-to-client.bin")),
    ]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    client.close()
    server.close()
    size = (out / f"conn-{number}-server-to-client.bin").stat().st_size
    print(f"conn-{number}: closed, {size} bytes from server", flush=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--listen", default="127.0.0.1:25566", help="address the client joins (default 127.0.0.1:25566)")
    parser.add_argument("--upstream", default="127.0.0.1:25565", help="server address (default 127.0.0.1:25565)")
    parser.add_argument("--out", required=True, type=Path, help="directory for the recordings (created if missing)")
    args = parser.parse_args()

    args.out.mkdir(parents=True, exist_ok=True)
    listen = parse_address(args.listen)
    upstream = parse_address(args.upstream)
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(listen)
    listener.listen(8)
    print(f"capture-proxy: {args.listen} -> {args.upstream}, writing to {args.out}/", flush=True)
    try:
        for number in itertools.count(1):
            client, _ = listener.accept()
            threading.Thread(target=serve, args=(client, upstream, args.out, number), daemon=True).start()
    except KeyboardInterrupt:
        print("capture-proxy: stopped", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
