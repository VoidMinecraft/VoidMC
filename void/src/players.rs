//! Sending clientbound packets to players.
//!
//! Two entry points share one API:
//!
//! - [`Players`] — a `SystemParam` for systems and observers.
//! - [`WorldPlayers`] — the same API over a `&World`, for command handlers,
//!   item behaviours and exclusive drain systems.
//!
//! Players are addressed by their `Entity`. Packets are accepted as
//! `impl Into<ClientboundPacket>`, so a bare packet struct works:
//!
//! ```ignore
//! fn my_system(players: Players, targets: Query<Entity, With<PlayerReady>>) {
//!     players.broadcast(SystemChat { content, overlay: false });
//!     players.ready().except(me).send(packet);
//!     players.ready().seeing_chunk(DimensionId::Overworld, pos).send(packet);
//! }
//! ```
//!
//! [`Players`] reads `ClientId`, `PlayerReady`, `PlayerDimension` and
//! `LoadedChunks`. A system that also mutates one of those must wrap the two
//! in a `ParamSet` (see `stream_chunks`), or Bevy rejects the system (B0001).
//!
//! Delivery failures are never silent: a missing entity logs at `debug`, an
//! entity without a [`ClientId`] logs at `warn`, and a closed outgoing channel
//! (the network thread is gone) logs one `error` for the whole process.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryEntityError;
use bevy_ecs::system::SystemParam;
use flume::Sender;
use voidmc_protocol::clientbound::ClientboundPacket;

use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
use crate::network::{NetworkChannels, OutgoingPacket};
use crate::world::{ChunkPos, DimensionId};

/// Set once the outgoing channel is observed closed, so the error is logged
/// exactly once instead of once per packet.
static CHANNEL_CLOSED_LOGGED: AtomicBool = AtomicBool::new(false);

/// One candidate recipient of a broadcast. Exposed to [`Recipients::filter`]
/// predicates.
#[derive(Clone, Copy)]
pub struct Recipient<'a> {
    entity: Entity,
    client_id: u32,
    dimension: Option<DimensionId>,
    loaded_chunks: Option<&'a HashSet<ChunkPos>>,
}

impl Recipient<'_> {
    pub fn entity(&self) -> Entity {
        self.entity
    }

    pub fn client_id(&self) -> u32 {
        self.client_id
    }

    /// The player's dimension, if known (`PlayerDimension` present).
    pub fn dimension(&self) -> Option<DimensionId> {
        self.dimension
    }

    /// Whether the client currently has `chunk` loaded in `dimension`.
    pub fn sees_chunk(&self, dimension: DimensionId, chunk: ChunkPos) -> bool {
        self.dimension == Some(dimension)
            && self
                .loaded_chunks
                .is_some_and(|loaded| loaded.contains(&chunk))
    }
}

/// A snapshot of recipients that composes filters and ends in [`send`].
///
/// [`send`]: Recipients::send
pub struct Recipients<'a> {
    sender: &'a Sender<OutgoingPacket>,
    targets: Vec<Recipient<'a>>,
}

impl<'a> Recipients<'a> {
    /// Drops `entity` from the recipients.
    pub fn except(mut self, entity: Entity) -> Self {
        self.targets.retain(|r| r.entity != entity);
        self
    }

    /// Keeps only players in `dimension`.
    pub fn in_dimension(mut self, dimension: DimensionId) -> Self {
        self.targets.retain(|r| r.dimension == Some(dimension));
        self
    }

    /// Keeps only players whose dimension matches `dimension`; `None` means
    /// "visible from every dimension" and keeps everyone. Mirrors the
    /// `Option<&EntityDimension>` convention on spawned entities.
    pub fn visible_from(self, dimension: Option<DimensionId>) -> Self {
        match dimension {
            Some(dimension) => self.in_dimension(dimension),
            None => self,
        }
    }

    /// Keeps only players that currently have `chunk` loaded in `dimension`.
    pub fn seeing_chunk(mut self, dimension: DimensionId, chunk: ChunkPos) -> Self {
        self.targets.retain(|r| r.sees_chunk(dimension, chunk));
        self
    }

    /// Keeps only players for which `predicate` returns `true`.
    pub fn filter(mut self, mut predicate: impl FnMut(&Recipient<'a>) -> bool) -> Self {
        self.targets.retain(|r| predicate(r));
        self
    }

    /// The remaining recipients' entities.
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.targets.iter().map(|r| r.entity)
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Sends one packet to every remaining recipient (cloned per recipient).
    pub fn send(&self, packet: impl Into<ClientboundPacket>) {
        let packet = packet.into();
        let Some((last, rest)) = self.targets.split_last() else {
            return;
        };
        for recipient in rest {
            deliver(
                self.sender,
                recipient.entity,
                recipient.client_id,
                packet.clone(),
            );
        }
        deliver(self.sender, last.entity, last.client_id, packet);
    }
}

/// Hands one packet to the network thread. The only place that touches the
/// channel, so a failure is logged exactly once.
fn deliver(
    sender: &Sender<OutgoingPacket>,
    entity: Entity,
    client_id: u32,
    packet: ClientboundPacket,
) {
    if sender.send(OutgoingPacket { client_id, packet }).is_err()
        && !CHANNEL_CLOSED_LOGGED.swap(true, Ordering::Relaxed)
    {
        tracing::error!(
            ?entity,
            client_id,
            "Outgoing packet channel is closed; the network thread is gone and packets are being dropped"
        );
    }
}

/// Resolves `entity` to a client id, logging when it cannot be a recipient.
fn resolve_client(world: &World, entity: Entity) -> Option<u32> {
    match world.get_entity(entity) {
        Ok(entity_ref) => match entity_ref.get::<ClientId>() {
            Some(client_id) => Some(client_id.0),
            None => {
                tracing::warn!(?entity, "Cannot send packet: entity has no ClientId");
                None
            }
        },
        Err(_) => {
            tracing::debug!(?entity, "Cannot send packet: entity no longer exists");
            None
        }
    }
}

/// Packet sending from systems and observers.
#[derive(SystemParam)]
pub struct Players<'w, 's> {
    channels: Res<'w, NetworkChannels>,
    clients: Query<'w, 's, &'static ClientId>,
    ready: Query<
        'w,
        's,
        (
            Entity,
            &'static ClientId,
            Option<&'static PlayerDimension>,
            Option<&'static LoadedChunks>,
        ),
        With<PlayerReady>,
    >,
}

impl Players<'_, '_> {
    /// Sends one packet to one client. Works for any connected client entity,
    /// including those still in the status/login/configuration phases.
    pub fn send(&self, entity: Entity, packet: impl Into<ClientboundPacket>) {
        match self.clients.get(entity) {
            Ok(client_id) => deliver(&self.channels.outgoing, entity, client_id.0, packet.into()),
            Err(QueryEntityError::NotSpawned(_)) => {
                tracing::debug!(?entity, "Cannot send packet: entity no longer exists");
            }
            Err(_) => {
                tracing::warn!(?entity, "Cannot send packet: entity has no ClientId");
            }
        }
    }

    /// Sends one packet to each of `entities` (cloned per recipient).
    pub fn send_to(
        &self,
        entities: impl IntoIterator<Item = Entity>,
        packet: impl Into<ClientboundPacket>,
    ) {
        let packet = packet.into();
        for entity in entities {
            self.send(entity, packet.clone());
        }
    }

    /// Every ready player, as a filterable set.
    pub fn ready(&self) -> Recipients<'_> {
        Recipients {
            sender: &self.channels.outgoing,
            targets: self
                .ready
                .iter()
                .map(|(entity, client_id, dimension, loaded)| Recipient {
                    entity,
                    client_id: client_id.0,
                    dimension: dimension.map(|d| d.0),
                    loaded_chunks: loaded.map(|l| &l.0),
                })
                .collect(),
        }
    }

    /// Sends one packet to every ready player.
    pub fn broadcast(&self, packet: impl Into<ClientboundPacket>) {
        self.ready().send(packet);
    }

    /// Sends one packet to every ready player except `entity`.
    pub fn broadcast_except(&self, entity: Entity, packet: impl Into<ClientboundPacket>) {
        self.ready().except(entity).send(packet);
    }

    /// Sends one packet to every ready player that has `chunk` loaded.
    pub fn broadcast_chunk(
        &self,
        dimension: DimensionId,
        chunk: ChunkPos,
        packet: impl Into<ClientboundPacket>,
    ) {
        self.ready().seeing_chunk(dimension, chunk).send(packet);
    }
}

/// Packet sending from exclusive-world code (`&World` is enough).
pub struct WorldPlayers<'w> {
    world: &'w World,
}

impl<'w> WorldPlayers<'w> {
    pub fn new(world: &'w World) -> Self {
        Self { world }
    }

    fn sender(&self) -> &'w Sender<OutgoingPacket> {
        &self.world.resource::<NetworkChannels>().outgoing
    }

    /// Sends one packet to one client. Works for any connected client entity,
    /// including those still in the status/login/configuration phases.
    pub fn send(&self, entity: Entity, packet: impl Into<ClientboundPacket>) {
        if let Some(client_id) = resolve_client(self.world, entity) {
            deliver(self.sender(), entity, client_id, packet.into());
        }
    }

    /// Sends one packet to each of `entities` (cloned per recipient).
    pub fn send_to(
        &self,
        entities: impl IntoIterator<Item = Entity>,
        packet: impl Into<ClientboundPacket>,
    ) {
        let packet = packet.into();
        for entity in entities {
            self.send(entity, packet.clone());
        }
    }

    /// Every ready player, as a filterable set.
    pub fn ready(&self) -> Recipients<'w> {
        let world = self.world;
        // `try_query_filtered` only fails when `ClientId` or `PlayerReady` was
        // never registered, in which case no player can be ready.
        let targets = world
            .try_query_filtered::<(Entity, &ClientId), With<PlayerReady>>()
            .map(|mut query| {
                query
                    .iter(world)
                    .map(|(entity, client_id)| Recipient {
                        entity,
                        client_id: client_id.0,
                        dimension: world.get::<PlayerDimension>(entity).map(|d| d.0),
                        loaded_chunks: world.get::<LoadedChunks>(entity).map(|l| &l.0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Recipients {
            sender: self.sender(),
            targets,
        }
    }

    /// Sends one packet to every ready player.
    pub fn broadcast(&self, packet: impl Into<ClientboundPacket>) {
        self.ready().send(packet);
    }

    /// Sends one packet to every ready player except `entity`.
    pub fn broadcast_except(&self, entity: Entity, packet: impl Into<ClientboundPacket>) {
        self.ready().except(entity).send(packet);
    }

    /// Sends one packet to every ready player that has `chunk` loaded.
    pub fn broadcast_chunk(
        &self,
        dimension: DimensionId,
        chunk: ChunkPos,
        packet: impl Into<ClientboundPacket>,
    ) {
        self.ready().seeing_chunk(dimension, chunk).send(packet);
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, Update};
    use flume::Receiver;
    use voidmc_protocol::clientbound::{self, KeepAlive};

    use super::*;
    use crate::network::IncomingPacket;

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        });
        // Keep the unused ends alive for the app's lifetime.
        app.insert_non_send_resource((_incoming_tx, _disconnect_tx, _kick_rx));
        (app, outgoing_rx)
    }

    fn keep_alive(id: i64) -> KeepAlive {
        KeepAlive { keep_alive_id: id }
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, i64)> {
        rx.try_iter()
            .map(|out| {
                let clientbound::ClientboundPacket::Play(clientbound::PlayPacket::KeepAlive(
                    packet,
                )) = out.packet
                else {
                    panic!("expected keep alive");
                };
                (out.client_id, packet.keep_alive_id)
            })
            .collect()
    }

    fn loaded(chunks: &[(i32, i32)]) -> LoadedChunks {
        LoadedChunks(chunks.iter().map(|&(x, z)| ChunkPos::new(x, z)).collect())
    }

    #[test]
    fn send_reaches_one_client_ready_or_not() {
        let (mut app, rx) = test_app();
        let ready = app.world_mut().spawn((ClientId(1), PlayerReady)).id();
        let joining = app.world_mut().spawn(ClientId(2)).id();

        app.add_systems(Update, move |players: Players| {
            players.send(ready, keep_alive(10));
            players.send(joining, keep_alive(20));
        });
        app.update();

        assert_eq!(drain(&rx), vec![(1, 10), (2, 20)]);
    }

    #[test]
    fn broadcast_reaches_only_ready_players() {
        let (mut app, rx) = test_app();
        app.world_mut().spawn((ClientId(1), PlayerReady));
        app.world_mut().spawn((ClientId(2), PlayerReady));
        app.world_mut().spawn(ClientId(3));

        app.add_systems(Update, |players: Players| players.broadcast(keep_alive(1)));
        app.update();

        let mut sent = drain(&rx);
        sent.sort();
        assert_eq!(sent, vec![(1, 1), (2, 1)]);
    }

    #[test]
    fn except_and_filter_exclude_recipients() {
        let (mut app, rx) = test_app();
        let me = app.world_mut().spawn((ClientId(1), PlayerReady)).id();
        app.world_mut().spawn((ClientId(2), PlayerReady));
        app.world_mut().spawn((ClientId(3), PlayerReady));

        app.add_systems(Update, move |players: Players| {
            players.broadcast_except(me, keep_alive(1));
            players
                .ready()
                .filter(|r| r.client_id() == 3)
                .send(keep_alive(2));
        });
        app.update();

        let mut sent = drain(&rx);
        sent.sort();
        assert_eq!(sent, vec![(2, 1), (3, 1), (3, 2)]);
    }

    #[test]
    fn dimension_and_chunk_filters() {
        let (mut app, rx) = test_app();
        let chunk = ChunkPos::new(3, -2);
        app.world_mut().spawn((
            ClientId(1),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            loaded(&[(3, -2)]),
        ));
        app.world_mut().spawn((
            ClientId(2),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            loaded(&[(0, 0)]),
        ));
        app.world_mut().spawn((
            ClientId(3),
            PlayerReady,
            PlayerDimension(DimensionId::Nether),
            loaded(&[(3, -2)]),
        ));
        app.world_mut().spawn((ClientId(4), PlayerReady));

        app.add_systems(Update, move |players: Players| {
            players.broadcast_chunk(DimensionId::Overworld, chunk, keep_alive(1));
            players
                .ready()
                .in_dimension(DimensionId::Nether)
                .send(keep_alive(2));
            players.ready().visible_from(None).send(keep_alive(3));
        });
        app.update();

        let mut sent = drain(&rx);
        sent.sort();
        assert_eq!(sent, vec![(1, 1), (1, 3), (2, 3), (3, 2), (3, 3), (4, 3)]);
    }

    #[test]
    fn missing_or_non_client_entities_send_nothing() {
        let (mut app, rx) = test_app();
        let gone = app.world_mut().spawn(ClientId(9)).id();
        app.world_mut().despawn(gone);
        let not_a_client = app.world_mut().spawn(PlayerReady).id();

        app.add_systems(Update, move |players: Players| {
            players.send(gone, keep_alive(1));
            players.send(not_a_client, keep_alive(2));
            // A ready player without ClientId cannot be reached by broadcast either.
            players.broadcast(keep_alive(3));
        });
        app.update();

        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn world_players_matches_system_param() {
        let (mut app, rx) = test_app();
        let me = app
            .world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                PlayerDimension(DimensionId::Overworld),
                loaded(&[(0, 0)]),
            ))
            .id();
        app.world_mut().spawn((
            ClientId(2),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            loaded(&[(0, 0)]),
        ));
        let joining = app.world_mut().spawn(ClientId(3)).id();
        let gone = app.world_mut().spawn(ClientId(4)).id();
        app.world_mut().despawn(gone);

        let players = WorldPlayers::new(app.world());
        players.send(joining, keep_alive(1));
        players.send(gone, keep_alive(2));
        players.broadcast_except(me, keep_alive(3));
        players.broadcast_chunk(DimensionId::Overworld, ChunkPos::new(0, 0), keep_alive(4));
        assert_eq!(players.ready().len(), 2);

        let mut sent = drain(&rx);
        sent.sort();
        assert_eq!(sent, vec![(1, 4), (2, 3), (2, 4), (3, 1)]);
    }

    #[test]
    fn world_players_broadcast_on_fresh_world_is_empty() {
        let (app, rx) = test_app();
        WorldPlayers::new(app.world()).broadcast(keep_alive(1));
        assert!(drain(&rx).is_empty());
    }
}
