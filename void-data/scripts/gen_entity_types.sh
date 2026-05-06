#!/usr/bin/env bash
# Generate void-data/src/entity_types.rs from a Paper server jar.
#
# Usage:
#   ./gen_entity_types.sh <version> <paper-jar-url>
#
# Example:
#   ./gen_entity_types.sh 26.1.2 https://fill-data.papermc.io/v1/objects/.../paper-26.1.2-1.jar
#
# Requires: java, curl, jq.

set -euo pipefail

VERSION="${1:?usage: gen_entity_types.sh <version> <paper-jar-url>}"
PAPER_URL="${2:?usage: gen_entity_types.sh <version> <paper-jar-url>}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_FILE="$SCRIPT_DIR/../src/entity_types.rs"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "==> Downloading Paper $VERSION"
curl -fsSL -o "$WORK_DIR/paper.jar" "$PAPER_URL"

echo "==> Running data generator (--reports)"
(
  cd "$WORK_DIR"
  java -DbundlerMainClass=net.minecraft.data.Main -jar paper.jar --reports --output generated >/dev/null 2>&1
)

REPORTS="$WORK_DIR/generated/reports/registries.json"
if [ ! -f "$REPORTS" ]; then
  echo "ERROR: data generator did not produce $REPORTS" >&2
  exit 1
fi

echo "==> Parsing entity_type registry"
# Entries sorted by protocol_id ascending; output one name per line.
ENTRIES="$(jq -r '
  .["minecraft:entity_type"].entries
  | to_entries
  | sort_by(.value.protocol_id)
  | .[].key
' "$REPORTS")"

COUNT="$(echo "$ENTRIES" | wc -l | tr -d ' ')"
echo "==> Found $COUNT entity types"

# Write Rust source file.
{
  echo "// Entity type protocol IDs for each supported version."
  echo "//"
  echo "// Each entry is the entity type name at the corresponding index (protocol_id)."
  echo "// Regenerate with: void-data/scripts/gen_entity_types.sh <version> <paper-jar-url>"
  echo ""
  echo "pub(crate) static ENTITY_TYPE_IDS_26_1_2: &[&str] = &["
  if [ "$VERSION" = "26.1.2" ]; then
    i=0
    while IFS= read -r name; do
      printf '    %s, // %d\n' "\"$name\"" "$i"
      i=$((i + 1))
    done <<< "$ENTRIES"
  fi
  echo "];"
} > "$SRC_FILE"

echo "==> Written to $SRC_FILE"
echo "==> Run: cargo build -p void-data"
