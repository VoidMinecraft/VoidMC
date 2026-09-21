use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::{self, PlayerInfoActions, PlayerInfoUpdate};

use crate::components::{
    ClientId, KeepAliveState, MinecraftEntityId, PlayerName, PlayerReady, PlayerUuid, Position,
    Rotation,
};
use crate::config::ServerConfigResource;
use crate::events::{PlayerQuitEvent, PlayerReadyEvent};
use crate::players::Players;
use crate::plugins::tab_list::{EntrySnapshot, TabEntry, TabEntryState};

/// Observer: when a player becomes ready, broadcast spawn info to/from all other ready players.
pub fn on_player_ready(
    event: On<PlayerReadyEvent>,
    players: Players,
    config: Res<ServerConfigResource>,
    mut commands: Commands,
    new_player: Query<(
        &ClientId,
        &MinecraftEntityId,
        &PlayerUuid,
        &PlayerName,
        &Position,
        &Rotation,
        Option<&TabEntry>,
        Option<&KeepAliveState>,
    )>,
    all_players: Query<
        (
            Entity,
            &MinecraftEntityId,
            &PlayerUuid,
            &PlayerName,
            &Position,
            &Rotation,
            Option<&TabEntry>,
            Option<&KeepAliveState>,
            Option<&TabEntryState>,
        ),
        With<PlayerReady>,
    >,
) {
    let new_entity = event.entity;
    let game_mode = config.game_mode;

    let Ok((new_client_id, new_mc_id, new_uuid, new_name, new_pos, new_rot, entry, keep_alive)) =
        new_player.get(new_entity)
    else {
        return;
    };

    tracing::info!(
        player_name = %new_name.0,
        player_uuid = %new_uuid.0,
        client_id = new_client_id.0,
        "Player connected"
    );

    let snapshot = EntrySnapshot::resolve(entry, keep_alive, game_mode);
    let own_entry = snapshot.initial(new_uuid.0, &new_name.0);
    commands
        .entity(new_entity)
        .insert_if_new(TabEntry::default())
        .insert(TabEntryState::sent(snapshot));

    let mut listing = Vec::with_capacity(all_players.iter().len());
    listing.push(own_entry.clone());
    for (other_entity, _, other_uuid, other_name, _, _, entry, keep_alive, state) in
        all_players.iter()
    {
        if new_entity == other_entity {
            continue;
        }
        let snapshot = match state.and_then(TabEntryState::last_sent) {
            Some(sent) => sent.clone(),
            None => EntrySnapshot::resolve(entry, keep_alive, game_mode),
        };
        listing.push(snapshot.initial(other_uuid.0, &other_name.0));
    }
    players.send(
        new_entity,
        PlayerInfoUpdate {
            actions: PlayerInfoActions::INITIALIZING,
            entries: listing,
        },
    );
    players
        .ready()
        .except(new_entity)
        .send(PlayerInfoUpdate::single(
            PlayerInfoActions::INITIALIZING,
            own_entry,
        ));

    for (other_entity, other_mc_id, other_uuid, _, other_pos, other_rot, ..) in all_players.iter() {
        if new_entity == other_entity {
            continue;
        }

        send_player_spawn(
            &players,
            new_entity,
            other_mc_id.0,
            other_uuid.0,
            other_pos,
            other_rot,
        );

        send_player_spawn(
            &players,
            other_entity,
            new_mc_id.0,
            new_uuid.0,
            new_pos,
            new_rot,
        );
    }
}

/// Observer: when a player quits, broadcast remove to all remaining ready players.
pub fn on_player_quit(
    event: On<PlayerQuitEvent>,
    players: Players,
    query: Query<(&MinecraftEntityId, &PlayerUuid, &PlayerName, &ClientId), With<PlayerReady>>,
) {
    let disc_entity = event.entity;
    let disc_client_id = event.client_id;

    let Ok((mc_entity_id, player_uuid, player_name, _)) = query.get(disc_entity) else {
        return;
    };

    let eid = mc_entity_id.0;
    let uuid = player_uuid.0;

    let others = players.ready().except(disc_entity);
    others.send(clientbound::RemoveEntities {
        entity_ids: vec![eid],
    });
    others.send(clientbound::PlayerInfoRemove { uuids: vec![uuid] });

    tracing::info!(
        player_name = %player_name.0,
        player_uuid = %player_uuid.0,
        client_id = disc_client_id,
        "Player disconnected"
    );
}

fn send_player_spawn(
    players: &Players,
    receiver: Entity,
    entity_id: i32,
    uuid: uuid::Uuid,
    pos: &Position,
    rot: &Rotation,
) {
    let yaw = (rot.yaw.rem_euclid(360.0) / 360.0 * 256.0) as u8;
    let pitch = (rot.pitch.rem_euclid(360.0) / 360.0 * 256.0) as u8;

    players.send(
        receiver,
        clientbound::SpawnEntity {
            entity_id,
            entity_uuid: uuid,
            entity_type: 155, // minecraft:player
            x: pos.x,
            y: pos.y,
            z: pos.z,
            velocity: voidmc_protocol::types::LpVec3::ZERO,
            pitch,
            yaw,
            head_yaw: yaw,
            data: 0,
        },
    );
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, Update};
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, ManualPlayPacket, PlayPacket};

    use super::*;
    use crate::config::{ServerConfigBuilder, ServerConfigResource};
    use crate::messages::TextColor;
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::plugins::tab_list::TabListPlugin;

    fn join_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_resource(ServerConfigResource::from(
            &ServerConfigBuilder::new().game_mode(2).build(),
        ))
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx))
        .add_plugins(TabListPlugin)
        .add_observer(on_player_ready)
        .add_observer(on_player_quit);
        (app, outgoing_rx)
    }

    fn join(app: &mut App, id: u32, name: &str, entry: Option<TabEntry>) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                ClientId(id),
                MinecraftEntityId(id as i32),
                PlayerUuid(uuid::Uuid::from_u128(id.into())),
                PlayerName(name.into()),
                Position::default(),
                Rotation::default(),
                KeepAliveState {
                    latency: 10 * id as i32,
                    ..Default::default()
                },
                PlayerReady,
            ))
            .id();
        if let Some(entry) = entry {
            app.world_mut().entity_mut(entity).insert(entry);
        }
        app.world_mut().trigger(PlayerReadyEvent {
            client_id: id,
            entity,
        });
        app.world_mut().flush();
        entity
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Sent {
        Info(u32, u8, Vec<(u32, i32, bool, i32, bool, i32)>),
        Remove(u32, Vec<u32>),
        Spawn(u32, i32),
        Despawn(u32, Vec<i32>),
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent = drain_in_order(rx);
        sent.sort();
        sent
    }

    fn drain_in_order(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        rx.try_iter()
            .map(|out| match out.packet {
                ClientboundPacket::ManualPlay(ManualPlayPacket::PlayerInfoUpdate(p)) => Sent::Info(
                    out.client_id,
                    p.actions.bits(),
                    p.entries
                        .iter()
                        .map(|e| {
                            (
                                e.uuid.as_u128() as u32,
                                e.game_mode,
                                e.listed,
                                e.latency,
                                e.display_name.is_some(),
                                e.list_order,
                            )
                        })
                        .collect(),
                ),
                ClientboundPacket::ManualPlay(ManualPlayPacket::PlayerInfoRemove(p)) => {
                    Sent::Remove(
                        out.client_id,
                        p.uuids.iter().map(|u| u.as_u128() as u32).collect(),
                    )
                }
                ClientboundPacket::ManualPlay(ManualPlayPacket::RemoveEntities(p)) => {
                    Sent::Despawn(out.client_id, p.entity_ids)
                }
                ClientboundPacket::Play(PlayPacket::SpawnEntity(p)) => {
                    Sent::Spawn(out.client_id, p.entity_id)
                }
                other => panic!("unexpected packet {other:?}"),
            })
            .collect()
    }

    #[test]
    fn join_sends_the_full_list_and_announces_to_others() {
        let (mut app, rx) = join_app();
        join(&mut app, 1, "alpha", None);
        assert_eq!(
            drain(&rx),
            vec![Sent::Info(1, 0xFF, vec![(1, 2, true, 10, false, 0)])]
        );
        app.update();
        assert!(drain(&rx).is_empty());

        let beta = join(
            &mut app,
            2,
            "beta",
            Some(TabEntry::new().display_name("Beta").list_order(4)),
        );
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Info(1, 0xFF, vec![(2, 2, true, 20, true, 4)]),
                Sent::Info(
                    2,
                    0xFF,
                    vec![(2, 2, true, 20, true, 4), (1, 2, true, 10, false, 0)]
                ),
                Sent::Spawn(1, 2),
                Sent::Spawn(2, 1),
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world().get::<TabEntry>(beta).unwrap().display_name,
            Some("Beta".into())
        );
    }

    #[test]
    fn join_lists_the_player_before_spawning_it_for_every_receiver() {
        let (mut app, rx) = join_app();
        join(&mut app, 1, "alpha", None);
        drain(&rx);
        join(&mut app, 2, "beta", None);

        let sent = drain_in_order(&rx);
        for receiver in [1, 2] {
            let listed = sent
                .iter()
                .position(|p| matches!(p, Sent::Info(id, 0xFF, _) if *id == receiver))
                .unwrap();
            let spawned = sent
                .iter()
                .position(|p| matches!(p, Sent::Spawn(id, _) if *id == receiver))
                .unwrap();
            assert!(listed < spawned, "receiver {receiver}: {sent:?}");
        }
    }

    #[test]
    fn join_uses_what_others_were_last_sent_then_diffs() {
        let (mut app, rx) = join_app();
        let alpha = join(&mut app, 1, "alpha", None);
        app.update();
        drain(&rx);

        {
            let mut entry = app.world_mut().get_mut::<TabEntry>(alpha).unwrap();
            entry.display_name = Some("Alpha".into());
            entry.color = TextColor::Gold;
        }
        join(&mut app, 2, "beta", None);
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Info(1, 0xFF, vec![(2, 2, true, 20, false, 0)]),
                Sent::Info(
                    2,
                    0xFF,
                    vec![(2, 2, true, 20, false, 0), (1, 2, true, 10, false, 0)]
                ),
                Sent::Spawn(1, 2),
                Sent::Spawn(2, 1),
            ]
        );

        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Info(1, 0x20, vec![(1, 2, true, 10, true, 0)]),
                Sent::Info(2, 0x20, vec![(1, 2, true, 10, true, 0)]),
            ]
        );
    }

    #[test]
    fn quit_removes_the_entry_and_the_entity_for_others() {
        let (mut app, rx) = join_app();
        let alpha = join(&mut app, 1, "alpha", None);
        join(&mut app, 2, "beta", None);
        app.update();
        drain(&rx);

        app.world_mut().trigger(PlayerQuitEvent {
            client_id: 1,
            entity: alpha,
        });
        app.world_mut().flush();
        assert_eq!(
            drain(&rx),
            vec![Sent::Remove(2, vec![1]), Sent::Despawn(2, vec![1])]
        );
    }

    #[test]
    fn player_spawn_wraps_negative_rotation() {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        });
        let receiver = app.world_mut().spawn(ClientId(7)).id();

        app.add_systems(Update, move |players: Players| {
            send_player_spawn(
                &players,
                receiver,
                42,
                uuid::Uuid::nil(),
                &Position {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
                &Rotation {
                    yaw: -90.0,
                    pitch: -45.0,
                },
            );
        });
        app.update();

        let spawn = outgoing_rx.recv().unwrap();
        assert_eq!(spawn.client_id, 7);
        let clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SpawnEntity(spawn)) =
            spawn.packet
        else {
            panic!("expected spawn entity packet");
        };

        assert_eq!(spawn.yaw, 192);
        assert_eq!(spawn.head_yaw, 192);
        assert_eq!(spawn.pitch, 224);

        drop((incoming_tx, disconnect_tx, kick_rx));
    }
}
