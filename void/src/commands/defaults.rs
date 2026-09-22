use std::sync::Arc;

use bevy_app::AppExit;
use bevy_ecs::prelude::{Entity, With};
use rand::Rng;

use crate::components::{
    PlayerDimension, PlayerName, PlayerReady, Position, Rotation, TeleportState, Wander,
};
use crate::entity::{EntityBuilder, EntityKind};
use crate::messages::TextColor;
use crate::plugins::tab_list::TabEntry;
use crate::world::DimensionId;
use voidmc_data::{Version, is_summonable_entity_type};

use super::parser::{
    GameMode, GameModeArg, GreedyStringArg, IntegerArg, ItemArg, PlayerArg, StringArg,
    SummonableEntityArg, Vec3Arg,
};
use super::{Command, CommandBuilder, CommandContext, CommandRegistry};
use crate::inventory::Inventory;
use crate::item::ItemId;
use crate::item::ItemStack;

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
    if !exclude.contains(&"give") {
        registry.register(give_command());
    }
    if !exclude.contains(&"clear") {
        registry.register(clear_command());
    }
    if !exclude.contains(&"stop") {
        registry.register(stop_command());
    }
}

pub fn give_command() -> Command {
    CommandBuilder::new("give")
        .description("Give yourself an item")
        .arg("item", Arc::new(ItemArg))
        .arg_optional("count", IntegerArg::new(1, 64))
        .handler(handle_give)
        .build()
}

fn handle_give(ctx: &mut CommandContext) {
    let item = *ctx.get::<ItemId>("item").unwrap();
    let item_name = item.name().unwrap_or("unknown").to_string();
    let count = ctx.get::<i32>("count").copied().unwrap_or(1).clamp(1, 64) as u8;
    let stack = ItemStack::new(item, count);

    let entity = ctx.entity;
    let leftover = ctx.with_world_mut(|world| {
        world
            .get_mut::<Inventory>(entity)
            .map(|mut inv| inv.give(stack))
            .map(|left| left.count)
            .unwrap_or(count)
    });

    if leftover == 0 {
        ctx.reply(&format!("Gave {count} x {item_name}"));
    } else {
        ctx.reply(&format!(
            "Gave {} x {item_name} ({leftover} didn't fit)",
            count - leftover
        ));
    }
}

pub fn clear_command() -> Command {
    CommandBuilder::new("clear")
        .description("Clear your inventory")
        .handler(handle_clear)
        .build()
}

fn handle_clear(ctx: &mut CommandContext) {
    let entity = ctx.entity;
    ctx.with_world_mut(|world| {
        if let Some(mut inv) = world.get_mut::<Inventory>(entity) {
            inv.clear();
        }
    });
    ctx.reply("Inventory cleared");
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
        .arg("mode", Arc::new(GameModeArg))
        .handler(handle_gamemode)
        .build()
}

pub fn kick_command() -> Command {
    CommandBuilder::new("kick")
        .description("Kick a player")
        .arg("player", Arc::new(PlayerArg))
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
    let mode = *ctx.get::<GameMode>("mode").unwrap();

    ctx.players().send(
        ctx.entity,
        voidmc_protocol::clientbound::GameEvent {
            event: voidmc_protocol::clientbound::GameEventType::ChangeGameMode,
            value: mode.id() as f32,
        },
    );
    let player = ctx.entity;
    ctx.with_world_mut(|world| {
        if let Some(mut entry) = world.get_mut::<TabEntry>(player) {
            entry.game_mode = Some(mode as u8);
        }
    });

    ctx.reply(&format!("Game mode set to {} ({})", mode.name(), mode.id()));
}

fn name_of(ctx: &CommandContext, player: Entity) -> String {
    ctx.with_world(|world| {
        world
            .get::<PlayerName>(player)
            .map(|name| name.0.clone())
            .unwrap_or_else(|| format!("{player:?}"))
    })
}

fn handle_kick(ctx: &mut CommandContext) {
    let target = *ctx.get::<Entity>("player").unwrap();
    let target_name = name_of(ctx, target);
    let reason = ctx
        .get::<String>("reason")
        .cloned()
        .unwrap_or_else(|| "Kicked by an operator".to_string());

    let reason_nbt = crate::messages::text_component(&reason, TextColor::Red);
    ctx.players().send(
        target,
        voidmc_protocol::clientbound::Disconnect { reason: reason_nbt },
    );

    ctx.reply(&format!("Kicked {} (reason: {})", target_name, reason));
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
        .arg("position", Arc::new(Vec3Arg))
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
        .arg("player", Arc::new(PlayerArg))
        .arg_variadic_required("message", Arc::new(GreedyStringArg))
        .handler(handle_tell)
        .build()
}

fn handle_tell(ctx: &mut CommandContext) {
    let target = *ctx.get::<Entity>("player").unwrap();
    let target_name = name_of(ctx, target);
    let message = ctx.get::<String>("message").unwrap().clone();
    let sender_name = ctx.player_name().unwrap_or_else(|| "Server".to_string());

    ctx.with_world(|world| {
        super::send_system_chat(
            world,
            target,
            &format!("{} whispers to you: {}", sender_name, message),
            TextColor::Gray,
        );
    });
    ctx.reply(&format!("You whisper to {}: {}", target_name, message));
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
    let [x, y, z] = *ctx.get::<[f64; 3]>("position").unwrap();
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
    ctx.players().send(
        ctx.entity,
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
    );

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
        .arg_optional("position", Arc::new(Vec3Arg))
        .flag("wander", Some('w'), "Attach the demo random-walk behavior")
        .flag(
            "gravity",
            Some('g'),
            "Enable gravity for the summoned entity",
        )
        .flag(
            "block-checks",
            Some('b'),
            "Enable block-collision checks for the summoned entity",
        )
        .handler(handle_summon)
        .build()
}

fn handle_summon(ctx: &mut CommandContext) {
    let entity_name = ctx.get::<String>("entity").unwrap().clone();

    let Some(kind) = EntityKind::from_name(&entity_name) else {
        ctx.reply_error(&format!("Unknown entity type: {}", entity_name));
        return;
    };

    if !is_summonable_entity_type(Version::V26_1_2, &entity_name) {
        ctx.reply_error(&format!("Entity type is not summonable: {}", entity_name));
        return;
    }

    let executor = ctx.entity;
    let position_arg = ctx.get::<[f64; 3]>("position").copied();
    let (x, y, z) = match position_arg {
        Some([x, y, z]) => (x, y, z),
        None => ctx.with_world(|world| {
            let pos = world
                .get::<Position>(executor)
                .expect("executor must have Position");
            (pos.x, pos.y, pos.z)
        }),
    };

    let dimension = ctx.with_world(|world| {
        world
            .get::<PlayerDimension>(executor)
            .map(|dimension| dimension.0)
            .unwrap_or(DimensionId::Overworld)
    });

    let wander = ctx.flag("wander");
    let builder = EntityBuilder::new(kind)
        .at(x, y, z)
        .in_dimension(dimension)
        .wander(wander)
        .gravity(ctx.flag("gravity"))
        .block_collision(ctx.flag("block-checks"));

    ctx.with_world_mut(|world| {
        let mut e = builder.spawn_in(world);
        if wander {
            let mut rng = rand::thread_rng();
            e.insert(Wander {
                ticks: rng.gen_range(40..140),
                speed: 0.08,
                yaw: rng.gen_range(0.0..360.0) as f32,
            });
        }
    });

    ctx.reply(&format!(
        "Summoned {} at {:.1}, {:.1}, {:.1}",
        entity_name, x, y, z
    ));
}

pub fn stop_command() -> Command {
    CommandBuilder::new("stop")
        .description("Gracefully stop the server")
        .handler(handle_stop)
        .build()
}

fn handle_stop(ctx: &mut CommandContext) {
    let who = ctx.player_name().unwrap_or_else(|| "console".to_string());
    tracing::info!("Server shutdown requested via /stop by {who}");
    ctx.broadcast("Server is shutting down...");
    ctx.reply("Stopping the server...");
    // Signal a graceful exit. The runner finishes the current tick (so plugins
    // such as world persistence can flush on `AppExit`) and then stops.
    ctx.with_world_mut(|world| {
        world.write_message(AppExit::Success);
    });
}

/// Optional resource listing plugin names — can be inserted by the user.
#[derive(Clone, bevy_ecs::prelude::Resource)]
pub struct PluginList(pub Vec<String>);

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::*;
    use flume::Receiver;

    use super::*;
    use crate::commands::dispatch_command;
    use crate::components::{
        ClientId, EntityCollider, EntityType, MovementConfig, PreviousPosition, SpawnedEntity,
    };
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    fn command_world() -> (World, Entity, Receiver<OutgoingPacket>) {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();

        let mut world = World::new();
        world.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        });

        let mut registry = CommandRegistry::new();
        registry.register(summon_command());
        registry.register(tp_command());
        registry.register(gamemode_command());
        registry.register(kick_command());
        registry.register(tell_command());
        world.insert_resource(registry);

        let player = world
            .spawn((
                ClientId(7),
                PlayerReady,
                PlayerName("Alice".into()),
                TeleportState {
                    next_id: 1,
                    pending_id: None,
                },
                Position {
                    x: 12.0,
                    y: 64.0,
                    z: -8.0,
                },
                Rotation {
                    yaw: 0.0,
                    pitch: 0.0,
                },
                PlayerDimension(DimensionId::Overworld),
            ))
            .id();

        (world, player, outgoing_rx)
    }

    fn spawned_entities(world: &mut World) -> Vec<(Position, PreviousPosition, EntityType)> {
        world
            .query_filtered::<(&Position, &PreviousPosition, &EntityType), With<SpawnedEntity>>()
            .iter(world)
            .map(|(pos, prev, entity_type)| {
                (
                    Position {
                        x: pos.x,
                        y: pos.y,
                        z: pos.z,
                    },
                    PreviousPosition {
                        x: prev.x,
                        y: prev.y,
                        z: prev.z,
                    },
                    EntityType(entity_type.0),
                )
            })
            .collect()
    }

    #[test]
    fn summon_without_coordinates_uses_executor_position() {
        let (mut world, player, _outgoing_rx) = command_world();

        dispatch_command(
            &mut world,
            7,
            player,
            "summon",
            vec!["minecraft:zombie".to_string()],
        );

        let entities = spawned_entities(&mut world);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].0.x, 12.0);
        assert_eq!(entities[0].0.y, 64.0);
        assert_eq!(entities[0].0.z, -8.0);
        assert_eq!(entities[0].1.x, 12.0);
        assert_eq!(entities[0].2.0, 150);
    }

    #[test]
    fn summon_with_all_coordinates_uses_provided_position() {
        let (mut world, player, _outgoing_rx) = command_world();

        dispatch_command(
            &mut world,
            7,
            player,
            "summon",
            vec![
                "minecraft:zombie".to_string(),
                "10".to_string(),
                "70".to_string(),
                "-3".to_string(),
            ],
        );

        let entities = spawned_entities(&mut world);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].0.x, 10.0);
        assert_eq!(entities[0].0.y, 70.0);
        assert_eq!(entities[0].0.z, -3.0);
    }

    #[test]
    fn summon_block_checks_enables_pig_collision_box() {
        let (mut world, player, _outgoing_rx) = command_world();

        dispatch_command(
            &mut world,
            7,
            player,
            "summon",
            vec![
                "minecraft:pig".to_string(),
                "--wander".to_string(),
                "--gravity".to_string(),
                "--block-checks".to_string(),
            ],
        );

        let mut query = world.query_filtered::<
            (&MovementConfig, &EntityCollider, Option<&Wander>),
            With<SpawnedEntity>,
        >();
        let (movement, collider, wander) = query.single(&world).expect("one summoned pig");

        assert!(movement.wander);
        assert!(movement.gravity_enabled);
        assert!(movement.block_collision_enabled);
        assert_eq!(*collider, EntityCollider::new(0.9, 0.9, 1.0));
        assert!(wander.is_some());
    }

    #[test]
    fn summon_with_relative_coordinates_uses_executor_position() {
        let (mut world, player, _outgoing_rx) = command_world();

        dispatch_command(
            &mut world,
            7,
            player,
            "summon",
            vec![
                "minecraft:zombie".to_string(),
                "~1".to_string(),
                "~".to_string(),
                "~-2".to_string(),
            ],
        );

        let entities = spawned_entities(&mut world);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].0.x, 13.0);
        assert_eq!(entities[0].0.y, 64.0);
        assert_eq!(entities[0].0.z, -10.0);
    }

    #[test]
    fn summon_rejects_partial_coordinates() {
        let (mut world, player, outgoing_rx) = command_world();

        dispatch_command(
            &mut world,
            7,
            player,
            "summon",
            vec!["minecraft:zombie".to_string(), "10".to_string()],
        );

        assert!(spawned_entities(&mut world).is_empty());
        assert_eq!(outgoing_rx.try_iter().count(), 2);
    }

    #[test]
    fn summon_rejects_non_summonable_entity_types() {
        let (mut world, player, outgoing_rx) = command_world();

        dispatch_command(
            &mut world,
            7,
            player,
            "summon",
            vec!["minecraft:player".to_string()],
        );

        assert!(spawned_entities(&mut world).is_empty());
        assert_eq!(outgoing_rx.try_iter().count(), 1);
    }

    fn args(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|token| token.to_string()).collect()
    }

    #[test]
    fn tp_resolves_relative_and_local_coordinates_against_executor() {
        let (mut world, player, outgoing_rx) = command_world();

        dispatch_command(&mut world, 7, player, "tp", args(&["~1", "~", "~-2"]));
        let position = *world.get::<Position>(player).unwrap();
        assert_eq!((position.x, position.y, position.z), (13.0, 64.0, -10.0));

        dispatch_command(&mut world, 7, player, "tp", args(&["^", "^", "^2"]));
        let position = *world.get::<Position>(player).unwrap();
        assert!((position.x - 13.0).abs() < 1e-4);
        assert!((position.y - 64.0).abs() < 1e-4);
        assert!((position.z + 8.0).abs() < 1e-4);

        let sync_packets = outgoing_rx
            .try_iter()
            .filter(|packet| {
                matches!(
                    packet.packet,
                    ClientboundPacket::Play(PlayPacket::SynchronizePlayerPosition(_))
                )
            })
            .count();
        assert_eq!(sync_packets, 2);
    }

    #[test]
    fn tp_rejects_partial_and_mixed_coordinates() {
        let (mut world, player, outgoing_rx) = command_world();

        dispatch_command(&mut world, 7, player, "tp", args(&["1", "2"]));
        dispatch_command(&mut world, 7, player, "tp", args(&["^1", "~", "3"]));

        let position = *world.get::<Position>(player).unwrap();
        assert_eq!((position.x, position.y, position.z), (12.0, 64.0, -8.0));
        let chat_packets = outgoing_rx
            .try_iter()
            .filter(|packet| {
                matches!(
                    packet.packet,
                    ClientboundPacket::Play(PlayPacket::SystemChat(_))
                )
            })
            .count();
        assert_eq!(chat_packets, 4);
    }

    #[test]
    fn gamemode_accepts_vanilla_names_and_legacy_ids() {
        let (mut world, player, outgoing_rx) = command_world();

        dispatch_command(&mut world, 7, player, "gamemode", args(&["spectator"]));
        dispatch_command(&mut world, 7, player, "gm", args(&["1"]));
        dispatch_command(&mut world, 7, player, "gamemode", args(&["hardcore"]));

        let values: Vec<f32> = outgoing_rx
            .try_iter()
            .filter_map(|packet| match packet.packet {
                ClientboundPacket::Play(PlayPacket::GameEvent(event)) => Some(event.value),
                _ => None,
            })
            .collect();
        assert_eq!(values, vec![3.0, 1.0]);
    }

    #[test]
    fn kick_and_tell_resolve_player_selectors() {
        let (mut world, player, outgoing_rx) = command_world();
        let bob = world
            .spawn((
                ClientId(8),
                PlayerReady,
                PlayerName("Bob".into()),
                Position {
                    x: 20.0,
                    y: 64.0,
                    z: -8.0,
                },
            ))
            .id();

        dispatch_command(&mut world, 7, player, "tell", args(&["bob", "hi"]));
        dispatch_command(&mut world, 7, player, "kick", args(&["Carol"]));
        dispatch_command(&mut world, 7, player, "kick", args(&["@a"]));

        let mut disconnects = 0;
        let mut chats_to_bob = 0;
        let mut chats_to_alice = 0;
        for packet in outgoing_rx.try_iter() {
            match (packet.client_id, &packet.packet) {
                (8, ClientboundPacket::Play(PlayPacket::SystemChat(_))) => chats_to_bob += 1,
                (7, ClientboundPacket::Play(PlayPacket::SystemChat(_))) => chats_to_alice += 1,
                (_, ClientboundPacket::Play(PlayPacket::Disconnect(_))) => disconnects += 1,
                _ => {}
            }
        }
        assert_eq!(chats_to_bob, 1);
        assert_eq!(chats_to_alice, 5);
        assert_eq!(disconnects, 0);

        dispatch_command(&mut world, 7, player, "kick", args(&["@p"]));
        let kicked: Vec<u32> = outgoing_rx
            .try_iter()
            .filter_map(|packet| match packet.packet {
                ClientboundPacket::Play(PlayPacket::Disconnect(_)) => Some(packet.client_id),
                _ => None,
            })
            .collect();
        assert_eq!(kicked, vec![7]);
        assert!(world.entities().contains(bob));
    }

    #[test]
    fn stop_command_requests_app_exit() {
        let (mut world, player, _outgoing_rx) = command_world();
        world.insert_resource(bevy_ecs::message::Messages::<AppExit>::default());
        world
            .resource_mut::<CommandRegistry>()
            .register(stop_command());

        dispatch_command(&mut world, 7, player, "stop", vec![]);

        let exits = world.resource::<bevy_ecs::message::Messages<AppExit>>();
        assert_eq!(exits.len(), 1, "/stop should write exactly one AppExit");
    }
}
