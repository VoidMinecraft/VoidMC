#!/usr/bin/env python3
"""Sends invalid, stale and forged inventory clicks to a VoidMC server (VMC-ATP-014).

A vanilla client cannot send these clicks, so this offline protocol client builds
ServerboundContainerClick (0x12) packets by hand. It needs:
  * the server started with VOID_EXAMPLE_OPERATORS=1 (the tool runs /clear and /give);
  * tools/atp/patches/inventory-trace.patch applied, so that the server log has an
    "ATP inventory" line with the server's own slots after every change.

Example:
    python3 tools/atp/inventory-clicks.py --server 127.0.0.1:25565 --server-log logs/void-1763650000000.log

Before every case the tool resets the inventory to 20 stone and 16 dirt. For each case it prints:
    === <case>
      total(before)=N total(after)=M  conn_alive_and_server_responds=True|False  server_resends=K  disconnect=...
total is the server's item count (slots + cursor) from the server log. A case passes when
total(after) <= total(before) (no duplication) and the server still answers /list.
Cases 09a-09d send malformed packets on a separate connection: the server must disconnect
that connection only and accept a new login afterwards. The tool ends with "done".
Exit status: 0 when every case passed, 1 otherwise.
"""

from __future__ import annotations

import argparse
import re
import struct
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atplib import OfflineClient, hashed_slot, parse_address, write_varint  # noqa: E402

ANSI = re.compile(r"\x1b\[[0-9;]*m")
TRACE = "ATP inventory"


class Session:
    def __init__(self, address, server_log: Path):
        self.address = address
        self.server_log = server_log
        self.failures = 0

    def log_lines(self) -> list[str]:
        try:
            return [ANSI.sub("", line) for line in self.server_log.read_text(errors="replace").splitlines()]
        except FileNotFoundError:
            return []

    def server_total(self, name: str):
        """Reads the last trace line for NAME. Accepts the console and the log-file formats."""
        for line in reversed(self.log_lines()):
            if TRACE in line and re.search(rf"player[:=] ?{re.escape(name)}\b", line):
                slots = re.findall(r'\((\d+), "([^"]*)", (\d+)\)', line)
                cursor = re.search(r"cursor_count[:=] ?(\d+)", line)
                total = sum(int(c) for _, _, c in slots) + (int(cursor.group(1)) if cursor else 0)
                described = [(int(a), b, int(c)) for a, b, c in slots]
                return total, f"slots={described} cursor={cursor.group(1) if cursor else '?'}"
        return None, "no trace line (is inventory-trace.patch applied?)"


def click(c: OfflineClient, container, state, slot, button, mode, changed=(), carried=None):
    body = write_varint(container) + write_varint(state) + struct.pack(">h", slot) + struct.pack(">b", button) + write_varint(mode)
    body += write_varint(len(changed))
    for changed_slot, stack in changed:
        body += struct.pack(">h", changed_slot) + hashed_slot(stack)
    body += hashed_slot(carried)
    c.send(0x12, body)


def close_window(c: OfflineClient, container=0):
    c.send(0x13, write_varint(container))


def reset(c: OfflineClient):
    close_window(c)
    time.sleep(0.2)
    c.command("clear"); time.sleep(0.4)
    c.command("give minecraft:stone 20"); time.sleep(0.3)
    c.command("give minecraft:dirt 16"); time.sleep(0.8)


def run_case(session: Session, c: OfflineClient, title: str, action, allow_decrease=False):
    reset(c)
    first_line = len(session.log_lines())
    first_event = len(c.events)
    before, before_text = session.server_total(c.name)
    action()
    time.sleep(1.2)
    chats = c.chat_count
    c.command("list")
    time.sleep(0.6)
    responds = c.alive and c.chat_count > chats
    after, after_text = session.server_total(c.name)
    resends = [e for e in c.events[first_event:] if e[1] in ("SetContainerContent", "SetContainerSlot", "SetCursorItem")]
    problems = [l for l in session.log_lines()[first_line:] if re.search(r"\b(ERROR|panicked)\b", l)]
    ok = responds and before is not None and after is not None and (after == before or (allow_decrease and after < before)) and not problems
    if not ok:
        session.failures += 1
    print(f"=== {title}")
    print(f"  total(before)={before} total(after)={after}  conn_alive_and_server_responds={responds}  "
          f"server_resends={len(resends)}  disconnect={c.disconnect_reason}  case={'PASS' if ok else 'FAIL'}")
    print(f"    before: {before_text}")
    for event in resends[:6]:
        print(f"    resend: {event[1]} {event[2]}")
    for line in problems[:5]:
        print(f"    LOG: {line[:300]}")
    print(f"    after : {after_text}")
    close_window(c)
    time.sleep(0.3)


def malformed(session: Session, label: str, body: bytes):
    offender = OfflineClient(session.address, "AtpMalformed")
    offender.connect()
    time.sleep(1.5)
    first_line = len(session.log_lines())
    offender.send(0x12, body)
    time.sleep(1.5)
    probe = OfflineClient(session.address, "AtpProbe")
    accepted = probe.connect() and probe.alive and not probe.disconnect_reason
    new_lines = session.log_lines()[first_line:]
    logged = [l for l in new_lines if "malformed packet" in l]
    problems = [l for l in new_lines if re.search(r"\b(ERROR|panicked)\b", l)]
    ok = (not offender.alive or offender.disconnect_reason is not None) and accepted and not problems
    if not ok:
        session.failures += 1
    print(f"=== {label}")
    print(f"  offender disconnected={not offender.alive or offender.disconnect_reason is not None}  "
          f"server accepts a new login={accepted}  case={'PASS' if ok else 'FAIL'}")
    for line in logged[:2] + problems[:3]:
        print(f"    LOG: {line[:300]}")
    offender.close()
    probe.close()
    time.sleep(1.0)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--server", default="127.0.0.1:25565", help="server address (default 127.0.0.1:25565)")
    parser.add_argument("--server-log", required=True, type=Path, help="the server log file, logs/void-<timestamp>.log")
    args = parser.parse_args()
    session = Session(parse_address(args.server), args.server_log)

    c = OfflineClient(session.address, "AtpClicks")
    if not c.connect() or c.disconnect_reason:
        print(f"login failed: {c.disconnect_reason} {c.events[-1:]}")
        return 1
    time.sleep(2)
    c.command("clear"); time.sleep(0.5)
    c.command("give minecraft:stone 20"); time.sleep(0.4)
    c.command("give minecraft:dirt 16"); time.sleep(0.4)
    c.command("give minecraft:diamond 1"); time.sleep(0.8)
    found = {}
    for slot, stack in c.slots.items():
        for label, count in (("stone", 20), ("dirt", 16), ("diamond", 1)):
            if stack[1] == count:
                found[label] = (slot, stack[0])
    if len(found) < 3:
        print(f"could not find the test items in the client view: {c.slots} (is VOID_EXAMPLE_OPERATORS=1 set?)")
        return 1
    S, stone = found["stone"]
    D, dirt = found["dirt"]
    diamond = found["diamond"][1]
    st = lambda: c.state_id  # noqa: E731
    print(f"test items: stone id {stone} in slot {S}, dirt id {dirt} in slot {D}, diamond id {diamond}")

    run_case(session, c, f"00 control: valid pickup of the stone in slot {S}, then place it back", lambda: (
        click(c, 0, st(), S, 0, 0, [(S, None)], (stone, 20)), time.sleep(0.3),
        click(c, 0, st(), S, 0, 0, [(S, (stone, 20))], None)))
    run_case(session, c, "01 stale state id (state-7, 0, -1, 2^31-1), pickup stone", lambda: (
        click(c, 0, st() - 7, S, 0, 0, [(S, None)], (stone, 20)),
        click(c, 0, 0, S, 0, 0, [(S, None)], (stone, 20)),
        click(c, 0, -1, S, 0, 0, [(S, None)], (stone, 20)),
        click(c, 0, 2**31 - 1, S, 1, 0, [(S, (stone, 10))], (stone, 10))))
    run_case(session, c, "02a slot 99", lambda: click(c, 0, st(), 99, 0, 0))
    run_case(session, c, "02b slot 32767", lambda: click(c, 0, st(), 32767, 0, 0))
    run_case(session, c, "02c slot -2", lambda: click(c, 0, st(), -2, 0, 0))
    run_case(session, c, "02d slots -32768/46/99/-2 with modes quickmove, swap, pickup-all, clone, throw, quickcraft", lambda: [
        click(c, 0, st(), slot, b, m) for slot in (-32768, 46, 99, -2) for b, m in ((0, 1), (0, 2), (0, 6), (0, 3), (0, 4), (0, 5))])
    run_case(session, c, "03a unknown modes 7/100/-1/200/2^31-1", lambda: [click(c, 0, st(), S, 0, m) for m in (7, 100, -1, 200, 2**31 - 1)])
    run_case(session, c, "03b invalid buttons for pickup, swap, quickcraft, pickup-all, quickmove", lambda: [
        click(c, 0, st(), S, b, m) for m, buttons in ((0, (2, 5, 127, -128, -1)), (2, (9, 50, -1, 127, -128)), (5, (3, 9, 127)), (6, (1, 5)), (1, (1, 7))) for b in buttons])
    run_case(session, c, "03c throw (mode 4) on the stone with buttons 0, 1, 2, 5, -1, 127", lambda: [
        click(c, 0, st(), S, b, 4) for b in (0, 1, 2, 5, -1, 127)], allow_decrease=True)
    run_case(session, c, "04a forged prediction: 64 diamonds in empty slot 40 and 64 carried", lambda: click(
        c, 0, st(), 40, 0, 0, [(40, (diamond, 64))], (diamond, 64)))
    run_case(session, c, "04b forged prediction: 64 diamonds carried and in every slot 0-45", lambda: click(
        c, 0, st(), S, 0, 0, [(slot, (diamond, 64)) for slot in range(46)], (diamond, 64)))
    run_case(session, c, "04c forged counts 99, 2^31-1, 0, -5", lambda: [
        click(c, 0, st(), S, 0, 0, [(S, (stone, n))], (stone, n)) for n in (99, 2**31 - 1, 0, -5)])
    run_case(session, c, "05 window ids that are not open: 1, 5, 127, 255, -1", lambda: [
        click(c, w, st(), S, 0, 0, [(S, None)], (stone, 20)) for w in (1, 5, 127, 255, -1)])
    run_case(session, c, "06a drag start only", lambda: click(c, 0, st(), -999, 0, 5))
    run_case(session, c, "06b drag add and end without start", lambda: (
        click(c, 0, st(), D, 1, 5), click(c, 0, st(), 10, 1, 5), click(c, 0, st(), -999, 2, 5)))
    run_case(session, c, "06c drag: start, 3 adds, restart with another type, empty end, duplicate adds, end", lambda: (
        click(c, 0, st(), S, 0, 0, [(S, None)], (stone, 20)), time.sleep(0.3),
        click(c, 0, st(), -999, 0, 5, carried=(stone, 20)),
        [click(c, 0, st(), slot, 1, 5, carried=(stone, 20)) for slot in (9, 10, 11)],
        click(c, 0, st(), -999, 4, 5, carried=(stone, 20)),
        click(c, 0, st(), -999, 6, 5, carried=(stone, 20)),
        click(c, 0, st(), 9, 1, 5, carried=(stone, 20)), click(c, 0, st(), 9, 1, 5, carried=(stone, 20)),
        click(c, 0, st(), -999, 2, 5, carried=(stone, 20))))
    run_case(session, c, "06d drag over result slot 0, armour 5, slots 99/-2, offhand 45, then end", lambda: (
        click(c, 0, st(), S, 0, 0, [(S, None)], (stone, 20)), time.sleep(0.3),
        click(c, 0, st(), -999, 0, 5, carried=(stone, 20)),
        [click(c, 0, st(), slot, 1, 5, carried=(stone, 20)) for slot in (0, 5, 99, -2, 45, 12)],
        click(c, 0, st(), -999, 2, 5, carried=(stone, 20))))

    def burst():
        for i in range(500):
            click(c, 0, st(), S if i % 2 == 0 else D, 0, 0)
        for _ in range(300):
            click(c, 0, st(), S, 0, 1)
        for _ in range(300):
            click(c, 0, st(), D, 0, 6)
        for i in range(300):
            click(c, 0, 12345, D, i % 2, 0, [(D, (dirt, 16)), (S, (stone, 20))], (stone, 64))
    run_case(session, c, "07 burst: 1400 clicks without waiting (pickups, quickmove, pickup-all, stale and forged)", burst)
    c.command("clear")
    time.sleep(0.5)
    c.close()

    header = write_varint(0) + write_varint(1) + struct.pack(">h", 36) + b"\x00" + write_varint(0)
    too_many = header + write_varint(129) + b"".join(struct.pack(">h", i) + b"\x00" for i in range(129)) + b"\x00"
    malformed(session, "09a ClickContainer with 129 changed slots (limit 128)", too_many)
    malformed(session, "09b truncated ClickContainer (3 bytes)", write_varint(0) + write_varint(1) + b"\x00")
    malformed(session, "09c changed-slot count 2^31-1", header + write_varint(2**31 - 1))
    malformed(session, "09d hashed item with 2^31-1 removed components",
              header + write_varint(0) + b"\x01" + write_varint(1) + write_varint(64) + write_varint(0) + write_varint(2**31 - 1))
    print(f"failed cases: {session.failures}")
    print("done")
    return 0 if session.failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
