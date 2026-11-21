#!/usr/bin/env python3
"""Queries the server-list status of a VoidMC server (VMC-ATP-002, VMC-ATP-024).

Sends Handshake (next state Status), StatusRequest and Ping, like the vanilla server list.

Example:
    python3 tools/atp/status-ping.py --server 127.0.0.1:25565

Prints one "name: value" line per field:
    version name, protocol, motd, max players, online players, ping echo
Exit status: 0 when the server answered both requests, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import socket
import struct
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atplib import PROTOCOL, parse_address, read_string, read_varint, write_string, write_varint  # noqa: E402


def frame(packet_id: int, body: bytes = b"") -> bytes:
    packet = write_varint(packet_id) + body
    return write_varint(len(packet)) + packet


def read_frame(sock: socket.socket) -> tuple[int, bytes]:
    def read_exact(count: int) -> bytes:
        data = b""
        while len(data) < count:
            chunk = sock.recv(count - len(data))
            if not chunk:
                raise EOFError("connection closed by server")
            data += chunk
        return data

    length = shift = 0
    while True:
        byte = read_exact(1)[0]
        length |= (byte & 0x7F) << shift
        shift += 7
        if byte < 0x80:
            break
    data = read_exact(length)
    packet_id, k = read_varint(data, 0)
    return packet_id, data[k:]


def motd_text(description) -> str:
    if isinstance(description, str):
        return description
    text = description.get("text", "")
    for extra in description.get("extra", []):
        text += motd_text(extra)
    return text


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--server", default="127.0.0.1:25565", help="server address (default 127.0.0.1:25565)")
    parser.add_argument("--timeout", type=float, default=5.0, help="seconds to wait for each answer (default 5)")
    args = parser.parse_args()
    host, port = parse_address(args.server)
    try:
        with socket.create_connection((host, port), timeout=args.timeout) as sock:
            sock.sendall(frame(0x00, write_varint(PROTOCOL) + write_string(host) + struct.pack(">H", port) + write_varint(1)))
            sock.sendall(frame(0x00))
            packet_id, body = read_frame(sock)
            if packet_id != 0x00:
                print(f"unexpected status packet id 0x{packet_id:02x}")
                return 1
            status = json.loads(read_string(body, 0)[0])
            token = int(time.time() * 1000)
            sock.sendall(frame(0x01, struct.pack(">q", token)))
            packet_id, body = read_frame(sock)
            echo = packet_id == 0x01 and struct.unpack(">q", body[:8])[0] == token
    except (OSError, EOFError, ValueError) as error:
        print(f"no answer from {args.server}: {error}")
        return 1
    version = status.get("version", {})
    players = status.get("players", {})
    print(f"version name: {version.get('name')}")
    print(f"protocol: {version.get('protocol')}")
    print(f"motd: {motd_text(status.get('description', ''))}")
    print(f"max players: {players.get('max')}")
    print(f"online players: {players.get('online')}")
    print(f"ping echo: {'yes' if echo else 'no'}")
    return 0 if echo else 1


if __name__ == "__main__":
    sys.exit(main())
