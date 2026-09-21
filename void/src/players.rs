//! Entity-addressed packet sending: [`Players`] from systems, [`WorldPlayers`]
//! from `&World`. `Players` reads `ClientId`, `PlayerReady`, `PlayerDimension`
//! and `LoadedChunks`; a system mutating one of those must hold both in a
//! `ParamSet` (B0001).

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryEntityError;
use bevy_ecs::system::SystemParam;
use flume::Sender;
use voidmc_protocol::clientbound::ClientboundPacket;

use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
use crate::network::{NetworkChannels, OutgoingPacket};
use crate::world::{ChunkPos, DimensionId};

static CHANNEL_CLOSED_LOGGED: AtomicBool = AtomicBool::new(false);

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

    pub fn dimension(&self) -> Option<DimensionId> {
        self.dimension
    }

    /// `None` means visible from every dimension.
    pub fn visible_from(&self, dimension: Option<DimensionId>) -> bool {
        dimension.is_none_or(|dimension| self.dimension == Some(dimension))
    }

    pub fn sees_chunk(&self, dimension: DimensionId, chunk: ChunkPos) -> bool {
        self.dimension == Some(dimension)
            && self
                .loaded_chunks
                .is_some_and(|loaded| loaded.contains(&chunk))
    }
}

/// A snapshot of ready players; filters narrow it, [`Recipients::send`] delivers.
pub struct Recipients<'a> {
    sender: &'a Sender<OutgoingPacket>,
    targets: Vec<Recipient<'a>>,
}

impl<'a> Recipients<'a> {
    pub fn except(mut self, entity: Entity) -> Self {
        self.targets.retain(|r| r.entity != entity);
        self
    }

    pub fn in_dimension(mut self, dimension: DimensionId) -> Self {
        self.targets.retain(|r| r.dimension == Some(dimension));
        self
    }

    /// `None` keeps everyone, matching `Option<&EntityDimension>` on spawned entities.
    pub fn visible_from(mut self, dimension: Option<DimensionId>) -> Self {
        self.targets.retain(|r| r.visible_from(dimension));
        self
    }

    pub fn seeing_chunk(mut self, dimension: DimensionId, chunk: ChunkPos) -> Self {
        self.targets.retain(|r| r.sees_chunk(dimension, chunk));
        self
    }

    pub fn filter(mut self, mut predicate: impl FnMut(&Recipient<'a>) -> bool) -> Self {
        self.targets.retain(|r| predicate(r));
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = &Recipient<'a>> + '_ {
        self.targets.iter()
    }

    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.targets.iter().map(|r| r.entity)
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    pub fn send(&self, packet: impl Into<ClientboundPacket>) {
        self.send_where(|_| true, packet);
    }

    /// Non-allocating alternatives to `except`/`filter` for use inside loops.
    pub fn send_except(&self, entity: Entity, packet: impl Into<ClientboundPacket>) {
        self.send_where(|r| r.entity != entity, packet);
    }

    pub fn send_where(
        &self,
        mut predicate: impl FnMut(&Recipient<'a>) -> bool,
        packet: impl Into<ClientboundPacket>,
    ) {
        let packet = packet.into();
        let mut pending: Option<&Recipient<'a>> = None;
        for recipient in self.targets.iter().filter(|r| predicate(r)) {
            if let Some(previous) = pending.replace(recipient) {
                deliver(
                    self.sender,
                    previous.entity,
                    previous.client_id,
                    packet.clone(),
                );
            }
        }
        if let Some(last) = pending {
            deliver(self.sender, last.entity, last.client_id, packet);
        }
    }
}

/// Which ready players a feature addresses; [`Audience::resolve`] narrows a
/// [`Recipients`] snapshot, [`Audience::includes`] tests one [`Recipient`].
#[derive(Clone, Default)]
pub enum Audience {
    #[default]
    All,
    InDimension(DimensionId),
    Explicit(HashSet<Entity>),
    Custom(Arc<dyn Fn(&Recipient) -> bool + Send + Sync>),
}

impl Audience {
    pub fn explicit(players: impl IntoIterator<Item = Entity>) -> Self {
        Audience::Explicit(players.into_iter().collect())
    }

    pub fn custom(predicate: impl Fn(&Recipient) -> bool + Send + Sync + 'static) -> Self {
        Audience::Custom(Arc::new(predicate))
    }

    pub fn includes(&self, recipient: &Recipient) -> bool {
        match self {
            Audience::All => true,
            Audience::InDimension(dimension) => recipient.dimension == Some(*dimension),
            Audience::Explicit(players) => players.contains(&recipient.entity),
            Audience::Custom(predicate) => predicate(recipient),
        }
    }

    pub fn resolve<'a>(&self, ready: Recipients<'a>) -> Recipients<'a> {
        match self {
            Audience::All => ready,
            _ => ready.filter(|r| self.includes(r)),
        }
    }
}

impl fmt::Debug for Audience {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Audience::All => f.write_str("All"),
            Audience::InDimension(dimension) => {
                f.debug_tuple("InDimension").field(dimension).finish()
            }
            Audience::Explicit(players) => f.debug_tuple("Explicit").field(players).finish(),
            Audience::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

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
    /// Works for any client entity, ready or still in status/login/configuration.
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

    pub fn broadcast(&self, packet: impl Into<ClientboundPacket>) {
        self.ready().send(packet);
    }

    pub fn broadcast_except(&self, entity: Entity, packet: impl Into<ClientboundPacket>) {
        self.ready().except(entity).send(packet);
    }

    pub fn broadcast_chunk(
        &self,
        dimension: DimensionId,
        chunk: ChunkPos,
        packet: impl Into<ClientboundPacket>,
    ) {
        self.ready().seeing_chunk(dimension, chunk).send(packet);
    }
}

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

    /// Works for any client entity, ready or still in status/login/configuration.
    pub fn send(&self, entity: Entity, packet: impl Into<ClientboundPacket>) {
        if let Some(client_id) = resolve_client(self.world, entity) {
            deliver(self.sender(), entity, client_id, packet.into());
        }
    }

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

    pub fn ready(&self) -> Recipients<'w> {
        let world = self.world;
        // None only when ClientId/PlayerReady were never registered: no ready player exists.
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

    pub fn broadcast(&self, packet: impl Into<ClientboundPacket>) {
        self.ready().send(packet);
    }

    pub fn broadcast_except(&self, entity: Entity, packet: impl Into<ClientboundPacket>) {
        self.ready().except(entity).send(packet);
    }

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
    fn borrowing_send_variants_match_consuming_filters() {
        let (mut app, rx) = test_app();
        let me = app
            .world_mut()
            .spawn((ClientId(1), PlayerReady, PlayerDimension(DimensionId::End)))
            .id();
        app.world_mut()
            .spawn((ClientId(2), PlayerReady, PlayerDimension(DimensionId::End)));
        app.world_mut().spawn((ClientId(3), PlayerReady));

        app.add_systems(Update, move |players: Players| {
            let ready = players.ready();
            for id in 1..=2 {
                ready.send_except(me, keep_alive(id));
            }
            ready.send_where(|r| r.visible_from(Some(DimensionId::End)), keep_alive(3));
            ready.send_where(|r| r.visible_from(None), keep_alive(4));
            ready.send_where(|_| false, keep_alive(5));
        });
        app.update();

        let mut sent = drain(&rx);
        sent.sort();
        assert_eq!(
            sent,
            vec![
                (1, 3),
                (1, 4),
                (2, 1),
                (2, 2),
                (2, 3),
                (2, 4),
                (3, 1),
                (3, 2),
                (3, 4)
            ]
        );
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
    fn audience_resolves_against_ready_snapshot() {
        let (mut app, rx) = test_app();
        let first = app
            .world_mut()
            .spawn((ClientId(1), PlayerReady, PlayerDimension(DimensionId::End)))
            .id();
        app.world_mut()
            .spawn((ClientId(2), PlayerReady, PlayerDimension(DimensionId::End)));
        app.world_mut().spawn((ClientId(3), PlayerReady));

        app.add_systems(Update, move |players: Players| {
            Audience::All.resolve(players.ready()).send(keep_alive(1));
            Audience::InDimension(DimensionId::End)
                .resolve(players.ready())
                .send(keep_alive(2));
            Audience::explicit([first])
                .resolve(players.ready())
                .send(keep_alive(3));
            Audience::custom(|r| r.client_id() == 3)
                .resolve(players.ready())
                .send(keep_alive(4));
            assert!(Audience::default().includes(players.ready().iter().next().unwrap()));
        });
        app.update();

        let mut sent = drain(&rx);
        sent.sort();
        assert_eq!(
            sent,
            vec![(1, 1), (1, 2), (1, 3), (2, 1), (2, 2), (3, 1), (3, 4)]
        );
    }

    #[test]
    fn world_players_broadcast_on_fresh_world_is_empty() {
        let (app, rx) = test_app();
        WorldPlayers::new(app.world()).broadcast(keep_alive(1));
        assert!(drain(&rx).is_empty());
    }
}
