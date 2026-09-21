use bevy_app::{App, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::SetPassengers;

use super::EntityShownEvent;
use crate::components::{EntityViewers, MinecraftEntityId, SpawnedEntity};
use crate::players::Players;
use crate::schedule::VoidSystems;

/// Entities riding this one, in seat order. Passengers must be visible to the
/// same viewers as the vehicle; a despawned passenger is pruned automatically.
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
    mut commands: Commands,
) {
    let Ok(passengers) = vehicles.get(event.entity) else {
        return;
    };
    for rider in &passengers.0 {
        if let Ok(mut rider) = commands.get_entity(*rider) {
            rider.remove::<Mount>();
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
    event: On<Remove, SpawnedEntity>,
    mut vehicles: Query<&mut Passengers>,
) {
    for mut passengers in vehicles.iter_mut() {
        if passengers.0.contains(&event.entity) {
            passengers.remove(event.entity);
        }
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
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::world::{ChunkPos, DimensionId};

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
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
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx))
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
        let teleports = rx
            .try_iter()
            .filter(|out| {
                out.client_id == 2
                    && matches!(
                        out.packet,
                        ClientboundPacket::Play(
                            voidmc_protocol::clientbound::PlayPacket::TeleportEntity(_)
                        )
                    )
            })
            .count();
        assert_eq!(teleports, 1);
        assert_eq!(mount(&app, pilot), None);
    }
}
