# Registry System

Minecraft requires servers to send registry data during the configuration phase. Void manages this through `RegistryDataStore`, an ECS resource containing all registries and their entries.

## RegistryDataStore API

```rust
pub struct RegistryDataStore { /* private fields */ }
```

The registries are read and changed through methods only.

### Methods

| Method | Returns | Description |
|---|---|---|
| `registries()` | `&[RegistryData]` | Every registry, in the order they are sent |
| `get_registry(id)` | `Option<&RegistryData>` | Look up a registry by ID (e.g., `"minecraft:worldgen/biome"`) |
| `get_entry(registry_id, entry_id)` | `Option<&RegistryEntry>` | Look up a single entry |
| `sent_to_clients()` | `bool` | Whether a client has already received the registries |
| `get_registry_mut(id)` | `Result<Option<&mut RegistryData>, RegistriesAlreadySent>` | Mutable lookup |
| `add_registry(registry)` | `Result<(), RegistriesAlreadySent>` | Append a new registry |
| `remove_registry(id)` | `Result<Option<RegistryData>, RegistriesAlreadySent>` | Remove a registry by ID, returning it |
| `get_entry_mut(registry_id, entry_id)` | `Result<Option<&mut RegistryEntry>, RegistriesAlreadySent>` | Mutable entry lookup |
| `add_entry(registry_id, entry)` | `Result<(), RegistriesAlreadySent>` | Add an entry to an existing registry (nothing happens if the registry does not exist) |
| `remove_entry(registry_id, entry_id)` | `Result<Option<RegistryEntry>, RegistriesAlreadySent>` | Remove an entry, returning it |

### When Registries Can Change

Registries can be changed until the first client receives them, during its
configuration phase: in `ServerConfigBuilder::configure_registries`, in a
plugin, or in a system that runs before anyone logs in.

From then on they are frozen. Every mutating method (`get_registry_mut`,
`add_registry`, `remove_registry`, `get_entry_mut`, `add_entry`,
`remove_entry`, and `BiomeBuilder::register`) returns
`Err(RegistriesAlreadySent)` and leaves the store unchanged, so every client,
the first one and those that join later, is sent the same registries. Reading
still works at any time. The freeze lasts until the server stops: a client
holds registry ids by their position, so an entry added, removed or reordered
after login would make the server and that client disagree on what an id
means (for biomes, a registry that crosses a power of two also changes the
palette width clients must use to read chunks).

`RegistriesAlreadySent` (`voidmc::RegistriesAlreadySent`) implements
`std::error::Error`. Check `sent_to_clients()` first to tell the two cases
apart without trying a change. The guard lives in the store itself:
replacing the whole `RegistryDataStore` resource after login (for example
with `insert_resource`) bypasses it and desynchronises clients the same way.

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

Use `ServerConfigBuilder::configure_registries` to modify registries before the server starts. No client has the registries yet, so the mutating methods return `Ok`:

```rust
use voidmc::{BiomeBuilder, ServerConfigBuilder};

let config = ServerConfigBuilder::new()
    .configure_registries(|registries| {
        // Add a custom biome (sent with its data inline)
        BiomeBuilder::new("mymod:cherry_grove")
            .temperature(0.7)
            .downfall(0.8)
            .sky_color(0xbd10e0)
            .fog_color(0xc0d8ff)
            .water_color(0x5db7ef)
            .register(registries)
            .expect("a new, valid biome");

        // Remove a default registry
        registries
            .remove_registry("minecraft:pig_variant")
            .expect("no client has the registries yet");

        // Modify an existing entry
        if let Some(entry) = registries
            .get_entry_mut("minecraft:dimension_type", "minecraft:overworld")
            .expect("no client has the registries yet")
        {
            // Modify the NBT data
        }
    })
    .build();
```

Prefer [`BiomeBuilder`](./biomes.md#custom-biomes) to a raw `add_entry` for
biomes: it writes the 26.1 network form (sky and fog colours are environment
attributes, colours are `#rrggbb` strings), validates the name, and refuses to
register once a client has received the registries.

### At Runtime (via Plugin)

Registries can also be modified in a plugin before clients connect:

```rust
use voidmc::RegistryDataStore;

VoidServer::new(config)
    .add_plugin(|app| {
        let mut registries = app.world_mut().resource_mut::<RegistryDataStore>();
        registries
            .remove_entry("minecraft:damage_type", "minecraft:campfire")
            .expect("plugins run before any client logs in");
    })
```

Plugins run before the first tick, so registry modifications are applied before any client receives data. A system that changes registries while the server runs gets `Err(RegistriesAlreadySent)` as soon as one client has received them (see [When Registries Can Change](#when-registries-can-change)).

## How Registries Are Sent

During the configuration phase, when the client sends `KnownPacks`:

1. The server marks the registries as sent (`sent_to_clients()` becomes `true`, and every later change is refused)
2. Each `RegistryData` in `RegistryDataStore` is sent as a separate `RegistryData` configuration packet
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
