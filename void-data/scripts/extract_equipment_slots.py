#!/usr/bin/env python3
"""Emit assets/<version>/equipment_slots.json from Mojang's item components report.

Usage: extract_equipment_slots.py <version> <generated-reports-dir>

<generated-reports-dir> is the `generated/reports` directory written by
`java -DbundlerMainClass=net.minecraft.data.Main -jar <server>.jar --reports`.
Its `minecraft/components/item/<item>.json` files hold every item's default
components; this keeps only the slot of the `minecraft:equippable` component,
for every item that has one.

The `allowed_entities` field of `equippable` is not kept: it only restricts
non-player slots (`body`, `saddle`), which a player never uses. The script
fails if that stops being true.
"""
import json
import sys
from pathlib import Path

PLAYER_SLOTS = {"mainhand", "offhand", "feet", "legs", "chest", "head"}
ALL_SLOTS = PLAYER_SLOTS | {"body", "saddle"}

version, reports = sys.argv[1], Path(sys.argv[2])
items_dir = reports / "minecraft" / "components" / "item"
if not items_dir.is_dir():
    sys.exit(f"{items_dir} is not a directory")

slots = {}
for path in sorted(items_dir.glob("*.json")):
    components = json.loads(path.read_text())["components"]
    equippable = components.get("minecraft:equippable")
    if equippable is None:
        continue
    slot = equippable["slot"]
    if slot not in ALL_SLOTS:
        sys.exit(f"{path.name}: unknown equipment slot {slot!r}")
    if slot in PLAYER_SLOTS and "allowed_entities" in equippable:
        sys.exit(f"{path.name}: a player slot restricted to {equippable['allowed_entities']!r}")
    slots[f"minecraft:{path.stem}"] = slot

out = Path(__file__).resolve().parent.parent / "assets" / version / "equipment_slots.json"
out.write_text(json.dumps(slots, indent=2) + "\n")
print(f"{out}: {len(slots)} equippable items")
