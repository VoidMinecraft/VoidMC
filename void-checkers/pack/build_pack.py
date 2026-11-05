#!/usr/bin/env python3
"""Generates checkers.zip: 3D piece models that replace six dye items.

The item-to-piece mapping must match `piece_item` and `ghost_item` in
src/plugin.rs. A disc is two square elements, the second turned by 45 degrees,
which reads as an octagon from above.
"""
import json
import struct
import zipfile
import zlib
from pathlib import Path

SIZE = 16
MIN, MAX = 1.5, 14.5
HEIGHT = 3.0
COLORS = {
    "white": (238, 230, 214, 255),
    "white_rim": (196, 184, 164, 255),
    "black": (58, 44, 44, 255),
    "black_rim": (32, 24, 24, 255),
    "gold": (236, 186, 52, 255),
    "ghost": (120, 200, 255, 110),
}
PIECES = {
    "white_dye": ("white_man", "white", "white_rim", False),
    "yellow_dye": ("white_king", "white", "white_rim", True),
    "black_dye": ("black_man", "black", "black_rim", False),
    "orange_dye": ("black_king", "black", "black_rim", True),
    "light_blue_dye": ("ghost_man", "ghost", "ghost", False),
    "cyan_dye": ("ghost_king", "ghost", "ghost", True),
}


def png(color):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\0" + bytes(color) * SIZE
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(row * SIZE, 9)) + chunk(b"IEND", b""))


def box(start, end, texture, turned=False):
    faces = {face: {"texture": texture}
             for face in ("up", "down", "north", "south", "east", "west")}
    element = {"from": start, "to": end, "faces": faces}
    if turned:
        element["rotation"] = {"origin": [8, 0, 8], "axis": "y", "angle": 45}
    return element


def disc(bottom, top, texture):
    # The turned half stops just short so the two top faces do not z-fight.
    return [box([MIN, bottom, MIN], [MAX, top, MAX], texture),
            box([MIN, bottom, MIN], [MAX, top - 0.05, MAX], texture, turned=True)]


def model(face, rim, king):
    elements = disc(0, HEIGHT, "#rim")
    elements.append(box([5, HEIGHT, 5], [11, HEIGHT + 0.4, 11], "#face"))
    top = HEIGHT + 0.4
    if king:
        elements += disc(top, top + HEIGHT, "#face")
        crown = "#gold" if face != "ghost" else "#face"
        elements.append(box([5, top + HEIGHT, 5], [11, top + HEIGHT + 1.5, 11], crown))
    textures = {"face": f"voidmc:item/{face}", "rim": f"voidmc:item/{rim}",
                "gold": "voidmc:item/gold", "particle": f"voidmc:item/{face}"}
    return {"textures": textures, "elements": elements}


def main():
    meta = {"pack": {"description": "VoidMC checkers pieces",
                     "min_format": 84, "max_format": 84}}
    files = {"pack.mcmeta": json.dumps(meta).encode()}
    for name, color in COLORS.items():
        files[f"assets/voidmc/textures/item/{name}.png"] = png(color)
    for item, (name, face, rim, king) in PIECES.items():
        files[f"assets/voidmc/models/item/{name}.json"] = json.dumps(
            model(face, rim, king)).encode()
        files[f"assets/minecraft/items/{item}.json"] = json.dumps(
            {"model": {"type": "minecraft:model", "model": f"voidmc:item/{name}"}}).encode()
    with zipfile.ZipFile(Path(__file__).with_name("checkers.zip"), "w") as archive:
        for name, data in files.items():
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            archive.writestr(info, data, zipfile.ZIP_DEFLATED)


if __name__ == "__main__":
    main()
