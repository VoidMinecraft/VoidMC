#!/usr/bin/env python3
"""Emit assets/<version>/attributes.json from Paper's Attributes.java.

Usage: extract_attributes.py <version> <path-to-Attributes.java>

Mojang's data generator reports no default, range or client-sync flag for
attributes; the only source is the `register("<name>", new
RangedAttribute("...", <default>, <min>, <max>)[.setSyncable(true)])` table
in Attributes.java. Paper patches four maxima to read SpigotConfig; VANILLA_MAX
restores Mojang's values for those. A `0.42F` literal is widened to a double
exactly like Java does (0.41999998688697815), which is what the client receives.
"""
import json
import re
import struct
import sys
from pathlib import Path

VANILLA_MAX = {
    "attack_damage": 2048.0,
    "max_absorption": 2048.0,
    "max_health": 1024.0,
    "movement_speed": 1024.0,
}


def java_number(literal):
    if literal[-1] in "Ff":
        return struct.unpack("f", struct.pack("f", float(literal[:-1])))[0]
    return float(literal.rstrip("Dd"))


version, source = sys.argv[1], Path(sys.argv[2])
text = source.read_text()
attributes = {}
number = r"([-0-9.E]+[FfDd]?)"
pattern = re.compile(
    r'register\(\s*"([a-z_]+)"\s*,\s*new\s+RangedAttribute\(\s*"[^"]*"\s*,\s*'
    + number + r"\s*,\s*" + number + r"\s*,\s*([^)]*)\)(.*?)\)\s*;",
    re.S,
)
for match in pattern.finditer(text):
    name, default, minimum, maximum, tail = match.groups()
    maximum = maximum.strip()
    try:
        maximum = java_number(maximum)
    except ValueError:
        maximum = VANILLA_MAX[name]
    attributes[f"minecraft:{name}"] = {
        "default": java_number(default),
        "min": java_number(minimum),
        "max": maximum,
        "syncable": "setSyncable(true)" in tail,
    }

out = Path(__file__).resolve().parent.parent / "assets" / version / "attributes.json"
out.write_text(json.dumps(attributes, indent=2) + "\n")
print(f"{out}: {len(attributes)} attributes")
