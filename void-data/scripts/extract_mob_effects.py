#!/usr/bin/env python3
"""Emit assets/<version>/mob_effects.json from Paper's MobEffects.java.

Usage: extract_mob_effects.py <version> <path-to-MobEffects.java>

Mojang's data generator reports no colour or category for mob effects; the
only source is the `register("<name>", new XxxMobEffect(MobEffectCategory.X,
<rgb>[, ParticleTypes.Y]))` table in MobEffects.java. A subclass may set the
particle itself through `super(category, color, ParticleTypes.Y)` in its own
constructor (effect/XxxMobEffect.java next to MobEffects.java); those are
followed too.
"""
import json
import re
import sys
from pathlib import Path

version, source = sys.argv[1], Path(sys.argv[2])
text = source.read_text()
effects = {}
pattern = re.compile(
    r'register\(\s*"([a-z_]+)"\s*,\s*new\s+(\w+)\(\s*MobEffectCategory\.(\w+)\s*,\s*(\d+)'
    r"(?:\s*,\s*ParticleTypes\.([A-Z_]+))?",
    re.S,
)
super_particle = re.compile(r"super\(\s*category\s*,\s*color\s*,\s*ParticleTypes\.([A-Z_]+)\s*\)")


def subclass_particle(class_name):
    subclass = source.with_name(f"{class_name}.java")
    if class_name == "MobEffect" or not subclass.exists():
        return None
    match = super_particle.search(subclass.read_text())
    return match.group(1) if match else None


for match in pattern.finditer(text):
    name, class_name, category, color, particle = match.groups()
    entry = {"category": category.lower(), "color": int(color)}
    particle = particle or subclass_particle(class_name)
    if particle:
        entry["particle"] = f"minecraft:{particle.lower()}"
    effects[f"minecraft:{name}"] = entry

out = Path(__file__).resolve().parent.parent / "assets" / version / "mob_effects.json"
out.write_text(json.dumps(effects, indent=2) + "\n")
print(f"{out}: {len(effects)} mob effects")
