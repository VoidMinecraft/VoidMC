//! Teleport with a loading barrier: insert a [`Teleport`] on a player and the
//! plugin takes control of their position, streams the destination chunks
//! under a [`ChunkSendBudget`], fences the stream with a play Ping/Pong so
//! the client has processed them, sends the position sync, waits for the
//! client's confirmation, then releases control and fires
//! [`PlayerTeleportEvent`]. A client that never answers is released once
//! `timeout_ticks` have elapsed instead of holding the player forever.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::lifecycle::{Insert, Remove};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use voidmc_protocol::clientbound;
use voidmc_protocol::serverbound::Pong;

use crate::components::{
    ChunkSendBudget, ChunkStreamBacklog, EffectiveViewDistance, LoadedChunks, PlayerDimension,
    Position, Rotation, ServerControlledPosition, TeleportState,
};
use crate::events::PlayerTeleportEvent;
use crate::network::PacketEvent;
use crate::players::Players;
use crate::schedule::VoidSystems;
use crate::world::{ChunkPos, DimensionId};

#[derive(Component, Clone, Debug, PartialEq)]
#[require(TeleportProgress)]
pub struct Teleport {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// `None` keeps the player's current look direction.
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    /// `None` stays in the player's current dimension. A change only updates
    /// `PlayerDimension` and restreams chunks: the client keeps its current
    /// dimension environment until a Respawn packet exists.
    pub dimension: Option<DimensionId>,
    /// Chunk packets per tick while the teleport is in flight; `None` leaves
    /// the player's own [`ChunkSendBudget`] (or lack of one) untouched.
    pub chunk_budget: Option<usize>,
    /// Chebyshev radius of chunks around the destination the client must have
    /// processed before the position sync is sent.
    pub preload_radius: i32,
    /// Ticks before an unresponsive client is released with
    /// [`TeleportOutcome::TimedOut`].
    pub timeout_ticks: u32,
}

impl Teleport {
    pub const DEFAULT_CHUNK_BUDGET: usize = 2;
    pub const DEFAULT_PRELOAD_RADIUS: i32 = 2;
    pub const DEFAULT_TIMEOUT_TICKS: u32 = 600;

    pub fn to(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            yaw: None,
            pitch: None,
            dimension: None,
            chunk_budget: Some(Self::DEFAULT_CHUNK_BUDGET),
            preload_radius: Self::DEFAULT_PRELOAD_RADIUS,
            timeout_ticks: Self::DEFAULT_TIMEOUT_TICKS,
        }
    }

    pub fn facing(mut self, yaw: f32, pitch: f32) -> Self {
        self.yaw = Some(yaw);
        self.pitch = Some(pitch);
        self
    }

    /// Switches the server-side dimension used for chunk streaming; the
    /// client's environment is not changed (no Respawn packet yet).
    pub fn in_dimension(mut self, dimension: DimensionId) -> Self {
        self.dimension = Some(dimension);
        self
    }

    pub fn chunk_budget(mut self, per_tick: usize) -> Self {
        self.chunk_budget = Some(per_tick);
        self
    }

    /// Streams destination chunks at the player's usual rate.
    pub fn unthrottled(mut self) -> Self {
        self.chunk_budget = None;
        self
    }

    pub fn preload_radius(mut self, radius: i32) -> Self {
        self.preload_radius = radius;
        self
    }

    pub fn timeout_ticks(mut self, ticks: u32) -> Self {
        self.timeout_ticks = ticks;
        self
    }

    fn destination(&self) -> Position {
        Position {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeleportOutcome {
    /// The client confirmed the position sync with the destination loaded.
    Confirmed,
    /// `timeout_ticks` elapsed; the sync was still sent and control released.
    TimedOut,
    /// The [`Teleport`] component was removed before the client confirmed.
    Cancelled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Streaming,
    Fence(i32),
    Sync,
}

#[derive(Clone, Copy)]
struct Restore {
    had_control: bool,
    budget: Option<ChunkSendBudget>,
}

#[derive(Component, Default)]
pub struct TeleportProgress {
    stage: Stage,
    elapsed: u32,
    restore: Option<Restore>,
    outcome: Option<TeleportOutcome>,
}

pub struct TeleportPlugin;

impl Plugin for TeleportPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(begin_teleport)
            .add_observer(end_teleport)
            .add_observer(fence_acknowledged)
            .configure_sets(
                Update,
                VoidSystems::TeleportBarrier.after(VoidSystems::CommandDrain),
            )
            .add_systems(
                Update,
                advance_teleports.in_set(VoidSystems::TeleportBarrier),
            );
    }
}

fn begin_teleport(
    event: On<Insert, Teleport>,
    mut player_and_players: ParamSet<(
        Query<(
            &Teleport,
            &mut TeleportProgress,
            &mut Position,
            Option<&mut Rotation>,
            Option<&mut PlayerDimension>,
            Option<&mut LoadedChunks>,
            Has<ServerControlledPosition>,
            Option<&ChunkSendBudget>,
        )>,
        Players,
    )>,
    mut commands: Commands,
) {
    let entity = event.entity;
    let mut unload = Vec::new();
    {
        let mut query = player_and_players.p0();
        let Ok((
            teleport,
            mut progress,
            mut position,
            rotation,
            dimension,
            loaded,
            has_control,
            budget,
        )) = query.get_mut(entity)
        else {
            return;
        };
        progress.stage = Stage::Streaming;
        progress.elapsed = 0;
        progress.outcome = None;
        if progress.restore.is_none() {
            progress.restore = Some(Restore {
                had_control: has_control,
                budget: budget.copied(),
            });
        }

        *position = teleport.destination();
        if let (Some(mut rotation), Some(yaw), Some(pitch)) =
            (rotation, teleport.yaw, teleport.pitch)
        {
            *rotation = Rotation { yaw, pitch };
        }
        if let (Some(mut dimension), Some(target)) = (dimension, teleport.dimension)
            && dimension.0 != target
        {
            dimension.0 = target;
            if let Some(mut loaded) = loaded {
                unload.extend(loaded.0.drain());
            }
            commands
                .entity(entity)
                .insert(ChunkStreamBacklog::default());
        }

        let mut entity_commands = commands.entity(entity);
        entity_commands.insert(ServerControlledPosition);
        if let Some(per_tick) = teleport.chunk_budget {
            entity_commands.insert(ChunkSendBudget(per_tick));
        }
    }
    let players = player_and_players.p1();
    for pos in unload {
        players.send(
            entity,
            clientbound::UnloadChunk {
                chunk_x: pos.x,
                chunk_z: pos.z,
            },
        );
    }
}

fn advance_teleports(
    players: Players,
    mut in_flight: Query<(
        Entity,
        &Teleport,
        &mut TeleportProgress,
        &mut TeleportState,
        &LoadedChunks,
        Option<&EffectiveViewDistance>,
        Option<&Rotation>,
    )>,
    mut commands: Commands,
) {
    for (entity, teleport, mut progress, mut state, loaded, view_distance, rotation) in
        in_flight.iter_mut()
    {
        progress.elapsed = progress.elapsed.saturating_add(1);
        if progress.elapsed > teleport.timeout_ticks {
            if progress.stage != Stage::Sync {
                send_sync(&players, entity, teleport, &mut state, rotation);
            }
            progress.outcome = Some(TeleportOutcome::TimedOut);
            commands.entity(entity).remove::<Teleport>();
            continue;
        }
        match progress.stage {
            Stage::Streaming => {
                let radius = view_distance
                    .map_or(teleport.preload_radius, |vd| {
                        teleport.preload_radius.min(vd.0)
                    })
                    .max(0);
                let center = ChunkPos::from_block(teleport.x, teleport.z);
                if !center
                    .chunks_in_radius(radius)
                    .iter()
                    .all(|p| loaded.0.contains(p))
                {
                    continue;
                }
                let id = state.next_id;
                state.next_id = state.next_id.wrapping_add(1);
                progress.stage = Stage::Fence(id);
                players.send(entity, clientbound::Ping { id });
            }
            Stage::Fence(_) => {}
            Stage::Sync => {
                if state.pending_id.is_none() {
                    progress.outcome = Some(TeleportOutcome::Confirmed);
                    commands.entity(entity).remove::<Teleport>();
                }
            }
        }
    }
}

fn send_sync(
    players: &Players,
    entity: Entity,
    teleport: &Teleport,
    state: &mut TeleportState,
    rotation: Option<&Rotation>,
) {
    players.send(entity, sync_packet(teleport, state, rotation));
}

fn sync_packet(
    teleport: &Teleport,
    state: &mut TeleportState,
    rotation: Option<&Rotation>,
) -> clientbound::SynchronizePlayerPosition {
    let id = state.next_id;
    state.next_id = state.next_id.wrapping_add(1);
    state.pending_id = Some(id);
    let current = rotation.copied().unwrap_or_default();
    clientbound::SynchronizePlayerPosition {
        teleport_id: id,
        x: teleport.x,
        y: teleport.y,
        z: teleport.z,
        vx: 0.0,
        vy: 0.0,
        vz: 0.0,
        yaw: teleport.yaw.unwrap_or(current.yaw),
        pitch: teleport.pitch.unwrap_or(current.pitch),
        flags: clientbound::TeleportFlags::empty(),
    }
}

fn fence_acknowledged(
    event: On<PacketEvent<Pong>>,
    mut in_flight_and_players: ParamSet<(
        Query<(
            &Teleport,
            &mut TeleportProgress,
            &mut TeleportState,
            Option<&Rotation>,
        )>,
        Players,
    )>,
) {
    let entity = event.entity;
    let sync = {
        let mut in_flight = in_flight_and_players.p0();
        let Ok((teleport, mut progress, mut state, rotation)) = in_flight.get_mut(entity) else {
            return;
        };
        if progress.stage != Stage::Fence(event.packet.id) {
            return;
        }
        progress.stage = Stage::Sync;
        sync_packet(teleport, &mut state, rotation)
    };
    in_flight_and_players.p1().send(entity, sync);
}

fn end_teleport(
    event: On<Remove, Teleport>,
    mut in_flight: Query<&mut TeleportProgress>,
    mut commands: Commands,
) {
    let entity = event.entity;
    let Ok(mut progress) = in_flight.get_mut(entity) else {
        return;
    };
    let outcome = progress
        .outcome
        .take()
        .unwrap_or(TeleportOutcome::Cancelled);
    let restore = progress.restore.take();
    let mut entity_commands = commands.entity(entity);
    entity_commands.try_remove::<TeleportProgress>();
    if let Some(restore) = restore {
        if !restore.had_control {
            entity_commands.try_remove::<ServerControlledPosition>();
        }
        match restore.budget {
            Some(budget) => {
                entity_commands.try_insert(budget);
            }
            None => {
                entity_commands.try_remove::<ChunkSendBudget>();
            }
        }
    }
    commands.queue(move |world: &mut World| {
        if world.get_entity(entity).is_ok() {
            world.trigger(PlayerTeleportEvent { entity, outcome });
        }
    });
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};
    use voidmc_protocol::serverbound::ConfirmTeleportation;

    use super::*;
    use crate::components::{ClientId, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::plugins::movement::MovementPlugin;

    #[derive(Resource, Default)]
    struct Outcomes(Vec<(Entity, TeleportOutcome)>);

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
        .init_resource::<Outcomes>()
        .add_plugins((TeleportPlugin, MovementPlugin))
        .add_observer(
            |event: On<PlayerTeleportEvent>, mut outcomes: ResMut<Outcomes>| {
                outcomes.0.push((event.entity, event.outcome));
            },
        );
        (app, outgoing_rx)
    }

    fn player(app: &mut App, loaded: impl IntoIterator<Item = ChunkPos>) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                Position::default(),
                Rotation {
                    yaw: 10.0,
                    pitch: 20.0,
                },
                TeleportState {
                    next_id: 1,
                    pending_id: None,
                },
                EffectiveViewDistance(8),
                LoadedChunks(loaded.into_iter().collect()),
                PlayerDimension(DimensionId::Overworld),
            ))
            .id()
    }

    fn pings(receiver: &Receiver<OutgoingPacket>) -> Vec<i32> {
        receiver
            .drain()
            .filter_map(|p| match p.packet {
                ClientboundPacket::Play(PlayPacket::Ping(ping)) => Some(ping.id),
                _ => None,
            })
            .collect()
    }

    fn syncs(receiver: &Receiver<OutgoingPacket>) -> Vec<clientbound::SynchronizePlayerPosition> {
        receiver
            .drain()
            .filter_map(|p| match p.packet {
                ClientboundPacket::Play(PlayPacket::SynchronizePlayerPosition(sync)) => Some(sync),
                _ => None,
            })
            .collect()
    }

    fn pong(app: &mut App, entity: Entity, id: i32) {
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: Pong { id },
        });
        app.world_mut().flush();
    }

    fn confirm(app: &mut App, entity: Entity, teleport_id: i32) {
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: ConfirmTeleportation { teleport_id },
        });
        app.world_mut().flush();
    }

    #[test]
    fn barrier_waits_for_chunks_then_pong_then_confirmation_before_releasing() {
        let (mut app, receiver) = test_app();
        let target = ChunkPos::new(10, 10);
        let mut around = target.chunks_in_radius(2);
        let missing = around.pop().unwrap();
        let entity = player(&mut app, around);

        app.world_mut()
            .entity_mut(entity)
            .insert(Teleport::to(168.0, 70.0, 168.0).facing(90.0, 0.0));
        app.update();
        let world = app.world();
        assert!(world.get::<ServerControlledPosition>(entity).is_some());
        assert_eq!(
            world.get::<ChunkSendBudget>(entity),
            Some(&ChunkSendBudget(2))
        );
        assert_eq!(world.get::<Position>(entity).unwrap().x, 168.0);
        assert_eq!(world.get::<Rotation>(entity).unwrap().yaw, 90.0);
        assert!(pings(&receiver).is_empty());

        app.world_mut()
            .get_mut::<LoadedChunks>(entity)
            .unwrap()
            .0
            .insert(missing);
        app.update();
        let ping = pings(&receiver);
        assert_eq!(ping.len(), 1);
        app.update();
        assert!(syncs(&receiver).is_empty());

        pong(&mut app, entity, ping[0] + 1);
        app.update();
        assert!(syncs(&receiver).is_empty());

        pong(&mut app, entity, ping[0]);
        let sync = syncs(&receiver);
        assert_eq!(sync.len(), 1);
        assert_eq!((sync[0].x, sync[0].y, sync[0].z), (168.0, 70.0, 168.0));
        assert_eq!((sync[0].yaw, sync[0].pitch), (90.0, 0.0));
        assert_eq!(
            app.world().get::<TeleportState>(entity).unwrap().pending_id,
            Some(sync[0].teleport_id)
        );

        confirm(&mut app, entity, sync[0].teleport_id + 1);
        app.update();
        assert!(app.world().get::<Teleport>(entity).is_some());
        assert!(
            app.world()
                .get::<ServerControlledPosition>(entity)
                .is_some()
        );

        confirm(&mut app, entity, sync[0].teleport_id);
        app.update();
        let world = app.world();
        assert!(world.get::<Teleport>(entity).is_none());
        assert!(world.get::<TeleportProgress>(entity).is_none());
        assert!(world.get::<ServerControlledPosition>(entity).is_none());
        assert!(world.get::<ChunkSendBudget>(entity).is_none());
        assert_eq!(
            world.resource::<Outcomes>().0,
            vec![(entity, TeleportOutcome::Confirmed)]
        );
        assert!(syncs(&receiver).is_empty());
    }

    #[test]
    fn unresponsive_client_is_released_after_the_timeout_with_the_sync_still_sent() {
        let (mut app, receiver) = test_app();
        let entity = player(&mut app, []);
        app.world_mut()
            .entity_mut(entity)
            .insert(Teleport::to(5.0, 5.0, 5.0).timeout_ticks(3));
        for _ in 0..3 {
            app.update();
            assert!(app.world().get::<Teleport>(entity).is_some());
        }
        app.update();
        let world = app.world();
        assert!(world.get::<Teleport>(entity).is_none());
        assert!(world.get::<ServerControlledPosition>(entity).is_none());
        assert_eq!(
            world.resource::<Outcomes>().0,
            vec![(entity, TeleportOutcome::TimedOut)]
        );
        let sync = syncs(&receiver);
        assert_eq!(sync.len(), 1);
        assert_eq!(sync[0].x, 5.0);
        assert_eq!(sync[0].yaw, 10.0);
        assert_eq!(
            app.world().get::<TeleportState>(entity).unwrap().pending_id,
            Some(sync[0].teleport_id)
        );
    }

    #[test]
    fn cancelling_restores_prior_control_and_budget() {
        let (mut app, _receiver) = test_app();
        let entity = player(&mut app, []);
        app.world_mut()
            .entity_mut(entity)
            .insert((ServerControlledPosition, ChunkSendBudget(7)));
        app.world_mut()
            .entity_mut(entity)
            .insert(Teleport::to(1.0, 2.0, 3.0).chunk_budget(1));
        app.update();
        assert_eq!(
            app.world().get::<ChunkSendBudget>(entity),
            Some(&ChunkSendBudget(1))
        );

        app.world_mut().entity_mut(entity).remove::<Teleport>();
        app.update();
        let world = app.world();
        assert!(world.get::<ServerControlledPosition>(entity).is_some());
        assert_eq!(
            world.get::<ChunkSendBudget>(entity),
            Some(&ChunkSendBudget(7))
        );
        assert!(world.get::<TeleportProgress>(entity).is_none());
        assert_eq!(
            world.resource::<Outcomes>().0,
            vec![(entity, TeleportOutcome::Cancelled)]
        );
    }

    #[test]
    fn despawning_mid_flight_delivers_no_outcome_and_observers_stay_safe() {
        #[derive(Component)]
        struct Marker;

        let (mut app, _receiver) = test_app();
        app.add_observer(|event: On<PlayerTeleportEvent>, mut commands: Commands| {
            commands.entity(event.entity).insert(Marker);
        });
        let entity = player(&mut app, []);
        app.world_mut()
            .entity_mut(entity)
            .insert(Teleport::to(1.0, 2.0, 3.0));
        app.update();
        assert!(app.world().get::<TeleportProgress>(entity).is_some());

        app.world_mut().despawn(entity);
        app.update();
        let world = app.world_mut();
        assert!(world.get_entity(entity).is_err());
        assert!(world.resource::<Outcomes>().0.is_empty());
        assert_eq!(world.query::<&Marker>().iter(world).count(), 0);
        assert_eq!(world.query::<&ChunkSendBudget>().iter(world).count(), 0);
        assert_eq!(
            world
                .query::<&ServerControlledPosition>()
                .iter(world)
                .count(),
            0
        );
    }

    #[test]
    fn changing_dimension_unloads_every_chunk_and_restreams() {
        let (mut app, receiver) = test_app();
        let loaded: HashSet<ChunkPos> = ChunkPos::new(0, 0)
            .chunks_in_radius(1)
            .into_iter()
            .collect();
        let entity = player(&mut app, loaded.clone());
        app.world_mut()
            .entity_mut(entity)
            .insert(Teleport::to(0.0, 64.0, 0.0).in_dimension(DimensionId::Nether));
        app.update();
        let unloaded: HashSet<ChunkPos> = receiver
            .drain()
            .filter_map(|p| match p.packet {
                ClientboundPacket::Play(PlayPacket::UnloadChunk(u)) => {
                    Some(ChunkPos::new(u.chunk_x, u.chunk_z))
                }
                _ => None,
            })
            .collect();
        assert_eq!(unloaded, loaded);
        let world = app.world();
        assert!(world.get::<LoadedChunks>(entity).unwrap().0.is_empty());
        assert_eq!(
            world.get::<PlayerDimension>(entity).unwrap().0,
            DimensionId::Nether
        );
        assert!(world.get::<ChunkStreamBacklog>(entity).is_some());
    }
}
