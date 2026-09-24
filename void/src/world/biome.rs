//! Biomes at 4×4×4 cell granularity: [`biome_at`] / [`set_biome`] on a world,
//! [`BiomeId`] resolution through the synced registry, and [`BiomeBuilder`]
//! for custom biomes registered before login.

use std::fmt;

use bevy_ecs::prelude::*;
use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use voidmc_data::{BIOME_SYNCABLE_ATTRIBUTES, Version};
use voidmc_protocol::clientbound::RegistryEntry;
use voidmc_protocol::clientbound::chunk::{
    ChunkBiomeData, ChunksBiomes, PaletteData, biome_direct_bits, biome_palette, biomes,
};
use voidmc_protocol::types::BlockPosition;

use super::chunk_entity::{ChunkData, ChunkDirty, ChunkIndex};
use super::chunk_pos::ChunkPos;
use super::dimension::DimensionId;
use crate::players::WorldPlayers;
use crate::registry::RegistryDataStore;

pub const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
const VERSION: Version = Version::V26_1_2;

/// Index of a biome in the synced `minecraft:worldgen/biome` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BiomeId(pub i32);

impl BiomeId {
    /// A vanilla biome by name (`"desert"` or `"minecraft:desert"`). Custom
    /// biomes resolve through [`RegistryDataStore::biome`].
    pub fn named(name: &str) -> Option<Self> {
        biomes::id(&qualify(name)).map(BiomeId)
    }

    pub fn plains() -> Self {
        BiomeId(biomes::plains())
    }
}

pub fn qualify(name: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    }
}

impl RegistryDataStore {
    /// Resolves a biome name, vanilla or custom, to its synced index.
    pub fn biome(&self, name: &str) -> Option<BiomeId> {
        let name = qualify(name);
        self.get_registry(BIOME_REGISTRY)?
            .entries
            .iter()
            .position(|entry| entry.entry_id == name)
            .map(|index| BiomeId(index as i32))
    }

    pub fn biome_name(&self, biome: BiomeId) -> Option<&str> {
        self.get_registry(BIOME_REGISTRY)?
            .entries
            .get(usize::try_from(biome.0).ok()?)
            .map(|entry| entry.entry_id.as_str())
    }

    pub fn biome_count(&self) -> usize {
        self.get_registry(BIOME_REGISTRY)
            .map(|registry| registry.entries.len())
            .unwrap_or(0)
    }
}

fn locate(position: BlockPosition) -> (ChunkPos, u8, i32, u8) {
    (
        ChunkPos::new(position.x >> 4, position.z >> 4),
        position.x.rem_euclid(16) as u8,
        position.y as i32,
        position.z.rem_euclid(16) as u8,
    )
}

/// Biome of the 4×4×4 cell containing `position`, or `None` when the chunk is
/// not loaded or `y` is out of range.
pub fn biome_at(world: &World, dimension: DimensionId, position: BlockPosition) -> Option<BiomeId> {
    let (chunk_pos, local_x, world_y, local_z) = locate(position);
    let entity = *world
        .resource::<ChunkIndex>()
        .0
        .get(&(dimension, chunk_pos))?;
    world
        .get::<ChunkData>(entity)?
        .get_biome(local_x, world_y, local_z)
        .map(BiomeId)
}

/// Sets the biome of the cell containing `position`, marks the chunk dirty and
/// sends `ChunksBiomes` to every player seeing the chunk. Returns the previous
/// biome, or `None` when the chunk is not loaded or `y` is out of range.
pub fn set_biome(
    world: &mut World,
    dimension: DimensionId,
    position: BlockPosition,
    biome: BiomeId,
) -> Option<BiomeId> {
    let (chunk_pos, local_x, world_y, local_z) = locate(position);
    let entity = *world
        .resource::<ChunkIndex>()
        .0
        .get(&(dimension, chunk_pos))?;
    let registry_size = world.resource::<RegistryDataStore>().biome_count();
    let mut chunk = world.get_mut::<ChunkData>(entity)?;
    let previous = chunk.set_biome(local_x, world_y, local_z, biome.0, registry_size)?;
    if previous == biome.0 {
        return Some(biome);
    }
    let packet = chunk.biomes_packet(chunk_pos.x, chunk_pos.z);
    world.entity_mut(entity).insert(ChunkDirty);
    WorldPlayers::new(world).broadcast_chunk(
        dimension,
        chunk_pos,
        ChunksBiomes {
            chunks: vec![packet],
        },
    );
    Some(BiomeId(previous))
}

impl ChunkData {
    pub fn get_biome(&self, local_x: u8, world_y: i32, local_z: u8) -> Option<i32> {
        let (section, local_y) = super::chunk_entity::world_y_to_section(world_y)?;
        Some(
            self.sections
                .get(section)?
                .get_biome(local_x / 4, local_y / 4, local_z / 4),
        )
    }

    pub fn set_biome(
        &mut self,
        local_x: u8,
        world_y: i32,
        local_z: u8,
        biome_id: i32,
        registry_size: usize,
    ) -> Option<i32> {
        let (section, local_y) = super::chunk_entity::world_y_to_section(world_y)?;
        Some(self.sections.get_mut(section)?.set_biome(
            local_x / 4,
            local_y / 4,
            local_z / 4,
            biome_id,
            registry_size,
        ))
    }

    pub fn biomes_packet(&self, chunk_x: i32, chunk_z: i32) -> ChunkBiomeData {
        ChunkBiomeData::from_sections(chunk_x, chunk_z, &self.sections)
    }

    /// Re-widens direct biome palettes to the synced registry size, so chunks
    /// generated or loaded with another biome count encode correctly.
    pub fn normalize_biomes(&mut self, registry_size: usize) {
        let direct = biome_direct_bits(registry_size);
        for section in &mut self.sections {
            if matches!(section.biome, PaletteData::Direct { .. })
                && section.biome.bits_per_entry() != direct
            {
                section.biome = biome_palette(&section.biome_cells(), registry_size);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrassColorModifier {
    None,
    DarkForest,
    Swamp,
}

/// An environment attribute value: a bare value or a modifier applied to the
/// dimension's value.
#[derive(Debug, Clone, PartialEq)]
pub enum Attribute {
    Value(Tag),
    Modified { modifier: String, argument: Tag },
}

impl Attribute {
    pub fn color(rgb: u32) -> Self {
        Attribute::Value(Tag::String(color(rgb).into()))
    }

    pub fn float(value: f32) -> Self {
        Attribute::Value(Tag::Float(value))
    }

    pub fn modified(modifier: impl Into<String>, argument: Tag) -> Self {
        Attribute::Modified {
            modifier: modifier.into(),
            argument,
        }
    }

    fn tag(&self) -> Tag {
        match self {
            Attribute::Value(tag) => tag.clone(),
            Attribute::Modified { modifier, argument } => Tag::Compound(compound([
                ("modifier", Tag::String(modifier.as_str().into())),
                ("argument", argument.clone()),
            ])),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BiomeError {
    UnqualifiedName(String),
    AlreadyRegistered(String),
    NotSyncable(String),
    NoBiomeRegistry,
}

impl fmt::Display for BiomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BiomeError::UnqualifiedName(name) => {
                write!(
                    f,
                    "biome name {name:?} needs a namespace (e.g. myserver:{name})"
                )
            }
            BiomeError::AlreadyRegistered(name) => {
                write!(f, "biome {name:?} is already registered")
            }
            BiomeError::NotSyncable(id) => write!(
                f,
                "attribute {id:?} is not sent to clients; only visual/audio syncable attributes belong in a biome"
            ),
            BiomeError::NoBiomeRegistry => write!(f, "the registry store has no {BIOME_REGISTRY}"),
        }
    }
}

impl std::error::Error for BiomeError {}

/// Builds the network form of a biome (`Biome.NETWORK_CODEC`): climate,
/// `effects` and syncable `attributes`.
#[derive(Debug, Clone)]
pub struct BiomeBuilder {
    name: String,
    has_precipitation: bool,
    temperature: f32,
    frozen: bool,
    downfall: f32,
    water_color: u32,
    foliage_color: Option<u32>,
    dry_foliage_color: Option<u32>,
    grass_color: Option<u32>,
    grass_color_modifier: GrassColorModifier,
    attributes: Vec<(String, Attribute)>,
}

impl BiomeBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            has_precipitation: true,
            temperature: 0.8,
            frozen: false,
            downfall: 0.4,
            water_color: 0x3f76e4,
            foliage_color: None,
            dry_foliage_color: None,
            grass_color: None,
            grass_color_modifier: GrassColorModifier::None,
            attributes: Vec::new(),
        }
    }

    pub fn precipitation(mut self, has_precipitation: bool) -> Self {
        self.has_precipitation = has_precipitation;
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// `temperature_modifier: frozen`.
    pub fn frozen(mut self) -> Self {
        self.frozen = true;
        self
    }

    pub fn downfall(mut self, downfall: f32) -> Self {
        self.downfall = downfall;
        self
    }

    pub fn water_color(mut self, rgb: u32) -> Self {
        self.water_color = rgb;
        self
    }

    pub fn foliage_color(mut self, rgb: u32) -> Self {
        self.foliage_color = Some(rgb);
        self
    }

    pub fn dry_foliage_color(mut self, rgb: u32) -> Self {
        self.dry_foliage_color = Some(rgb);
        self
    }

    pub fn grass_color(mut self, rgb: u32) -> Self {
        self.grass_color = Some(rgb);
        self
    }

    pub fn grass_color_modifier(mut self, modifier: GrassColorModifier) -> Self {
        self.grass_color_modifier = modifier;
        self
    }

    pub fn fog_color(self, rgb: u32) -> Self {
        self.attribute("minecraft:visual/fog_color", Attribute::color(rgb))
    }

    pub fn sky_color(self, rgb: u32) -> Self {
        self.attribute("minecraft:visual/sky_color", Attribute::color(rgb))
    }

    pub fn water_fog_color(self, rgb: u32) -> Self {
        self.attribute("minecraft:visual/water_fog_color", Attribute::color(rgb))
    }

    /// Looping ambient sound (`audio/ambient_sounds.loop`).
    pub fn ambient_sound(self, sound: impl Into<String>) -> Self {
        let value = Tag::Compound(compound([("loop", Tag::String(sound.into().into()))]));
        self.attribute("minecraft:audio/ambient_sounds", Attribute::Value(value))
    }

    /// Background music (`audio/background_music.default`), delays in ticks.
    pub fn music(self, sound: impl Into<String>, min_delay: i32, max_delay: i32) -> Self {
        let track = compound([
            ("sound", Tag::String(sound.into().into())),
            ("min_delay", Tag::Int(min_delay)),
            ("max_delay", Tag::Int(max_delay)),
        ]);
        let value = Tag::Compound(compound([("default", Tag::Compound(track))]));
        self.attribute("minecraft:audio/background_music", Attribute::Value(value))
    }

    /// One `visual/ambient_particles` entry.
    pub fn ambient_particle(mut self, particle: impl Into<String>, probability: f32) -> Self {
        let entry = compound([
            (
                "particle",
                Tag::Compound(compound([("type", Tag::String(particle.into().into()))])),
            ),
            ("probability", Tag::Float(probability)),
        ]);
        let key = "minecraft:visual/ambient_particles";
        match self.attributes.iter_mut().find(|(id, _)| id == key) {
            Some((_, Attribute::Value(Tag::List(List::Compound(entries))))) => entries.push(entry),
            _ => {
                self.attributes.push((
                    key.into(),
                    Attribute::Value(Tag::List(List::Compound(vec![entry]))),
                ));
            }
        }
        self
    }

    /// Any environment attribute by id; validated against the syncable set at
    /// [`BiomeBuilder::register`].
    pub fn attribute(mut self, id: impl Into<String>, value: Attribute) -> Self {
        let id = id.into();
        self.attributes.retain(|(existing, _)| *existing != id);
        self.attributes.push((id, value));
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn nbt(&self) -> Nbt {
        let mut root = vec![
            (
                "has_precipitation".into(),
                Tag::Byte(self.has_precipitation as u8),
            ),
            ("temperature".into(), Tag::Float(self.temperature)),
        ];
        if self.frozen {
            root.push(("temperature_modifier".into(), Tag::String("frozen".into())));
        }
        root.push(("downfall".into(), Tag::Float(self.downfall)));

        let mut effects = vec![(
            "water_color".into(),
            Tag::String(color(self.water_color).into()),
        )];
        for (key, value) in [
            ("foliage_color", self.foliage_color),
            ("dry_foliage_color", self.dry_foliage_color),
            ("grass_color", self.grass_color),
        ] {
            if let Some(rgb) = value {
                effects.push((key.into(), Tag::String(color(rgb).into())));
            }
        }
        let modifier = match self.grass_color_modifier {
            GrassColorModifier::None => None,
            GrassColorModifier::DarkForest => Some("dark_forest"),
            GrassColorModifier::Swamp => Some("swamp"),
        };
        if let Some(modifier) = modifier {
            effects.push(("grass_color_modifier".into(), Tag::String(modifier.into())));
        }
        root.push(("effects".into(), Tag::Compound(Compound { tags: effects })));

        if !self.attributes.is_empty() {
            let tags = self
                .attributes
                .iter()
                .map(|(id, value)| (id.as_str().into(), value.tag()))
                .collect();
            root.push(("attributes".into(), Tag::Compound(Compound { tags })));
        }
        Nbt {
            name: "".into(),
            compound: Compound { tags: root },
        }
    }

    pub fn validate(&self) -> Result<(), BiomeError> {
        if !self.name.contains(':') {
            return Err(BiomeError::UnqualifiedName(self.name.clone()));
        }
        for (id, _) in &self.attributes {
            if !BIOME_SYNCABLE_ATTRIBUTES.contains(&id.as_str()) {
                return Err(BiomeError::NotSyncable(id.clone()));
            }
        }
        Ok(())
    }

    /// Appends the biome to the registry sent at login and returns its index.
    /// Only valid before any client has received registries: a client that
    /// already logged in would disagree with every later chunk palette.
    pub fn register(self, store: &mut RegistryDataStore) -> Result<BiomeId, BiomeError> {
        debug_assert!(
            !store.sent_to_clients(),
            "BiomeBuilder::register after registries were sent to a client"
        );
        self.validate()?;
        if store.biome(&self.name).is_some() {
            return Err(BiomeError::AlreadyRegistered(self.name));
        }
        let registry = store
            .get_registry_mut(BIOME_REGISTRY)
            .ok_or(BiomeError::NoBiomeRegistry)?;
        registry.entries.push(RegistryEntry {
            entry_id: self.name.clone(),
            data: Some(self.nbt()),
        });
        Ok(BiomeId(registry.entries.len() as i32 - 1))
    }
}

fn color(rgb: u32) -> String {
    format!("#{:06x}", rgb & 0xff_ffff)
}

fn compound<const N: usize>(tags: [(&str, Tag); N]) -> Compound {
    Compound {
        tags: tags
            .into_iter()
            .map(|(key, tag)| (key.into(), tag))
            .collect(),
    }
}

pub fn vanilla_biome_count() -> usize {
    voidmc_data::registry(VERSION, BIOME_REGISTRY)
        .map(|entries| entries.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
    use crate::network::{NetworkChannels, OutgoingPacket};
    use crate::world::ChunkPosition;
    use crate::world::chunk_entity::ChunkDimension;

    fn test_app() -> (App, flume::Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
        .insert_resource(RegistryDataStore::default())
        .init_resource::<ChunkIndex>();
        (app, outgoing_rx)
    }

    fn spawn_chunk(app: &mut App, pos: ChunkPos, biome: i32) -> Entity {
        let sections = (0..24).map(|_| ChunkSection::filled(1, biome)).collect();
        let data = ChunkData::new(sections, ChunkHeightmaps::empty(), LightData::empty());
        let entity = app
            .world_mut()
            .spawn((
                ChunkPosition(pos),
                data,
                ChunkDimension(DimensionId::Overworld),
            ))
            .id();
        app.world_mut()
            .resource_mut::<ChunkIndex>()
            .0
            .insert((DimensionId::Overworld, pos), entity);
        entity
    }

    fn pos(x: i32, y: i16, z: i32) -> BlockPosition {
        BlockPosition { x, y, z }
    }

    fn key(name: &ussr_nbt::mutf8::MString) -> String {
        name.decode().unwrap().into_owned()
    }

    #[test]
    fn names_resolve_through_the_registry_never_literals() {
        let store = RegistryDataStore::default();
        let desert = BiomeId::named("minecraft:desert").unwrap();
        assert_eq!(BiomeId::named("desert"), Some(desert));
        assert_eq!(store.biome("desert"), Some(desert));
        assert_eq!(store.biome_name(desert), Some("minecraft:desert"));
        assert_eq!(BiomeId::named("minecraft:nope"), None);
        assert_eq!(store.biome_count(), vanilla_biome_count());
        assert!(store.biome_count() > 60);
    }

    #[test]
    fn get_and_set_work_at_cell_granularity_and_notify_viewers() {
        let (mut app, rx) = test_app();
        let plains = BiomeId::plains();
        let desert = BiomeId::named("desert").unwrap();
        let chunk = ChunkPos::new(1, -1);
        spawn_chunk(&mut app, chunk, plains.0);
        app.world_mut().spawn((
            ClientId(1),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            LoadedChunks([chunk].into_iter().collect()),
        ));
        app.world_mut().spawn((
            ClientId(2),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            LoadedChunks([ChunkPos::new(0, 0)].into_iter().collect()),
        ));

        let world = app.world_mut();
        assert_eq!(
            biome_at(world, DimensionId::Overworld, pos(17, 70, -3)),
            Some(plains)
        );
        assert_eq!(biome_at(world, DimensionId::Overworld, pos(0, 70, 0)), None);
        assert_eq!(
            biome_at(world, DimensionId::Overworld, pos(17, 400, -3)),
            None
        );

        assert_eq!(
            set_biome(world, DimensionId::Overworld, pos(17, 70, -3), desert),
            Some(plains)
        );
        for (x, y, z) in [(16, 68, -4), (19, 71, -1), (17, 70, -3)] {
            assert_eq!(
                biome_at(world, DimensionId::Overworld, pos(x, y, z)),
                Some(desert)
            );
        }
        for (x, y, z) in [(20, 70, -3), (17, 72, -3), (17, 70, -5)] {
            assert_eq!(
                biome_at(world, DimensionId::Overworld, pos(x, y, z)),
                Some(plains)
            );
        }
        assert_eq!(
            set_biome(world, DimensionId::Overworld, pos(17, 70, -3), desert),
            Some(desert)
        );

        let sent: Vec<OutgoingPacket> = rx.try_iter().collect();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].client_id, 1);
        let ClientboundPacket::Play(PlayPacket::ChunksBiomes(packet)) = &sent[0].packet else {
            panic!("expected ChunksBiomes");
        };
        assert_eq!(packet.chunks.len(), 1);
        assert_eq!(
            (packet.chunks[0].chunk_x, packet.chunks[0].chunk_z),
            (1, -1)
        );
        let entity = world.resource::<ChunkIndex>().0[&(DimensionId::Overworld, chunk)];
        assert!(world.get::<ChunkDirty>(entity).is_some());
    }

    #[test]
    fn builder_emits_the_network_schema() {
        let biome = BiomeBuilder::new("myserver:crimson_sky")
            .precipitation(false)
            .temperature(1.5)
            .frozen()
            .downfall(0.1)
            .water_color(0x112233)
            .grass_color(0x00ff00)
            .grass_color_modifier(GrassColorModifier::Swamp)
            .sky_color(0xff0044)
            .fog_color(0x220011)
            .ambient_sound("minecraft:ambient.cave")
            .music("minecraft:music.nether.basalt_deltas", 12000, 24000)
            .ambient_particle("minecraft:white_ash", 0.118)
            .attribute(
                "minecraft:visual/water_fog_end_distance",
                Attribute::modified("multiply", Tag::Float(0.85)),
            );
        let nbt = biome.nbt();
        let root = &nbt.compound.tags;
        let get = |name: &str| root.iter().find(|(k, _)| key(k) == name).map(|(_, t)| t);
        assert_eq!(get("has_precipitation"), Some(&Tag::Byte(0)));
        assert_eq!(get("temperature"), Some(&Tag::Float(1.5)));
        assert_eq!(
            get("temperature_modifier"),
            Some(&Tag::String("frozen".into()))
        );
        assert_eq!(get("downfall"), Some(&Tag::Float(0.1)));
        let Some(Tag::Compound(effects)) = get("effects") else {
            panic!("effects");
        };
        assert_eq!(
            effects.tags,
            vec![
                ("water_color".into(), Tag::String("#112233".into())),
                ("grass_color".into(), Tag::String("#00ff00".into())),
                ("grass_color_modifier".into(), Tag::String("swamp".into())),
            ]
        );
        assert!(get("sky_color").is_none());
        let Some(Tag::Compound(attributes)) = get("attributes") else {
            panic!("attributes");
        };
        let keys: Vec<String> = attributes.tags.iter().map(|(k, _)| key(k)).collect();
        assert_eq!(
            keys,
            [
                "minecraft:visual/sky_color",
                "minecraft:visual/fog_color",
                "minecraft:audio/ambient_sounds",
                "minecraft:audio/background_music",
                "minecraft:visual/ambient_particles",
                "minecraft:visual/water_fog_end_distance",
            ]
        );
        assert_eq!(attributes.tags[0].1, Tag::String("#ff0044".into()));
        let Tag::Compound(modified) = &attributes.tags[5].1 else {
            panic!("modifier");
        };
        assert_eq!(
            modified.tags,
            vec![
                ("modifier".into(), Tag::String("multiply".into())),
                ("argument".into(), Tag::Float(0.85)),
            ]
        );
        let Tag::List(List::Compound(particles)) = &attributes.tags[4].1 else {
            panic!("particles");
        };
        assert_eq!(particles.len(), 1);
        assert!(biome.validate().is_ok());
    }

    #[test]
    fn minimal_builder_only_carries_required_fields() {
        let nbt = BiomeBuilder::new("myserver:plain").nbt();
        let keys: Vec<String> = nbt.compound.tags.iter().map(|(k, _)| key(k)).collect();
        assert_eq!(
            keys,
            ["has_precipitation", "temperature", "downfall", "effects"]
        );
    }

    #[test]
    fn registration_appends_and_widens_palettes() {
        let mut store = RegistryDataStore::default();
        let before = store.biome_count();
        let id = BiomeBuilder::new("myserver:crimson_sky")
            .register(&mut store)
            .unwrap();
        assert_eq!(id, BiomeId(before as i32));
        assert_eq!(store.biome("myserver:crimson_sky"), Some(id));
        assert_eq!(store.biome_count(), before + 1);
        assert_eq!(
            BiomeBuilder::new("myserver:crimson_sky").register(&mut store),
            Err(BiomeError::AlreadyRegistered("myserver:crimson_sky".into()))
        );
        assert_eq!(
            BiomeBuilder::new("crimson").register(&mut store),
            Err(BiomeError::UnqualifiedName("crimson".into()))
        );
        assert_eq!(
            BiomeBuilder::new("myserver:raid")
                .attribute(
                    "minecraft:gameplay/can_start_raid",
                    Attribute::Value(Tag::Byte(1))
                )
                .register(&mut store),
            Err(BiomeError::NotSyncable(
                "minecraft:gameplay/can_start_raid".into()
            ))
        );

        for i in 0..70 {
            BiomeBuilder::new(format!("myserver:b{i}"))
                .register(&mut store)
                .unwrap();
        }
        assert!(store.biome_count() > 128);
        let mut sections: Vec<ChunkSection> = (0..24).map(|_| ChunkSection::filled(1, 0)).collect();
        for i in 0..9 {
            sections[0].set_biome(i % 4, i / 4, 0, i as i32, 65);
        }
        assert_eq!(sections[0].biome.bits_per_entry(), 7);
        let mut data = ChunkData::new(sections, ChunkHeightmaps::empty(), LightData::empty());
        data.normalize_biomes(store.biome_count());
        assert_eq!(data.sections[0].biome.bits_per_entry(), 8);
        assert_eq!(data.get_biome(5, -60, 0), Some(5));
        assert_eq!(data.sections[1].biome.bits_per_entry(), 0);
    }
}
