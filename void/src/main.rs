use std::time::Duration;

use bevy_app::{App, ScheduleRunnerPlugin, TaskPoolPlugin};
use tracing_subscriber::prelude::*;
use ussr_nbt::owned::{Compound, Nbt, Tag};
use void::{
    Server,
    components::EntityIdCounter,
    handlers::{HandlerPlugin, RegistryDataStore},
    network::{IncomingPacket, NetworkPlugin, OutgoingPacket},
    systems::GameSystemsPlugin,
};
use void_protocol::clientbound::{RegistryData, RegistryEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging()?;

    let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
    let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();

    // Start the server in a separate thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            let mut server = Server::new("127.0.0.1:25565")
                .await
                .expect("Failed to start server");
            server.run(incoming_tx, outgoing_rx).await;
        })
    });

    App::new()
        .add_plugins((
            TaskPoolPlugin::default(),
            ScheduleRunnerPlugin::run_loop(Duration::from_millis(1000 / 20)),
        ))
        .add_plugins(NetworkPlugin::new(incoming_rx, outgoing_tx))
        .add_plugins(HandlerPlugin)
        .add_plugins(GameSystemsPlugin)
        .insert_resource(EntityIdCounter(1))
        .insert_resource(RegistryDataStore {
            registries: build_registry_data(),
        })
        .run();

    Ok(())
}

fn nbt(tags: Vec<(&str, Tag)>) -> Nbt {
    Nbt {
        name: Default::default(),
        compound: compound(tags),
    }
}

fn compound(tags: Vec<(&str, Tag)>) -> Compound {
    Compound {
        tags: tags
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect(),
    }
}

fn build_registry_data() -> Vec<RegistryData> {
    vec![
        // Minimal dimension type registry
        RegistryData {
            registry_id: "minecraft:dimension_type".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:overworld".to_string(),
                data: Some(nbt(vec![
                    ("has_skylight", Tag::Byte(1)),
                    ("has_ceiling", Tag::Byte(0)),
                    ("ultrawarm", Tag::Byte(0)),
                    ("natural", Tag::Byte(1)),
                    ("coordinate_scale", Tag::Double(1.0)),
                    ("bed_works", Tag::Byte(1)),
                    ("respawn_anchor_works", Tag::Byte(0)),
                    ("min_y", Tag::Int(-64)),
                    ("height", Tag::Int(384)),
                    ("logical_height", Tag::Int(384)),
                    ("infiniburn", Tag::String("#minecraft:infiniburn_overworld".into())),
                    ("effects", Tag::String("minecraft:overworld".into())),
                    ("ambient_light", Tag::Float(0.0)),
                    ("piglin_safe", Tag::Byte(0)),
                    ("has_raids", Tag::Byte(1)),
                    ("monster_spawn_light_level", Tag::Int(0)),
                    ("monster_spawn_block_light_limit", Tag::Int(0)),
                ])),
            }],
        },
        // Minimal biome registry
        RegistryData {
            registry_id: "minecraft:worldgen/biome".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:plains".to_string(),
                data: Some(nbt(vec![
                    ("has_precipitation", Tag::Byte(1)),
                    ("temperature", Tag::Float(0.8)),
                    ("downfall", Tag::Float(0.4)),
                    ("effects", Tag::Compound(compound(vec![
                        ("sky_color", Tag::Int(7907327)),
                        ("water_fog_color", Tag::Int(329011)),
                        ("fog_color", Tag::Int(12638463)),
                        ("water_color", Tag::Int(4159204)),
                    ]))),
                ])),
            }],
        },
        // Painting variant registry (required)
        RegistryData {
            registry_id: "minecraft:painting_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:kebab".to_string(),
                data: Some(nbt(vec![
                    ("asset_id", Tag::String("minecraft:kebab".into())),
                    ("width", Tag::Int(1)),
                    ("height", Tag::Int(1)),
                ])),
            }],
        },
        // Damage type registry (required — all 25 mandatory types)
        RegistryData {
            registry_id: "minecraft:damage_type".to_string(),
            entries: [
                ("minecraft:generic", "generic", "never"),
                ("minecraft:generic_kill", "genericKill", "never"),
                ("minecraft:fall", "fall", "when_caused_by_living_non_player"),
                ("minecraft:in_fire", "inFire", "never"),
                ("minecraft:on_fire", "onFire", "never"),
                ("minecraft:lava", "lava", "never"),
                ("minecraft:hot_floor", "hotFloor", "never"),
                ("minecraft:in_wall", "inWall", "never"),
                ("minecraft:cramming", "cramming", "never"),
                ("minecraft:drown", "drown", "never"),
                ("minecraft:starve", "starve", "never"),
                ("minecraft:cactus", "cactus", "never"),
                ("minecraft:sweet_berry_bush", "sweetBerryBush", "never"),
                ("minecraft:freeze", "freeze", "never"),
                ("minecraft:fly_into_wall", "flyIntoWall", "never"),
                ("minecraft:out_of_world", "outOfWorld", "never"),
                ("minecraft:magic", "magic", "never"),
                ("minecraft:wither", "wither", "never"),
                ("minecraft:dragon_breath", "dragonBreath", "never"),
                ("minecraft:dry_out", "dryout", "never"),
                ("minecraft:campfire", "inFire", "never"),
                ("minecraft:lightning_bolt", "lightningBolt", "never"),
                ("minecraft:stalagmite", "stalagmite", "never"),
                ("minecraft:outside_border", "outsideBorder", "never"),
                ("minecraft:ender_pearl", "fall", "never"),
            ]
            .into_iter()
            .map(|(id, msg, scaling)| RegistryEntry {
                entry_id: id.to_string(),
                data: Some(nbt(vec![
                    ("message_id", Tag::String(msg.into())),
                    ("scaling", Tag::String(scaling.into())),
                    ("exhaustion", Tag::Float(0.0)),
                ])),
            })
            .collect(),
        },
        // Wolf variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:wolf_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:pale".to_string(),
                data: Some(nbt(vec![
                    ("wild_texture", Tag::String("minecraft:textures/entity/wolf/wolf.png".into())),
                    ("tame_texture", Tag::String("minecraft:textures/entity/wolf/wolf_tame.png".into())),
                    ("angry_texture", Tag::String("minecraft:textures/entity/wolf/wolf_angry.png".into())),
                    ("biomes", Tag::String("minecraft:plains".into())),
                ])),
            }],
        },
        // Wolf sound variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:wolf_sound_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:classic".to_string(),
                data: Some(nbt(vec![
                    ("ambient_sound", Tag::String("minecraft:entity.wolf.ambient".into())),
                    ("death_sound", Tag::String("minecraft:entity.wolf.death".into())),
                    ("growl_sound", Tag::String("minecraft:entity.wolf.growl".into())),
                    ("hurt_sound", Tag::String("minecraft:entity.wolf.hurt".into())),
                    ("pant_sound", Tag::String("minecraft:entity.wolf.pant".into())),
                    ("whine_sound", Tag::String("minecraft:entity.wolf.whine".into())),
                ])),
            }],
        },
        // Cat variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:cat_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:tabby".to_string(),
                data: Some(nbt(vec![
                    ("texture", Tag::String("minecraft:textures/entity/cat/tabby.png".into())),
                ])),
            }],
        },
        // Chicken variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:chicken_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:temperate".to_string(),
                data: Some(nbt(vec![
                    ("texture", Tag::String("minecraft:textures/entity/chicken/temperate.png".into())),
                ])),
            }],
        },
        // Cow variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:cow_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:temperate".to_string(),
                data: Some(nbt(vec![
                    ("texture", Tag::String("minecraft:textures/entity/cow/temperate.png".into())),
                ])),
            }],
        },
        // Frog variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:frog_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:temperate".to_string(),
                data: Some(nbt(vec![
                    ("texture", Tag::String("minecraft:textures/entity/frog/temperate.png".into())),
                ])),
            }],
        },
        // Pig variant registry (required non-empty)
        RegistryData {
            registry_id: "minecraft:pig_variant".to_string(),
            entries: vec![RegistryEntry {
                entry_id: "minecraft:temperate".to_string(),
                data: Some(nbt(vec![
                    ("texture", Tag::String("minecraft:textures/entity/pig/temperate.png".into())),
                ])),
            }],
        },
    ]
}

fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    // Create logs directory if it doesn't exist
    std::fs::create_dir_all("logs")?;

    let file_appender = tracing_appender::rolling::daily("logs", "void.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_level(true)
        .with_file(true)
        .pretty()
        .with_writer(std::io::stderr);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_level(true)
        .with_file(true)
        .with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(())
}
