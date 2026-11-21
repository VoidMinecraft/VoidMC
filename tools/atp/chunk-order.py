#!/usr/bin/env python3
"""Checks chunk streaming in a recording made by capture-proxy.py (VMC-ATP-019).

Reads a server-to-client recording, compressed or not, and replays the chunk packets
(ChunkDataAndLight 0x2D, UnloadChunk 0x25, SetCenterChunk 0x5E) of the Play state.

Definitions:
  * view square: the chunks with max(|dx|, |dz|) <= R around the centre chunk, where R is
    the effective view distance, min(client render distance, server view distance).
  * interval: the packets between two SetCenterChunk packets. The first interval starts
    at the join; its centre is (0, 0) until the server sends a SetCenterChunk.
  * order: inside an interval, every chunk must have a squared distance dx*dx + dz*dz from
    the centre that is not smaller than that of the chunk sent before it. A chunk that is
    nearer than the chunk before it is an order inversion. The join is included.

Example:
    python3 tools/atp/chunk-order.py --radius 10 atp-019/conn-1-server-to-client.bin

It prints one "name: value" line per check, then "result: PASS" or "result: FAIL".
Exit status: 0 on PASS, 1 on FAIL, 2 when the recording has no Play-state chunks.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atplib import (  # noqa: E402
    PLAY_CHUNK_DATA,
    PLAY_SET_CENTER_CHUNK,
    PLAY_UNLOAD_CHUNK,
    iter_server_stream,
    read_varint,
)


def chebyshev(a, b) -> int:
    return max(abs(a[0] - b[0]), abs(a[1] - b[1]))


def distance_squared(a, b) -> int:
    return (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2


def square(center, radius) -> set:
    return {(center[0] + dx, center[1] + dz)
            for dx in range(-radius, radius + 1) for dz in range(-radius, radius + 1)}


def analyse(data: bytes, radius: int | None):
    center = (0, 0)
    loaded: dict[tuple[int, int], str] = {}
    first_hash: dict[tuple[int, int], str] = {}
    intervals = [{"center": center, "chunks": []}]
    interval_ends = []  # (center, loaded set) at the end of every interval
    sent_while_loaded = resent_different = unloads = chunk_packets = 0
    for _, phase, packet_id, body in iter_server_stream(data):
        if phase != "play":
            continue
        if packet_id == PLAY_SET_CENTER_CHUNK:
            x, k = read_varint(body, 0)
            z, _ = read_varint(body, k)
            interval_ends.append((center, set(loaded)))
            center = (x, z)
            intervals.append({"center": center, "chunks": []})
        elif packet_id == PLAY_CHUNK_DATA:
            position = struct.unpack_from(">ii", body, 0)
            digest = hashlib.sha256(body).hexdigest()
            chunk_packets += 1
            if position in loaded:
                sent_while_loaded += 1
            if position in first_hash and first_hash[position] != digest:
                resent_different += 1
            first_hash.setdefault(position, digest)
            loaded[position] = digest
            intervals[-1]["chunks"].append(position)
        elif packet_id == PLAY_UNLOAD_CHUNK:
            z, x = struct.unpack_from(">ii", body, 0)  # z first in this packet
            loaded.pop((x, z), None)
            unloads += 1
    interval_ends.append((center, set(loaded)))
    if chunk_packets == 0:
        return None

    # The join interval is the first one with chunks: the server may send a
    # SetCenterChunk before the join batch.
    join_index = next(i for i, interval in enumerate(intervals) if interval["chunks"])
    join_center, join_loaded = interval_ends[join_index]
    observed_radius = max(chebyshev(c, join_center) for c in join_loaded)
    r = observed_radius if radius is None else radius

    outside_sent = 0
    inversions = []
    for interval in intervals:
        c = interval["center"]
        previous = None
        for position in interval["chunks"]:
            if chebyshev(position, c) > r:
                outside_sent += 1
            d2 = distance_squared(position, c)
            if previous is not None and d2 < previous[1]:
                inversions.append((c, previous[0], previous[1], position, d2))
            previous = (position, d2)

    extra_at_interval_end = sum(1 for c, s in interval_ends if any(chebyshev(p, c) > r for p in s))
    final_center, final_loaded = interval_ends[-1]
    return {
        "observed_radius": observed_radius,
        "radius": r,
        "join_equal": join_loaded == square(join_center, r),
        "join_count": len(join_loaded),
        "final_equal": final_loaded == square(final_center, r),
        "final_center": final_center,
        "final_count": len(final_loaded),
        "centers": [i["center"] for i in intervals[1:]],
        "chunk_packets": chunk_packets,
        "unloads": unloads,
        "outside_sent": outside_sent,
        "extra_at_interval_end": extra_at_interval_end,
        "sent_while_loaded": sent_while_loaded,
        "resent_different": resent_different,
        "inversions": inversions,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("recording", type=Path, help="a conn-N-server-to-client.bin file")
    parser.add_argument("--radius", type=int, help="expected effective view distance; default: the radius seen at join")
    args = parser.parse_args()

    result = analyse(args.recording.read_bytes(), args.radius)
    if result is None:
        print("no Play-state chunk packets in this recording (status ping or failed join?)")
        return 2
    r = result["radius"]
    side = 2 * r + 1
    yes = lambda flag: "yes" if flag else "no"  # noqa: E731
    checks = {
        f"effective view distance": str(result["observed_radius"]),
        f"loaded set after join equals square of radius {r} ({side * side} chunks)":
            f"{yes(result['join_equal'])} ({result['join_count']} loaded)",
        f"loaded set at end equals square of radius {r}":
            f"{yes(result['final_equal'])} ({result['final_count']} loaded around {result['final_center']})",
        "chunks sent outside the view square of the current centre": str(result["outside_sent"]),
        "intervals ending with a chunk outside the view square": str(result["extra_at_interval_end"]),
        "chunks sent while loaded": str(result["sent_while_loaded"]),
        "re-sent chunks with different content": str(result["resent_different"]),
        "order inversions": str(len(result["inversions"])),
    }
    print(f"recording: {args.recording}")
    print(f"chunk packets: {result['chunk_packets']}, unloads: {result['unloads']}, centre changes: {len(result['centers'])}")
    for name, value in checks.items():
        print(f"{name}: {value}")
    for c, a, da, b, db in result["inversions"][:10]:
        print(f"  inversion at centre {c}: chunk {a} d2={da} sent before chunk {b} d2={db}")
    passed = (
        (args.radius is None or result["observed_radius"] == args.radius)
        and result["join_equal"] and result["final_equal"]
        and result["outside_sent"] == 0 and result["extra_at_interval_end"] == 0
        and result["sent_while_loaded"] == 0 and result["resent_different"] == 0
        and not result["inversions"]
    )
    print("result:", "PASS" if passed else "FAIL")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
