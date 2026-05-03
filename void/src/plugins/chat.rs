use bevy_app::{App, Plugin};
use bevy_ecs::{
    entity::Entity,
    observer::On,
    query::With,
    system::{Commands, Query, Res, ResMut},
};
use voidmc_protocol::{
    clientbound,
    serverbound::{ChatCommand, ChatMessage, CommandSuggestionsRequest, SignedChatCommand},
};

use crate::{
    CommandRegistry,
    commands::{CommandEnqueueSequence, CommandQueue, enqueue_command},
    components::{ClientId, PlayerName, PlayerReady},
    events::{ChatCommandEvent, ChatMessageEvent},
    network::{NetworkChannels, OutgoingPacket, PacketEvent},
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
    mut commands: Commands,
) {
    let parts: Vec<String> = command.split_whitespace().map(String::from).collect();
    let (command_name, args) = match parts.split_first() {
        Some((name, rest)) => (name.clone(), rest.to_vec()),
        None => return,
    };

    enqueue_command(
        queue,
        sequence,
        client_id,
        entity,
        command_name.clone(),
        args.clone(),
    );

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
    commands: Commands,
) {
    handle_command(
        event.client_id,
        event.entity,
        &event.packet.command,
        queue.into_inner(),
        sequence.into_inner(),
        commands,
    );
}

fn handle_signed_chat_command(
    event: On<PacketEvent<SignedChatCommand>>,
    queue: ResMut<CommandQueue>,
    sequence: ResMut<CommandEnqueueSequence>,
    commands: Commands,
) {
    handle_command(
        event.client_id,
        event.entity,
        &event.packet.command,
        queue.into_inner(),
        sequence.into_inner(),
        commands,
    );
}

fn handle_chat_message(
    event: On<PacketEvent<ChatMessage>>,
    mut commands: Commands,
    queue: ResMut<CommandQueue>,
    sequence: ResMut<CommandEnqueueSequence>,
    channels: Res<NetworkChannels>,
    player_names: Query<&PlayerName>,
    ready_clients: Query<&ClientId, With<PlayerReady>>,
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
            commands,
        );
        return;
    }

    let player_name = player_names
        .get(event.entity)
        .map(|n| n.0.clone())
        .unwrap_or_else(|_| "Unknown".to_string());

    let formatted = format!("<{}> {}", player_name, event.packet.message);
    let nbt = crate::commands::text_to_nbt(&formatted, "white");
    let packet = clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SystemChat(
        clientbound::SystemChat {
            content: nbt,
            overlay: false,
        },
    ));

    // Broadcast the chat message to all ready players
    for client in ready_clients.iter() {
        let _ = channels.outgoing.send(OutgoingPacket {
            client_id: client.0,
            packet: packet.clone(),
        });
    }

    commands.trigger(ChatMessageEvent {
        entity: event.entity,
        client_id: event.client_id,
        message: event.packet.message.to_string(),
    });
}

fn handle_command_suggestions(
    event: On<PacketEvent<CommandSuggestionsRequest>>,
    command_registry: Res<CommandRegistry>,
    ready_players: Query<&PlayerName, With<PlayerReady>>,
    channels: Res<NetworkChannels>,
) {
    // text is e.g. "/kick dan" — split into command + partial arg
    let without_slash = event
        .packet
        .text
        .strip_prefix('/')
        .unwrap_or(&event.packet.text);
    let parts: Vec<&str> = without_slash.splitn(2, ' ').collect();
    let command_name = parts[0];

    // Verify the command exists
    if !command_registry.resolve(command_name).is_some() {
        return;
    }

    // Extract partial token being typed
    let arg_text = parts.get(1).copied().unwrap_or("");
    let completing_new = event.packet.text.ends_with(' ');
    let partial = if completing_new || arg_text.is_empty() {
        ""
    } else {
        arg_text.split_whitespace().last().unwrap_or("")
    };

    // Collect online player names matching the partial input
    let names: Vec<String> = ready_players
        .iter()
        .map(|n| n.0.clone())
        .filter(|name| name.to_lowercase().starts_with(&partial.to_lowercase()))
        .collect();

    // Calculate start position: position of the partial token in the original text
    let start = if partial.is_empty() {
        event.packet.text.len() as i32
    } else {
        (event.packet.text.len() - partial.len()) as i32
    };

    let response = voidmc_protocol::clientbound::CommandSuggestionsResponse {
        transaction_id: event.packet.transaction_id,
        start,
        length: partial.len() as i32,
        matches: names,
    };

    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: event.client_id,
        packet: voidmc_protocol::clientbound::ClientboundPacket::ManualPlay(
            voidmc_protocol::clientbound::ManualPlayPacket::CommandSuggestionsResponse(response),
        ),
    });
}
