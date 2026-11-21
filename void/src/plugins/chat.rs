use bevy_app::{App, Plugin};
use bevy_ecs::{
    entity::Entity,
    observer::On,
    system::{Commands, Query, ResMut},
    world::World,
};
use voidmc_protocol::{
    State,
    serverbound::{
        ChatCommand, ChatMessage, ChatMode, CommandSuggestionsRequest, SignedChatCommand,
    },
};

use crate::{
    CommandRegistry,
    commands::{CommandEnqueueSequence, CommandLines, CommandQueue, enqueue_command_line},
    components::{ClientSettings, ConnectionState, PlayerName},
    events::{ChatCommandEvent, ChatMessageEvent},
    messages::{Messages, TextColor},
    network::PacketEvent,
    players::Players,
};

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_chat_command);
        app.add_observer(handle_signed_chat_command);
        app.add_observer(handle_chat_message);
        app.add_observer(handle_command_suggestions);
    }
}

fn handle_command(
    client_id: u32,
    entity: Entity,
    command: &str,
    queue: &mut CommandQueue,
    sequence: &mut CommandEnqueueSequence,
    lines: &mut CommandLines,
    mut commands: Commands,
) {
    // A line with no command name (`/` alone, or only spaces) still goes
    // through the queue, so the player gets the same "Unknown command" reply
    // as for any other name, in order with their other commands.
    let (command_name, args) =
        enqueue_command_line(queue, sequence, lines, client_id, entity, command);

    commands.trigger(ChatCommandEvent {
        entity,
        client_id,
        command: command_name,
        args,
    });
}

fn handle_chat_command(
    event: On<PacketEvent<ChatCommand>>,
    queue: ResMut<CommandQueue>,
    sequence: ResMut<CommandEnqueueSequence>,
    lines: ResMut<CommandLines>,
    commands: Commands,
) {
    handle_command(
        event.client_id,
        event.entity,
        &event.packet.command,
        queue.into_inner(),
        sequence.into_inner(),
        lines.into_inner(),
        commands,
    );
}

fn handle_signed_chat_command(
    event: On<PacketEvent<SignedChatCommand>>,
    queue: ResMut<CommandQueue>,
    sequence: ResMut<CommandEnqueueSequence>,
    lines: ResMut<CommandLines>,
    commands: Commands,
) {
    handle_command(
        event.client_id,
        event.entity,
        &event.packet.command,
        queue.into_inner(),
        sequence.into_inner(),
        lines.into_inner(),
        commands,
    );
}

fn handle_chat_message(
    event: On<PacketEvent<ChatMessage>>,
    mut commands: Commands,
    queue: ResMut<CommandQueue>,
    sequence: ResMut<CommandEnqueueSequence>,
    lines: ResMut<CommandLines>,
    messages: Messages,
    players: Players,
    player_names: Query<&PlayerName>,
    recipients: Query<(Entity, &ConnectionState, Option<&ClientSettings>)>,
) {
    // If the client doesn't recognise a command in its tree, it sends
    // "/command args" as a ChatMessage instead of ChatCommand.  Intercept that.
    if let Some(cmd) = event.packet.message.strip_prefix('/') {
        handle_command(
            event.client_id,
            event.entity,
            cmd,
            queue.into_inner(),
            sequence.into_inner(),
            lines.into_inner(),
            commands,
        );
        return;
    }

    let player_name = player_names
        .get(event.entity)
        .map(|n| n.0.clone())
        .unwrap_or_else(|_| "Unknown".to_string());

    if let Some(reason) = refusal(&event.packet.message) {
        tracing::info!(
            player_name = %player_name,
            client_id = event.client_id,
            message = ?event.packet.message,
            reason,
            "Chat message refused"
        );
        messages
            .message(event.entity, reason)
            .color(TextColor::Red)
            .send();
        return;
    }

    tracing::info!(
        player_name = %player_name,
        client_id = event.client_id,
        message = %event.packet.message,
        "Chat message"
    );

    // Like vanilla: every connection in the play state, loaded or not, whose
    // chat visibility is "Shown". A client that never sent its settings is
    // treated as "Shown", the vanilla default.
    let line = messages
        .message(
            event.entity,
            format!("<{}> {}", player_name, event.packet.message),
        )
        .packet();
    players.send_to(
        recipients
            .iter()
            .filter(|(_, state, settings)| {
                state.0 == State::Play && settings.is_none_or(accepts_player_chat)
            })
            .map(|(entity, ..)| entity),
        line,
    );

    commands.trigger(ChatMessageEvent {
        entity: event.entity,
        client_id: event.client_id,
        message: event.packet.message.to_string(),
    });
}

/// Vanilla's `ServerPlayer.acceptsChatMessages`.
fn accepts_player_chat(settings: &ClientSettings) -> bool {
    settings.chat_mode == ChatMode::Enabled
}

/// The longest chat line a vanilla client sends, in UTF-16 code units. A
/// vanilla server cannot decode a longer one.
const MAX_CHAT_MESSAGE_LENGTH: usize = 256;

/// Why a chat line is not broadcast. A vanilla client never sends these
/// lines; a vanilla server disconnects whoever does. Here only the line is
/// dropped: `§` would restyle the sender's line and a line break would let
/// them forge a line in another player's name.
fn refusal(message: &str) -> Option<&'static str> {
    if message.encode_utf16().count() > MAX_CHAT_MESSAGE_LENGTH {
        Some("Chat message too long")
    } else if !message.chars().all(is_allowed_chat_character) {
        Some("Illegal characters in chat")
    } else {
        None
    }
}

/// Vanilla's `StringUtil.isAllowedChatCharacter`.
fn is_allowed_chat_character(c: char) -> bool {
    c != '§' && c >= ' ' && c != '\u{7f}'
}

fn handle_command_suggestions(
    event: On<PacketEvent<CommandSuggestionsRequest>>,
    world: &World,
    players: Players,
) {
    let Some(completion) =
        world
            .resource::<CommandRegistry>()
            .complete(&event.packet.text, world, event.entity)
    else {
        return;
    };

    players.send(
        event.entity,
        voidmc_protocol::clientbound::CommandSuggestionsResponse {
            transaction_id: event.packet.transaction_id,
            start: completion.start as i32,
            length: completion.length as i32,
            matches: completion.matches,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_chat_lines_are_accepted() {
        for message in [
            "hello",
            "",
            "   ",
            "  two  spaces  ",
            "Grüße 你好 😀",
            "{\"text\":\"x\"} <Bob> hi %s",
            "\u{80}\u{9f}\u{a0}",
        ] {
            assert_eq!(refusal(message), None, "{message:?}");
        }
    }

    #[test]
    fn the_length_limit_counts_utf16_units() {
        assert_eq!(refusal(&"a".repeat(256)), None);
        assert_eq!(refusal(&"é".repeat(256)), None);
        assert_eq!(refusal(&"😀".repeat(128)), None);
        for message in ["a".repeat(257), "é".repeat(257), "😀".repeat(128) + "a"] {
            assert_eq!(refusal(&message), Some("Chat message too long"));
        }
    }

    #[test]
    fn section_signs_and_control_characters_are_refused() {
        for message in [
            "§cred",
            "hi\n<Bob> forged",
            "tab\there",
            "\0",
            "\u{1f}",
            "del\u{7f}",
        ] {
            assert_eq!(
                refusal(message),
                Some("Illegal characters in chat"),
                "{message:?}"
            );
        }
    }
}
