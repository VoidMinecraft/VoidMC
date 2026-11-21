"""Shared helpers for the VoidMC acceptance-test tools (protocol 775, Minecraft 26.1.2).

Standard library only. The tools in this directory import it from their own folder.
"""

from __future__ import annotations

import socket
import struct
import threading
import time
import uuid
import zlib

PROTOCOL = 775

# Clientbound packet ids used by the tools (void-protocol/src/clientbound/*.rs).
LOGIN_DISCONNECT = 0x00
LOGIN_SUCCESS = 0x02
LOGIN_SET_COMPRESSION = 0x03
CONFIG_FINISH = 0x03
CONFIG_KNOWN_PACKS = 0x0E
PLAY_CHUNK_BATCH_FINISHED = 0x0B
PLAY_CHUNK_BATCH_START = 0x0C
PLAY_SET_CONTAINER_CONTENT = 0x12
PLAY_SET_CONTAINER_SLOT = 0x14
PLAY_DISCONNECT = 0x20
PLAY_UNLOAD_CHUNK = 0x25
PLAY_KEEP_ALIVE = 0x2C
PLAY_CHUNK_DATA = 0x2D
PLAY_LOGIN = 0x31
PLAY_PING = 0x3D
PLAY_SYNC_POSITION = 0x48
PLAY_SET_CENTER_CHUNK = 0x5E
PLAY_SET_CURSOR_ITEM = 0x60
PLAY_SYSTEM_CHAT = 0x79


def write_varint(value: int) -> bytes:
    value &= 0xFFFFFFFF
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def read_varint(data: bytes, index: int) -> tuple[int, int]:
    """Returns (value, next index). Raises IndexError on a truncated varint."""
    result = shift = 0
    while True:
        byte = data[index]
        index += 1
        result |= (byte & 0x7F) << shift
        shift += 7
        if byte < 0x80:
            if result >= 1 << 31:
                result -= 1 << 32
            return result, index
        if shift > 35:
            raise ValueError("varint too long")


def write_string(text: str) -> bytes:
    raw = text.encode()
    return write_varint(len(raw)) + raw


def read_string(data: bytes, index: int) -> tuple[str, int]:
    length, index = read_varint(data, index)
    return data[index:index + length].decode(errors="replace"), index + length


def parse_address(text: str) -> tuple[str, int]:
    host, _, port = text.rpartition(":")
    if not host or not port.isdigit():
        raise ValueError(f"expected HOST:PORT, got {text!r}")
    return host, int(port)


def offline_uuid(name: str) -> uuid.UUID:
    """Any stable UUID works: the server runs in offline mode."""
    return uuid.uuid3(uuid.NAMESPACE_DNS, name)


def iter_server_stream(data: bytes):
    """Decodes a recorded server-to-client byte stream.

    Yields (index, phase, packet_id, body) with phase in "login", "configuration", "play".
    Compression is detected from the login SetCompression packet, so the stream may be
    compressed or not. Stops quietly at a truncated final frame.
    """
    pos = 0
    compressed = False
    phase = "login"
    index = 0
    while pos < len(data):
        try:
            length, start = read_varint(data, pos)
        except IndexError:
            return
        end = start + length
        if end > len(data):
            return
        frame = data[start:end]
        pos = end
        if compressed:
            raw_length, k = read_varint(frame, 0)
            frame = zlib.decompress(frame[k:]) if raw_length else frame[k:]
        packet_id, k = read_varint(frame, 0)
        body = frame[k:]
        yield index, phase, packet_id, body
        index += 1
        if phase == "login":
            if packet_id == LOGIN_SET_COMPRESSION:
                compressed = True
            elif packet_id == LOGIN_SUCCESS:
                phase = "configuration"
        elif phase == "configuration" and packet_id == CONFIG_FINISH:
            phase = "play"


class OfflineClient:
    """A minimal offline-mode protocol client that reaches the Play state.

    It answers keep-alives, pings and the first teleport, and records inventory packets.
    It is test instrumentation, not a vanilla client.
    """

    def __init__(self, address: tuple[str, int], name: str, view_distance: int = 2):
        self.address = address
        self.name = name
        self.view_distance = view_distance
        self.sock = socket.create_connection(address, timeout=30)
        self.compressed = False
        self.phase = "login"
        self.alive = True
        self.disconnect_reason: str | None = None
        self.state_id = 0
        self.slots: dict[int, tuple] = {}
        self.cursor = None
        self.chat_count = 0
        self.events: list[tuple[float, str, str]] = []
        self.in_play = threading.Event()
        self._buffer = b""
        self._send_lock = threading.Lock()

    # -- framing ---------------------------------------------------------------------------
    def send(self, packet_id: int, body: bytes = b"") -> None:
        packet = write_varint(packet_id) + body
        if self.compressed:
            packet = write_varint(0) + packet
        try:
            with self._send_lock:
                self.sock.sendall(write_varint(len(packet)) + packet)
        except OSError:
            self.alive = False

    def _read_exact(self, count: int) -> bytes:
        while len(self._buffer) < count:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise EOFError("connection closed by server")
            self._buffer += chunk
        out, self._buffer = self._buffer[:count], self._buffer[count:]
        return out

    def _read_varint_stream(self) -> int:
        result = shift = 0
        while True:
            byte = self._read_exact(1)[0]
            result |= (byte & 0x7F) << shift
            shift += 7
            if byte < 0x80:
                return result

    # -- session ---------------------------------------------------------------------------
    def connect(self, timeout: float = 20.0) -> bool:
        host, port = self.address
        self.send(0x00, write_varint(PROTOCOL) + write_string(host) + struct.pack(">H", port) + write_varint(2))
        self.send(0x00, write_string(self.name) + offline_uuid(self.name).bytes)
        threading.Thread(target=self._reader, daemon=True).start()
        return self.in_play.wait(timeout)

    def close(self) -> None:
        try:
            self.sock.close()
        except OSError:
            pass

    def _reader(self) -> None:
        try:
            while True:
                length = self._read_varint_stream()
                frame = self._read_exact(length)
                if self.compressed:
                    raw_length, k = read_varint(frame, 0)
                    frame = zlib.decompress(frame[k:]) if raw_length else frame[k:]
                packet_id, k = read_varint(frame, 0)
                self._handle(packet_id, frame[k:])
        except Exception as error:  # noqa: BLE001 - any failure ends the session
            self.alive = False
            self.events.append((time.time(), "connection-closed", repr(error)))

    def _client_information(self) -> bytes:
        # locale, view distance, chat mode, chat colours, skin parts, main hand,
        # text filtering, server listing, particle status
        return (write_string("en_us") + bytes([self.view_distance]) + write_varint(0) + b"\x01"
                + b"\x7f" + write_varint(1) + b"\x00" + b"\x01" + write_varint(0))

    def _handle(self, packet_id: int, body: bytes) -> None:
        now = time.time()
        if self.phase == "login":
            if packet_id == LOGIN_SET_COMPRESSION:
                self.compressed = True
            elif packet_id == LOGIN_SUCCESS:
                self.send(0x03)  # LoginAcknowledged
                self.phase = "configuration"
                self.send(0x00, self._client_information())
            elif packet_id == LOGIN_DISCONNECT:
                self.disconnect_reason = "login: " + body.hex()
            return
        if self.phase == "configuration":
            if packet_id == CONFIG_KNOWN_PACKS:
                self.send(0x07, write_varint(0))  # KnownPacks: none
            elif packet_id == CONFIG_FINISH:
                self.send(0x03)  # FinishConfigurationAcknowledged
                self.phase = "play"
            return
        if packet_id == PLAY_KEEP_ALIVE:
            self.send(0x1C, body[:8])
        elif packet_id == PLAY_PING:
            self.send(0x2D, body[:4])
        elif packet_id == PLAY_SYNC_POSITION:
            teleport_id, _ = read_varint(body, 0)
            self.send(0x00, write_varint(teleport_id))  # ConfirmTeleportation
            if not self.in_play.is_set():
                self.send(0x2C)  # PlayerLoaded
                self.in_play.set()
        elif packet_id == PLAY_DISCONNECT:
            self.disconnect_reason = "play: " + body.hex()
            self.events.append((now, "Disconnect", body.hex()))
        elif packet_id == PLAY_SET_CONTAINER_CONTENT:
            container, i = read_varint(body, 0)
            state, i = read_varint(body, i)
            count, i = read_varint(body, i)
            items = []
            for _ in range(count):
                stack, i = read_slot(body, i)
                items.append(stack)
            carried, i = read_slot(body, i)
            content = {slot: stack for slot, stack in enumerate(items) if stack}
            if container == 0:
                self.state_id = state
                self.slots = content
                self.cursor = carried
            self.events.append((now, "SetContainerContent", f"container={container} state={state} items={content} carried={carried}"))
        elif packet_id == PLAY_SET_CONTAINER_SLOT:
            container, i = read_varint(body, 0)
            state, i = read_varint(body, i)
            slot = struct.unpack_from(">h", body, i)[0]
            stack, _ = read_slot(body, i + 2)
            if container == 0:
                self.state_id = state
                if stack:
                    self.slots[slot] = stack
                else:
                    self.slots.pop(slot, None)
            self.events.append((now, "SetContainerSlot", f"container={container} state={state} slot={slot} item={stack}"))
        elif packet_id == PLAY_SET_CURSOR_ITEM:
            stack, _ = read_slot(body, 0)
            self.cursor = stack
            self.events.append((now, "SetCursorItem", f"{stack}"))
        elif packet_id == PLAY_SYSTEM_CHAT:
            self.chat_count += 1

    # -- actions ---------------------------------------------------------------------------
    def command(self, text: str) -> None:
        """Sends a chat command without its leading slash."""
        self.send(0x07, write_string(text))


def read_slot(data: bytes, index: int):
    """Reads an item stack. Returns ((item_id, count), next index) or (None, next index)."""
    count, index = read_varint(data, index)
    if count <= 0:
        return None, index
    item_id, index = read_varint(data, index)
    added, index = read_varint(data, index)
    removed, index = read_varint(data, index)
    if added or removed:
        return (item_id, count, "components"), index
    return (item_id, count), index


def hashed_slot(stack) -> bytes:
    """Encodes a hashed item stack as sent in ServerboundContainerClick."""
    if stack is None:
        return b"\x00"
    item_id, count = stack
    return b"\x01" + write_varint(item_id) + write_varint(count) + write_varint(0) + write_varint(0)
