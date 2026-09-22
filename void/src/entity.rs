//! Server-owned entities: [`EntityBuilder`] spawns them, the visibility
//! tracker replicates them to the players whose loaded chunks contain them,
//! and despawning is the only removal API — a `RemoveEntities` packet always
//! follows, so clients never keep ghosts.

pub mod metadata;
pub mod passengers;
pub mod visibility_index;

use std::collections::HashSet;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use bevy_ecs::system::EntityCommands;
use voidmc_protocol::clientbound;

pub use metadata::{
    Billboard, BlockDisplay, CustomName, Display, DisplayTransform, EndCrystal, EntityMetadata,
    Glowing, Invisible, ItemDisplay, ItemDisplayContext, MetadataSource, MetadataSourceAppExt,
    NoGravity, Silent, TextAlignment, TextDisplay,
};
pub use passengers::{Mount, Passengers};
pub use voidmc_data::v26_1_2::EntityKind;

use crate::components::{
    EntityCollider, EntityDimension, EntityType, EntityUuid, EntityViewers, Grounded,
    MinecraftEntityId, MovementConfig, Position, PreviousPosition, RecentlySpawned, Rotation,
    SpawnedEntity, Velocity, VerticalVelocity,
};
use crate::entity::visibility_index::{ChunkViewerIndex, DirtyChunks};
use crate::events::EntityDespawnEvent;
use crate::players::Players;
use crate::schedule::VoidSystems;
use crate::systems::entities::spawn_entity_packet;
use crate::world::{ChunkPos, DimensionId};

// settle.rs may snap a gravity-enabled spawn down onto the ground during its
// first ticks; the window must outlast chunk generation for the spawn column.
const SETTLE_TICKS: u8 = 15;

/// A component or bundle staged by [`EntityBuilder::with`], applied to the
/// spawned entity right after its base bundle so the `Commands` and the `World`
/// spawn paths land the same components in the same order.
trait DeferredInsert {
    fn apply_commands(self: Box<Self>, entity: &mut EntityCommands);
    fn apply_world(self: Box<Self>, entity: &mut EntityWorldMut);
}

struct Insert<B>(B);

impl<B: Bundle> DeferredInsert for Insert<B> {
    fn apply_commands(self: Box<Self>, entity: &mut EntityCommands) {
        entity.insert(self.0);
    }

    fn apply_world(self: Box<Self>, entity: &mut EntityWorldMut) {
        entity.insert(self.0);
    }
}

pub struct EntityBuilder {
    kind: EntityKind,
    position: Position,
    rotation: Rotation,
    velocity: Velocity,
    dimension: DimensionId,
    collider: Option<EntityCollider>,
    movement: MovementConfig,
    settle_ticks: u8,
    extras: Vec<Box<dyn DeferredInsert>>,
}

impl std::fmt::Debug for EntityBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntityBuilder")
            .field("kind", &self.kind)
            .field("position", &self.position)
            .field("rotation", &self.rotation)
            .field("velocity", &self.velocity)
            .field("dimension", &self.dimension)
            .field("collider", &self.collider)
            .field("movement", &self.movement)
            .field("settle_ticks", &self.settle_ticks)
            .field("extras", &self.extras.len())
            .finish()
    }
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
            extras: Vec::new(),
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

    /// Stage an extra component to attach the moment the entity is spawned, so
    /// it can be declared before `spawn`/`spawn_in` instead of inserted after.
    /// Staged components are applied in call order, on both spawn paths.
    pub fn with<C: Component>(mut self, component: C) -> Self {
        self.extras.push(Box::new(Insert(component)));
        self
    }

    /// Stage a whole bundle at once, the multi-component form of [`Self::with`].
    pub fn with_bundle<B: Bundle>(mut self, bundle: B) -> Self {
        self.extras.push(Box::new(Insert(bundle)));
        self
    }

    /// The base bundle every spawned entity needs. This is the raw escape hatch
    /// for handing a bundle straight to Bevy (`commands.spawn(builder.bundle())`);
    /// it does not carry components staged with [`Self::with`], since those are
    /// applied by `spawn`/`spawn_in` after the base bundle lands.
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

    pub fn spawn<'a>(mut self, commands: &'a mut Commands) -> EntityCommands<'a> {
        let extras = std::mem::take(&mut self.extras);
        let mut entity = commands.spawn(self.bundle());
        for extra in extras {
            extra.apply_commands(&mut entity);
        }
        entity
    }

    pub fn spawn_in(mut self, world: &mut World) -> EntityWorldMut<'_> {
        let extras = std::mem::take(&mut self.extras);
        let mut entity = world.spawn(self.bundle());
        for extra in extras {
            extra.apply_world(&mut entity);
        }
        entity
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

/// Hides a tracked entity from every player without despawning it. Inserting
/// it sends `RemoveEntities` to the current viewers and empties
/// [`EntityViewers`], so no movement, metadata or passenger packet goes out
/// while it is present; removing it re-runs the normal spawn path for every
/// player in range (`SpawnEntity`, full metadata, passengers). Passengers are
/// separate entities and stay visible unless hidden themselves.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Hidden;

pub struct EntityPlugin;

impl Plugin for EntityPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_despawn_event)
            .add_observer(remove_from_clients)
            .add_observer(on_hidden_inserted)
            .add_observer(on_hidden_removed)
            .add_systems(
                PostUpdate,
                track_entity_visibility.in_set(VoidSystems::EntityVisibility),
            );
        visibility_index::register(app);
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

fn on_hidden_inserted(
    event: On<bevy_ecs::lifecycle::Insert, Hidden>,
    mut dirty: ResMut<DirtyChunks>,
    entities: Query<&EntityViewers>,
) {
    mark_tracked_chunk(event.entity, &mut dirty, &entities);
}

fn on_hidden_removed(
    event: On<Remove, Hidden>,
    mut dirty: ResMut<DirtyChunks>,
    entities: Query<&EntityViewers>,
) {
    mark_tracked_chunk(event.entity, &mut dirty, &entities);
}

fn mark_tracked_chunk(entity: Entity, dirty: &mut DirtyChunks, entities: &Query<&EntityViewers>) {
    if let Some(key) = entities.get(entity).ok().and_then(|v| v.chunk) {
        dirty.mark(key);
    }
}

pub(crate) fn chunk_of(position: &Position) -> ChunkPos {
    ChunkPos::from_block(position.x, position.z)
}

pub fn track_entity_visibility(
    players: Players,
    mut commands: Commands,
    index: Res<ChunkViewerIndex>,
    mut dirty: ResMut<DirtyChunks>,
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
            Has<Hidden>,
        ),
        With<SpawnedEntity>,
    >,
) {
    let empty = HashSet::new();
    for (entity, id, uuid, kind, position, rotation, velocity, dimension, mut viewers, hidden) in
        entities.iter_mut()
    {
        let key = (dimension.0, chunk_of(position));
        let moved = viewers.chunk != Some(key);
        if !moved && !dirty.contains(&key) {
            continue;
        }
        viewers.chunk = Some(key);

        let desired = if hidden {
            &empty
        } else {
            index.viewers(key).unwrap_or(&empty)
        };
        if *desired == viewers.players {
            continue;
        }

        let gone: Vec<Entity> = viewers.players.difference(desired).copied().collect();
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

        viewers.players.clone_from(desired);
    }

    dirty.clear();
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, ManualPlayPacket, PlayPacket};

    use bevy_ecs::system::RunSystemOnce;

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
    use crate::entity::visibility_index::ChunkViewerIndex;
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

    fn index_viewers(app: &App, dimension: DimensionId, chunk: (i32, i32)) -> HashSet<Entity> {
        app.world()
            .resource::<ChunkViewerIndex>()
            .viewers((dimension, ChunkPos::new(chunk.0, chunk.1)))
            .cloned()
            .unwrap_or_default()
    }

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
        Metadata(u32, i32),
        Passengers(u32, i32),
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
                ClientboundPacket::Play(PlayPacket::SetEntityData(p)) => {
                    Sent::Metadata(out.client_id, p.entity_id)
                }
                ClientboundPacket::ManualPlay(ManualPlayPacket::SetPassengers(p)) => {
                    Sent::Passengers(out.client_id, p.entity_id)
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
    fn with_puts_a_single_component_on_the_spawned_entity() {
        let (mut app, _rx) = test_app();
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .with(CustomName::new("Bob"))
            .spawn_in(app.world_mut())
            .id();

        let world = app.world();
        assert_eq!(world.get::<CustomName>(zombie).unwrap().text, "Bob");
        assert!(world.get::<SpawnedEntity>(zombie).is_some());
    }

    #[test]
    fn chained_with_calls_all_land_in_order() {
        let (mut app, _rx) = test_app();
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .with(CustomName::new("Bob"))
            .with(Glowing)
            .with_bundle((Invisible, NoGravity))
            .spawn_in(app.world_mut())
            .id();

        let world = app.world();
        assert_eq!(world.get::<CustomName>(zombie).unwrap().text, "Bob");
        assert!(world.get::<Glowing>(zombie).is_some());
        assert!(world.get::<Invisible>(zombie).is_some());
        assert!(world.get::<NoGravity>(zombie).is_some());
    }

    #[test]
    fn with_lands_on_the_commands_spawn_path_too() {
        let (mut app, _rx) = test_app();
        let entity = app.world_mut().run_system_once(|mut commands: Commands| {
            EntityBuilder::new(EntityKind::Zombie)
                .with(Glowing)
                .spawn(&mut commands)
                .id()
        });
        assert!(app.world().get::<Glowing>(entity.unwrap()).is_some());
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

    fn broadcast_app() -> (App, Receiver<OutgoingPacket>) {
        use bevy_app::PostUpdate;

        use crate::systems::entities::{
            broadcast_entity_movement, update_previous_entity_positions,
        };

        let (mut app, rx) = test_app();
        app.configure_sets(
            PostUpdate,
            (
                VoidSystems::EntityBroadcast,
                VoidSystems::EntityMetadataSync,
                VoidSystems::EntityVisibility,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            (broadcast_entity_movement, update_previous_entity_positions)
                .chain()
                .in_set(VoidSystems::EntityBroadcast),
        );
        (app, rx)
    }

    fn viewer_set(app: &App, entity: Entity) -> HashSet<Entity> {
        app.world()
            .get::<EntityViewers>(entity)
            .unwrap()
            .iter()
            .collect()
    }

    #[test]
    fn hiding_removes_the_entity_from_every_viewer_and_silences_movement() {
        let (mut app, rx) = broadcast_app();
        let _a = player(&mut app, 1, &[(0, 0)]);
        let _b = player(&mut app, 2, &[(0, 0)]);
        let pig = EntityBuilder::new(EntityKind::Pig)
            .at(8.0, 64.0, 8.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, pig);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id), Sent::Spawn(2, id)]);

        app.world_mut().entity_mut(pig).insert(Hidden);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id), Sent::Remove(2, id)]);
        assert!(viewer_set(&app, pig).is_empty());

        app.world_mut().get_mut::<Position>(pig).unwrap().x = 9.0;
        app.world_mut().entity_mut(pig).insert(Glowing);
        app.update();
        assert!(drain(&rx).is_empty());
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn showing_replays_the_spawn_path_with_metadata_and_passengers() {
        let (mut app, rx) = broadcast_app();
        let near = player(&mut app, 1, &[(0, 0)]);
        let _far = player(&mut app, 2, &[(9, 9)]);
        let chicken = EntityBuilder::new(EntityKind::Chicken)
            .at(8.0, 64.0, 8.0)
            .spawn_in(app.world_mut())
            .id();
        let pig = EntityBuilder::new(EntityKind::Pig)
            .at(8.0, 64.0, 8.0)
            .with_bundle((Glowing, Passengers::new([chicken]), Hidden))
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, pig);
        let chicken_id = network_id(&app, chicken);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, chicken_id)]);
        assert!(viewer_set(&app, pig).is_empty());

        app.world_mut().entity_mut(pig).remove::<Hidden>();
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Spawn(1, id),
                Sent::Metadata(1, id),
                Sent::Passengers(1, id)
            ]
        );
        assert_eq!(viewer_set(&app, pig), HashSet::from([near]));

        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn hidden_entity_is_never_spawned_for_a_late_joiner() {
        let (mut app, rx) = test_app();
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .at(1.0, 64.0, 1.0)
            .with(Hidden)
            .spawn_in(app.world_mut())
            .id();
        app.update();
        let joiner = app
            .world_mut()
            .spawn((ClientId(9), PlayerDimension(DimensionId::Overworld)))
            .id();
        app.update();
        app.world_mut().entity_mut(joiner).insert((
            PlayerReady,
            LoadedChunks(HashSet::from([ChunkPos::new(0, 0)])),
        ));
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut()
            .get_mut::<LoadedChunks>(joiner)
            .unwrap()
            .0
            .clear();
        app.update();
        app.world_mut()
            .get_mut::<LoadedChunks>(joiner)
            .unwrap()
            .0
            .insert(ChunkPos::new(0, 0));
        app.update();
        assert!(drain(&rx).is_empty());
        assert!(viewer_set(&app, zombie).is_empty());
    }

    #[test]
    fn hidden_entity_crossing_chunks_and_dimensions_emits_nothing() {
        let (mut app, rx) = broadcast_app();
        let _left = player(&mut app, 1, &[(0, 0)]);
        let _right = player(&mut app, 2, &[(1, 0)]);
        let pig = EntityBuilder::new(EntityKind::Pig)
            .at(8.0, 64.0, 8.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, pig);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);

        app.world_mut().entity_mut(pig).insert(Hidden);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);

        app.world_mut().get_mut::<Position>(pig).unwrap().x = 20.0;
        app.update();
        assert!(drain(&rx).is_empty());
        app.world_mut().get_mut::<EntityDimension>(pig).unwrap().0 = DimensionId::Nether;
        app.update();
        assert!(drain(&rx).is_empty());
        assert!(viewer_set(&app, pig).is_empty());

        app.world_mut().get_mut::<EntityDimension>(pig).unwrap().0 = DimensionId::Overworld;
        app.world_mut().entity_mut(pig).remove::<Hidden>();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(2, id)]);
    }

    #[test]
    fn hide_and_show_in_the_same_tick_is_silent() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);

        app.world_mut().entity_mut(zombie).insert(Hidden);
        app.world_mut().entity_mut(zombie).remove::<Hidden>();
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(viewer_set(&app, zombie).len(), 1);

        app.world_mut().entity_mut(zombie).remove::<Hidden>();
        app.world_mut().entity_mut(zombie).insert(Hidden);
        app.world_mut().entity_mut(zombie).remove::<Hidden>();
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn hidden_events_fire_for_hide_and_shown_events_for_show() {
        #[derive(Resource, Default)]
        struct Log(Vec<&'static str>);

        let (mut app, _rx) = test_app();
        app.init_resource::<Log>()
            .add_observer(|_: On<EntityShownEvent>, mut log: ResMut<Log>| log.0.push("shown"))
            .add_observer(|_: On<EntityHiddenEvent>, mut log: ResMut<Log>| log.0.push("hidden"));
        let _viewer = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        app.update();
        app.world_mut().entity_mut(zombie).insert(Hidden);
        app.update();
        app.world_mut().entity_mut(zombie).remove::<Hidden>();
        app.update();
        assert_eq!(
            app.world().resource::<Log>().0,
            vec!["shown", "hidden", "shown"]
        );
    }

    #[test]
    fn index_reflects_loaded_chunks_and_updates_on_unload() {
        let (mut app, _rx) = test_app();
        let p = player(&mut app, 1, &[(0, 0), (1, 0)]);
        app.update();

        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (0, 0)),
            HashSet::from([p])
        );
        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (1, 0)),
            HashSet::from([p])
        );
        assert!(index_viewers(&app, DimensionId::Overworld, (2, 2)).is_empty());

        app.world_mut()
            .get_mut::<LoadedChunks>(p)
            .unwrap()
            .0
            .remove(&ChunkPos::new(0, 0));
        app.update();

        assert!(index_viewers(&app, DimensionId::Overworld, (0, 0)).is_empty());
        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (1, 0)),
            HashSet::from([p])
        );
    }

    #[test]
    fn moving_entity_viewer_set_follows_chunk_boundaries() {
        let (mut app, rx) = test_app();
        let left = player(&mut app, 1, &[(0, 0)]);
        let right = player(&mut app, 2, &[(1, 0)]);
        let pig = EntityBuilder::new(EntityKind::Pig)
            .at(8.0, 64.0, 8.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, pig);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);
        assert_eq!(
            app.world()
                .get::<EntityViewers>(pig)
                .unwrap()
                .iter()
                .collect::<HashSet<_>>(),
            HashSet::from([left])
        );

        app.world_mut().get_mut::<Position>(pig).unwrap().x = 20.0;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(2, id), Sent::Remove(1, id)]);
        assert_eq!(
            app.world()
                .get::<EntityViewers>(pig)
                .unwrap()
                .iter()
                .collect::<HashSet<_>>(),
            HashSet::from([right])
        );
    }

    #[test]
    fn dimension_change_re_scopes_index_and_viewers() {
        let (mut app, rx) = test_app();
        let traveller = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .at(1.0, 64.0, 1.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);
        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (0, 0)),
            HashSet::from([traveller])
        );

        app.world_mut()
            .get_mut::<PlayerDimension>(traveller)
            .unwrap()
            .0 = DimensionId::Nether;
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);
        assert!(index_viewers(&app, DimensionId::Overworld, (0, 0)).is_empty());
        assert_eq!(
            index_viewers(&app, DimensionId::Nether, (0, 0)),
            HashSet::from([traveller])
        );
    }

    #[test]
    fn leaving_player_is_dropped_from_the_index() {
        let (mut app, _rx) = test_app();
        let leaver = player(&mut app, 1, &[(0, 0)]);
        let stayer = player(&mut app, 2, &[(0, 0)]);
        app.update();
        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (0, 0)),
            HashSet::from([leaver, stayer])
        );

        app.world_mut().despawn(leaver);
        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (0, 0)),
            HashSet::from([stayer])
        );

        app.world_mut().despawn(stayer);
        assert!(index_viewers(&app, DimensionId::Overworld, (0, 0)).is_empty());
    }

    #[test]
    fn player_despawned_before_the_tracker_on_its_first_ready_tick_leaves_no_ghost() {
        use bevy_app::PostUpdate;

        let (mut app, rx) = test_app();
        app.add_systems(
            PostUpdate,
            (|ready: Query<Entity, Added<PlayerReady>>, mut commands: Commands| {
                for player in &ready {
                    commands.entity(player).despawn();
                }
            })
            .after(VoidSystems::ChunkStreaming)
            .before(VoidSystems::EntityVisibility),
        );
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let player = player(&mut app, 1, &[(0, 0)]);
        app.update();
        app.update();

        assert!(app.world().get_entity(player).is_err());
        assert!(
            app.world()
                .resource::<ChunkViewerIndex>()
                .viewers((DimensionId::Overworld, ChunkPos::new(0, 0)))
                .is_none()
        );
        assert!(app.world().get::<EntityViewers>(zombie).unwrap().is_empty());
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn unready_player_gets_remove_and_re_ready_gets_spawn_again() {
        let (mut app, rx) = test_app();
        let viewer = player(&mut app, 1, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);

        app.world_mut().entity_mut(viewer).remove::<PlayerReady>();
        assert!(index_viewers(&app, DimensionId::Overworld, (0, 0)).is_empty());
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);
        assert!(app.world().get::<EntityViewers>(zombie).unwrap().is_empty());

        app.world_mut().entity_mut(viewer).insert(PlayerReady);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);
        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (0, 0)),
            HashSet::from([viewer])
        );
    }

    #[test]
    fn loaded_chunks_inserted_after_player_ready_are_indexed() {
        let (mut app, rx) = test_app();
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        let joiner = app
            .world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                PlayerDimension(DimensionId::Overworld),
            ))
            .id();
        app.update();
        assert!(drain(&rx).is_empty());
        assert!(index_viewers(&app, DimensionId::Overworld, (0, 0)).is_empty());

        app.world_mut()
            .entity_mut(joiner)
            .insert(LoadedChunks(HashSet::from([ChunkPos::new(0, 0)])));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);
        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (0, 0)),
            HashSet::from([joiner])
        );
    }

    #[test]
    fn entity_crossing_into_a_chunk_unloaded_the_same_tick_gets_a_single_remove() {
        let (mut app, rx) = test_app();
        let viewer = player(&mut app, 1, &[(0, 0), (1, 0)]);
        let pig = EntityBuilder::new(EntityKind::Pig)
            .at(8.0, 64.0, 8.0)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, pig);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id)]);

        app.world_mut().get_mut::<Position>(pig).unwrap().x = 20.0;
        app.world_mut()
            .get_mut::<LoadedChunks>(viewer)
            .unwrap()
            .0
            .remove(&ChunkPos::new(1, 0));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Remove(1, id)]);
        assert!(app.world().get::<EntityViewers>(pig).unwrap().is_empty());

        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn player_despawned_after_the_tracker_ran_is_cleaned_next_tick() {
        use bevy_app::Last;

        #[derive(Component)]
        struct Leaving;

        let (mut app, rx) = test_app();
        app.add_systems(
            Last,
            |leaving: Query<Entity, With<Leaving>>, mut commands: Commands| {
                for player in &leaving {
                    commands.entity(player).despawn();
                }
            },
        );
        let leaver = player(&mut app, 1, &[(0, 0)]);
        let stayer = player(&mut app, 2, &[(0, 0)]);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .id();
        let id = network_id(&app, zombie);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Spawn(1, id), Sent::Spawn(2, id)]);

        app.world_mut().entity_mut(leaver).insert(Leaving);
        app.update();
        assert!(app.world().get_entity(leaver).is_err());
        assert_eq!(
            index_viewers(&app, DimensionId::Overworld, (0, 0)),
            HashSet::from([stayer])
        );
        assert!(
            app.world()
                .get::<EntityViewers>(zombie)
                .unwrap()
                .contains(leaver)
        );

        app.update();
        assert!(drain(&rx).is_empty());
        let viewers = app.world().get::<EntityViewers>(zombie).unwrap();
        assert!(!viewers.contains(leaver));
        assert!(viewers.contains(stayer));
        assert_eq!(viewers.len(), 1);
    }
}
