#!/usr/bin/env python3
"""Emit assets/<version>/entity_attributes.json from Paper's DefaultAttributes.java.

Usage: extract_entity_attributes.py <version> <path-to-DefaultAttributes.java>

Mojang's data generator reports no per-entity attribute defaults; the only
source is the `.put(EntityType.X, Class.createAttributes().build())` table in
DefaultAttributes.java. Each `createAttributes()` is a static method returning
a chain of `.add(Attributes.NAME[, value])` on top of another such method
(`LivingEntity.createLivingAttributes()`, `Mob.createMobAttributes()`, ...);
this script resolves the chains through every class under world/entity. An
`add` without a value takes the attribute's registry default from
attributes.json, so run extract_attributes.py first; the `Attributes.NAME`
constants map to registry names through the `register("<name>", ...)` table of
Attributes.java next to DefaultAttributes.java. A `0.23F` literal is widened
to a double exactly like Java does (0.2300000041723251), which is what the
client receives.
"""
import json
import re
import struct
import sys
from pathlib import Path

version, source = sys.argv[1], Path(sys.argv[2])
entity_root = source.parents[2]
assets = Path(__file__).resolve().parent.parent / "assets" / version
registry_defaults = {
    name.removeprefix("minecraft:"): record["default"]
    for name, record in json.loads((assets / "attributes.json").read_text()).items()
}
constants = dict(
    re.findall(
        r'([A-Z_]+) = register\(\s*"([a-z_]+)"',
        source.with_name("Attributes.java").read_text(),
    )
)

METHOD = re.compile(
    r"public static AttributeSupplier\.Builder (\w+)\(\)\s*\{\s*return\s+(.*?);\s*\}",
    re.S,
)
EXTENDS = re.compile(r"class \w+(?:<[^>]*>)? extends (\w+)")
ADD = re.compile(r"\.add\(\s*Attributes\.([A-Z_]+)\s*(?:,\s*([-0-9.E]+[FfDd]?))?\s*\)")


def java_number(literal):
    if literal[-1] in "Ff":
        return struct.unpack("f", struct.pack("f", float(literal[:-1])))[0]
    return float(literal.rstrip("Dd"))

HEAD = re.compile(r"^(?:(\w+)\.)?(\w+)\(\)")
ENTRY = re.compile(r"\.put\(\s*EntityType\.([A-Z_]+)\s*,\s*(\w+)\.(\w+)\(\)")

methods = {}
parents = {}
for java in entity_root.rglob("*.java"):
    text = java.read_text()
    class_name = java.stem
    if extends := EXTENDS.search(text):
        parents[class_name] = extends.group(1)
    for name, body in METHOD.findall(text):
        methods[(class_name, name)] = body


def lookup(class_name, method):
    while class_name is not None:
        if (class_name, method) in methods:
            return methods[(class_name, method)]
        class_name = parents.get(class_name)
    sys.exit(f"cannot resolve {method}() from {class_name}")


def resolve(class_name, method):
    body = lookup(class_name, method)
    head = HEAD.match(body)
    if head is None:
        sys.exit(f"unexpected body for {class_name}.{method}: {body!r}")
    qualifier, callee = head.groups()
    if qualifier == "AttributeSupplier" and callee == "builder":
        table = {}
    else:
        table = resolve(qualifier or class_name, callee)
    for attribute, value in ADD.findall(body):
        key = constants[attribute]
        table[key] = java_number(value) if value else registry_defaults[key]
    return table


entities = {}
for entity, class_name, method in ENTRY.findall(source.read_text()):
    table = resolve(class_name, method)
    entities[f"minecraft:{entity.lower()}"] = {
        f"minecraft:{attribute}": table[attribute] for attribute in sorted(table)
    }

out = assets / "entity_attributes.json"
out.write_text(json.dumps(dict(sorted(entities.items())), indent=2) + "\n")
print(f"{out}: {len(entities)} entity kinds")
