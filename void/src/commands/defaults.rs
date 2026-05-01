use std::sync::Arc;

use bevy_ecs::prelude::With;

use crate::components::{ClientId, PlayerName, PlayerReady, Position, Rotation, TeleportState};
use crate::network::{NetworkChannels, OutgoingPacket};

use super::parser::{DoubleArg, GameProfileArg, GreedyStringArg, IntegerArg, StringArg};
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
            packet: void_protocol::clientbound::ClientboundPacket::Play(
                void_protocol::clientbound::PlayPacket::GameEvent(
                    void_protocol::clientbound::GameEvent {
                        event: void_protocol::clientbound::GameEventType::ChangeGameMode,
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
                    packet: void_protocol::clientbound::ClientboundPacket::Play(
                        void_protocol::clientbound::PlayPacket::Disconnect(
                            void_protocol::clientbound::Disconnect { reason: reason_nbt },
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
            packet: void_protocol::clientbound::ClientboundPacket::Play(
                void_protocol::clientbound::PlayPacket::SynchronizePlayerPosition(
                    void_protocol::clientbound::SynchronizePlayerPosition {
                        teleport_id,
                        x,
                        y,
                        z,
                        vx: 0.0,
                        vy: 0.0,
                        vz: 0.0,
                        yaw,
                        pitch,
                        flags: void_protocol::clientbound::TeleportFlags::empty(),
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

/// Optional resource listing plugin names — can be inserted by the user.
#[derive(Clone, bevy_ecs::prelude::Resource)]
pub struct PluginList(pub Vec<String>);
