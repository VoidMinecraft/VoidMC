//! Server-owned entities: [`EntityBuilder`] spawns them, the visibility
//! tracker replicates them to the players whose loaded chunks contain them,
//! and despawning is the only removal API — a `RemoveEntities` packet always
//! follows, so clients never keep ghosts.

pub mod metadata;
pub mod passengers;

use std::collections::HashSet;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use bevy_ecs::system::EntityCommands;
use voidmc_protocol::clientbound;

pub use metadata::{
    Billboard, BlockDisplay, CustomName, Display, DisplayTransform, EntityMetadata, Glowing,
    Invisible, ItemDisplay, ItemDisplayContext, MetadataSource, MetadataSourceAppExt, NoGravity,
    Silent, TextAlignment, TextDisplay,
};
pub use passengers::Passengers;
pub use voidmc_data::v26_1_2::EntityKind;

use crate::components::{
    EntityCollider, EntityDimension, EntityType, EntityUuid, EntityViewers, Grounded, LoadedChunks,
    MinecraftEntityId, MovementConfig, PlayerDimension, PlayerReady, Position, PreviousPosition,
    RecentlySpawned, Rotation, SpawnedEntity, Velocity, VerticalVelocity,
};
use crate::events::EntityDespawnEvent;
use crate::players::{Players, Recipients};
use crate::schedule::VoidSystems;
use crate::systems::entities::spawn_entity_packet;
use crate::world::{ChunkPos, DimensionId};

// settle.rs may snap a gravity-enabled spawn down onto the ground during its
// first ticks; the window must outlast chunk generation for the spawn column.
const SETTLE_TICKS: u8 = 15;

#[derive(Debug, Clone)]
pub struct EntityBuilder {
    kind: EntityKind,
    position: Position,
    rotation: Rotation,
    velocity: Velocity,
    dimension: DimensionId,
    collider: Option<EntityCollider>,
    movement: MovementConfig,
    settle_ticks: u8,
}

impl EntityBuilder {
    pub fn new(kind: EntityKind) -> Self {
        Self {
            kind,
            position: Position::default(),
            rotation: Rotation::default(),
            velocity: Velocity::default(),
            dimension: DimensionId::Overworld,
            collider: None,
            movement: MovementConfig::default(),
            settle_ticks: SETTLE_TICKS,
        }
    }

    pub fn at(mut self, x: f64, y: f64, z: f64) -> Self {
        self.position = Position { x, y, z };
        self
    }

    pub fn position(mut self, position: Position) -> Self {
        self.position = position;
        self
    }

    pub fn rotation(mut self, yaw: f32, pitch: f32) -> Self {
        self.rotation = Rotation { yaw, pitch };
        self
    }

    pub fn velocity(mut self, velocity: Velocity) -> Self {
        self.velocity = velocity;
        self
    }

    pub fn in_dimension(mut self, dimension: DimensionId) -> Self {
        self.dimension = dimension;
        self
    }

    pub fn collider(mut self, collider: EntityCollider) -> Self {
        self.collider = Some(collider);
        self
    }

    pub fn movement(mut self, movement: MovementConfig) -> Self {
        self.movement = movement;
        self
    }

    pub fn gravity(mut self, enabled: bool) -> Self {
        self.movement.gravity_enabled = enabled;
        self
    }

    pub fn block_collision(mut self, enabled: bool) -> Self {
        self.movement.block_collision_enabled = enabled;
        self
    }

    pub fn wander(mut self, enabled: bool) -> Self {
        self.movement.wander = enabled;
        self
    }

    pub fn settle_ticks(mut self, ticks: u8) -> Self {
        self.settle_ticks = ticks;
        self
    }

    pub fn kind(&self) -> EntityKind {
        self.kind
    }

    pub fn bundle(self) -> impl Bundle {
        let gravity = self.movement.gravity_enabled;
        (
            SpawnedEntity(()),
            MinecraftEntityId::allocate(),
            EntityUuid::default(),
            EntityType(self.kind.id()),
            self.position,
            PreviousPosition {
                x: self.position.x,
                y: self.position.y,
                z: self.position.z,
            },
            self.rotation,
            self.velocity,
            EntityDimension(self.dimension),
            self.collider
                .unwrap_or_else(|| EntityCollider::for_entity_name(self.kind.name())),
            self.movement,
            VerticalVelocity(self.velocity.y),
            Grounded(!gravity),
            RecentlySpawned(self.settle_ticks),
        )
    }

    pub fn spawn<'a>(self, commands: &'a mut Commands) -> EntityCommands<'a> {
        commands.spawn(self.bundle())
    }

    pub fn spawn_in(self, world: &mut World) -> EntityWorldMut<'_> {
        world.spawn(self.bundle())
    }
}

/// Fired when `viewer` starts receiving packets for `entity`, right after its
/// `SpawnEntity` packet. Delivered at the end of the tracker system, so both
/// entities still exist unless despawned in the same tick; features that
/// attach extra packets (metadata, passengers, equipment) send them from an
/// observer of this event.
#[derive(Event, Debug, Clone, Copy)]
pub struct EntityShownEvent {
    pub entity: Entity,
    pub viewer: Entity,
}

/// Fired when `viewer` stops receiving packets for `entity`, after
/// `RemoveEntities`. Delivered deferred: `entity` may already be despawned
/// and `viewer` may have disconnected, hence the ids carried in the payload.
#[derive(Event, Debug, Clone, Copy)]
pub struct EntityHiddenEvent {
    pub entity: Entity,
    pub network_id: i32,
    pub viewer: Entity,
}

pub struct EntityPlugin;

impl Plugin for EntityPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_despawn_event)
            .add_observer(remove_from_clients)
            .add_systems(
                PostUpdate,
                track_entity_visibility.in_set(VoidSystems::EntityVisibility),
            );
        metadata::register(app);
        passengers::register(app);
    }
}

fn on_despawn_event(event: On<EntityDespawnEvent>, mut commands: Commands) {
    if let Ok(mut entity) = commands.get_entity(event.entity) {
        entity.despawn();
    }
}

fn remove_from_clients(
    event: On<Remove, SpawnedEntity>,
    players: Players,
    mut entities: Query<(&MinecraftEntityId, &mut EntityViewers)>,
    mut commands: Commands,
) {
    let Ok((id, mut viewers)) = entities.get_mut(event.entity) else {
        return;
    };
    let packet = clientbound::RemoveEntities {
        entity_ids: vec![id.0],
    };
    for viewer in viewers.players.drain() {
        players.send(viewer, packet.clone());
        commands.trigger(EntityHiddenEvent {
            entity: event.entity,
            network_id: id.0,
            viewer,
        });
    }
}

pub(crate) fn chunk_of(position: &Position) -> ChunkPos {
    ChunkPos::from_block(position.x, position.z)
}

fn desired_viewers(
    ready: &Recipients<'_>,
    dimension: DimensionId,
    chunk: ChunkPos,
    out: &mut HashSet<Entity>,
) {
    out.clear();
    out.extend(
        ready
            .iter()
            .filter(|r| r.sees_chunk(dimension, chunk))
            .map(|r| r.entity()),
    );
}

#[derive(Default)]
pub struct TrackerState {
    ready_count: usize,
}

pub fn track_entity_visibility(
    players: Players,
    mut commands: Commands,
    mut state: Local<TrackerState>,
    changed_players: Query<
        (),
        (
            With<PlayerReady>,
            Or<(
                Changed<LoadedChunks>,
                Changed<PlayerDimension>,
                Added<PlayerReady>,
            )>,
        ),
    >,
    mut entities: Query<
        (
            Entity,
            &MinecraftEntityId,
            &EntityUuid,
            &EntityType,
            &Position,
            &Rotation,
            &Velocity,
            &EntityDimension,
            &mut EntityViewers,
        ),
        With<SpawnedEntity>,
    >,
) {
    let ready = players.ready();
    let players_changed = !changed_players.is_empty() || ready.len() != state.ready_count;
    state.ready_count = ready.len();

    let mut desired = HashSet::new();
    for (entity, id, uuid, kind, position, rotation, velocity, dimension, mut viewers) in
        entities.iter_mut()
    {
        let chunk = chunk_of(position);
        let moved = viewers.chunk != Some((dimension.0, chunk));
        if !players_changed && !moved {
            continue;
        }
        viewers.chunk = Some((dimension.0, chunk));

        desired_viewers(&ready, dimension.0, chunk, &mut desired);
        if desired == viewers.players {
            continue;
        }

        let gone: Vec<Entity> = viewers.players.difference(&desired).copied().collect();
        if !gone.is_empty() {
            let packet = clientbound::RemoveEntities {
                entity_ids: vec![id.0],
            };
            for viewer in &gone {
                players.send(*viewer, packet.clone());
                commands.trigger(EntityHiddenEvent {
                    entity,
                    network_id: id.0,
                    viewer: *viewer,
                });
            }
        }

        let spawn = spawn_entity_packet(id.0, uuid.0, kind.0, position, rotation, velocity);
        for viewer in desired.difference(&viewers.players) {
            players.send(*viewer, spawn.clone());
            commands.trigger(EntityShownEvent {
                entity,
                viewer: *viewer,
            });
        }

        viewers.players.clone_from(&desired);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, ManualPlayPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

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
        .add_plugins(EntityPlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32, chunks: &[(i32, i32)]) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(id),
                PlayerReady,
                PlayerDimension(DimensionId::Overworld),
                LoadedChunks(chunks.iter().map(|&(x, z)| ChunkPos::new(x, z)).collect()),
            ))
            .id()
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Sent {
        Spawn(u32, i32),
        Remove(u32, i32),
        Move(u32, i32),
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent: Vec<Sent> = rx
            .try_iter()
            .map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::SpawnEntity(p)) => {
                    Sent::Spawn(out.client_id, p.entity_id)
                }
                ClientboundPacket::ManualPlay(ManualPlayPacket::RemoveEntities(p)) => {
                    assert_eq!(p.entity_ids.len(), 1);
                    Sent::Remove(out.client_id, p.entity_ids[0])
                }
                ClientboundPacket::Play(PlayPacket::UpdateEntityPosition(p)) => {
                    Sent::Move(out.client_id, p.entity_id)
                }
                other => panic!("unexpected packet {other:?}"),
            })
            .collect();
        sent.sort();
        sent
    }

    fn network_id(app: &App, entity: Entity) -> i32 {
        app.world().get::<MinecraftEntityId>(entity).unwrap().0
    }

    #[test]
    fn builder_spawns_every_required_component_with_unique_ids() {
        let (mut app, _rx) = test_app();
        let a = EntityBuilder::new(EntityKind::Zombie)
            .at(1.0, 2.0, 3.0)
            .rotation(90.0, 10.0)
            .in_dimension(DimensionId::Nether)
            .spawn_in(app.world_mut())
            .id();
        let b = EntityBuilder::new(EntityKind::Pig)
            .gravity(true)
            .spawn_in(app.world_mut())
            .id();

        let world = app.world();
        assert_eq!(
            world.get::<EntityType>(a).unwrap().0,
            EntityKind::Zombie.id()
        );
        assert_eq!(
            *world.get::<Position>(a).unwrap(),
            Position {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }
        );
        assert_eq!(world.get::<PreviousPosition>(a).unwrap().z, 3.0);
        assert_eq!(world.get::<Rotation>(a).unwrap().yaw, 90.0);
        assert_eq!(
            world.get::<EntityDimension>(a).unwrap().0,
            DimensionId::Nether
        );
        assert!(world.get::<Grounded>(a).unwrap().0);
        assert!(world.get::<EntityViewers>(a).unwrap().is_empty());
        assert!(world.get::<RecentlySpawned>(a).is_some());
        assert!(world.get::<VerticalVelocity>(a).is_some());
        assert!(world.get::<MovementConfig>(a).is_some());
        assert!(world.get::<EntityCollider>(a).is_some());
        assert!(world.get::<SpawnedEntity>(a).is_some());

        assert_ne!(network_id(&app, a), network_id(&app, b));
        assert_ne!(
            world.get::<EntityUuid>(a).unwrap().0,
            world.get::<EntityUuid>(b).unwrap().0
        );
        assert!(!world.get::<Grounded>(b).unwrap().0);
        assert_eq!(*world.get::<Velocity>(b).unwrap(), Velocity::default());
        assert_eq!(world.get::<RecentlySpawned>(b).unwrap().0, SETTLE_TICKS);
        assert_eq!(
            *world.get::<EntityCollider>(b).unwrap(),
            EntityCollider::for_entity_name("minecraft:pig")
        );
    }

    #[test]
    fn viewers_follow_loaded_chunks() {
        let (mut app, rx) = test_app();
        let near = player(&mut app, 1, &[(0, 0)]);
        let far = player(&mut app, 2, &[(5, 5)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .at(8.0, 64.0, 8.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);

        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);
        assert!(
            app.world()
                .get::<EntityViewers>(zombie)
                .unwrap()
                .contains(near)
        );

        app.world_mut()
            .get_mut::<LoadedChunks>(far)
            .unwrap()
            .0
            .insert(ChunkPos::new(0, 0));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(2, id)]);

        app.world_mut()
            .get_mut::<LoadedChunks>(near)
            .unwrap()
            .0
            .clear();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);
        assert_eq!(
            app.world()
                .get::<EntityViewers>(zombie)
                .unwrap()
                .iter()
                .collect::<HashSet<_>>(),
            HashSet::from([far])
        );

        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn entity_moving_between_chunks_and_dimensions_updates_viewers() {
        let (mut app, rx) = test_app();
        let _a = player(&mut app, 1, &[(0, 0)]);
        let _b = player(&mut app, 2, &[(1, 0)]);
        let pig = EntityBuilder::new(EntityKind::Pig)
            .at(8.0, 64.0, 8.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, pig);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);

        app.world_mut().get_mut::<Position>(pig).unwrap().x = 20.0;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(2, id), Sent::Remove(1, id)]);

        app.world_mut().get_mut::<EntityDimension>(pig).unwrap().0 = DimensionId::End;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(2, id)]);
    }

    #[test]
    fn late_joiner_receives_existing_entities_once_chunks_load() {
        let (mut app, rx) = test_app();
        let cow = EntityBuilder::new(EntityKind::Cow)
            .at(-3.0, 64.0, 40.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, cow);
        app.update();
        assert!(drain(&rx).is_empty());

        let joiner = app
            .world_mut()
            .spawn((ClientId(9), PlayerDimension(DimensionId::Overworld)))
            .id();
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().entity_mut(joiner).insert((
            PlayerReady,
            LoadedChunks(HashSet::from([ChunkPos::new(-1, 2)])),
        ));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(9, id)]);
    }

    #[test]
    fn despawn_is_ghost_proof() {
        let (mut app, rx) = test_app();
        let viewer = player(&mut app, 1, &[(0, 0)]);
        let _elsewhere = player(&mut app, 2, &[(9, 9)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .at(1.0, 64.0, 1.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        drain(&rx);

        app.world_mut().despawn(zombie);
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);
        app.update();
        assert!(drain(&rx).is_empty());

        let creeper = EntityBuilder::new(EntityKind::Creeper)
            .at(1.0, 64.0, 1.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, creeper);
        app.update();
        drain(&rx);
        app.world_mut()
            .trigger(EntityDespawnEvent { entity: creeper });
        app.world_mut().flush();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);
        assert!(app.world().get_entity(creeper).is_err());
        let _ = viewer;
    }

    #[test]
    fn disconnected_viewer_is_dropped_without_packets() {
        let (mut app, rx) = test_app();
        let leaver = player(&mut app, 1, &[(0, 0)]);
        let stayer = player(&mut app, 2, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .at(1.0, 64.0, 1.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id), Sent::Spawn(2, id)]);

        app.world_mut().despawn(leaver);
        app.update();
        assert!(drain(&rx).is_empty());
        let viewers = app.world().get::<EntityViewers>(zombie).unwrap();
        assert!(viewers.contains(stayer));
        assert!(!viewers.contains(leaver));
        assert_eq!(viewers.len(), 1);
    }

    #[test]
    fn shown_and_hidden_events_fire_per_viewer() {
        #[derive(Resource, Default)]
        struct Log(Vec<(&'static str, Entity)>);

        let (mut app, _rx) = test_app();
        app.init_resource::<Log>()
            .add_observer(|e: On<EntityShownEvent>, mut log: ResMut<Log>| {
                log.0.push(("shown", e.viewer))
            })
            .add_observer(|e: On<EntityHiddenEvent>, mut log: ResMut<Log>| {
                log.0.push(("hidden", e.viewer))
            });
        let viewer = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        app.update();
        app.world_mut().despawn(zombie);
        assert_eq!(
            app.world().resource::<Log>().0,
            vec![("shown", viewer), ("hidden", viewer)]
        );
    }

    #[test]
    fn movement_goes_only_to_current_viewers_and_never_doubles_a_spawn() {
        use bevy_app::PostUpdate;

        use crate::systems::entities::{
            broadcast_entity_movement, update_previous_entity_positions,
        };

        let (mut app, rx) = test_app();
        app.configure_sets(
            PostUpdate,
            (VoidSystems::EntityBroadcast, VoidSystems::EntityVisibility).chain(),
        )
        .add_systems(
            PostUpdate,
            (broadcast_entity_movement, update_previous_entity_positions)
                .chain()
                .in_set(VoidSystems::EntityBroadcast),
        );
        let _viewer = player(&mut app, 1, &[(0, 0), (1, 0)]);
        let _blind = player(&mut app, 2, &[(7, 7)]);
        let newcomer = player(&mut app, 3, &[(7, 7)]);
        let pig = EntityBuilder::new(EntityKind::Pig)
            .at(8.0, 64.0, 8.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, pig);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);

        app.world_mut().get_mut::<Position>(pig).unwrap().x = 9.0;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Move(1, id)]);

        app.world_mut().get_mut::<Position>(pig).unwrap().x = 10.0;
        app.world_mut()
            .get_mut::<LoadedChunks>(newcomer)
            .unwrap()
            .0
            .insert(ChunkPos::new(0, 0));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(3, id), Sent::Move(1, id)]);
    }

    #[test]
    fn spawn_and_despawn_in_the_same_tick_send_nothing() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        app.world_mut().despawn(zombie);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn removing_the_marker_component_removes_the_entity_from_clients() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        drain(&rx);
        app.world_mut().entity_mut(zombie).remove::<SpawnedEntity>();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn player_changing_dimension_stops_seeing_the_entity() {
        let (mut app, rx) = test_app();
        let traveller = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);

        app.world_mut()
            .get_mut::<PlayerDimension>(traveller)
            .unwrap()
            .0 = DimensionId::Nether;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);

        app.world_mut()
            .get_mut::<PlayerDimension>(traveller)
            .unwrap()
            .0 = DimensionId::Overworld;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);
    }

    #[test]
    fn unchanged_world_does_no_diff_work() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1, &[(0, 0)]);
        EntityBuilder::new(EntityKind::Zombie).spawn_in(app.world_mut());
        app.update();
        drain(&rx);
        for _ in 0..3 {
            app.update();
        }
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn hidden_event_carries_ids_after_despawn() {
        #[derive(Resource, Default)]
        struct Seen(Vec<i32>);

        let (mut app, _rx) = test_app();
        app.init_resource::<Seen>().add_observer(
            |e: On<EntityHiddenEvent>, mut seen: ResMut<Seen>| seen.0.push(e.network_id),
        );
        let _viewer = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        app.world_mut().despawn(zombie);
        assert_eq!(app.world().resource::<Seen>().0, vec![id]);
    }
}
