# Registry System

Minecraft requires servers to send registry data during the configuration phase. Void manages this through `RegistryDataStore`, an ECS resource containing all registries and their entries.

## RegistryDataStore API

```rust
pub struct RegistryDataStore {
    pub registries: Vec<RegistryData>,
}
```

### Methods

| Method | Description |
|---|---|
| `get_registry(id)` | Look up a registry by ID (e.g., `"minecraft:worldgen/biome"`) |
| `get_registry_mut(id)` | Mutable lookup |
| `add_registry(registry)` | Append a new registry |
| `remove_registry(id)` | Remove a registry by ID, returning it |
| `get_entry(registry_id, entry_id)` | Look up a single entry |
| `get_entry_mut(registry_id, entry_id)` | Mutable entry lookup |
| `add_entry(registry_id, entry)` | Add an entry to an existing registry |
| `remove_entry(registry_id, entry_id)` | Remove an entry, returning it |

## Default Registries

`RegistryDataStore::default()` (via `default_registry_data()`) provides the minimum set of registries required by a vanilla-compatible client:

| Registry ID | Entries | Description |
|---|---|---|
| `minecraft:dimension_type` | `minecraft:overworld` | Dimension properties (skylight, height, min_y, etc.) |
| `minecraft:worldgen/biome` | `minecraft:plains` | Biome data (temperature, precipitation, colors, sounds) |
| `minecraft:painting_variant` | `minecraft:kebab` | Painting textures and dimensions |
| `minecraft:damage_type` | 25 entries | All required damage types (generic, fall, fire, drown, etc.) |
| `minecraft:wolf_variant` | `minecraft:pale` | Wolf textures |
| `minecraft:wolf_sound_variant` | `minecraft:classic` | Wolf sound effects |
| `minecraft:cat_variant` | `minecraft:tabby` | Cat textures |
| `minecraft:chicken_variant` | `minecraft:temperate` | Chicken textures |
| `minecraft:cow_variant` | `minecraft:temperate` | Cow textures |
| `minecraft:frog_variant` | `minecraft:temperate` | Frog textures |
| `minecraft:pig_variant` | `minecraft:temperate` | Pig textures |

## Registry Entry Format

Each entry is a `RegistryEntry`:

```rust
pub struct RegistryEntry {
    pub entry_id: String,       // e.g., "minecraft:overworld"
    pub data: Option<Nbt>,      // NBT compound with entry properties
}
```

The `data` field contains an NBT compound (`ussr_nbt::owned::Nbt`) whose tags vary by registry type.

## Customizing Registries

### At Build Time

Use `ServerBuilder::configure_registries` to modify registries before the server starts:

```rust
use voidmc::ServerBuilder;
use voidmc_protocol::clientbound::{RegistryData, RegistryEntry};
use ussr_nbt::owned::{Nbt, Tag, Compound};

let config = ServerBuilder::new()
    .configure_registries(|registries| {
        // Add a custom biome
        registries.add_entry(
            "minecraft:worldgen/biome",
            RegistryEntry {
                entry_id: "mymod:cherry_grove".to_string(),
                data: Some(Nbt {
                    name: Default::default(),
                    compound: Compound {
                        tags: vec![
                            ("has_precipitation".into(), Tag::Byte(1)),
                            ("temperature".into(), Tag::Float(0.7)),
                            ("downfall".into(), Tag::Float(0.8)),
                            ("effects".into(), Tag::Compound(Compound {
                                tags: vec![
                                    ("sky_color".into(), Tag::Int(12390624)),
                                    ("water_fog_color".into(), Tag::Int(329011)),
                                    ("fog_color".into(), Tag::Int(12638463)),
                                    ("water_color".into(), Tag::Int(6141935)),
                                ].into(),
                            })),
                        ].into(),
                    },
                }),
            },
        );

        // Remove a default registry
        registries.remove_registry("minecraft:pig_variant");

        // Modify an existing entry
        if let Some(entry) = registries.get_entry_mut(
            "minecraft:dimension_type",
            "minecraft:overworld",
        ) {
            // Modify the NBT data
        }
    })
    .build();
```

### At Runtime (via Plugin)

Registries can also be modified in a plugin before clients connect:

```rust
use voidmc::RegistryDataStore;

VoidServer::new(config)
    .add_plugin(|app| {
        let mut registries = app.world_mut().resource_mut::<RegistryDataStore>();
        registries.remove_entry("minecraft:damage_type", "minecraft:campfire");
    })
```

Plugins run before the first tick, so registry modifications are applied before any client receives data.

## How Registries Are Sent

During the configuration phase, when the client sends `KnownPacks`:

1. The server clones all registries from `RegistryDataStore`
2. Each `RegistryData` is sent as a separate `RegistryData` configuration packet
3. After all registries are sent, `FinishConfiguration` is sent

The client uses this data to set up its local registry state for rendering, gameplay, and validation.

## NBT Format

Registry entry data uses `ussr_nbt::owned::Nbt`. Common patterns:

```rust
use ussr_nbt::owned::{Nbt, Tag, Compound};

// Create an NBT compound
let nbt = Nbt {
    name: Default::default(),  // Empty root name
    compound: Compound {
        tags: vec![
            ("has_skylight".into(), Tag::Byte(1)),
            ("min_y".into(), Tag::Int(-64)),
            ("height".into(), Tag::Int(384)),
            ("effects".into(), Tag::String("minecraft:overworld".into())),
            ("ambient_light".into(), Tag::Float(0.0)),
        ].into(),
    },
};
```

### Common Tag Types

| Tag | Rust Type | Usage |
|---|---|---|
| `Tag::Byte(i8)` | `i8` | Boolean-like values (0/1) |
| `Tag::Int(i32)` | `i32` | Integer properties (height, min_y, etc.) |
| `Tag::Float(f32)` | `f32` | Float properties (temperature, light) |
| `Tag::Double(f64)` | `f64` | Double properties (coordinate_scale) |
| `Tag::String(String)` | `String` | String identifiers and paths |
| `Tag::Compound(Compound)` | Nested object | Nested structures (effects, mood_sound) |
