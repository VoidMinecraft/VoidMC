use std::sync::Arc;

use bevy_ecs::prelude::With;

use crate::components::{ClientId, PlayerName, PlayerReady};
use crate::network::{NetworkChannels, OutgoingPacket};

use super::parser::{GameProfileArg, GreedyStringArg, IntegerArg, StringArg};
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
        let (canonical, desc, usage) = {
            let registry = ctx.world.resource::<CommandRegistry>();
            match registry.resolve(&cmd_name).map(|s| s.to_string()) {
                Some(canonical) => {
                    let desc = registry
                        .description(&canonical)
                        .unwrap_or("")
                        .to_string();
                    let usage = registry
                        .usage(&canonical)
                        .unwrap_or_else(|| format!("/{}", canonical));
                    (canonical, desc, usage)
                }
                None => {
                    let msg = format!("Unknown command: /{}", cmd_name);
                    // registry borrow ends here at block boundary
                    return ctx.reply_error(&msg);
                }
            }
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
    let registry = ctx.world.resource::<CommandRegistry>();
    let mut entries: Vec<(String, String)> = registry
        .command_names()
        .iter()
        .map(|name| {
            let desc = registry.description(name).unwrap_or("").to_string();
            (name.to_string(), desc)
        })
        .collect();
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
    let channels = ctx.world.resource::<NetworkChannels>();
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

    ctx.reply(&format!("Game mode set to {} ({})", mode_name, mode));
}

fn handle_kick(ctx: &mut CommandContext) {
    let target_name = ctx.get::<String>("player").unwrap().clone();
    let reason = ctx
        .get::<String>("reason")
        .cloned()
        .unwrap_or_else(|| "Kicked by an operator".to_string());

    // Find the target player
    let target: Option<u32> = {
        let mut query = ctx
            .world
            .query_filtered::<(&ClientId, &PlayerName), With<PlayerReady>>();
        query
            .iter(ctx.world)
            .find(|(_, name)| name.0.eq_ignore_ascii_case(&target_name))
            .map(|(cid, _)| cid.0)
    };

    match target {
        Some(target_cid) => {
            // Send Disconnect packet
            let reason_nbt = crate::commands::text_to_nbt(&reason, "red");
            let channels = ctx.world.resource::<NetworkChannels>();
            let _ = channels.outgoing.send(OutgoingPacket {
                client_id: target_cid,
                packet: void_protocol::clientbound::ClientboundPacket::Play(
                    void_protocol::clientbound::PlayPacket::Disconnect(
                        void_protocol::clientbound::Disconnect {
                            reason: reason_nbt,
                        },
                    ),
                ),
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
    if let Some(plugin_list) = ctx.world.get_resource::<PluginList>() {
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

/// Optional resource listing plugin names — can be inserted by the user.
#[derive(bevy_ecs::prelude::Resource)]
pub struct PluginList(pub Vec<String>);
