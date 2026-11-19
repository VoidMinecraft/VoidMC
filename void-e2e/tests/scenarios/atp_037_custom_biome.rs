//! VMC-ATP-037: an application adds its own biome, with client-visible
//! effects, to a marked test area using public API only.
//!
//! The scenario fixture registers `atp037:crimson_sky` (red sky and fog,
//! dark water, purple grass, no precipitation) through
//! `ServerConfigBuilder::configure_registries`, optionally after filler
//! biomes that bring the registry to 128 entries, so that appending the
//! custom biome widens direct biome palettes from 7 to 8 bits and gives it
//! id 128, which does not fit in 7 bits. The world generator places the
//! custom biome in the marked area, the cells with `x` in `120..136`, `y` in
//! `56..80` and `z` in `-8..8` (four chunks, two sections), and in a mosaic
//! section of nine biomes (chunk (8, 1), `y` in `0..16`), which needs a
//! direct palette. The section above it, and the same section of chunk
//! (0, 1) near spawn, mix nine vanilla biomes: direct palettes the generator
//! encodes before knowing the registry grew, in a chunk generated on demand
//! and in one generated at startup. At runtime, `set_biome` marks the cells
//! with `x` in `140..148`, `y` in `60..68` and `z` in `-4..4` (again four
//! chunks and two sections) and one plains cell of the mosaic. Everything
//! else is plains. The area is beyond the spawn chunks: players travel there.
//! Clients decode registries, tags, chunks and `ChunksBiomes` with Azalea,
//! which shares no code with VoidMC.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Cursor;

use azalea_world::palette::PalettedContainer;
use voidmc::components::PlayerUuid;
use voidmc::{
    Attribute, BiomeBuilder, BiomeError, BiomeId, BlockPosition, ChunkPos as ServerChunkPos,
    DimensionId, RegistryDataStore, SpawnPosition, Tag, WorldGenerator, set_biome,
};
use voidmc_e2e::core::position::{ChunkPos, ChunkSectionBiomePos, Vec3};
use voidmc_e2e::protocol::common::tags::TagMap;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::simdnbt::owned::{NbtCompound, NbtTag};
use voidmc_e2e::registry::DataRegistry;
use voidmc_e2e::registry::data::Biome;
use voidmc_e2e::registry::identifier::Identifier;
use voidmc_e2e::{Bot, BotOptions, TestServer};
use voidmc_protocol::clientbound::chunk::{Chunk, ChunkBuilder, blocks};

use crate::feet;

const CUSTOM: &str = "atp037:crimson_sky";
const DEFAULT: &str = "minecraft:plains";
const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
/// Filler biomes that bring the registry to 128 entries before the custom
/// one is appended.
const FILLERS_TO_128: usize = 63;

/// The marked area the generator places, in block coordinates (max excluded).
const AREA_MIN: [i32; 3] = [120, 56, -8];
const AREA_MAX: [i32; 3] = [136, 80, 8];
/// The middle of the marked area, in chunk (8, 0).
const AREA_CENTER: (f64, f64) = (128.5, 0.5);
const AREA_CHUNK: (i32, i32) = (8, 0);
/// The cells marked at runtime with `set_biome`.
const MARK_MIN: [i32; 3] = [140, 60, -4];
const MARK_MAX: [i32; 3] = [148, 68, 4];
/// A section of nine biomes, the custom one included: a direct palette.
/// The section above holds the same with taiga instead of the custom biome.
const MOSAIC_CHUNK: (i32, i32) = (8, 1);
const MOSAIC_MAX_Y: i32 = 32;
/// A chunk near spawn whose section at `y` in `16..32` mixes vanilla biomes.
const SPAWN_MOSAIC_CHUNK: (i32, i32) = (0, 1);
const MOSAIC: [&str; 9] = [
    DEFAULT,
    "minecraft:desert",
    "minecraft:forest",
    "minecraft:snowy_plains",
    "minecraft:beach",
    "minecraft:ocean",
    "minecraft:savanna",
    "minecraft:jungle",
    CUSTOM,
];
/// A plains cell of the mosaic, marked at runtime inside a direct palette.
const MOSAIC_MARK: [i32; 3] = [128, 0, 16];
/// Every chunk holding a cell marked at runtime.
const MARKED_CHUNKS: [(i32, i32); 5] = [(8, -1), (8, 0), (9, -1), (9, 0), MOSAIC_CHUNK];
/// Far enough from the area that every chunk around it is forgotten.
const FAR_AWAY: (f64, f64) = (300.5, 0.5);
const SPAWN: (f64, f64) = (0.5, 0.5);
const SURFACE_Y: i32 = 64;
const VIEW_DISTANCE: u8 = 2;
const MIN_Y: i32 = -64;
const SECTIONS: usize = 24;

fn inside(min: [i32; 3], max: [i32; 3], x: i32, y: i32, z: i32) -> bool {
    (min[0]..max[0]).contains(&x) && (min[1]..max[1]).contains(&y) && (min[2]..max[2]).contains(&z)
}

/// The biome of the cell whose lowest corner is at these block coordinates,
/// as generated, or after the runtime marking when `marked`.
fn configured_biome(x: i32, y: i32, z: i32, marked: bool) -> &'static str {
    if marked && (inside(MARK_MIN, MARK_MAX, x, y, z) || [x, y, z] == MOSAIC_MARK) {
        return CUSTOM;
    }
    if inside(AREA_MIN, AREA_MAX, x, y, z) {
        return CUSTOM;
    }
    let chunk = (x >> 4, z >> 4);
    if (chunk == MOSAIC_CHUNK && (0..MOSAIC_MAX_Y).contains(&y))
        || (chunk == SPAWN_MOSAIC_CHUNK && (16..MOSAIC_MAX_Y).contains(&y))
    {
        let (cx, cy, cz) = (
            x.rem_euclid(16) / 4,
            y.rem_euclid(16) / 4,
            z.rem_euclid(16) / 4,
        );
        return match MOSAIC[((cx + 4 * cz + 16 * cy) % 9) as usize] {
            CUSTOM if y >= 16 => "minecraft:taiga",
            name => name,
        };
    }
    DEFAULT
}

/// The lowest corners of the cells marked at runtime.
fn runtime_marks() -> Vec<BlockPosition> {
    let mut cells = vec![BlockPosition {
        x: MOSAIC_MARK[0],
        y: MOSAIC_MARK[1] as i16,
        z: MOSAIC_MARK[2],
    }];
    for x in (MARK_MIN[0]..MARK_MAX[0]).step_by(4) {
        for y in (MARK_MIN[1]..MARK_MAX[1]).step_by(4) {
            for z in (MARK_MIN[2]..MARK_MAX[2]).step_by(4) {
                cells.push(BlockPosition { x, y: y as i16, z });
            }
        }
    }
    cells
}

/// The application's biome: only a few properties set, every one of them
/// visible to a player.
fn crimson_sky() -> BiomeBuilder {
    BiomeBuilder::new(CUSTOM)
        .precipitation(false)
        .temperature(1.2)
        .sky_color(0xff0044)
        .fog_color(0x220011)
        .water_color(0x5f1030)
        .grass_color(0x9a3a4f)
}

/// The registry entry a client must receive for `crimson_sky()`, flattened:
/// the defaults of the builder for the climate fields it does not set, the
/// colours as `#rrggbb` strings like the vanilla data pack.
fn expected_entry() -> Vec<(String, String)> {
    [
        ("has_precipitation", "byte 0"),
        ("temperature", "float 1.2"),
        ("downfall", "float 0.4"),
        ("effects.water_color", "string #5f1030"),
        ("effects.grass_color", "string #9a3a4f"),
        ("attributes.minecraft:visual/sky_color", "string #ff0044"),
        ("attributes.minecraft:visual/fog_color", "string #220011"),
    ]
    .into_iter()
    .map(|(path, value)| (path.to_string(), value.to_string()))
    .collect()
}

struct MarkedWorld {
    custom: BiomeId,
}

impl WorldGenerator for MarkedWorld {
    fn generate_chunk(&self, pos: &ServerChunkPos) -> Chunk {
        ChunkBuilder::new(pos.x, pos.z)
            .biomes_from(|x, y, z| self.cell_biome(x, y, z).0)
            .fill_below(SURFACE_Y - 1, blocks::STONE)
            .fill_range(SURFACE_Y - 1, SURFACE_Y, blocks::GRASS_BLOCK)
            .build()
    }

    fn surface_height_at(&self, _block_x: i32, _block_z: i32) -> i32 {
        SURFACE_Y
    }

    fn cell_biome(&self, x: i32, y: i32, z: i32) -> BiomeId {
        match configured_biome(x, y, z, false) {
            CUSTOM => self.custom,
            name => BiomeId::named(name).expect("a vanilla biome"),
        }
    }
}

/// Starts the test application: the custom biome is registered after
/// `fillers` other custom biomes, and the generator places it.
async fn start(fillers: usize) -> TestServer {
    TestServer::builder()
        .config(move |config| {
            let mut custom = None;
            let config = config.configure_registries(|registries| {
                for i in 0..fillers {
                    BiomeBuilder::new(format!("atp037:filler_{i}"))
                        .register(registries)
                        .expect("a filler biome");
                }
                custom = Some(
                    crimson_sky()
                        .register(registries)
                        .expect("the custom biome"),
                );
            });
            config
                .world_generator(MarkedWorld {
                    custom: custom.expect("registered"),
                })
                .spawn_position(SpawnPosition {
                    x: SPAWN.0,
                    z: SPAWN.1,
                    y: None,
                })
                .view_distance(i32::from(VIEW_DISTANCE))
        })
        .start()
        .await
}

async fn join(server: &TestServer, name: &str) -> Bot {
    server
        .join_with(
            name,
            BotOptions {
                view_distance: VIEW_DISTANCE,
            },
        )
        .await
}

fn square(center: ChunkPos, radius: i32) -> HashSet<ChunkPos> {
    (-radius..=radius)
        .flat_map(|dx| {
            (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
        })
        .collect()
}

async fn wait_for_chunks_around(bot: &Bot, center: ChunkPos) {
    let expected = square(center, i32::from(VIEW_DISTANCE));
    bot.wait_until(&format!("the chunks around {center:?}"), |view| {
        view.chunks.keys().copied().collect::<HashSet<_>>() == expected
    })
    .await;
}

/// Walks in steps of at most 8 blocks, like the other movement scenarios.
async fn walk_to(bot: &Bot, x: f64, z: f64) {
    let mut position = bot.position();
    while position.x != x || position.z != z {
        position = Vec3::new(
            position.x + (x - position.x).clamp(-8.0, 8.0),
            position.y,
            position.z + (z - position.z).clamp(-8.0, 8.0),
        );
        bot.move_to(position).await;
    }
}

/// Travels from wherever the bot is to the marked area.
async fn travel_to_the_area(bot: &Bot) {
    walk_to(bot, AREA_CENTER.0, AREA_CENTER.1).await;
    wait_for_chunks_around(bot, ChunkPos::new(AREA_CHUNK.0, AREA_CHUNK.1)).await;
}

// ---- the registry and tags, as the client decoded them

fn biome_registry(bot: &Bot) -> Vec<(String, NbtCompound)> {
    bot.transcript()
        .registry_data
        .extra
        .get(&Identifier::new(BIOME_REGISTRY))
        .unwrap_or_else(|| panic!("{} received no biome registry", bot.name()))
        .map
        .iter()
        .map(|(name, entry)| (name.to_string(), entry.clone()))
        .collect()
}

fn biome_names(bot: &Bot) -> Vec<String> {
    biome_registry(bot)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

fn tags(bot: &Bot) -> TagMap {
    bot.transcript()
        .tags
        .clone()
        .unwrap_or_else(|| panic!("{} received no tags", bot.name()))
}

/// Every leaf of an NBT compound as `(dotted path, "type value")`.
fn flatten(compound: &NbtCompound) -> Vec<(String, String)> {
    fn walk(prefix: &str, compound: &NbtCompound, out: &mut Vec<(String, String)>) {
        for (key, tag) in compound.iter() {
            let path = if prefix.is_empty() {
                key.to_string()
            } else {
                format!("{prefix}.{key}")
            };
            let value = match tag {
                NbtTag::Compound(inner) => {
                    walk(&path, inner, out);
                    continue;
                }
                NbtTag::Byte(value) => format!("byte {value}"),
                NbtTag::Int(value) => format!("int {value}"),
                NbtTag::Float(value) => format!("float {value}"),
                NbtTag::String(value) => format!("string {}", value.to_str()),
                other => format!("{other:?}"),
            };
            out.push((path, value));
        }
    }
    let mut out = Vec::new();
    walk("", compound, &mut out);
    out
}

/// The type of every leaf, by path.
fn leaf_types(compound: &NbtCompound) -> BTreeMap<String, String> {
    flatten(compound)
        .into_iter()
        .map(|(path, value)| {
            let kind = value.split(' ').next().unwrap_or_default().to_string();
            (path, kind)
        })
        .collect()
}

/// Checks the biome registry and the tags `bot` received against `reference`,
/// a client of a server without custom biomes: the vanilla entries come first,
/// unchanged and in the same order, then the fillers, then the custom biome
/// with exactly the configured properties. Returns the custom biome's index.
fn assert_registry_carries_the_custom_biome(bot: &Bot, reference: &Bot, fillers: usize) -> usize {
    let name = bot.name();
    let registry = biome_registry(bot);
    let vanilla = biome_registry(reference);
    assert_eq!(
        registry.len(),
        vanilla.len() + fillers + 1,
        "{name}'s biome registry size"
    );
    for (index, ((got, got_data), (want, want_data))) in registry.iter().zip(&vanilla).enumerate() {
        assert_eq!(got, want, "{name}'s biome {index}");
        assert!(got_data == want_data, "{name}'s {got} differs from vanilla");
    }
    for (offset, (got, _)) in registry[vanilla.len()..registry.len() - 1]
        .iter()
        .enumerate()
    {
        assert_eq!(*got, format!("atp037:filler_{offset}"));
    }
    let custom = registry.len() - 1;
    let (custom_name, custom_data) = &registry[custom];
    assert_eq!(custom_name, CUSTOM, "{name}'s last biome");
    assert_eq!(
        flatten(custom_data),
        expected_entry(),
        "{name}'s {CUSTOM} entry"
    );

    // The client rejects an entry missing a mandatory field of the network
    // codec. Whatever every vanilla biome carries, with its type, the custom
    // one carries too.
    let mut always: Option<BTreeMap<String, String>> = None;
    for (_, data) in &vanilla {
        let types = leaf_types(data);
        always = Some(match always {
            None => types,
            Some(common) => common
                .into_iter()
                .filter(|(path, kind)| types.get(path) == Some(kind))
                .collect(),
        });
    }
    let always = always.expect("vanilla biomes");
    assert!(
        always.contains_key("has_precipitation") && always.contains_key("effects.water_color"),
        "the vanilla fields every biome carries: {always:?}"
    );
    let custom_types = leaf_types(custom_data);
    for (path, kind) in &always {
        assert_eq!(
            custom_types.get(path),
            Some(kind),
            "{CUSTOM} lacks the field {path} every vanilla biome has"
        );
    }

    // Tags reference entries by index: appending must leave them valid and
    // unchanged, and the custom biome is in no vanilla tag.
    let received = tags(bot);
    assert!(
        received == tags(reference),
        "{name}'s tags differ from those of a server without custom biomes"
    );
    let biome_tags = received
        .0
        .get(&Identifier::new(BIOME_REGISTRY))
        .unwrap_or_else(|| panic!("{name} received no biome tags"));
    assert!(!biome_tags.is_empty());
    for tag in biome_tags {
        assert!(
            tag.elements
                .iter()
                .all(|&element| (0..vanilla.len() as i32).contains(&element)),
            "biome tag {} points outside the vanilla entries: {:?}",
            tag.name,
            tag.elements
        );
    }
    // Resolved through the registry, tags name the biomes of the vanilla
    // data pack.
    for (tag_name, members) in [
        (
            "minecraft:is_badlands",
            &[
                "minecraft:badlands",
                "minecraft:eroded_badlands",
                "minecraft:wooded_badlands",
            ][..],
        ),
        (
            "minecraft:is_beach",
            &["minecraft:beach", "minecraft:snowy_beach"][..],
        ),
        (
            "minecraft:is_river",
            &["minecraft:river", "minecraft:frozen_river"][..],
        ),
    ] {
        let tag = biome_tags
            .iter()
            .find(|tag| tag.name.to_string() == tag_name)
            .unwrap_or_else(|| panic!("{name} received no tag {tag_name}"));
        let resolved: BTreeSet<&str> = tag
            .elements
            .iter()
            .map(|&element| registry[element as usize].0.as_str())
            .collect();
        assert_eq!(
            resolved,
            members.iter().copied().collect(),
            "{name}'s {tag_name}"
        );
    }
    custom
}

/// What a client of a server without custom biomes receives.
async fn vanilla_reference() -> Bot {
    let server = TestServer::builder()
        .config(|config| config.view_distance(i32::from(VIEW_DISTANCE)))
        .start()
        .await;
    let reference = join(&server, "Reference").await;
    reference.assert_healthy();
    server.stop().await;
    reference
}

// ---- biome cells, as the client decoded them

fn cell_coordinates() -> impl Iterator<Item = (u8, u8, u8)> {
    (0..4u8).flat_map(|y| (0..4u8).flat_map(move |z| (0..4u8).map(move |x| (x, y, z))))
}

fn section_ids(container: &PalettedContainer<Biome>) -> impl Iterator<Item = u32> + '_ {
    cell_coordinates().map(|(x, y, z)| {
        container
            .get(ChunkSectionBiomePos { x, y, z })
            .protocol_id()
    })
}

fn direct_bits(registry_size: usize) -> u8 {
    (usize::BITS - (registry_size - 1).leading_zeros()) as u8
}

/// A vanilla client reads a direct biome palette at `ceil(log2(registry
/// size))` bits whatever the packet announces; Azalea reads it at the
/// announced width, so the width is checked here.
fn assert_vanilla_width(
    bot: &Bot,
    what: &str,
    container: &PalettedContainer<Biome>,
    registry_size: usize,
) -> bool {
    let bits = container.bits_per_entry;
    if bits <= 3 {
        return false;
    }
    assert_eq!(
        bits,
        direct_bits(registry_size),
        "{} got a direct biome palette of the wrong width in {what}",
        bot.name()
    );
    true
}

/// The biome ids of every cell of every chunk the bot holds, bottom section
/// first, as its last chunk packets set them. Also returns how many sections
/// use a direct palette.
fn held_ids(bot: &Bot, registry_size: usize) -> (BTreeMap<(i32, i32), Vec<u32>>, usize) {
    let mut direct = 0;
    let held = bot.view(|view| {
        view.chunks
            .iter()
            .map(|(pos, chunk)| {
                assert_eq!(chunk.sections.len(), SECTIONS);
                let mut ids = Vec::new();
                for (index, section) in chunk.sections.iter().enumerate() {
                    let what = format!("chunk {pos:?} section {index}");
                    if assert_vanilla_width(bot, &what, &section.biomes, registry_size) {
                        direct += 1;
                    }
                    ids.extend(section_ids(&section.biomes));
                }
                ((pos.x, pos.z), ids)
            })
            .collect()
    });
    (held, direct)
}

/// Decodes one chunk's `ChunksBiomes` buffer: exactly one biome container per
/// section, nothing left over.
fn decode_chunk_biomes(
    bot: &Bot,
    pos: ChunkPos,
    buffer: &[u8],
    registry_size: usize,
) -> (Vec<u32>, usize) {
    let mut cursor = Cursor::new(buffer);
    let mut ids = Vec::new();
    let mut direct = 0;
    for section in 0..SECTIONS {
        let container = PalettedContainer::<Biome>::read(&mut cursor).unwrap_or_else(|error| {
            panic!(
                "{} could not decode the biomes of chunk {pos:?} section {section}: {error}",
                bot.name()
            )
        });
        let what = format!("the ChunksBiomes of chunk {pos:?} section {section}");
        if assert_vanilla_width(bot, &what, &container, registry_size) {
            direct += 1;
        }
        ids.extend(section_ids(&container));
    }
    assert_eq!(
        cursor.position() as usize,
        buffer.len(),
        "{} got bytes after the biomes of chunk {pos:?}",
        bot.name()
    );
    (ids, direct)
}

/// Compares every cell of `held` with the fixture, resolving ids through the
/// registry the bot received.
fn assert_cells_match(bot: &Bot, held: &BTreeMap<(i32, i32), Vec<u32>>, marked: bool, when: &str) {
    let names = biome_names(bot);
    assert!(!held.is_empty(), "{} holds no chunk {when}", bot.name());
    let mut wrong = Vec::new();
    for (&(chunk_x, chunk_z), ids) in held {
        assert_eq!(ids.len(), SECTIONS * 64);
        let expected = (0..SECTIONS as i32).flat_map(|section| {
            cell_coordinates().map(move |(x, y, z)| {
                configured_biome(
                    chunk_x * 16 + i32::from(x) * 4,
                    MIN_Y + section * 16 + i32::from(y) * 4,
                    chunk_z * 16 + i32::from(z) * 4,
                    marked,
                )
            })
        });
        for (index, (&id, want)) in ids.iter().zip(expected).enumerate() {
            let got = names
                .get(id as usize)
                .map(String::as_str)
                .unwrap_or("<outside the registry>");
            if got != want {
                wrong.push(format!(
                    "chunk ({chunk_x}, {chunk_z}) cell {index}: {got} (id {id}), expected {want}"
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} sees {} misplaced biome cell(s) {when}, first ones: {:#?}",
        bot.name(),
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );
}

/// The biome the bot's client renders at its feet, and so on its F3 screen.
fn biome_at_feet(bot: &Bot) -> String {
    let feet = feet(bot);
    let names = biome_names(bot);
    bot.view(|view| {
        let chunk = view
            .chunks
            .get(&ChunkPos::from(feet))
            .expect("the chunk under the bot");
        let section = ((feet.y - MIN_Y) / 16) as usize;
        let id = chunk.sections[section]
            .biomes
            .get(ChunkSectionBiomePos {
                x: (feet.x.rem_euclid(16) / 4) as u8,
                y: (feet.y.rem_euclid(16) / 4) as u8,
                z: (feet.z.rem_euclid(16) / 4) as u8,
            })
            .protocol_id();
        names[id as usize].clone()
    })
}

/// The marked area as the bot sees it on arrival: every held cell matches
/// the fixture, the direct palettes have the vanilla width, and the bot
/// stands in the custom biome.
fn assert_in_the_marked_area(
    bot: &Bot,
    marked: bool,
    when: &str,
) -> BTreeMap<(i32, i32), Vec<u32>> {
    let registry_size = biome_names(bot).len();
    let (held, direct) = held_ids(bot, registry_size);
    assert_cells_match(bot, &held, marked, when);
    assert_eq!(
        direct,
        2,
        "{} must hold the two direct palettes of the mosaic {when}",
        bot.name()
    );
    for (chunk_x, chunk_z) in MARKED_CHUNKS {
        assert!(held.contains_key(&(chunk_x, chunk_z)));
    }
    assert_eq!(biome_at_feet(bot), CUSTOM, "{} {when}", bot.name());
    held
}

fn queued_biome_updates(bot: &Bot) -> Vec<(ChunkPos, Vec<u8>)> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::ChunksBiomes(packet) => Some(
            packet
                .chunk_biome_data
                .iter()
                .map(|chunk| (chunk.pos, chunk.buffer.clone()))
                .collect::<Vec<_>>(),
        ),
        _ => None,
    })
    .into_iter()
    .flatten()
    .collect()
}

// ---- scenarios

/// Steps 1–5: the marked area uses the custom biome, the client knows its
/// identifier and effects, cells marked at runtime arrive as decodable
/// `ChunksBiomes`, and everything is the same after leaving and re-entering,
/// after a reconnect and for a late joiner.
async fn the_marked_area_uses_the_custom_biome(fillers: usize) {
    let reference = vanilla_reference().await;
    let server = start(fillers).await;
    let custom_id = server
        .with_world(|world| world.resource::<RegistryDataStore>().biome(CUSTOM))
        .await
        .expect("the server resolves the custom biome");

    let alice = join(&server, "Alice").await;
    let custom = assert_registry_carries_the_custom_biome(&alice, &reference, fillers);
    assert_eq!(
        custom, custom_id.0 as usize,
        "the client's id is the server's"
    );
    let registry_size = biome_names(&alice).len();
    wait_for_chunks_around(&alice, ChunkPos::new(0, 0)).await;
    let (at_spawn, direct) = held_ids(&alice, registry_size);
    assert_cells_match(&alice, &at_spawn, false, "at spawn");
    assert_eq!(direct, 1, "the direct palette of the spawn mosaic");
    assert_eq!(biome_at_feet(&alice), DEFAULT);

    travel_to_the_area(&alice).await;
    assert_in_the_marked_area(&alice, false, "on arrival");

    // Mark more cells at runtime, while Alice holds every chunk concerned.
    alice.clear();
    let previous = server
        .with_world(move |world| {
            runtime_marks()
                .into_iter()
                .map(|cell| set_biome(world, DimensionId::Overworld, cell, custom_id))
                .collect::<Vec<_>>()
        })
        .await;
    assert!(
        previous
            .iter()
            .all(|biome| *biome == BiomeId::named(DEFAULT)),
        "set_biome replaced other biomes: {previous:?}"
    );
    alice.sync().await;
    let updates = queued_biome_updates(&alice);
    let updated: BTreeSet<(i32, i32)> = updates.iter().map(|(pos, _)| (pos.x, pos.z)).collect();
    assert_eq!(
        updated,
        MARKED_CHUNKS.into_iter().collect(),
        "the chunks Alice got biome updates for"
    );
    let (mut client, _) = held_ids(&alice, registry_size);
    let mut direct = 0;
    for (pos, buffer) in &updates {
        let (ids, direct_sections) = decode_chunk_biomes(&alice, *pos, buffer, registry_size);
        direct += direct_sections;
        *client
            .get_mut(&(pos.x, pos.z))
            .expect("a biome update for a held chunk") = ids;
    }
    assert!(direct >= 1, "no update carried a direct palette");
    assert_cells_match(&alice, &client, true, "after the runtime marking");
    alice.clear();

    // Leave the area until its chunks are forgotten, then come back.
    walk_to(&alice, FAR_AWAY.0, FAR_AWAY.1).await;
    let far_chunk = ChunkPos::new(FAR_AWAY.0 as i32 >> 4, FAR_AWAY.1 as i32 >> 4);
    wait_for_chunks_around(&alice, far_chunk).await;
    assert_eq!(biome_at_feet(&alice), DEFAULT);
    travel_to_the_area(&alice).await;
    let revisit = assert_in_the_marked_area(&alice, true, "after re-entering");
    alice.sync().await;
    assert!(
        queued_biome_updates(&alice).is_empty(),
        "the marks must come with the chunks, not as biome updates"
    );

    // A late joiner and a reconnecting player get the same registry and cells.
    let bob = join(&server, "Bob").await;
    travel_to_the_area(&bob).await;
    alice.assert_healthy();
    let uuid = alice.uuid();
    alice.disconnect().await;
    server
        .wait_for("Alice to leave", move |world| {
            let gone = !world
                .query::<&PlayerUuid>()
                .iter(world)
                .any(|player| player.0 == uuid);
            gone.then_some(())
        })
        .await;
    let alice = join(&server, "Alice").await;
    travel_to_the_area(&alice).await;
    for bot in [&bob, &alice] {
        assert_eq!(
            assert_registry_carries_the_custom_biome(bot, &reference, fillers),
            custom
        );
        let held = assert_in_the_marked_area(bot, true, "after joining later");
        assert!(held == revisit, "{} sees another area", bot.name());
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn a_custom_biome_marks_the_test_area() {
    the_marked_area_uses_the_custom_biome(0).await;
}

/// The same with 128 entries before the custom biome: appending it widens
/// direct palettes to 8 bits, and its id, 128, does not fit in 7.
#[tokio::test]
async fn a_custom_biome_that_widens_direct_palettes_marks_the_test_area() {
    the_marked_area_uses_the_custom_biome(FILLERS_TO_128).await;
}

/// Mistakes in the application's biome definitions are reported, and leave
/// the registry clients receive untouched: a duplicate, an unqualified or
/// malformed identifier, a server-only attribute, and a registration after
/// players received the registries.
#[tokio::test]
async fn invalid_custom_biomes_are_refused_without_reaching_clients() {
    let reference = vanilla_reference().await;
    let server = TestServer::builder()
        .config(|config| {
            config
                .configure_registries(|registries| {
                    crimson_sky()
                        .register(registries)
                        .expect("the custom biome");
                    assert_eq!(
                        crimson_sky().register(registries),
                        Err(BiomeError::AlreadyRegistered(CUSTOM.into()))
                    );
                    assert_eq!(
                        BiomeBuilder::new("crimson_sky").register(registries),
                        Err(BiomeError::UnqualifiedName("crimson_sky".into()))
                    );
                    for name in ["atp037:Crimson Sky", "ATP037:sky", "atp037:", "atp037:a:b"] {
                        assert_eq!(
                            BiomeBuilder::new(name).register(registries),
                            Err(BiomeError::InvalidName(name.into()))
                        );
                    }
                    assert_eq!(
                        BiomeBuilder::new("atp037:raid")
                            .attribute(
                                "minecraft:gameplay/can_start_raid",
                                Attribute::Value(Tag::Byte(1)),
                            )
                            .register(registries),
                        Err(BiomeError::NotSyncable(
                            "minecraft:gameplay/can_start_raid".into()
                        ))
                    );
                })
                .view_distance(i32::from(VIEW_DISTANCE))
        })
        .start()
        .await;
    let alice = join(&server, "Alice").await;
    assert_registry_carries_the_custom_biome(&alice, &reference, 0);

    let late = server
        .with_world(|world| {
            let mut registries = world.resource_mut::<RegistryDataStore>();
            let result = BiomeBuilder::new("atp037:late").register(&mut registries);
            (result, registries.biome_count())
        })
        .await;
    assert_eq!(
        late,
        (
            Err(BiomeError::RegistriesAlreadySent("atp037:late".into())),
            biome_names(&alice).len()
        )
    );

    let bob = join(&server, "Bob").await;
    assert_registry_carries_the_custom_biome(&bob, &reference, 0);
    assert_eq!(biome_names(&bob), biome_names(&alice));
    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}
