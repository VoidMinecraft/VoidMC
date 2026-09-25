use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use flume::{Receiver, Sender};
use tracing::instrument;
use voidmc_net::socket::Packet;
use voidmc_protocol::serverbound;

use crate::components::{Client, ClientId, ConnectionState, PlayerReady};
use crate::config::ServerConfigResource;
use crate::events::PlayerQuitEvent;
use crate::schedule::VoidSystems;

pub struct IncomingPacket {
    pub client_id: u32,
    pub packet: Packet,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnectionId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisconnectReason {
    PeerClosed,
    Server(String),
    Overloaded,
    Error(String),
}

pub struct CloseRequest {
    pub reason: DisconnectReason,
    pub packet: Option<voidmc_protocol::clientbound::ClientboundPacket>,
    pub deadline: Instant,
}

pub enum ConnectionCommand {
    Close {
        id: ConnectionId,
        request: Box<CloseRequest>,
    },
    Shutdown,
}

#[derive(Clone)]
pub struct ConnectionHandle {
    pub id: ConnectionId,
    outgoing: Sender<OutgoingPacket>,
    control: Sender<ConnectionCommand>,
    closing: Arc<AtomicBool>,
}

impl ConnectionHandle {
    pub fn new(
        id: ConnectionId,
        outgoing: Sender<OutgoingPacket>,
        control: Sender<ConnectionCommand>,
    ) -> Self {
        Self {
            id,
            outgoing,
            control,
            closing: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn outgoing(&self) -> &Sender<OutgoingPacket> {
        &self.outgoing
    }
    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Relaxed)
    }

    /// Only the first close request wins. A closed actor is also considered closed.
    pub fn close(&self, request: CloseRequest) -> bool {
        if self.closing.swap(true, Ordering::Relaxed) {
            return false;
        }
        self.control
            .send(ConnectionCommand::Close {
                id: self.id,
                request: Box::new(request),
            })
            .is_ok()
    }
}

pub enum ConnectionEvent {
    Connected(ConnectionHandle),
    Packet(IncomingPacket),
    Disconnected {
        id: ConnectionId,
        reason: DisconnectReason,
    },
}

pub struct OutgoingPacket {
    pub client_id: u32,
    pub packet: voidmc_protocol::clientbound::ClientboundPacket,
}

/// Packets queued for one client beyond this are a stalled connection, not a
/// burst: the biggest single-tick burst (a join at view distance 10) is ~450
/// chunk packets, and a busy steady state is a few hundred packets per tick.
pub const OUTBOUND_QUEUE_CAPACITY: usize = 16 * 1024;

pub struct NetworkPlugin {
    events: Receiver<ConnectionEvent>,
    outgoing_tx: Sender<OutgoingPacket>,
}

impl NetworkPlugin {
    pub fn new(events: Receiver<ConnectionEvent>, outgoing_tx: Sender<OutgoingPacket>) -> Self {
        Self {
            events,
            outgoing_tx,
        }
    }
}

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkChannels {
            events: self.events.clone(),
            outgoing: self.outgoing_tx.clone(),
        })
        .insert_resource(ClientToEntityMap(HashMap::new()))
        .insert_resource(ClientSenders::default())
        .add_systems(
            PreUpdate,
            ingest_network_packets.in_set(VoidSystems::NetworkIngest),
        );
    }
}

#[derive(Resource)]
pub struct NetworkChannels {
    pub events: Receiver<ConnectionEvent>,
    /// Fallback for client entities without a direct sender in [`ClientSenders`];
    /// the running server never reads it, tests use it as their packet sink.
    pub outgoing: Sender<OutgoingPacket>,
}

#[derive(Resource)]
pub struct ClientToEntityMap(pub HashMap<u32, Entity>);

/// Direct connection handles keyed by client entity.
#[derive(Resource, Default)]
pub struct ClientSenders {
    active: HashMap<Entity, ConnectionHandle>,
}

impl ClientSenders {
    pub fn get(&self, entity: Entity) -> Option<&ConnectionHandle> {
        self.active.get(&entity)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.active.contains_key(&entity)
    }

    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    pub fn register(&mut self, entity: Entity, handle: ConnectionHandle) {
        self.active.insert(entity, handle);
    }

    pub fn detach(&mut self, entity: Option<Entity>) {
        if let Some(entity) = entity {
            self.active.remove(&entity);
        }
    }
}

#[instrument(level = "info", skip(world))]
pub fn ingest_network_packets(world: &mut World) {
    // TODO: This batch-draining approach is simple but may lead to increased latency under high load.
    // Batch-drain all packets from channel
    let (max_packets_per_tick, packet_budget_ms) = {
        let config = world.resource::<ServerConfigResource>();
        (config.max_packets_per_tick, config.packet_ingest_budget_ms)
    };
    let packet_limit = if max_packets_per_tick == 0 {
        None
    } else {
        Some(max_packets_per_tick)
    };
    let packet_budget = if packet_budget_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(packet_budget_ms))
    };
    let start = Instant::now();
    let mut hit_limit = false;
    let mut hit_budget = false;
    let mut backlog = 0usize;

    let events: Vec<ConnectionEvent> =
        world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
            let mut events = Vec::new();
            let mut packets = 0;
            loop {
                if packet_limit.is_some_and(|limit| packets >= limit) {
                    hit_limit = true;
                    break;
                }
                if packet_budget.is_some_and(|budget| start.elapsed() >= budget) {
                    hit_budget = true;
                    break;
                }
                match channels.events.try_recv() {
                    Ok(event) => {
                        packets += usize::from(matches!(event, ConnectionEvent::Packet(_)));
                        events.push(event);
                    }
                    Err(_) => break,
                }
            }
            if hit_limit || hit_budget {
                backlog = channels.events.len();
            }
            events
        });

    if hit_limit || hit_budget {
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        tracing::warn!(
            events_processed = events.len(),
            backlog,
            max_packets_per_tick,
            packet_ingest_budget_ms = packet_budget_ms,
            elapsed_ms,
            "Packet ingest throttled"
        );
    }

    for event in events {
        match event {
            ConnectionEvent::Connected(handle) => {
                let id = handle.id.0;
                let entity = world
                    .spawn((
                        Client,
                        ClientId(id),
                        ConnectionState(voidmc_protocol::State::Handshake),
                    ))
                    .id();
                world
                    .resource_mut::<ClientToEntityMap>()
                    .0
                    .insert(id, entity);
                world
                    .resource_mut::<ClientSenders>()
                    .register(entity, handle);
            }
            ConnectionEvent::Packet(incoming) => {
                let entity = world
                    .resource::<ClientToEntityMap>()
                    .0
                    .get(&incoming.client_id)
                    .copied();
                let Some(entity) = entity else {
                    tracing::debug!(
                        client_id = incoming.client_id,
                        "Dropping packet from a client whose connection already ended"
                    );
                    continue;
                };
                if let Err(error) =
                    dispatch_packet(world, incoming.client_id, entity, incoming.packet)
                {
                    if matches!(error, voidmc_codec::DecodeError::InvalidPacketId(_)) {
                        tracing::warn!(client_id = incoming.client_id, %error, "Unrecognized packet");
                    } else {
                        tracing::error!(client_id = incoming.client_id, %error, "Failed to handle packet");
                    }
                }
            }
            ConnectionEvent::Disconnected { id, reason } => {
                tracing::debug!(client_id = id.0, ?reason, "Client disconnected");
                let entity = world.resource_mut::<ClientToEntityMap>().0.remove(&id.0);
                if let Some(entity) = entity {
                    if world.get::<PlayerReady>(entity).is_some() {
                        world.trigger(PlayerQuitEvent {
                            client_id: id.0,
                            entity,
                        });
                        world.flush();
                    }
                    world.despawn(entity);
                }
                world.resource_mut::<ClientSenders>().detach(entity);
            }
        }
    }
}

#[derive(Debug, Event)]
pub struct PacketEvent<T> {
    pub client_id: u32,
    pub entity: Entity,
    pub packet: T,
}

#[instrument(level = "info", skip(world, packet))]
fn dispatch_packet(
    world: &mut World,
    client_id: u32,
    entity: Entity,
    packet: Packet,
) -> Result<(), voidmc_codec::DecodeError> {
    let state = world
        .get::<ConnectionState>(entity)
        .expect("Client must have a ConnectionState component");

    match state.0 {
        voidmc_protocol::State::Handshake => {
            let packet = packet.decode::<serverbound::HandshakePacket>()?;
            tracing::debug!("[Client {}][Handshake] Received {:?}", client_id, packet);

            match packet {
                serverbound::HandshakePacket::Handshake(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
            }
        }
        voidmc_protocol::State::Status => {
            let packet = packet.decode::<serverbound::StatusPacket>()?;
            tracing::debug!("[Client {}][Status] Received {:?}", client_id, packet);

            match packet {
                serverbound::StatusPacket::PingRequest(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::StatusPacket::StatusRequest(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
            }
        }
        voidmc_protocol::State::Login => {
            let packet = packet.decode::<serverbound::LoginPacket>()?;
            tracing::debug!("[Client {}][Login] Received {:?}", client_id, packet);

            match packet {
                serverbound::LoginPacket::LoginStart(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::LoginPacket::LoginAcknowledged(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
            }
        }
        voidmc_protocol::State::Configuration => {
            let packet = packet.decode::<serverbound::ConfigurationPacket>()?;
            tracing::debug!(
                "[Client {}][Configuration] Received {:?}",
                client_id,
                packet
            );

            match packet {
                serverbound::ConfigurationPacket::ClientInformation(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::ConfigurationPacket::PluginMessage(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::ConfigurationPacket::KnownPacks(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::ConfigurationPacket::FinishConfigurationAcknowledged(packet) => world
                    .trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    }),
            }
        }
        voidmc_protocol::State::Play => {
            let packet = packet.decode::<serverbound::PlayPacket>()?;
            tracing::debug!("[Client {}][Play] Received {:?}", client_id, packet);

            match packet {
                serverbound::PlayPacket::ChatCommand(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::ChatMessage(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::ClientInformation(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::ClickContainer(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::CloseContainer(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::CommandSuggestionsRequest(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::PlayPacket::ConfirmTeleportation(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::PlayPacket::Interact(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::KeepAlive(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerAbilities(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerAction(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerCommand(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerInput(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::PlayerLoaded(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::Pong(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SetHeldItem(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SetCreativeModeSlot(packet) => {
                    world.trigger(PacketEvent {
                        client_id,
                        entity,
                        packet,
                    })
                }
                serverbound::PlayPacket::SetPlayerPos(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SetPlayerPosAndRot(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SetPlayerRotation(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SignedChatCommand(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::SwingArm(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::TickEnd(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::UseItem(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
                serverbound::PlayPacket::UseItemOn(packet) => world.trigger(PacketEvent {
                    client_id,
                    entity,
                    packet,
                }),
            }
        }
        _ => {
            tracing::warn!("Unhandled protocol state: {:?}", state.0);
        }
    }

    // Ensure all events are processed and any resulting state changes are applied before processing the next packet
    world.flush();

    Ok(())
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use tokio::io::AsyncWriteExt;
    use voidmc_codec::{Encode, VarI32};
    use voidmc_net::socket::ServerSocket;
    use voidmc_protocol::PROTOCOL_VERSION;

    use super::*;
    use crate::components::PlayerName;
    use crate::players::Players;
    use std::sync::{Arc, Mutex};

    struct Harness {
        app: App,
        events_tx: Sender<ConnectionEvent>,
        control_tx: Sender<ConnectionCommand>,
        outgoing_rx: Option<Receiver<OutgoingPacket>>,
    }

    #[derive(Clone, Default)]
    struct LogCapture(Arc<Mutex<Vec<(tracing::Level, String)>>>);

    impl LogCapture {
        fn errors(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|(level, _)| *level == tracing::Level::ERROR)
                .map(|(_, message)| message.clone())
                .collect()
        }

        fn contains(&self, level: tracing::Level, needle: &str) -> bool {
            self.0
                .lock()
                .unwrap()
                .iter()
                .any(|(l, message)| *l == level && message.contains(needle))
        }
    }

    struct MessageVisitor(String);

    impl tracing::field::Visit for MessageVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}");
            }
        }
    }

    impl tracing::Subscriber for LogCapture {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut visitor = MessageVisitor(String::new());
            event.record(&mut visitor);
            self.0
                .lock()
                .unwrap()
                .push((*event.metadata().level(), visitor.0));
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    fn captured(run: impl FnOnce()) -> LogCapture {
        let capture = LogCapture::default();
        tracing::subscriber::with_default(capture.clone(), run);
        capture
    }

    fn harness() -> Harness {
        let (events_tx, events_rx) = flume::unbounded::<ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (control_tx, _control_rx) = flume::unbounded::<ConnectionCommand>();
        let mut app = App::new();
        app.add_plugins(NetworkPlugin::new(events_rx, outgoing_tx))
            .insert_resource(ServerConfigResource::from(&crate::ServerConfig::default()));
        Harness {
            app,
            events_tx,
            control_tx,
            outgoing_rx: Some(outgoing_rx),
        }
    }

    fn handshake_packet() -> Packet {
        let packet = serverbound::HandshakePacket::Handshake(serverbound::Handshake {
            protocol_version: PROTOCOL_VERSION,
            server_address: "localhost".to_string(),
            server_port: 25565,
            next_state: voidmc_protocol::State::Login,
        });
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        let mut frame = Vec::new();
        VarI32(bytes.len() as i32).encode(&mut frame);
        frame.extend(bytes);

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut peer = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (mut reader, _writer) = ServerSocket::new(listener, Default::default())
                .accept()
                .await
                .unwrap()
                .into_split();
            peer.write_all(&frame).await.unwrap();
            reader.receive().await.unwrap()
        })
    }

    impl Harness {
        fn connect(&self, client_id: u32) -> Receiver<OutgoingPacket> {
            let (tx, rx) = flume::bounded(OUTBOUND_QUEUE_CAPACITY);
            self.events_tx
                .send(ConnectionEvent::Connected(ConnectionHandle::new(
                    ConnectionId(client_id),
                    tx,
                    self.control_tx.clone(),
                )))
                .unwrap();
            rx
        }

        fn packet(&self, client_id: u32) {
            self.events_tx
                .send(ConnectionEvent::Packet(IncomingPacket {
                    client_id,
                    packet: handshake_packet(),
                }))
                .unwrap();
        }

        fn disconnect(&self, client_id: u32) {
            self.events_tx
                .send(ConnectionEvent::Disconnected {
                    id: ConnectionId(client_id),
                    reason: DisconnectReason::PeerClosed,
                })
                .unwrap();
        }

        fn entity_of(&self, client_id: u32) -> Option<Entity> {
            self.app
                .world()
                .resource::<ClientToEntityMap>()
                .0
                .get(&client_id)
                .copied()
        }

        fn senders(&self) -> &ClientSenders {
            self.app.world().resource::<ClientSenders>()
        }
    }

    #[derive(Resource, Default)]
    struct Handshakes(Vec<(u32, Entity)>);

    #[test]
    fn first_packet_spawns_a_registered_client_entity() {
        let mut h = harness();
        h.app.init_resource::<Handshakes>().add_observer(
            |event: On<PacketEvent<serverbound::Handshake>>, mut seen: ResMut<Handshakes>| {
                seen.0.push((event.client_id, event.entity));
            },
        );
        let _rx = h.connect(1);
        h.packet(1);
        h.app.update();

        let entity = h.entity_of(1).expect("client entity spawned");
        assert!(h.senders().contains(entity));
        assert_eq!(h.app.world().get::<ClientId>(entity).unwrap().0, 1);
        assert_eq!(h.app.world().resource::<Handshakes>().0, vec![(1, entity)]);
    }

    #[test]
    fn announcement_in_the_same_tick_as_the_packet_is_seen() {
        let mut h = harness();
        h.app.update();
        let _rx = h.connect(1);
        h.packet(1);
        let _rx2 = h.connect(2);
        h.packet(2);
        h.app.update();

        assert!(h.entity_of(1).is_some());
        assert!(h.entity_of(2).is_some());
        assert_eq!(h.senders().active_len(), 2);
    }

    #[test]
    fn packet_from_unannounced_client_spawns_nothing() {
        let mut h = harness();
        h.packet(7);
        h.app.update();

        assert!(h.entity_of(7).is_none());
        assert_eq!(h.senders().active_len(), 0);
        assert_eq!(
            h.app
                .world_mut()
                .query::<&ClientId>()
                .iter(h.app.world())
                .count(),
            0
        );
    }

    #[test]
    fn disconnect_despawns_and_removes_the_sender() {
        let mut h = harness();
        let rx = h.connect(1);
        h.packet(1);
        h.app.update();
        let entity = h.entity_of(1).unwrap();
        h.app
            .world_mut()
            .entity_mut(entity)
            .insert((PlayerReady, PlayerName("slow".to_string())));

        h.disconnect(1);
        h.app.update();

        assert!(h.entity_of(1).is_none());
        assert!(h.app.world().get_entity(entity).is_err());
        assert!(!h.senders().contains(entity));
        assert_eq!(h.senders().active_len(), 0);
        assert!(rx.is_disconnected());
    }

    #[test]
    fn packet_arriving_after_disconnect_is_dropped_quietly() {
        let mut h = harness();
        let rx = h.connect(1);
        h.packet(1);
        h.app.update();
        let entity = h.entity_of(1).unwrap();

        drop(rx);
        h.disconnect(1);
        h.app.update();
        assert!(h.app.world().get_entity(entity).is_err());

        h.packet(1);
        let logs = captured(|| h.app.update());

        assert!(h.entity_of(1).is_none());
        assert_eq!(h.senders().active_len(), 0);
        assert_eq!(
            h.app
                .world_mut()
                .query::<&ClientId>()
                .iter(h.app.world())
                .count(),
            0
        );
        assert_eq!(logs.errors(), Vec::<String>::new());
        assert!(logs.contains(
            tracing::Level::DEBUG,
            "Dropping packet from a client whose connection already ended"
        ));
    }

    #[test]
    fn inbound_backlog_preserves_disconnect_order() {
        let mut h = harness();
        h.app
            .insert_resource(ServerConfigResource::from(&crate::ServerConfig {
                max_packets_per_tick: 1,
                ..crate::ServerConfig::default()
            }));
        let _rx = h.connect(7);
        h.packet(7);
        h.packet(7);
        h.disconnect(7);
        h.packet(7); // Simulates a stale producer after the close event.

        h.app.update();
        let entity = h
            .entity_of(7)
            .expect("connection stays live through first packet");
        h.app.update();
        assert_eq!(h.entity_of(7), Some(entity));
        h.app.update();
        assert!(h.entity_of(7).is_none());
        assert!(h.app.world().get_entity(entity).is_err());
        h.app.update();
        assert!(h.entity_of(7).is_none());
        assert_eq!(h.senders().active_len(), 0);
    }

    #[derive(Resource, Default)]
    struct QuitSends(Vec<(Entity, bool)>);

    #[test]
    fn quit_observer_can_send_to_the_quitting_player_without_error() {
        let mut h = harness();
        h.outgoing_rx.take();
        h.app.init_resource::<QuitSends>().add_observer(
            |event: On<PlayerQuitEvent>,
             players: Players,
             senders: Res<ClientSenders>,
             mut sends: ResMut<QuitSends>| {
                sends.0.push((event.entity, senders.contains(event.entity)));
                players.send(
                    event.entity,
                    voidmc_protocol::clientbound::Disconnect {
                        reason: crate::commands::text_to_nbt("bye", "red"),
                    },
                );
            },
        );
        let rx = h.connect(1);
        h.packet(1);
        h.app.update();
        let entity = h.entity_of(1).unwrap();
        h.app
            .world_mut()
            .entity_mut(entity)
            .insert((PlayerReady, PlayerName("quitter".to_string())));

        drop(rx);
        h.disconnect(1);
        let logs = captured(|| h.app.update());

        assert_eq!(
            h.app.world().resource::<QuitSends>().0,
            vec![(entity, true)]
        );
        assert!(h.app.world().get_entity(entity).is_err());
        assert!(!h.senders().contains(entity));
        assert_eq!(logs.errors(), Vec::<String>::new());
        assert!(logs.contains(tracing::Level::DEBUG, "Client connection already closed"));
    }

    #[test]
    fn status_only_connections_leave_no_pending_sender() {
        let mut h = harness();
        let rx = h.connect(1);
        h.disconnect(1);
        h.app.update();

        assert!(h.entity_of(1).is_none());
        assert!(rx.is_disconnected());
    }
}
