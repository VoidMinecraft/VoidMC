use bevy_app::{App, PostUpdate};
use bevy_ecs::lifecycle::{Despawn, Remove};
use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::SetPassengers;

use super::EntityShownEvent;
use crate::components::{EntityViewers, MinecraftEntityId, SpawnedEntity};
use crate::players::Players;
use crate::schedule::VoidSystems;

/// Entities riding this one, in seat order. Passengers must be visible to the
/// same viewers as the vehicle; a passenger that despawns or stops being
/// [`SpawnedEntity`] is pruned automatically.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct Passengers(pub Vec<Entity>);

impl Passengers {
    pub fn new(passengers: impl IntoIterator<Item = Entity>) -> Self {
        Self(passengers.into_iter().collect())
    }

    pub fn push(&mut self, passenger: Entity) {
        if !self.0.contains(&passenger) {
            self.0.push(passenger);
        }
    }

    pub fn remove(&mut self, passenger: Entity) -> bool {
        let before = self.0.len();
        self.0.retain(|p| *p != passenger);
        before != self.0.len()
    }
}

/// The vehicle this entity currently rides, mirrored from the vehicle's
/// [`Passengers`] list. Read-only for user code: it is added, retargeted and
/// removed by the passengers sync and disappears with the vehicle.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mount(pub Entity);

pub(super) fn register(app: &mut App) {
    app.add_observer(send_passengers_on_shown)
        .add_observer(prune_despawned_passenger)
        .add_observer(prune_hidden_passenger)
        .add_observer(unmount_on_vehicle_removed)
        .add_systems(
            PostUpdate,
            (
                sync_mounts.before(VoidSystems::PlayerBroadcast),
                sync_passengers,
            )
                .in_set(VoidSystems::EntityMetadataSync),
        );
}

fn sync_mounts(
    vehicles: Query<(Entity, &Passengers), Changed<Passengers>>,
    mounts: Query<(Entity, &Mount)>,
    mut commands: Commands,
) {
    for (vehicle, passengers) in vehicles.iter() {
        for (rider, mount) in mounts.iter() {
            if mount.0 == vehicle && !passengers.0.contains(&rider) {
                commands.entity(rider).remove::<Mount>();
            }
        }
    }
    for (vehicle, passengers) in vehicles.iter() {
        for rider in &passengers.0 {
            if mounts
                .get(*rider)
                .is_ok_and(|(_, mount)| mount.0 == vehicle)
            {
                continue;
            }
            if let Ok(mut rider) = commands.get_entity(*rider) {
                rider.insert(Mount(vehicle));
            }
        }
    }
}

fn unmount_on_vehicle_removed(
    event: On<Remove, Passengers>,
    vehicles: Query<&Passengers>,
    mounts: Query<&Mount>,
    mut commands: Commands,
) {
    let Ok(passengers) = vehicles.get(event.entity) else {
        return;
    };
    for rider in &passengers.0 {
        if mounts
            .get(*rider)
            .is_ok_and(|mount| mount.0 == event.entity)
        {
            commands.entity(*rider).remove::<Mount>();
        }
    }
}

fn packet(vehicle: i32, passengers: &Passengers, ids: &Query<&MinecraftEntityId>) -> SetPassengers {
    SetPassengers {
        entity_id: vehicle,
        passengers: passengers
            .0
            .iter()
            .filter_map(|p| ids.get(*p).ok().map(|id| id.0))
            .collect(),
    }
}

fn sync_passengers(
    players: Players,
    vehicles: Query<
        (&MinecraftEntityId, &EntityViewers, &Passengers),
        (With<SpawnedEntity>, Changed<Passengers>),
    >,
    ids: Query<&MinecraftEntityId>,
) {
    let ready = players.ready();
    for (id, viewers, passengers) in vehicles.iter() {
        if viewers.is_empty() {
            continue;
        }
        let packet = packet(id.0, passengers, &ids);
        ready.send_where(|r| viewers.contains(r.entity()), packet);
    }
}

fn send_passengers_on_shown(
    event: On<EntityShownEvent>,
    players: Players,
    vehicles: Query<(&MinecraftEntityId, &Passengers)>,
    ids: Query<&MinecraftEntityId>,
) {
    let Ok((id, passengers)) = vehicles.get(event.entity) else {
        return;
    };
    if passengers.0.is_empty() {
        return;
    }
    players.send(event.viewer, packet(id.0, passengers, &ids));
}

fn prune_despawned_passenger(
    event: On<Despawn, MinecraftEntityId>,
    mounts: Query<&Mount>,
    mut vehicles: Query<&mut Passengers>,
) {
    let passenger = event.entity;
    if let Some(mut list) = mounts
        .get(passenger)
        .ok()
        .and_then(|mount| vehicles.get_mut(mount.0).ok())
        && list.remove(passenger)
    {
        return;
    }
    for mut passengers in vehicles.iter_mut() {
        if passengers.0.contains(&passenger) {
            passengers.remove(passenger);
        }
    }
}

fn prune_hidden_passenger(
    event: On<Remove, SpawnedEntity>,
    mounts: Query<&Mount>,
    mut vehicles: Query<&mut Passengers>,
) {
    if let Ok(mount) = mounts.get(event.entity)
        && let Ok(mut list) = vehicles.get_mut(mount.0)
    {
        list.remove(event.entity);
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, ManualPlayPacket};

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
    use crate::entity::{EntityBuilder, EntityKind, EntityPlugin};
    use crate::network::{NetworkChannels, OutgoingPacket};
    use crate::world::{ChunkPos, DimensionId};

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
        .configure_sets(
            PostUpdate,
            (
                VoidSystems::EntityMetadataSync,
                VoidSystems::EntityVisibility,
            )
                .chain(),
        )
        .add_plugins(EntityPlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(id),
                PlayerReady,
                PlayerDimension(DimensionId::Overworld),
                LoadedChunks([ChunkPos::new(0, 0)].into_iter().collect()),
            ))
            .id()
    }

    fn passengers(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, i32, Vec<i32>)> {
        rx.try_iter()
            .filter_map(|out| match out.packet {
                ClientboundPacket::ManualPlay(ManualPlayPacket::SetPassengers(p)) => {
                    Some((out.client_id, p.entity_id, p.passengers))
                }
                _ => None,
            })
            .collect()
    }

    fn id(app: &App, e: Entity) -> i32 {
        app.world().get::<MinecraftEntityId>(e).unwrap().0
    }

    #[test]
    fn passenger_list_is_sent_on_show_on_change_and_after_a_passenger_despawns() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1);
        let rider = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let horse = EntityBuilder::new(EntityKind::Horse)
            .spawn_in(app.world_mut())
            .insert(Passengers::new([rider]))
            .id();
        app.update();
        assert_eq!(
            passengers(&rx),
            vec![(1, id(&app, horse), vec![id(&app, rider)])]
        );

        let second = EntityBuilder::new(EntityKind::Chicken)
            .spawn_in(app.world_mut())
            .id();
        app.world_mut()
            .get_mut::<Passengers>(horse)
            .unwrap()
            .push(second);
        app.update();
        assert_eq!(
            passengers(&rx),
            vec![(1, id(&app, horse), vec![id(&app, rider), id(&app, second)])]
        );

        app.world_mut().despawn(rider);
        app.update();
        assert_eq!(
            passengers(&rx),
            vec![(1, id(&app, horse), vec![id(&app, second)])]
        );

        let late = player(&mut app, 2);
        app.update();
        assert_eq!(
            passengers(&rx),
            vec![(2, id(&app, horse), vec![id(&app, second)])]
        );
        let _ = late;

        app.update();
        assert!(passengers(&rx).is_empty());
    }

    fn mount(app: &App, e: Entity) -> Option<Entity> {
        app.world().get::<Mount>(e).map(|mount| mount.0)
    }

    #[test]
    fn mount_follows_the_passenger_list_and_dies_with_the_vehicle() {
        let (mut app, _rx) = test_app();
        let rider = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let horse = EntityBuilder::new(EntityKind::Horse)
            .spawn_in(app.world_mut())
            .insert(Passengers::new([rider]))
            .id();
        app.update();
        assert_eq!(mount(&app, rider), Some(horse));

        let second = EntityBuilder::new(EntityKind::Chicken)
            .spawn_in(app.world_mut())
            .id();
        let mut list = app.world_mut().get_mut::<Passengers>(horse).unwrap();
        list.push(second);
        list.remove(rider);
        app.update();
        assert_eq!(mount(&app, rider), None);
        assert_eq!(mount(&app, second), Some(horse));

        let boat = EntityBuilder::new(EntityKind::Pig)
            .spawn_in(app.world_mut())
            .insert(Passengers::new([second]))
            .id();
        app.world_mut()
            .get_mut::<Passengers>(horse)
            .unwrap()
            .remove(second);
        app.update();
        assert_eq!(mount(&app, second), Some(boat));

        app.world_mut().despawn(boat);
        app.update();
        assert_eq!(mount(&app, second), None);
        assert!(app.world().get::<Passengers>(horse).is_some());
    }

    #[test]
    fn mounted_player_broadcasts_rotation_only_while_riding() {
        use crate::components::{MinecraftEntityId, Position, PreviousPosition, Rotation};
        use crate::systems::position::{broadcast_position, update_previous_positions};
        use bevy_app::PostUpdate;
        use bevy_ecs::schedule::IntoScheduleConfigs;

        let (mut app, rx) = test_app();
        app.add_systems(
            PostUpdate,
            (broadcast_position, update_previous_positions)
                .chain()
                .in_set(VoidSystems::PlayerBroadcast),
        );
        let pilot = player(&mut app, 1);
        app.world_mut().entity_mut(pilot).insert((
            MinecraftEntityId(9),
            Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            PreviousPosition {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
        ));
        let _viewer = player(&mut app, 2);
        let kart = EntityBuilder::new(EntityKind::Pig)
            .spawn_in(app.world_mut())
            .insert(Passengers::default())
            .id();
        app.update();
        let _ = rx.try_iter().count();

        app.world_mut()
            .get_mut::<Passengers>(kart)
            .unwrap()
            .push(pilot);
        for tick in 0..3 {
            let mut pos = app.world_mut().get_mut::<Position>(pilot).unwrap();
            pos.x += 0.5;
            pos.z -= 0.25;
            app.update();
            let moves = rx
                .try_iter()
                .filter(|out| {
                    matches!(
                        out.packet,
                        ClientboundPacket::Play(
                            voidmc_protocol::clientbound::PlayPacket::UpdateEntityPosition(_)
                                | voidmc_protocol::clientbound::PlayPacket::UpdateEntityPositionAndRotation(_)
                                | voidmc_protocol::clientbound::PlayPacket::TeleportEntity(_)
                                | voidmc_protocol::clientbound::PlayPacket::EntityPositionSync(_)
                        )
                    )
                })
                .count();
            assert_eq!(
                moves, 0,
                "tick {tick} leaked a position packet for a passenger"
            );
        }
        assert_eq!(mount(&app, pilot), Some(kart));

        app.world_mut()
            .get_mut::<Passengers>(kart)
            .unwrap()
            .remove(pilot);
        app.update();
        let syncs = rx
            .try_iter()
            .filter(|out| {
                out.client_id == 2
                    && matches!(
                        out.packet,
                        ClientboundPacket::Play(
                            voidmc_protocol::clientbound::PlayPacket::EntityPositionSync(_)
                        )
                    )
            })
            .count();
        assert_eq!(syncs, 1);
        assert_eq!(mount(&app, pilot), None);
    }

    #[test]
    fn transfer_to_a_vehicle_that_dies_late_in_the_same_tick_keeps_the_mount() {
        use bevy_app::Last;

        #[derive(Resource, Default)]
        struct Doomed(Option<Entity>);

        let (mut app, _rx) = test_app();
        app.init_resource::<Doomed>().add_systems(
            Last,
            |mut doomed: ResMut<Doomed>, mut commands: Commands| {
                if let Some(vehicle) = doomed.0.take() {
                    commands.entity(vehicle).despawn();
                }
            },
        );
        let rider = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let old = EntityBuilder::new(EntityKind::Horse)
            .spawn_in(app.world_mut())
            .insert(Passengers::new([rider]))
            .id();
        app.update();
        assert_eq!(mount(&app, rider), Some(old));

        let new = EntityBuilder::new(EntityKind::Pig)
            .spawn_in(app.world_mut())
            .insert(Passengers::new([rider]))
            .id();
        app.world_mut().resource_mut::<Doomed>().0 = Some(old);
        app.update();
        assert!(app.world().get_entity(old).is_err());
        assert_eq!(mount(&app, rider), Some(new));

        app.update();
        assert_eq!(mount(&app, rider), Some(new));
    }

    #[test]
    fn a_despawned_player_pilot_is_pruned_and_the_list_resent() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1);
        let pilot = player(&mut app, 2);
        app.world_mut()
            .entity_mut(pilot)
            .insert(MinecraftEntityId(9));
        let kart = EntityBuilder::new(EntityKind::Pig)
            .spawn_in(app.world_mut())
            .insert(Passengers::new([pilot]))
            .id();
        app.update();
        assert_eq!(mount(&app, pilot), Some(kart));
        let _ = passengers(&rx);

        app.world_mut().despawn(pilot);
        assert!(app.world().get::<Passengers>(kart).unwrap().0.is_empty());
        app.update();
        assert_eq!(passengers(&rx), vec![(1, id(&app, kart), vec![])]);
    }

    fn broadcast_app() -> (App, Receiver<OutgoingPacket>) {
        use crate::systems::position::{broadcast_position, update_previous_positions};
        use bevy_app::PostUpdate;
        use bevy_ecs::schedule::IntoScheduleConfigs;

        let (mut app, rx) = test_app();
        app.add_systems(
            PostUpdate,
            (broadcast_position, update_previous_positions)
                .chain()
                .in_set(VoidSystems::PlayerBroadcast),
        );
        (app, rx)
    }

    fn pilot(app: &mut App, client: u32, entity_id: i32) -> Entity {
        use crate::components::{Position, PreviousPosition, Rotation};

        let pilot = player(app, client);
        app.world_mut().entity_mut(pilot).insert((
            MinecraftEntityId(entity_id),
            Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            PreviousPosition {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            Rotation::default(),
        ));
        pilot
    }

    fn movement_packets(
        rx: &Receiver<OutgoingPacket>,
    ) -> Vec<voidmc_protocol::clientbound::PlayPacket> {
        use voidmc_protocol::clientbound::PlayPacket;

        rx.try_iter()
            .filter_map(|out| match out.packet {
                ClientboundPacket::Play(
                    packet @ (PlayPacket::UpdateEntityPosition(_)
                    | PlayPacket::UpdateEntityPositionAndRotation(_)
                    | PlayPacket::UpdateEntityRotation(_)
                    | PlayPacket::SetHeadRotation(_)
                    | PlayPacket::TeleportEntity(_)
                    | PlayPacket::EntityPositionSync(_)),
                ) => Some(packet),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_vehicle_transfer_does_not_resync_the_rider() {
        let (mut app, rx) = broadcast_app();
        let pilot = pilot(&mut app, 1, 9);
        let _viewer = player(&mut app, 2);
        let old = EntityBuilder::new(EntityKind::Pig)
            .spawn_in(app.world_mut())
            .insert(Passengers::new([pilot]))
            .id();
        let new = EntityBuilder::new(EntityKind::Horse)
            .spawn_in(app.world_mut())
            .insert(Passengers::default())
            .id();
        app.update();
        assert_eq!(mount(&app, pilot), Some(old));
        let _ = rx.try_iter().count();

        app.world_mut()
            .get_mut::<Passengers>(old)
            .unwrap()
            .remove(pilot);
        app.world_mut()
            .get_mut::<Passengers>(new)
            .unwrap()
            .push(pilot);
        app.update();
        assert_eq!(mount(&app, pilot), Some(new));
        assert!(movement_packets(&rx).is_empty());
    }

    #[test]
    fn a_riding_client_repeating_its_look_sends_one_rotation() {
        use crate::components::Rotation;
        use crate::network::PacketEvent;
        use crate::plugins::movement::MovementPlugin;
        use voidmc_protocol::clientbound::PlayPacket;
        use voidmc_protocol::serverbound::SetPlayerPosAndRot;

        let (mut app, rx) = broadcast_app();
        app.add_plugins(MovementPlugin);
        let pilot = pilot(&mut app, 1, 9);
        let _viewer = player(&mut app, 2);
        let _kart = EntityBuilder::new(EntityKind::Pig)
            .spawn_in(app.world_mut())
            .insert(Passengers::new([pilot]))
            .id();
        app.update();
        let _ = rx.try_iter().count();

        let look = |app: &mut App| {
            app.world_mut().trigger(PacketEvent {
                client_id: 1,
                entity: pilot,
                packet: SetPlayerPosAndRot {
                    x: 0.1,
                    y: -999.0,
                    z: 0.0,
                    yaw: 90.0,
                    pitch: 0.0,
                    flags: 0,
                },
            });
            app.update();
        };

        look(&mut app);
        let sent = movement_packets(&rx);
        assert!(
            matches!(
                sent.as_slice(),
                [
                    PlayPacket::UpdateEntityRotation(_),
                    PlayPacket::SetHeadRotation(_)
                ]
            ),
            "{sent:?}"
        );
        let rotation = app.world().get::<Rotation>(pilot).unwrap();
        assert_eq!((rotation.yaw, rotation.pitch), (90.0, 0.0));

        look(&mut app);
        assert!(movement_packets(&rx).is_empty());
    }
}
