use std::sync::Arc;

use bevy_ecs::prelude::With;
use rand::Rng;

use crate::components::{
    CirclePig, CirclePigState, ClientId, EntityDimension, EntityIdCounter, EntityType, EntityUuid,
    Grounded, MinecraftEntityId, MovementConfig, MovementUpdateCooldown, PlayerName, PlayerReady,
    Position, PreviousPosition, Rotation, SpawnedEntity, TeleportState, Velocity, VerticalVelocity,
    Wander,
};
use crate::world::DimensionId;
use crate::network::{NetworkChannels, OutgoingPacket};
use voidmc_data::{Version, entity_type_id};

use super::parser::{
    DoubleArg, GameProfileArg, GreedyStringArg, IntegerArg, StringArg, SummonableEntityArg,
};
use super::{Command, CommandBuilder, CommandContext, CommandRegistry};

/// Registers all default commands except those listed in `exclude`.
pub fn register_default_commands(registry: &mut CommandRegistry, exclude: &[&str]) {
    if !exclude.contains(&"help") {
        registry.register(help_command());
    }
    if !exclude.contains(&"gamemode") {
        registry.register(gamemode_command());
    }
    if !exclude.contains(&"kick") {
        registry.register(kick_command());
    }
    if !exclude.contains(&"ping") {
        registry.register(ping_command());
    }
    if !exclude.contains(&"plugins") {
        registry.register(plugins_command());
    }
    if !exclude.contains(&"tp") {
        registry.register(tp_command());
    }
    if !exclude.contains(&"broadcast") {
        registry.register(broadcast_command());
    }
    if !exclude.contains(&"tell") {
        registry.register(tell_command());
    }
    if !exclude.contains(&"list") {
        registry.register(list_command());
    }
    if !exclude.contains(&"say") {
        registry.register(say_command());
    }
    if !exclude.contains(&"summon") {
        registry.register(summon_command());
    }
    if !exclude.contains(&"circle") {
        registry.register(circle_command());
    }
}

pub fn help_command() -> Command {
    CommandBuilder::new("help")
        .description("List available commands")
        .arg_optional("command", StringArg::single_word())
        .handler(handle_help)
        .build()
}

pub fn gamemode_command() -> Command {
    CommandBuilder::new("gamemode")
        .description("Change game mode")
        .alias("gm")
        .arg("mode", IntegerArg::new(0, 3))
        .handler(handle_gamemode)
        .build()
}

pub fn kick_command() -> Command {
    CommandBuilder::new("kick")
        .description("Kick a player")
        .arg("player", Arc::new(GameProfileArg))
        .arg_variadic("reason", Arc::new(GreedyStringArg))
        .handler(handle_kick)
        .build()
}

pub fn ping_command() -> Command {
    CommandBuilder::new("ping")
        .description("Pong!")
        .handler(handle_ping)
        .build()
}

pub fn plugins_command() -> Command {
    CommandBuilder::new("plugins")
        .description("List plugins")
        .alias("pl")
        .handler(handle_plugins)
        .build()
}

fn handle_help(ctx: &mut CommandContext) {
    // Check if a specific command was requested
    if let Some(cmd_name) = ctx.get::<String>("command").cloned() {
        let resolved = ctx.with_world(|world| {
            let registry = world.resource::<CommandRegistry>();
            registry.resolve(&cmd_name).map(|canonical_name| {
                let canonical = canonical_name.to_string();
                let desc = registry.description(&canonical).unwrap_or("").to_string();
                let usage = registry
                    .usage(&canonical)
                    .unwrap_or_else(|| format!("/{}", canonical));
                (canonical, desc, usage)
            })
        });

        let Some((canonical, desc, usage)) = resolved else {
            ctx.reply_error(&format!("Unknown command: /{}", cmd_name));
            return;
        };

        let mut lines = vec![format!("--- /{} ---", canonical)];
        if !desc.is_empty() {
            lines.push(format!("  {}", desc));
        }
        lines.push(format!("  Usage: {}", usage));
        ctx.reply(&lines.join("\n"));
        return;
    }

    // List all commands
    let mut entries: Vec<(String, String)> = ctx.with_world(|world| {
        let registry = world.resource::<CommandRegistry>();
        registry
            .command_names()
            .iter()
            .map(|name| {
                let desc = registry.description(name).unwrap_or("").to_string();
                (name.to_string(), desc)
            })
            .collect()
    });
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut lines = vec!["--- Available Commands ---".to_string()];
    for (name, desc) in &entries {
        if desc.is_empty() {
            lines.push(format!("  /{}", name));
        } else {
            lines.push(format!("  /{} - {}", name, desc));
        }
    }

    ctx.reply(&lines.join("\n"));
}

fn handle_gamemode(ctx: &mut CommandContext) {
    let mode = *ctx.get::<i32>("mode").unwrap();

    let mode_name = match mode {
        0 => "Survival",
        1 => "Creative",
        2 => "Adventure",
        3 => "Spectator",
        _ => "Unknown",
    };

    // Send GameEvent to change gamemode
    ctx.with_world(|world| {
        let channels = world.resource::<NetworkChannels>();
        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: ctx.client_id,
            packet: voidmc_protocol::clientbound::ClientboundPacket::Play(
                voidmc_protocol::clientbound::PlayPacket::GameEvent(
                    voidmc_protocol::clientbound::GameEvent {
                        event: voidmc_protocol::clientbound::GameEventType::ChangeGameMode,
                        value: mode as f32,
                    },
                ),
            ),
        });
    });

    ctx.reply(&format!("Game mode set to {} ({})", mode_name, mode));
}

fn handle_kick(ctx: &mut CommandContext) {
    let target_name = ctx.get::<String>("player").unwrap().clone();
    let reason = ctx
        .get::<String>("reason")
        .cloned()
        .unwrap_or_else(|| "Kicked by an operator".to_string());

    // Find the target player
    let target: Option<u32> = ctx.with_world_mut(|world| {
        let mut query = world.query_filtered::<(&ClientId, &PlayerName), With<PlayerReady>>();
        query
            .iter(world)
            .find(|(_, name)| name.0.eq_ignore_ascii_case(&target_name))
            .map(|(cid, _)| cid.0)
    });

    match target {
        Some(target_cid) => {
            // Send Disconnect packet
            let reason_nbt = crate::commands::text_to_nbt(&reason, "red");
            ctx.with_world(|world| {
                let channels = world.resource::<NetworkChannels>();
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: target_cid,
                    packet: voidmc_protocol::clientbound::ClientboundPacket::Play(
                        voidmc_protocol::clientbound::PlayPacket::Disconnect(
                            voidmc_protocol::clientbound::Disconnect { reason: reason_nbt },
                        ),
                    ),
                });
            });

            ctx.reply(&format!("Kicked {} (reason: {})", target_name, reason));
        }
        None => {
            ctx.reply_error(&format!("Player '{}' not found", target_name));
        }
    }
}

fn handle_ping(ctx: &mut CommandContext) {
    ctx.reply("Pong!");
}

fn handle_plugins(ctx: &mut CommandContext) {
    if let Some(plugin_list) = ctx.with_world(|world| world.get_resource::<PluginList>().cloned()) {
        if plugin_list.0.is_empty() {
            ctx.reply("No plugins loaded.");
        } else {
            let list = plugin_list.0.join(", ");
            ctx.reply(&format!("Plugins ({}): {}", plugin_list.0.len(), list));
        }
    } else {
        ctx.reply("No plugin list available.");
    }
}

pub fn tp_command() -> Command {
    CommandBuilder::new("tp")
        .description("Teleport to coordinates")
        .arg("x", DoubleArg::unbounded())
        .arg("y", DoubleArg::unbounded())
        .arg("z", DoubleArg::unbounded())
        .handler(handle_tp)
        .build()
}

pub fn broadcast_command() -> Command {
    CommandBuilder::new("broadcast")
        .description("Broadcast a message to all players")
        .arg_variadic_required("message", Arc::new(GreedyStringArg))
        .handler(handle_broadcast)
        .build()
}

pub fn tell_command() -> Command {
    CommandBuilder::new("tell")
        .description("Send a private message to a player")
        .alias("msg")
        .arg("player", Arc::new(GameProfileArg))
        .arg_variadic_required("message", Arc::new(GreedyStringArg))
        .handler(handle_tell)
        .build()
}

fn handle_tell(ctx: &mut CommandContext) {
    let target_name = ctx.get::<String>("player").unwrap().clone();
    let message = ctx.get::<String>("message").unwrap().clone();
    let sender_name = ctx.player_name().unwrap_or_else(|| "Server".to_string());

    // Find the target player
    let target: Option<u32> = ctx.with_world_mut(|world| {
        let mut query = world.query_filtered::<(&ClientId, &PlayerName), With<PlayerReady>>();
        query
            .iter(world)
            .find(|(_, name)| name.0.eq_ignore_ascii_case(&target_name))
            .map(|(cid, _)| cid.0)
    });

    match target {
        Some(target_cid) => {
            ctx.with_world(|world| {
                super::send_system_chat(
                    world,
                    target_cid,
                    &format!("{} whispers to you: {}", sender_name, message),
                    "gray",
                );
            });
            ctx.reply(&format!("You whisper to {}: {}", target_name, message));
        }
        None => {
            ctx.reply_error(&format!("Player '{}' not found", target_name));
        }
    }
}

fn handle_broadcast(ctx: &mut CommandContext) {
    let message = ctx.get::<String>("message").unwrap().clone();
    ctx.broadcast(&format!("[Broadcast] {}", message));
}

pub fn list_command() -> Command {
    CommandBuilder::new("list")
        .description("List online players")
        .handler(handle_list)
        .build()
}

fn handle_list(ctx: &mut CommandContext) {
    let names: Vec<String> = ctx.with_world_mut(|world| {
        let mut query = world.query_filtered::<&PlayerName, With<PlayerReady>>();
        query.iter(world).map(|n| n.0.clone()).collect()
    });

    if names.is_empty() {
        ctx.reply("There are 0 player(s) online.");
    } else {
        let list = names.join(", ");
        ctx.reply(&format!(
            "There are {} player(s) online: {}",
            names.len(),
            list
        ));
    }
}

fn handle_tp(ctx: &mut CommandContext) {
    let x = *ctx.get::<f64>("x").unwrap();
    let y = *ctx.get::<f64>("y").unwrap();
    let z = *ctx.get::<f64>("z").unwrap();
    let entity = ctx.entity;

    // Read current rotation to preserve yaw/pitch
    let (yaw, pitch) = ctx.with_world(|world| {
        let rot = world.get::<Rotation>(entity);
        match rot {
            Some(r) => (r.yaw, r.pitch),
            None => (0.0, 0.0),
        }
    });

    // Update TeleportState: assign teleport_id and mark as pending
    let teleport_id = ctx.with_world_mut(|world| {
        let mut tp_state = world.get_mut::<TeleportState>(entity).unwrap();
        let id = tp_state.next_id;
        tp_state.pending_id = Some(id);
        tp_state.next_id += 1;
        id
    });

    // Update Position component
    ctx.with_world_mut(|world| {
        let mut pos = world.get_mut::<Position>(entity).unwrap();
        pos.x = x;
        pos.y = y;
        pos.z = z;
    });

    // Send SynchronizePlayerPosition packet
    ctx.with_world(|world| {
        let channels = world.resource::<NetworkChannels>();
        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: ctx.client_id,
            packet: voidmc_protocol::clientbound::ClientboundPacket::Play(
                voidmc_protocol::clientbound::PlayPacket::SynchronizePlayerPosition(
                    voidmc_protocol::clientbound::SynchronizePlayerPosition {
                        teleport_id,
                        x,
                        y,
                        z,
                        vx: 0.0,
                        vy: 0.0,
                        vz: 0.0,
                        yaw,
                        pitch,
                        flags: voidmc_protocol::clientbound::TeleportFlags::empty(),
                    },
                ),
            ),
        });
    });

    ctx.reply(&format!("Teleported to {:.1}, {:.1}, {:.1}", x, y, z));
}

pub fn say_command() -> Command {
    CommandBuilder::new("say")
        .description("Send a message as yourself")
        .arg_variadic_required("message", Arc::new(GreedyStringArg))
        .handler(handle_say)
        .build()
}

fn handle_say(ctx: &mut CommandContext) {
    let name = ctx.player_name().unwrap_or_else(|| "Server".to_string());
    let message = ctx.get::<String>("message").unwrap().clone();
    ctx.broadcast(&format!("[{}] {}", name, message));
}

pub fn summon_command() -> Command {
    CommandBuilder::new("summon")
        .description("Summon an entity at a position")
        .arg("entity", Arc::new(SummonableEntityArg))
        .arg_optional("x", DoubleArg::unbounded())
        .arg_optional("y", DoubleArg::unbounded())
        .arg_optional("z", DoubleArg::unbounded())
        .flag("wander", Some('w'), "Attach the demo random-walk behavior")
        .flag("gravity", Some('g'), "Enable gravity for the summoned entity")
        .flag("block-checks", Some('b'), "Enable block-collision checks for the summoned entity")
        .handler(handle_summon)
        .build()
}

fn handle_summon(ctx: &mut CommandContext) {
    let entity_name = ctx.get::<String>("entity").unwrap().clone();

    let entity_type_id = match entity_type_id(Version::V26_1_2, &entity_name) {
        Some(id) => id,
        None => {
            ctx.reply_error(&format!("Unknown entity type: {}", entity_name));
            return;
        }
    };

    let executor = ctx.entity;
    let (x, y, z) = match (
        ctx.get::<f64>("x").copied(),
        ctx.get::<f64>("y").copied(),
        ctx.get::<f64>("z").copied(),
    ) {
        (Some(x), Some(y), Some(z)) => (x, y, z),
        _ => ctx.with_world(|world| {
            let pos = world
                .get::<Position>(executor)
                .expect("executor must have Position");
            (pos.x, pos.y, pos.z)
        }),
    };

    let entity_id = ctx.with_world_mut(|world| {
        let mut counter = world.resource_mut::<EntityIdCounter>();
        let id = counter.0;
        counter.0 += 1;
        id
    });
    let entity_uuid = uuid::Uuid::new_v4();
    let entity_dimension = ctx.with_world(|world| {
        world
            .get::<crate::components::PlayerDimension>(executor)
            .map(|dimension| dimension.0)
            .unwrap_or(DimensionId::Overworld)
    });

    let movement_config = MovementConfig {
        wander: ctx.flag("wander"),
        gravity_enabled: ctx.flag("gravity"),
        block_collision_enabled: ctx.flag("block-checks"),
    };

    let initial_velocity_y = if movement_config.gravity_enabled { -0.08 } else { 0.0 };

    ctx.with_world_mut(|world| {
        let mut e = world.spawn((
            MinecraftEntityId(entity_id),
            EntityUuid(entity_uuid),
            Position { x, y, z },
            PreviousPosition { x, y, z },
            Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            Velocity { x: 0, y: 0, z: 0 },
            EntityType(entity_type_id),
            EntityDimension(entity_dimension),
            SpawnedEntity,
            movement_config,
            VerticalVelocity(0.0),
            Grounded(false),
            MovementUpdateCooldown(0),
            crate::components::RecentlySpawned(15),
        ));

        // Attach simple Wander AI only when explicitly requested.
        if movement_config.wander {
            let mut rng = rand::thread_rng();
            let yaw = rng.gen_range(0.0..360.0) as f32;
            let ticks = rng.gen_range(40..140);
            e.insert(Wander { ticks, speed: 0.08, yaw });
        }
    });

    let spawn_packet = voidmc_protocol::clientbound::ClientboundPacket::Play(
        voidmc_protocol::clientbound::PlayPacket::SpawnEntity(
            voidmc_protocol::clientbound::SpawnEntity {
                entity_id,
                entity_uuid,
                entity_type: entity_type_id,
                x,
                y,
                z,
                pitch: 0,
                yaw: 0,
                head_yaw: 0,
                data: 0,
                velocity: voidmc_codec::LpVec3 {
                    x: 0.0,
                    y: initial_velocity_y,
                    z: 0.0,
                },
            },
        ),
    );

    ctx.with_world_mut(|world| {
        let ready_client_ids: Vec<u32> = world
            .query_filtered::<&ClientId, With<PlayerReady>>()
            .iter(world)
            .map(|c| c.0)
            .collect();

        let channels = world.resource::<NetworkChannels>();
        for cid in ready_client_ids {
            let _ = channels.outgoing.send(OutgoingPacket {
                client_id: cid,
                packet: spawn_packet.clone(),
            });
        }
    });

    ctx.reply(&format!(
        "Summoned {} at {:.1}, {:.1}, {:.1}",
        entity_name, x, y, z
    ));
}

pub fn circle_command() -> Command {
    CommandBuilder::new("circle")
        .description("Spawn a ring of 36 entities around you or a player")
        .arg_optional("entity", Arc::new(SummonableEntityArg))
        .arg_optional("player", Arc::new(GameProfileArg))
        .flag("stop", Some('s'), "Remove your active circle")
        .handler(handle_circle)
        .build()
}

fn dismiss_circle(ctx: &mut CommandContext, executor: bevy_ecs::prelude::Entity) -> bool {
    let existing: Vec<(bevy_ecs::prelude::Entity, i32)> = ctx.with_world_mut(|world| {
        world
            .query_filtered::<(bevy_ecs::prelude::Entity, &MinecraftEntityId, &CirclePigState), With<CirclePig>>()
            .iter(world)
            .filter_map(|(e, mc_id, state)| {
                if state.owner == executor { Some((e, mc_id.0)) } else { None }
            })
            .collect()
    });

    if existing.is_empty() {
        return false;
    }

    let mc_ids: Vec<i32> = existing.iter().map(|(_, id)| *id).collect();
    ctx.with_world_mut(|world| {
        for (entity, _) in &existing {
            world.despawn(*entity);
        }

        let ready_cids: Vec<u32> = world
            .query_filtered::<&ClientId, With<PlayerReady>>()
            .iter(world)
            .map(|c| c.0)
            .collect();

        let channels = world.resource::<NetworkChannels>();
        let packet = voidmc_protocol::clientbound::ClientboundPacket::ManualPlay(
            voidmc_protocol::clientbound::ManualPlayPacket::RemoveEntities(
                voidmc_protocol::clientbound::RemoveEntities { entity_ids: mc_ids },
            ),
        );
        for cid in ready_cids {
            let _ = channels.outgoing.send(OutgoingPacket { client_id: cid, packet: packet.clone() });
        }
    });
    true
}

fn handle_circle(ctx: &mut CommandContext) {
    let executor = ctx.entity;

    // --stop: remove any active circle and exit.
    if ctx.flag("stop") {
        if dismiss_circle(ctx, executor) {
            ctx.reply("Your circle has dispersed.");
        } else {
            ctx.reply("You have no active circle.");
        }
        return;
    }

    // Which entity type to spawn (defaults to pig).
    let entity_name = ctx.get::<String>("entity").cloned()
        .unwrap_or_else(|| "minecraft:pig".to_string());

    let pig_type_id = match entity_type_id(Version::V26_1_2, &entity_name) {
        Some(id) => id,
        None => {
            ctx.reply_error(&format!("Unknown entity type '{}'.", entity_name));
            return;
        }
    };

    // Which player to orbit (defaults to self).
    let target: bevy_ecs::prelude::Entity = if let Some(player_name) = ctx.get::<String>("player").cloned() {
        let found = ctx.with_world_mut(|world| {
            world
                .query_filtered::<(bevy_ecs::prelude::Entity, &PlayerName), With<PlayerReady>>()
                .iter(world)
                .find_map(|(e, name)| if name.0 == player_name { Some(e) } else { None })
        });
        match found {
            Some(e) => e,
            None => {
                ctx.reply_error(&format!("Player '{}' is not online.", player_name));
                return;
            }
        }
    } else {
        executor
    };

    // Remove any existing circle this player owns, then spawn a fresh one.
    dismiss_circle(ctx, executor);

    let (target_pos, player_dimension) = ctx.with_world_mut(|world| {
        let pos = world.get::<Position>(target).expect("target has Position").clone();
        let dim = world
            .get::<crate::components::PlayerDimension>(executor)
            .map(|d| d.0)
            .unwrap_or(DimensionId::Overworld);
        (pos, dim)
    });

    const RADIUS: f64 = 2.0;
    let mut spawned: Vec<(i32, uuid::Uuid, f64, f64, f64)> = Vec::with_capacity(36);

    for i in 0..36u32 {
        let angle_deg = (i * 10) as f32;
        let angle_rad = angle_deg.to_radians() as f64;
        let x = target_pos.x + angle_rad.sin() * RADIUS;
        let y = target_pos.y;
        let z = target_pos.z + angle_rad.cos() * RADIUS;

        let entity_id = ctx.with_world_mut(|world| {
            let mut counter = world.resource_mut::<EntityIdCounter>();
            let id = counter.0;
            counter.0 += 1;
            id
        });
        let entity_uuid = uuid::Uuid::new_v4();

        ctx.with_world_mut(|world| {
            world.spawn((
                (
                    MinecraftEntityId(entity_id),
                    EntityUuid(entity_uuid),
                    Position { x, y, z },
                    PreviousPosition { x, y, z },
                    Rotation { yaw: 0.0, pitch: 0.0 },
                    Velocity { x: 0, y: 0, z: 0 },
                    EntityType(pig_type_id),
                    EntityDimension(player_dimension),
                    SpawnedEntity,
                    MovementConfig::default(),
                    VerticalVelocity(0.0),
                    Grounded(true),
                    MovementUpdateCooldown(0),
                    crate::components::RecentlySpawned(5),
                ),
                (CirclePig, CirclePigState { angle: angle_deg, owner: executor, target }),
            ));
        });

        spawned.push((entity_id, entity_uuid, x, y, z));
    }

    // Broadcast SpawnEntity to all ready players.
    ctx.with_world_mut(|world| {
        let ready_cids: Vec<u32> = world
            .query_filtered::<&ClientId, With<PlayerReady>>()
            .iter(world)
            .map(|c| c.0)
            .collect();

        let channels = world.resource::<NetworkChannels>();

        for (entity_id, entity_uuid, x, y, z) in &spawned {
            let packet = voidmc_protocol::clientbound::ClientboundPacket::Play(
                voidmc_protocol::clientbound::PlayPacket::SpawnEntity(
                    voidmc_protocol::clientbound::SpawnEntity {
                        entity_id: *entity_id,
                        entity_uuid: *entity_uuid,
                        entity_type: pig_type_id,
                        x: *x,
                        y: *y,
                        z: *z,
                        pitch: 0,
                        yaw: 0,
                        head_yaw: 0,
                        data: 0,
                        velocity: voidmc_codec::LpVec3 { x: 0.0, y: 0.0, z: 0.0 },
                    },
                ),
            );
            for &cid in &ready_cids {
                let _ = channels.outgoing.send(OutgoingPacket { client_id: cid, packet: packet.clone() });
            }
        }
    });

    let label = if target == executor {
        "you".to_string()
    } else {
        ctx.get::<String>("target").cloned().unwrap_or_else(|| "the target".to_string())
    };
    ctx.reply(&format!("36 pigs now orbit around {}. Use /circle --stop to dismiss them.", label));
}

/// Optional resource listing plugin names — can be inserted by the user.
#[derive(Clone, bevy_ecs::prelude::Resource)]
pub struct PluginList(pub Vec<String>);
