#!/usr/bin/env python3
"""Emit assets/<version>/block_entity_hosts.json from Paper's BlockEntityType.java.

Usage: extract_block_entity_hosts.py <version> <path-to-BlockEntityType.java>

Mojang's data generator has no report for which blocks host which block
entity type; the only source is the `register("<type>", ..., Blocks.X, ...)`
table in BlockEntityType.java.
"""
import json
import re
import sys
from pathlib import Path

version, source = sys.argv[1], Path(sys.argv[2])
text = source.read_text()
hosts = {}
for match in re.finditer(r'register\(\s*"([a-z_]+)"\s*,[^;]*?\)\s*;', text, re.S):
    body = match.group(0)
    blocks = re.findall(r"Blocks\.([A-Z0-9_]+)", body)
    hosts[f"minecraft:{match.group(1)}"] = [f"minecraft:{b.lower()}" for b in blocks]

out = Path(__file__).resolve().parent.parent / "assets" / version / "block_entity_hosts.json"
out.write_text(json.dumps(hosts, indent=2) + "\n")
print(f"{out}: {len(hosts)} block entity types")
