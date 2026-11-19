//! Keeps each client told what it may do: its permission level (the entity
//! status 24 + level on its own player) and the command tree it may run.
//! Both are sent at join by the configuration plugin, then again whenever
//! [`Operator`] is added to or removed from a player in the play state, so a
//! vanilla client offers operator commands and entity selectors only to an
//! operator, like a vanilla server.

use bevy_app::{App, Plugin};
use bevy_ecs::lifecycle::{Add, Remove};
use bevy_ecs::prelude::*;
use voidmc_protocol::{State, clientbound};

use crate::CommandRegistry;
use crate::components::{ConnectionState, MinecraftEntityId, Operator};
use crate::players::WorldPlayers;

/// The permission level an [`Operator`] is sent; any other player is level 0.
pub const OPERATOR_PERMISSION_LEVEL: u8 = 4;

pub struct PermissionsPlugin;

impl Plugin for PermissionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(operator_added)
            .add_observer(operator_removed);
    }
}

/// The permission level `player` is sent: 4 with [`Operator`], else 0.
pub fn permission_level(world: &World, player: Entity) -> u8 {
    if world.get::<Operator>(player).is_some() {
        OPERATOR_PERMISSION_LEVEL
    } else {
        0
    }
}

/// Sends `player`, whose network entity id is `entity_id`, its permission
/// level and then the command tree it may run.
pub(crate) fn send_permissions(world: &World, player: Entity, entity_id: i32) {
    let players = WorldPlayers::new(world);
    players.send(
        player,
        clientbound::EntityEvent::permission_level(entity_id, permission_level(world, player)),
    );
    if let Some(registry) = world.get_resource::<CommandRegistry>() {
        players.send(player, registry.build_command_tree_for(world, player));
    }
}

fn operator_added(event: On<Add, Operator>, mut commands: Commands) {
    let player = event.entity;
    commands.queue(move |world: &mut World| resend(world, player));
}

/// Runs as a command, once the component is gone.
fn operator_removed(event: On<Remove, Operator>, mut commands: Commands) {
    let player = event.entity;
    commands.queue(move |world: &mut World| resend(world, player));
}

/// Re-sends both to a player in the play state. Before that, the join sends
/// them; after a despawn there is no one to tell.
fn resend(world: &mut World, player: Entity) {
    let Ok(entity) = world.get_entity(player) else {
        return;
    };
    let playing = entity
        .get::<ConnectionState>()
        .is_some_and(|state| state.0 == State::Play);
    let Some(id) = entity.get::<MinecraftEntityId>().copied() else {
        return;
    };
    if playing {
        send_permissions(world, player, id.0);
    }
}

#[cfg(test)]
mod tests {
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, ManualPlayPacket, PlayPacket};

    use super::*;
    use crate::CommandBuilder;
    use crate::components::ClientId;
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

    #[derive(Debug, PartialEq)]
    enum Sent {
        Level(i32, u8),
        Tree(Vec<String>),
    }

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let mut registry = CommandRegistry::new();
        registry.register(CommandBuilder::new("open").handler(|_| {}).build());
        registry.register(
            CommandBuilder::new("admin")
                .requires(|world, player| world.get::<Operator>(player).is_some())
                .handler(|_| {})
                .build(),
        );
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_resource(registry)
        .add_plugins(PermissionsPlugin);
        (app, outgoing_rx)
    }

    fn sent(receiver: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        receiver
            .try_iter()
            .filter_map(|packet| match packet.packet {
                ClientboundPacket::Play(PlayPacket::EntityEvent(event)) => {
                    Some(Sent::Level(event.entity_id, event.event_id))
                }
                ClientboundPacket::ManualPlay(ManualPlayPacket::Commands(tree)) => {
                    let mut names: Vec<String> = tree.nodes[tree.root_index as usize]
                        .children
                        .iter()
                        .filter_map(|child| tree.nodes[*child as usize].name.clone())
                        .collect();
                    names.sort();
                    Some(Sent::Tree(names))
                }
                _ => None,
            })
            .collect()
    }

    fn player(app: &mut App, state: State) -> Entity {
        app.world_mut()
            .spawn((ClientId(1), ConnectionState(state), MinecraftEntityId(42)))
            .id()
    }

    #[test]
    fn adding_and_removing_operator_resends_the_level_then_the_tree() {
        let (mut app, outgoing) = test_app();
        let player = player(&mut app, State::Play);

        app.world_mut().entity_mut(player).insert(Operator);
        assert_eq!(
            sent(&outgoing),
            [
                Sent::Level(42, 28),
                Sent::Tree(vec!["admin".into(), "open".into()])
            ]
        );

        // Inserting it again adds nothing.
        app.world_mut().entity_mut(player).insert(Operator);
        assert_eq!(sent(&outgoing), []);

        // Through commands too, and the tree is built once it is gone.
        app.world_mut()
            .commands()
            .entity(player)
            .remove::<Operator>();
        app.world_mut().flush();
        assert_eq!(
            sent(&outgoing),
            [Sent::Level(42, 24), Sent::Tree(vec!["open".into()])]
        );
    }

    #[test]
    fn nothing_is_sent_before_the_play_state_or_after_a_despawn() {
        let (mut app, outgoing) = test_app();
        let joining = player(&mut app, State::Configuration);
        app.world_mut().entity_mut(joining).insert(Operator);
        app.world_mut().entity_mut(joining).remove::<Operator>();
        assert_eq!(sent(&outgoing), []);

        let leaving = player(&mut app, State::Play);
        app.world_mut().entity_mut(leaving).insert(Operator);
        sent(&outgoing);
        app.world_mut().entity_mut(leaving).despawn();
        assert_eq!(sent(&outgoing), []);
    }
}
