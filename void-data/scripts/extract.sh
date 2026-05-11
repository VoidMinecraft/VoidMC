#!/usr/bin/env bash
# Extract Minecraft registry data from a Paper server jar.
#
# Usage:
#   ./extract.sh <version> <paper-jar-url>
#
# Example:
#   ./extract.sh 1.21.4 https://fill-data.papermc.io/v1/objects/.../paper-1.21.4-232.jar
#
# Requires: java, curl, jq (optional).

set -euo pipefail

VERSION="${1:?usage: extract.sh <version> <paper-jar-url>}"
PAPER_URL="${2:?usage: extract.sh <version> <paper-jar-url>}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ASSETS_DIR="$CRATE_DIR/assets/$VERSION"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

REGISTRIES=(
  damage_type
  dimension_type
  painting_variant
  worldgen/biome
  cat_variant
  cat_sound_variant
  chicken_variant
  chicken_sound_variant
  cow_variant
  cow_sound_variant
  frog_variant
  pig_variant
  pig_sound_variant
  wolf_variant
  wolf_sound_variant
  zombie_nautilus_variant
  timeline
  world_clock
)

TAG_REGISTRIES=(
  tags/damage_type
  tags/painting_variant
  tags/timeline
  tags/worldgen/biome
)

echo "==> Downloading Paper $VERSION"
curl -fsSL -o "$WORK_DIR/paper.jar" "$PAPER_URL"

echo "==> Running data generator (--all)"
(
  cd "$WORK_DIR"
  java -DbundlerMainClass=net.minecraft.data.Main -jar paper.jar --all --output generated >/dev/null 2>&1
)

DATA="$WORK_DIR/generated/data/minecraft"
if [ ! -d "$DATA" ]; then
  echo "ERROR: data generator did not produce $DATA" >&2
  exit 1
fi

echo "==> Copying registries to $ASSETS_DIR"
mkdir -p "$ASSETS_DIR"
for REG in "${REGISTRIES[@]}" "${TAG_REGISTRIES[@]}"; do
  SRC="$DATA/$REG"
  DEST="$ASSETS_DIR/$REG"
  if [ -d "$SRC" ]; then
    rm -rf "$DEST"
    mkdir -p "$DEST"
    cp -R "$SRC"/. "$DEST/"
    COUNT="$(find "$DEST" -name '*.json' | wc -l | tr -d ' ')"
    printf '  %-32s %s entries\n' "$REG" "$COUNT"
  else
    echo "  WARNING: $REG missing in source" >&2
  fi
done

echo "==> Copying Mojang block list report"
REPORTS="$WORK_DIR/generated/reports"
if [ -f "$REPORTS/blocks.json" ]; then
  cp "$REPORTS/blocks.json" "$ASSETS_DIR/blocks.json"
  printf '  %-32s %s blocks\n' "blocks.json" \
    "$(grep -c '^  "minecraft:' "$ASSETS_DIR/blocks.json" || true)"
else
  echo "  WARNING: $REPORTS/blocks.json missing — block codegen will skip" >&2
fi

# ---- Prismarine block-collision shapes -----------------------------------
# We pin a release branch / commit so the data is reproducible. The vendored
# JSON ships next to blocks.json and is consumed by void-data/build.rs to
# emit the typed `shapes` module. Override `PRISMARINE_REF` /
# `PRISMARINE_SHAPE_VERSION` if you need a different snapshot.
PRISMARINE_REPO="${PRISMARINE_REPO:-https://github.com/PrismarineJS/minecraft-data.git}"
PRISMARINE_REF="${PRISMARINE_REF:-master}"
PRISMARINE_SHAPE_VERSION="${PRISMARINE_SHAPE_VERSION:-1.21.9}"

echo "==> Cloning prismarine ($PRISMARINE_REF) for collision shapes"
git clone --depth 1 --branch "$PRISMARINE_REF" --quiet \
  "$PRISMARINE_REPO" "$WORK_DIR/prismarine"
PRISM_COMMIT="$(git -C "$WORK_DIR/prismarine" rev-parse HEAD)"
SHAPE_SRC="$WORK_DIR/prismarine/data/pc/$PRISMARINE_SHAPE_VERSION/blockCollisionShapes.json"
if [ -f "$SHAPE_SRC" ]; then
  cp "$SHAPE_SRC" "$ASSETS_DIR/blockCollisionShapes.json"
  printf '  %-32s %s\n' "blockCollisionShapes.json" \
    "$(wc -c < "$ASSETS_DIR/blockCollisionShapes.json") bytes"
else
  echo "  WARNING: prismarine has no $PRISMARINE_SHAPE_VERSION shapes" >&2
fi

# ---- Provenance ----------------------------------------------------------
cat > "$ASSETS_DIR/PROVENANCE.txt" <<EOF
Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)
Mojang server jar: $PAPER_URL
Prismarine ref:    $PRISMARINE_REF
Prismarine commit: $PRISM_COMMIT
Shape source ver:  $PRISMARINE_SHAPE_VERSION
EOF

echo "==> Done. Run: cargo build -p void-data"
