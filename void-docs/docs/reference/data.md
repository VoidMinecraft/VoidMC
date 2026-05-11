# Vanilla Data (`voidmc-data`)

`voidmc-data` ships every static piece of vanilla Minecraft information the
server needs at runtime — registries, block-state ids, property enums, and
collision shapes — as **fully typed, compile-time data**. The crate is a build
artefact: its `build.rs` reads JSON assets shipped in `void-data/assets/` and
emits Rust source that's `include!`-d from `src/lib.rs`. There is no JSON
parsing, no `HashMap` allocation, and no I/O at runtime.

## Crate Layout

```
void-data/
├── Cargo.toml
├── build.rs                 # Codegen entry point
├── src/lib.rs               # include!(...) the generated files
├── scripts/extract.sh       # Refresh assets from a server jar + prismarine
├── tests/blocks.rs          # Round-trip + sanity tests for the codegen
└── assets/<version>/
    ├── blocks.json                    # Mojang block-state report
    ├── blockCollisionShapes.json      # Prismarine collision boxes
    ├── PROVENANCE.txt                 # Source provenance audit trail
    ├── damage_type/<entry>.json       # Per-registry, per-entry data
    ├── worldgen/biome/<entry>.json
    └── tags/<registry>/<tag>.json
```

## What the Build Script Generates

`build.rs` runs once per `cargo build` and writes two files into `OUT_DIR`:

| File | Contents |
|---|---|
| `registries.rs` | `REGISTRIES`, `TAGS` static slices — NBT blobs for every entry. |
| `blocks.rs` | `vXX_Y_Z::{props, blocks, state, shapes}` modules per version. |

`src/lib.rs` includes both via `include!`. The compiler sees them as ordinary
Rust source — every constant becomes a `const`, every match becomes a `const
fn`, no static initialisers run.

## Registries

Registry entries (biomes, dimensions, painting variants, damage types, etc.)
are converted from JSON to NBT at build time and stored as raw byte arrays.
At runtime they're embedded via `include_bytes!`, so a registry lookup is a
slice-into-static-memory operation; the NBT is parsed lazily on first access
and cached behind a `OnceLock`.

The high-level `RegistryDataStore` API on the server side rebuilds these into
`RegistryEntry` values during configuration. See
[Registry System](/reference/gameplay/registry) for the runtime API; the
data shipped here is what `default_registry_data()` returns.

```rust
use voidmc_data::{Version, registry_index, registry};

// Look up the network id for a biome — used by chunk streaming when packing
// the biome palette.
let plains_id = registry_index(
    Version::V26_1_2,
    "minecraft:worldgen/biome",
    "minecraft:plains",
).unwrap_or(0);

// Iterate every shipped entry of a registry.
for (entry_id, _nbt_bytes) in registry(Version::V26_1_2, "minecraft:dimension_type")
    .unwrap_or(&[])
{
    println!("dimension: {entry_id}");
}
```

## Blocks, States, and Shapes

Everything below lives under `voidmc_data::v26_1_2::*`. New supported
versions get their own sibling module (`v1_21_9::*`, etc.).

### `blocks` — Default state ids

One `pub const` per block — the value is the default block-state id for that
block in the targeted protocol version.

```rust
use voidmc_data::v26_1_2::blocks;

assert_eq!(blocks::AIR, 0);
assert_eq!(blocks::STONE, 1);
assert_eq!(blocks::OAK_STAIRS, 3918);   // facing=north, half=bottom, ...
```

Use these whenever you need a block id and don't care about properties
(spawn world generation, hotbar palette defaults, world fill commands, etc.).

### `props` — Deduplicated property enums

Vanilla blocks share a small set of properties — `facing`, `half`, `axis`,
`shape`, … — but the *value sets* differ between blocks (a piston's `facing`
has 6 values, a stair's only 4). The build script collects every distinct
`(property name, value set)` pair across all blocks and emits **one enum
variant per unique set**.

Naming rules:

1. If a property name appears with only one value set → just the PascalCase
   name (`Hinge`, `Tilt`, `Orientation`).
2. If the same name has multiple value sets, distinct cardinalities → suffix
   with the cardinality (`Facing4`, `Facing5`, `Facing6`; `Axis2`, `Axis3`).
3. If still ambiguous (same name, same cardinality, different values) → also
   suffix with the first PascalCased value (`Half2Top` vs `Half2Upper`,
   `Type3Top` vs `Type3Single`).

Boolean-valued properties (e.g. `waterlogged`, `lit`, `powered`) are emitted
as plain `bool` fields — no enum is generated for them. Numeric ranges
(`age`, `level`, `distance`) become `u8`.

Each enum is `#[repr(u8)]`, `Copy`, and exposes a `from_index(u32) ->
Option<Self>` const fn.

```rust
use voidmc_data::v26_1_2::props::*;

let f = Facing4::East;          // 4-cardinality cardinal facing
let h = Half2Top::Bottom;       // top/bottom (vs Half2Upper for upper/lower)
let s = Shape5::Straight;       // 5-cardinality stair shape
```

### `state` — Typed structs per stateful block

Every block with at least one property gets a struct in
`voidmc_data::v26_1_2::state`. Each struct exposes:

- `MIN_STATE_ID`, `MAX_STATE_ID`, `DEFAULT_STATE_ID` — `const i32`.
- `DEFAULT: Self` — vanilla default.
- `to_state_id(self) -> i32` — encode to the global block-state id.
- `from_state_id(i32) -> Option<Self>` — decode, returning `None` for ids
  outside the block's range.

All four are `const fn`, so any usage is a compile-time computation when the
inputs are known.

```rust
use voidmc_data::v26_1_2::state;
use voidmc_data::v26_1_2::props::*;

// Build a custom stair state.
let stair = state::OakStairs {
    facing: Facing4::East,
    half: Half2Top::Top,
    shape: Shape5::InnerLeft,
    waterlogged: true,
};
let id: i32 = stair.to_state_id();

// Decode an unknown id back into typed properties.
let raw: i32 = world.get_block(pos).unwrap_or(0);
if let Some(s) = state::OakStairs::from_state_id(raw) {
    if s.waterlogged {
        // ...
    }
}

// Default invariant — the const block id always matches the struct default.
const _: () = assert!(blocks::OAK_STAIRS == state::OakStairs::DEFAULT_STATE_ID);
```

The state-id encoding mirrors vanilla's lexicographic property iteration:
properties are listed in the same order Mojang reports them, and the
right-most property changes fastest. The per-property strides are computed
at codegen time and inlined into `to_state_id`.

### `shapes` — Collision boxes

`shapes::for_state(state_id) -> &'static [Aabb]` returns the axis-aligned
boxes (block-local, in `[0,1]³`) that make up a state's collision shape.
Distinct shape sets are deduplicated; consecutive states sharing a shape are
collapsed into single `match` arms. Unknown ids fall back to `FULL_CUBE`.

```rust
use voidmc_data::v26_1_2::{blocks, shapes};

assert!(shapes::for_state(blocks::AIR).is_empty());

let cube = shapes::for_state(blocks::STONE);
assert_eq!(cube.len(), 1);
assert_eq!((cube[0].x0, cube[0].x1), (0.0, 1.0));

// Stairs decompose into multiple AABBs; the exact count depends on the
// state's `shape` property (straight vs inner_corner vs outer_corner).
let stair = shapes::for_state(blocks::OAK_STAIRS);
assert!(!stair.is_empty());
```

Shape data currently comes from prismarine 1.21.9 (see *Asset Workflow*
below). 26.1.2 blocks not present in 1.21.9 fall through to `FULL_CUBE`;
this is acceptable as a beta default but should be revisited once
prismarine ships its own 26.1.2 dataset.

## Asset Workflow

Assets live under `void-data/assets/<version>/` and are committed to the
repo. **Building the crate never touches the network** — the build script
reads files purely from disk. The refresh loop is:

```bash
cd void-data/scripts
./extract.sh 26.1.2 https://fill-data.papermc.io/v1/objects/<sha>/paper-26.1.2-<build>.jar
```

What `extract.sh` does:

1. Downloads the bundled Paper jar (Mojang server jar with all libs).
2. Runs `net.minecraft.data.Main --all` to produce both
   `generated/data/minecraft/...` (registries, tags) and
   `generated/reports/blocks.json` (block-state palette).
3. Copies every shipped registry directory into `assets/<version>/`.
4. Copies `blocks.json` straight from the report.
5. Clones the prismarine fork pinned by `PRISMARINE_REF`
   (default: `master`, override per-call) and copies
   `data/pc/$PRISMARINE_SHAPE_VERSION/blockCollisionShapes.json` into the
   asset directory.
6. Writes `PROVENANCE.txt` with timestamps, the jar URL, the prismarine
   commit hash, and the shape-source version — this file is committed
   alongside the JSONs so future maintainers can audit how the data was
   produced.

After extraction, `cargo build -p voidmc-data` regenerates everything from
the committed JSONs and the build is fully reproducible.

### Pinning prismarine

The branch `pc_26_1_2` on PrismarineJS/minecraft-data only ships
`protocol.json` + `version.json`. Until block data lands there, override the
shape source explicitly:

```bash
PRISMARINE_REF=master \
PRISMARINE_SHAPE_VERSION=1.21.9 \
./extract.sh 26.1.2 <paper-jar-url>
```

The current `assets/26.1.2/PROVENANCE.txt` records this pairing.

## Adding a New Version

1. Run `extract.sh <new-version> <paper-jar-url>` — assets land under
   `assets/<new-version>/`.
2. Append the version to the `VERSIONS` list at the top of
   `void-data/build.rs`.
3. Optionally update the `Version` enum in `src/lib.rs` and any version
   helpers (`Version::id()`).
4. `cargo build -p voidmc-data` — `OUT_DIR/blocks.rs` will gain a
   `vNEW::{...}` module. No version-specific Rust needs to be hand-written.

## Compile-Time Guarantees

- All generated types are `Copy` and `const`-constructible.
- `to_state_id` / `from_state_id` / `for_state` are `const fn`.
- The generated module is `#[allow(clippy::identity_op, clippy::erasing_op,
  clippy::eq_op, dead_code)]` — codegen frequently produces `* 1` and
  `* 0` factors for single-value strides; tagging the module silences these
  benign lints without poisoning user code.
- The output is roughly 2.8 MB of Rust source for the 26.1.2 dataset
  (1168 blocks, 778 stateful types, 40 deduplicated property enums, 5128
  unique collision shapes). `rustc` compiles it cleanly in a single pass.

## Runtime Cost

- **Registries**: one `&'static [u8]` per entry; NBT parsed lazily into a
  cached `&'static Nbt` on first access.
- **Block constants**: zero runtime cost — pure `const i32`.
- **State structs**: zero runtime cost; encoding/decoding is a couple of
  `imul` / `idiv` ops per property.
- **Shapes**: a single `match` jump and a `&'static [Aabb]` return.

There is no global initialisation, no thread-local, and no allocation. The
crate compiles into a few KB of code paths that only do arithmetic and table
lookups.
