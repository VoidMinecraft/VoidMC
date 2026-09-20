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

pub(super) fn register(app: &mut App) {
    app.add_observer(send_passengers_on_shown)
        .add_observer(prune_despawned_passenger)
        .add_systems(
            PostUpdate,
            sync_passengers.in_set(VoidSystems::EntityMetadataSync),
        );
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
}
