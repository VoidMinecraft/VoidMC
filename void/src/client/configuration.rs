use std::sync::Arc;
use tokio::sync::Mutex;
use ussr_nbt::owned::{Nbt, Tag};

use super::login::ClientIdentity;
use crate::game::Game;
use void_net::{
    ClientSocket,
    clientbound::{self, FinishConfiguration, KnownPack, KnownPacks, RegistryData, RegistryEntry},
    serverbound,
};

pub struct ConfigurationClient {
    socket: ClientSocket,
    game: Arc<Mutex<Game>>,
    identity: ClientIdentity,
}

const DAMAGE_TYPES_ENTRY_IDS: [&str; 49] = [
    "arrow",
    "bad_respawn_point",
    "cactus",
    "campfire",
    "cramming",
    "dragon_breath",
    "drown",
    "dry_out",
    "ender_pearl",
    "explosion",
    "fall",
    "falling_anvil",
    "falling_block",
    "falling_stalactite",
    "fireball",
    "fireworks",
    "fly_into_wall",
    "freeze",
    "generic",
    "generic_kill",
    "hot_floor",
    "in_fire",
    "in_wall",
    "indirect_magic",
    "lava",
    "lightning_bolt",
    "mace_smash",
    "magic",
    "mob_attack",
    "mob_attack_no_aggro",
    "mob_projectile",
    "on_fire",
    "out_of_world",
    "outside_border",
    "player_attack",
    "player_explosion",
    "sonic_boom",
    "spit",
    "stalagmite",
    "starve",
    "sting",
    "sweet_berry_bush",
    "thorns",
    "thrown",
    "trident",
    "unattributed_fireball",
    "wind_charge",
    "wither",
    "wither_skull",
];

impl ConfigurationClient {
    pub async fn new(
        mut socket: ClientSocket,
        game: Arc<Mutex<Game>>,
        identity: ClientIdentity,
    ) -> std::io::Result<Self> {
        socket
            .send(&clientbound::ConfigurationPacket::KnownPacks(KnownPacks {
                known_packs: vec![KnownPack {
                    namespace: "minecraft".into(),
                    id: "core".into(),
                    version: "1.21.4".into(),
                }],
            }))
            .await?;

        // Wolf variants
        socket
            .send(&clientbound::ConfigurationPacket::RegistryData(
                RegistryData {
                    registry_id: "minecraft:wolf_variant".into(),
                    entries: vec![RegistryEntry {
                        entry_id: "minecraft:black".to_string(),
                        data: Some(Nbt {
                            name: "".into(),
                            compound: vec![
                                (
                                    "wild_texture".into(),
                                    Tag::String("minecraft:entity/wolf/wolf_ashen".into()),
                                ),
                                (
                                    "tame_texture".into(),
                                    Tag::String("minecraft:entity/wolf/wolf_ashen".into()),
                                ),
                                (
                                    "angry_texture".into(),
                                    Tag::String("minecraft:entity/wolf/wolf_ashen".into()),
                                ),
                                (
                                    "biomes".into(),
                                    Tag::String("minecraft:old_growth_pine_taiga".into()),
                                ),
                            ]
                            .into(),
                        }),
                    }],
                },
            ))
            .await?;

        // Biomes
        socket
            .send(&clientbound::ConfigurationPacket::RegistryData(
                RegistryData {
                    registry_id: "minecraft:worldgen/biome".into(),
                    entries: vec![
                        RegistryEntry {
                            entry_id: "minecraft:old_growth_pine_taiga".into(),
                            data: Some(Nbt {
                                name: "".into(),
                                compound: vec![
                                    ("has_precipitation".into(), Tag::Byte(0)),
                                    ("temperature".into(), Tag::Float(1.0)),
                                    ("downfall".into(), Tag::Float(0.0)),
                                    (
                                        "effects".into(),
                                        Tag::Compound(
                                            vec![
                                                ("fog_color".into(), Tag::Int(8364543)),
                                                ("water_color".into(), Tag::Int(8364543)),
                                                ("water_fog_color".into(), Tag::Int(8364543)),
                                                ("sky_color".into(), Tag::Int(8364543)),
                                            ]
                                            .into(),
                                        ),
                                    ),
                                ]
                                .into(),
                            }),
                        },
                        RegistryEntry {
                            entry_id: "minecraft:plains".to_string(),
                            data: Some(Nbt {
                                name: "".into(),
                                compound: vec![
                                    ("has_precipitation".into(), Tag::Byte(0)),
                                    ("temperature".into(), Tag::Float(1.0)),
                                    ("downfall".into(), Tag::Float(0.0)),
                                    (
                                        "effects".into(),
                                        Tag::Compound(
                                            vec![
                                                ("fog_color".into(), Tag::Int(8364543)),
                                                ("water_color".into(), Tag::Int(8364543)),
                                                ("water_fog_color".into(), Tag::Int(8364543)),
                                                ("sky_color".into(), Tag::Int(8364543)),
                                            ]
                                            .into(),
                                        ),
                                    ),
                                ]
                                .into(),
                            }),
                        },
                    ],
                },
            ))
            .await?;

        // Painting variants
        socket
            .send(&clientbound::ConfigurationPacket::RegistryData(RegistryData {
                registry_id: "minecraft:painting_variant".into(),
                entries: vec![
                    RegistryEntry {
                        entry_id: "minecraft:backyard".into(),
                        data: Some(Nbt {
                            name: "".into(),
                            compound: vec![
                                ("asset_id".into(), Tag::String("minecraft:backyard".into())),
                                ("height".into(), Tag::Int(1)),
                                ("width".into(), Tag::Int(1)),
                                ("title".into(), Tag::String("{\"color\": \"gray\", \"translate\": \"painting.minecraft.skeleton.title\"}".into())),
                                ("author".into(), Tag::String("{\"color\": \"gray\", \"translate\": \"painting.minecraft.skeleton.title\"}".into())),
                            ].into()
                        })
                    },
                ]
            }))
            .await?;

        // Dimension types
        socket
            .send(&clientbound::ConfigurationPacket::RegistryData(
                RegistryData {
                    registry_id: "minecraft:dimension_type".into(),
                    entries: vec![RegistryEntry {
                        entry_id: "minecraft:overworld".into(),
                        data: Some(Nbt {
                            name: "".into(),
                            compound: vec![
                                ("fixed_time".into(), Tag::Long(12000)),
                                ("has_skylight".into(), Tag::Byte(1)),
                                ("has_ceiling".into(), Tag::Byte(0)),
                                ("ultrawarm".into(), Tag::Byte(0)),
                                ("natural".into(), Tag::Byte(1)),
                                ("coordinate_scale".into(), Tag::Double(1.0)),
                                ("bed_works".into(), Tag::Byte(1)),
                                ("respawn_anchor_works".into(), Tag::Byte(1)),
                                ("min_y".into(), Tag::Int(-64)),
                                ("height".into(), Tag::Int(256)),
                                ("logical_height".into(), Tag::Int(255)),
                                (
                                    "infiniburn".into(),
                                    Tag::String("#minecraft:infiniburn_overworld".into()),
                                ),
                                ("effects".into(), Tag::String("minecraft:overworld".into())),
                                ("ambient_light".into(), Tag::Float(1.0)),
                                ("piglin_safe".into(), Tag::Byte(1)),
                                ("has_raids".into(), Tag::Byte(0)),
                                ("monster_spawn_light_level".into(), Tag::Byte(0)),
                                ("monster_spawn_block_light_limit".into(), Tag::Int(0)),
                            ]
                            .into(),
                        }),
                    }],
                },
            ))
            .await?;

        // Damage types
        socket
            .send(&clientbound::ConfigurationPacket::RegistryData(
                RegistryData {
                    registry_id: "minecraft:damage_type".into(),
                    entries: DAMAGE_TYPES_ENTRY_IDS
                        .map(|entry_id| RegistryEntry {
                            entry_id: "minecraft:".to_string() + entry_id,
                            data: Some(Nbt {
                                name: "".into(),
                                compound: vec![
                                    ("message_id".into(), Tag::String("arrow".into())),
                                    ("scaling".into(), Tag::String("never".into())),
                                    ("exhaustion".into(), Tag::Float(0.0)),
                                ]
                                .into(),
                            }),
                        })
                        .into(),
                },
            ))
            .await?;

        socket
            .send(&clientbound::ConfigurationPacket::FinishConfiguration(
                FinishConfiguration {},
            ))
            .await?;

        Ok(Self {
            socket,
            game,
            identity: identity,
        })
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        loop {
            match self
                .socket
                .receive::<serverbound::ConfigurationPacket>()
                .await
            {
                Ok(packet) => match packet {
                    serverbound::ConfigurationPacket::ClientInformation(_) => {}
                    serverbound::ConfigurationPacket::KnownPacks(_) => {}
                    serverbound::ConfigurationPacket::FinishConfigurationAcknowledged(_) => {
                        println!("Configuration complete");
                    }
                },
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        return Err(e);
                    }
                    eprintln!("Failed to receive packet: {:?}", e);
                }
            }
        }
    }
}
