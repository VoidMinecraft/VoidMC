use bevy_app::{App, Plugin};
use bevy_ecs::{
    entity::Entity,
    observer::On,
    system::{Commands, Query, ResMut},
    world::World,
};
use voidmc_protocol::serverbound::{
    ChatCommand, ChatMessage, CommandSuggestionsRequest, SignedChatCommand,
};

use crate::{
    CommandRegistry,
    commands::{CommandEnqueueSequence, CommandQueue, enqueue_command},
    components::PlayerName,
    events::{ChatCommandEvent, ChatMessageEvent},
    messages::Messages,
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
    mut commands: Commands,
) {
    let parts: Vec<String> = command.split_whitespace().map(String::from).collect();
    let (command_name, args) = match parts.split_first() {
        Some((name, rest)) => (name.clone(), rest.to_vec()),
        None => {
            return;
        }
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
    messages: Messages,
    player_names: Query<&PlayerName>,
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

    tracing::info!(
        player_name = %player_name,
        client_id = event.client_id,
        message = %event.packet.message,
        "Chat message"
    );

    messages
        .broadcast(format!("<{}> {}", player_name, event.packet.message))
        .send();

    commands.trigger(ChatMessageEvent {
        entity: event.entity,
        client_id: event.client_id,
        message: event.packet.message.to_string(),
    });
}

fn handle_command_suggestions(
    event: On<PacketEvent<CommandSuggestionsRequest>>,
    world: &World,
    players: Players,
) {
    let Some(completion) = world
        .resource::<CommandRegistry>()
        .complete(&event.packet.text, world)
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
