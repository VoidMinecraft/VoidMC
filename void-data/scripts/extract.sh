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

echo "==> Done. Run: cargo build -p void-data"
