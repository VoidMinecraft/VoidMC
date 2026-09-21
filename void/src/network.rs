use std::collections::HashMap;
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

pub struct OutgoingPacket {
    pub client_id: u32,
    pub packet: voidmc_protocol::clientbound::ClientboundPacket,
}

/// Packets queued for one client beyond this are a stalled connection, not a
/// burst: the biggest single-tick burst (a join at view distance 10) is ~450
/// chunk packets, and a busy steady state is a few hundred packets per tick.
pub const OUTBOUND_QUEUE_CAPACITY: usize = 16 * 1024;

/// Sent by the network thread on accept, before the client's task starts, so it
/// is always visible to the game thread before any packet from that client.
pub struct ClientConnected {
    pub client_id: u32,
    pub outgoing: Sender<OutgoingPacket>,
}

pub struct NetworkPlugin {
    incoming_rx: Receiver<IncomingPacket>,
    outgoing_tx: Sender<OutgoingPacket>,
    disconnect_rx: Receiver<u32>,
    kick_tx: Sender<u32>,
    connected_rx: Receiver<ClientConnected>,
}

impl NetworkPlugin {
    pub fn new(
        incoming_rx: Receiver<IncomingPacket>,
        outgoing_tx: Sender<OutgoingPacket>,
        disconnect_rx: Receiver<u32>,
        kick_tx: Sender<u32>,
        connected_rx: Receiver<ClientConnected>,
    ) -> Self {
        Self {
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
            kick_tx,
            connected_rx,
        }
    }
}

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkChannels {
            incoming: self.incoming_rx.clone(),
            outgoing: self.outgoing_tx.clone(),
            disconnect: self.disconnect_rx.clone(),
            kick: self.kick_tx.clone(),
        })
        .insert_resource(ClientToEntityMap(HashMap::new()))
        .insert_resource(ClientSenders::new(self.connected_rx.clone()))
        .add_systems(
            PreUpdate,
            ingest_network_packets.in_set(VoidSystems::NetworkIngest),
        );
    }
}

#[derive(Resource)]
pub struct NetworkChannels {
    pub incoming: Receiver<IncomingPacket>,
    /// Fallback for client entities without a direct sender in [`ClientSenders`];
    /// the running server never reads it, tests use it as their packet sink.
    pub outgoing: Sender<OutgoingPacket>,
    pub disconnect: Receiver<u32>,
    pub kick: Sender<u32>,
}

#[derive(Resource)]
pub struct ClientToEntityMap(pub HashMap<u32, Entity>);

pub struct ClientSender {
    outgoing: Sender<OutgoingPacket>,
    kicked: AtomicBool,
}

impl ClientSender {
    pub fn new(outgoing: Sender<OutgoingPacket>) -> Self {
        Self {
            outgoing,
            kicked: AtomicBool::new(false),
        }
    }

    pub fn outgoing(&self) -> &Sender<OutgoingPacket> {
        &self.outgoing
    }

    pub fn kicked(&self) -> bool {
        self.kicked.load(Ordering::Relaxed)
    }

    /// Returns `true` the first time only, so the kick is requested once.
    pub fn mark_kicked(&self) -> bool {
        !self.kicked.swap(true, Ordering::Relaxed)
    }
}

/// Direct per-client outbound senders, keyed by client entity once the entity
/// exists and parked by client id until then.
#[derive(Resource)]
pub struct ClientSenders {
    connected: Receiver<ClientConnected>,
    pending: HashMap<u32, Sender<OutgoingPacket>>,
    active: HashMap<Entity, ClientSender>,
}

impl ClientSenders {
    pub fn new(connected: Receiver<ClientConnected>) -> Self {
        Self {
            connected,
            pending: HashMap::new(),
            active: HashMap::new(),
        }
    }

    pub fn get(&self, entity: Entity) -> Option<&ClientSender> {
        self.active.get(&entity)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.active.contains_key(&entity)
    }

    pub fn is_pending(&self, client_id: u32) -> bool {
        self.pending.contains_key(&client_id)
    }

    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn sync(&mut self) {
        while let Ok(connected) = self.connected.try_recv() {
            self.pending.insert(connected.client_id, connected.outgoing);
        }
    }

    pub fn take_pending(&mut self, client_id: u32) -> Option<Sender<OutgoingPacket>> {
        if !self.pending.contains_key(&client_id) {
            self.sync();
        }
        self.pending.remove(&client_id)
    }

    pub fn register(&mut self, entity: Entity, outgoing: Sender<OutgoingPacket>) {
        self.active.insert(entity, ClientSender::new(outgoing));
    }

    pub fn detach(&mut self, client_id: u32, entity: Option<Entity>) {
        self.pending.remove(&client_id);
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

    world.resource_mut::<ClientSenders>().sync();

    let packets: Vec<IncomingPacket> =
        world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
            let mut packets = Vec::new();
            loop {
                if let Some(limit) = packet_limit {
                    if packets.len() >= limit {
                        hit_limit = true;
                        break;
                    }
                }
                if let Some(budget) = packet_budget {
                    if start.elapsed() >= budget {
                        hit_budget = true;
                        break;
                    }
                }

                match channels.incoming.try_recv() {
                    Ok(packet) => packets.push(packet),
                    Err(_) => break,
                }
            }

            if hit_limit || hit_budget {
                backlog = channels.incoming.len();
            }

            packets
        });

    if hit_limit || hit_budget {
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        tracing::warn!(
            packets_processed = packets.len(),
            backlog,
            max_packets_per_tick,
            packet_ingest_budget_ms = packet_budget_ms,
            elapsed_ms,
            "Packet ingest throttled"
        );
    }

    for incoming_packet in packets {
        let Some(client_entity) = client_entity(world, incoming_packet.client_id) else {
            tracing::error!(
                client_id = incoming_packet.client_id,
                "Dropping packet from a client the network thread never announced"
            );
            continue;
        };

        if let Err(e) = dispatch_packet(
            world,
            incoming_packet.client_id,
            client_entity,
            incoming_packet.packet,
        ) {
            if matches!(e, voidmc_codec::DecodeError::InvalidPacketId(_)) {
                // Unrecognized packet (e.g. one we don't handle yet): expected and
                // non-fatal, so warn instead of error.
                tracing::warn!(
                    "Unrecognized packet from client {}: {}",
                    incoming_packet.client_id,
                    e
                );
            } else {
                tracing::error!(
                    "Failed to handle packet from client {}: {}",
                    incoming_packet.client_id,
                    e
                );
            }
        }
    }

    // Drain disconnect channel and handle disconnects
    let disconnected: Vec<u32> = world.resource_scope(|_world, channels: Mut<NetworkChannels>| {
        let mut disc = Vec::new();
        while let Ok(client_id) = channels.disconnect.try_recv() {
            disc.push(client_id);
        }
        disc
    });

    if !disconnected.is_empty() {
        world.resource_mut::<ClientSenders>().sync();
    }

    for disc_client_id in disconnected {
        let entity = world
            .resource_mut::<ClientToEntityMap>()
            .0
            .remove(&disc_client_id);
        world
            .resource_mut::<ClientSenders>()
            .detach(disc_client_id, entity);
        let Some(entity) = entity else {
            continue;
        };

        // Trigger quit event (observer will broadcast to other players)
        let is_ready = world.get::<PlayerReady>(entity).is_some();
        if is_ready {
            world.trigger(PlayerQuitEvent {
                client_id: disc_client_id,
                entity,
            });
            world.flush();
        }

        world.despawn(entity);
    }
}

fn client_entity(world: &mut World, client_id: u32) -> Option<Entity> {
    if let Some(entity) = world.resource::<ClientToEntityMap>().0.get(&client_id) {
        return Some(*entity);
    }

    let outgoing = world
        .resource_mut::<ClientSenders>()
        .take_pending(client_id)?;
    let entity = world
        .spawn((
            Client,
            ClientId(client_id),
            ConnectionState(voidmc_protocol::State::Handshake),
        ))
        .id();
    world
        .resource_mut::<ClientSenders>()
        .register(entity, outgoing);
    world
        .resource_mut::<ClientToEntityMap>()
        .0
        .insert(client_id, entity);
    Some(entity)
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

    struct Harness {
        app: App,
        incoming_tx: Sender<IncomingPacket>,
        connected_tx: Sender<ClientConnected>,
        disconnect_tx: Sender<u32>,
        _kick_rx: Receiver<u32>,
        _outgoing_rx: Receiver<OutgoingPacket>,
    }

    fn harness() -> Harness {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, _outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let (connected_tx, connected_rx) = flume::unbounded::<ClientConnected>();
        let mut app = App::new();
        app.add_plugins(NetworkPlugin::new(
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
            kick_tx,
            connected_rx,
        ))
        .insert_resource(ServerConfigResource::from(&crate::ServerConfig::default()));
        Harness {
            app,
            incoming_tx,
            connected_tx,
            disconnect_tx,
            _kick_rx,
            _outgoing_rx,
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
            self.connected_tx
                .send(ClientConnected {
                    client_id,
                    outgoing: tx,
                })
                .unwrap();
            rx
        }

        fn packet(&self, client_id: u32) {
            self.incoming_tx
                .send(IncomingPacket {
                    client_id,
                    packet: handshake_packet(),
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
        assert!(!h.senders().is_pending(1));
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
        assert_eq!(h.senders().pending_len(), 0);
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

        h.disconnect_tx.send(1).unwrap();
        h.app.update();

        assert!(h.entity_of(1).is_none());
        assert!(h.app.world().get_entity(entity).is_err());
        assert!(!h.senders().contains(entity));
        assert_eq!(h.senders().active_len(), 0);
        assert!(rx.is_disconnected());
    }

    #[test]
    fn status_only_connections_leave_no_pending_sender() {
        let mut h = harness();
        let rx = h.connect(1);
        h.disconnect_tx.send(1).unwrap();
        h.app.update();

        assert!(h.entity_of(1).is_none());
        assert!(!h.senders().is_pending(1));
        assert_eq!(h.senders().pending_len(), 0);
        assert!(rx.is_disconnected());
    }
}
