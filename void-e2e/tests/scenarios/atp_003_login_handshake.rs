//! VMC-ATP-003: a client completes the Handshake, Login, Configuration and
//! Play transitions and becomes ready, without online authentication,
//! encryption or compression.
//!
//! The scenario drives an Azalea connection by hand instead of using
//! `Bot`, so that it can record every packet exchanged before the play
//! state. The client-side trace and the server-side trace (the serverbound
//! packets the server dispatched, with the connection state it dispatched
//! them in) are asserted on and printed, so the packet trace is reproduced on
//! every run (`cargo test atp_003 -- --nocapture` shows it).

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy_app::App;
use bevy_ecs::prelude::*;
use uuid::Uuid;
use voidmc::components::{
    ClientSettings, ConnectionState, PlayerReady, PlayerUuid, ProtocolVersion, TeleportState,
};
use voidmc::network::PacketEvent;
use voidmc::{ChatMode, CommandRegistry, RegistryDataStore};
use voidmc_e2e::TestServer;
use voidmc_e2e::core::entity_id::MinecraftEntityId;
use voidmc_e2e::protocol::common::client_information::{ChatVisibility, ClientInformation};
use voidmc_e2e::protocol::common::movements::MoveFlags;
use voidmc_e2e::protocol::connect::Connection;
use voidmc_e2e::protocol::packets::config::{
    ClientboundConfigPacket, s_client_information::ServerboundClientInformation,
    s_finish_configuration::ServerboundFinishConfiguration,
    s_select_known_packs::ServerboundSelectKnownPacks,
};
use voidmc_e2e::protocol::packets::game::c_commands::NodeType;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, s_accept_teleportation::ServerboundAcceptTeleportation,
    s_client_information::ServerboundClientInformation as PlayClientInformation,
    s_keep_alive::ServerboundKeepAlive, s_move_player_pos_rot::ServerboundMovePlayerPosRot,
    s_player_loaded::ServerboundPlayerLoaded,
};
use voidmc_e2e::protocol::packets::handshake::s_intention::ServerboundIntention;
use voidmc_e2e::protocol::packets::login::{
    ClientboundLoginPacket, s_hello::ServerboundHello,
    s_login_acknowledged::ServerboundLoginAcknowledged,
};
use voidmc_e2e::protocol::packets::{ClientIntention, PROTOCOL_VERSION, Packet, ProtocolPacket};
use voidmc_protocol::{State, serverbound};

const TIMEOUT: Duration = Duration::from_secs(15);
const NAME: &str = "Alice";

/// The serverbound packets the server dispatched, as `"<state> <packet>"`.
type ServerTrace = Arc<Mutex<Vec<String>>>;

/// Records every dispatch of `P` with the connection state it arrived in.
fn record<P: Send + Sync + 'static>(app: &mut App, trace: &ServerTrace, name: &'static str) {
    let trace = trace.clone();
    app.add_observer(
        move |event: On<PacketEvent<P>>, states: Query<&ConnectionState>| {
            let state = states
                .get(event.entity)
                .map_or("<none>".to_string(), |state| format!("{:?}", state.0));
            trace.lock().unwrap().push(format!("{state} {name}"));
        },
    );
}

/// The packets the client exchanged, as `"<state> <direction> <packet>"`.
#[derive(Default)]
struct ClientTrace(Vec<String>);

impl ClientTrace {
    fn received(&mut self, state: &str, packet: &impl ProtocolPacket) {
        self.0.push(format!("{state} S->C {}", packet.name()));
    }

    /// The trace with runs of the same entry folded into `"<entry> xN"`.
    fn folded(&self) -> Vec<String> {
        let mut folded: Vec<(String, usize)> = Vec::new();
        for entry in &self.0 {
            match folded.last_mut() {
                Some((last, count)) if last == entry => *count += 1,
                _ => folded.push((entry.clone(), 1)),
            }
        }
        folded
            .into_iter()
            .map(|(entry, count)| match count {
                1 => entry,
                count => format!("{entry} x{count}"),
            })
            .collect()
    }
}

async fn within<T>(what: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .unwrap_or_else(|_| panic!("timed out after {TIMEOUT:?} waiting for {what}"))
}

async fn send<R, W>(
    connection: &mut Connection<R, W>,
    trace: &mut ClientTrace,
    state: &str,
    packet: impl Packet<W>,
) where
    R: ProtocolPacket + Debug,
    W: ProtocolPacket + Packet<W> + Debug,
{
    let packet = packet.into_variant();
    trace.0.push(format!("{state} C->S {}", packet.name()));
    connection
        .write(packet)
        .await
        .unwrap_or_else(|error| panic!("could not send a {state} packet: {error}"));
}

async fn read<R, W>(connection: &mut Connection<R, W>, trace: &mut ClientTrace, state: &str) -> R
where
    R: ProtocolPacket + Debug,
    W: ProtocolPacket + Debug,
{
    let packet = within(&format!("a {state} packet"), connection.read())
        .await
        .unwrap_or_else(|error| panic!("could not read a {state} packet: {error}"));
    trace.received(state, &packet);
    packet
}

/// The connection state the server holds for the player, and whether it
/// is ready.
async fn server_side(server: &TestServer, uuid: Uuid) -> (State, bool) {
    server
        .with_world(move |world| {
            world
                .query::<(&PlayerUuid, &ConnectionState, Has<PlayerReady>)>()
                .iter(world)
                .find(|(id, ..)| id.0 == uuid)
                .map(|(_, state, ready)| (state.0, ready))
        })
        .await
        .expect("the server holds an entity for the player")
}

#[tokio::test]
async fn atp_003_handshake_reaches_the_ready_play_state() {
    let server_trace = ServerTrace::default();
    let recorder = server_trace.clone();
    let server = TestServer::builder()
        .operators()
        .compression_threshold(None)
        .plugin(move |app| {
            record::<serverbound::Handshake>(app, &recorder, "handshake");
            record::<serverbound::LoginStart>(app, &recorder, "login_start");
            record::<serverbound::LoginAcknowledged>(app, &recorder, "login_acknowledged");
            record::<serverbound::ClientInformation>(app, &recorder, "client_information");
            record::<serverbound::KnownPacks>(app, &recorder, "known_packs");
            record::<serverbound::FinishConfigurationAcknowledged>(
                app,
                &recorder,
                "finish_configuration_acknowledged",
            );
            record::<serverbound::PlayerLoaded>(app, &recorder, "player_loaded");
        })
        .start()
        .await;
    let (registry_ids, command_names) = server
        .with_world(|world| {
            let registries = world
                .resource::<RegistryDataStore>()
                .registries()
                .iter()
                .map(|registry| (registry.registry_id.clone(), registry.entries.len()))
                .collect::<Vec<_>>();
            let commands = world
                .resource::<CommandRegistry>()
                .command_names()
                .into_iter()
                .map(String::from)
                .collect::<BTreeSet<_>>();
            (registries, commands)
        })
        .await;
    assert!(command_names.contains("tp"), "the default commands are on");
    let uuid = Uuid::from_u128(0x0003_0000_0000_3000_8000_0000_0000_0003);
    let address = server.address();
    let mut trace = ClientTrace::default();

    // Handshake -> Login.
    let mut connection = within("the connection", Connection::new(&address))
        .await
        .unwrap_or_else(|error| panic!("could not connect to {address}: {error}"));
    send(
        &mut connection,
        &mut trace,
        "handshake",
        ServerboundIntention {
            protocol_version: PROTOCOL_VERSION,
            hostname: address.ip().to_string(),
            port: address.port(),
            intention: ClientIntention::Login,
        },
    )
    .await;
    let mut connection = connection.login();
    send(
        &mut connection,
        &mut trace,
        "login",
        ServerboundHello {
            name: NAME.to_string(),
            profile_id: uuid,
        },
    )
    .await;

    // Login: the only answer is LoginSuccess with the supplied offline
    // profile. No encryption request (which online mode needs), no
    // compression, no plugin query.
    let ClientboundLoginPacket::LoginFinished(success) =
        read(&mut connection, &mut trace, "login").await
    else {
        panic!("expected LoginSuccess first, trace: {:#?}", trace.0);
    };
    assert_eq!(success.game_profile.uuid, uuid, "LoginSuccess UUID");
    assert_eq!(success.game_profile.name, NAME, "LoginSuccess name");
    assert!(
        success.game_profile.properties.map.is_empty(),
        "LoginSuccess has no profile properties: {:?}",
        success.game_profile.properties
    );
    assert_eq!(server_side(&server, uuid).await, (State::Login, false));
    let protocol_version = server
        .with_world(move |world| {
            world
                .query::<(&PlayerUuid, &ProtocolVersion)>()
                .iter(world)
                .find(|(id, _)| id.0 == uuid)
                .map(|(_, version)| version.0)
        })
        .await;
    assert_eq!(protocol_version, Some(voidmc_protocol::PROTOCOL_VERSION));

    // Login -> Configuration. Like the vanilla client, ClientInformation
    // follows LoginAcknowledged without waiting for the server.
    send(
        &mut connection,
        &mut trace,
        "login",
        ServerboundLoginAcknowledged {},
    )
    .await;
    let mut connection = connection.config();
    send(
        &mut connection,
        &mut trace,
        "configuration",
        ServerboundClientInformation {
            information: ClientInformation {
                view_distance: 6,
                ..Default::default()
            },
        },
    )
    .await;

    let mut known_packs = Vec::new();
    let mut registries = Vec::new();
    let mut tags = None;
    loop {
        match read(&mut connection, &mut trace, "configuration").await {
            ClientboundConfigPacket::SelectKnownPacks(packet) => {
                assert_eq!(
                    server_side(&server, uuid).await,
                    (State::Configuration, false)
                );
                known_packs = packet
                    .known_packs
                    .iter()
                    .map(|pack| format!("{}:{}@{}", pack.namespace, pack.id, pack.version))
                    .collect();
                send(
                    &mut connection,
                    &mut trace,
                    "configuration",
                    ServerboundSelectKnownPacks {
                        known_packs: packet.known_packs,
                    },
                )
                .await;
            }
            ClientboundConfigPacket::RegistryData(packet) => {
                registries.push((packet.registry_id.to_string(), packet.entries.len()));
            }
            ClientboundConfigPacket::UpdateTags(packet) => tags = Some(packet.tags),
            ClientboundConfigPacket::FinishConfiguration(_) => {
                send(
                    &mut connection,
                    &mut trace,
                    "configuration",
                    ServerboundFinishConfiguration {},
                )
                .await;
                break;
            }
            other => panic!("unexpected configuration packet: {other:?}"),
        }
    }
    assert_eq!(
        known_packs,
        [format!(
            "minecraft:core@{}",
            voidmc_protocol::MINECRAFT_VERSION
        )]
    );
    assert_eq!(
        registries, registry_ids,
        "every registry the server holds is delivered, with all its entries"
    );
    assert!(
        registries
            .iter()
            .any(|(id, entries)| id == "minecraft:dimension_type" && *entries > 0)
    );
    let tags = tags.expect("UpdateTags was delivered");
    assert!(!tags.0.is_empty(), "UpdateTags carries tags");
    for (registry, registry_tags) in &tags.0 {
        let registry = registry.to_string();
        let (_, entries) = registries
            .iter()
            .find(|(id, _)| *id == registry)
            .unwrap_or_else(|| panic!("tags for {registry}, which was not delivered"));
        assert!(!registry_tags.is_empty(), "{registry} has tags");
        for tag in registry_tags {
            assert!(
                tag.elements
                    .iter()
                    .all(|&index| index >= 0 && (index as usize) < *entries),
                "{registry} tag {} points inside the registry",
                tag.name
            );
        }
    }

    // Configuration -> Play: Login, the command tree, the initial chunks and
    // the position synchronization.
    let mut connection = connection.game();
    let mut chunks_before_sync = BTreeSet::new();
    let mut entity_id = None;
    let sync = loop {
        match read(&mut connection, &mut trace, "play").await {
            ClientboundGamePacket::Login(login) => {
                assert_eq!(
                    server_side(&server, uuid).await,
                    (State::Play, false),
                    "the server holds the play state, not ready yet"
                );
                assert_eq!(login.common.dimension.to_string(), "minecraft:overworld");
                assert!(
                    login
                        .levels
                        .iter()
                        .any(|level| level.to_string() == "minecraft:overworld")
                );
                assert!(!login.enforces_secure_chat);
                entity_id = Some(login.player_id);
            }
            ClientboundGamePacket::Commands(commands) => {
                let root = &commands.entries[commands.root_index as usize];
                assert_eq!(root.node_type, NodeType::Root);
                let literals: BTreeSet<String> = root
                    .children
                    .iter()
                    .filter_map(|&child| match &commands.entries[child as usize].node_type {
                        NodeType::Literal { name } => Some(name.clone()),
                        _ => None,
                    })
                    .collect();
                assert!(
                    command_names.is_subset(&literals),
                    "the command tree lists every registered command: {literals:?}"
                );
            }
            ClientboundGamePacket::LevelChunkWithLight(chunk) => {
                chunks_before_sync.insert((chunk.x, chunk.z));
            }
            ClientboundGamePacket::KeepAlive(keep_alive) => {
                send(
                    &mut connection,
                    &mut trace,
                    "play",
                    ServerboundKeepAlive { id: keep_alive.id },
                )
                .await;
            }
            ClientboundGamePacket::PlayerPosition(sync) => break sync,
            _ => {}
        }
    };
    let entity_id: MinecraftEntityId = entity_id.expect("Login arrived before the position");
    let spawn_chunk = (
        (sync.change.pos.x / 16.0).floor() as i32,
        (sync.change.pos.z / 16.0).floor() as i32,
    );
    let around = |radius: i32| {
        let (x, z) = spawn_chunk;
        (-radius..=radius)
            .flat_map(move |dx| (-radius..=radius).map(move |dz| (x + dx, z + dz)))
            .collect::<BTreeSet<_>>()
    };
    assert!(
        around(1).is_subset(&chunks_before_sync),
        "the chunks around spawn {spawn_chunk:?} arrive before the position: {chunks_before_sync:?}"
    );

    // Accept the teleport like the vanilla client. Until PlayerLoaded the
    // player is not ready, even once the server has taken the confirmation.
    send(
        &mut connection,
        &mut trace,
        "play",
        ServerboundAcceptTeleportation { id: sync.id },
    )
    .await;
    send(
        &mut connection,
        &mut trace,
        "play",
        ServerboundMovePlayerPosRot {
            pos: sync.change.pos,
            look_direction: sync.change.look_direction,
            flags: MoveFlags {
                on_ground: false,
                horizontal_collision: false,
            },
        },
    )
    .await;
    server
        .wait_for(
            "the server to take the teleport confirmation",
            move |world| {
                world
                    .query::<(&PlayerUuid, &TeleportState)>()
                    .iter(world)
                    .find(|(id, state)| id.0 == uuid && state.pending_id.is_none())
                    .map(|_| ())
            },
        )
        .await;
    assert_eq!(server_side(&server, uuid).await, (State::Play, false));

    // PlayerLoaded -> ready: the server lists the player and sends its
    // inventory.
    send(
        &mut connection,
        &mut trace,
        "play",
        ServerboundPlayerLoaded {},
    )
    .await;
    let mut chunks = chunks_before_sync.clone();
    let mut listed = false;
    let mut inventory = false;
    while !(listed && inventory) {
        match read(&mut connection, &mut trace, "play").await {
            ClientboundGamePacket::PlayerInfoUpdate(update) => {
                listed |= update.entries.iter().any(|entry| {
                    entry.profile.uuid == uuid
                        && entry.profile.name == NAME
                        && entry.profile.properties.map.is_empty()
                });
            }
            ClientboundGamePacket::ContainerSetContent(_) => inventory = true,
            ClientboundGamePacket::LevelChunkWithLight(chunk) => {
                chunks.insert((chunk.x, chunk.z));
            }
            ClientboundGamePacket::KeepAlive(keep_alive) => {
                send(
                    &mut connection,
                    &mut trace,
                    "play",
                    ServerboundKeepAlive { id: keep_alive.id },
                )
                .await;
            }
            ClientboundGamePacket::Disconnect(disconnect) => {
                panic!("kicked: {}", disconnect.reason)
            }
            _ => {}
        }
    }
    assert_eq!(server_side(&server, uuid).await, (State::Play, true));
    assert!(
        around(2).is_subset(&chunks),
        "every initial chunk within radius 2 of spawn arrives: {chunks:?}"
    );
    let (locale, view_distance, server_entity_id) = server
        .with_world(move |world| {
            world
                .query::<(
                    &PlayerUuid,
                    &ClientSettings,
                    &voidmc::components::MinecraftEntityId,
                )>()
                .iter(world)
                .find(|(id, ..)| id.0 == uuid)
                .map(|(_, settings, id)| (settings.locale.clone(), settings.view_distance, id.0))
        })
        .await
        .expect("the ready player has client settings");
    assert_eq!((locale.as_str(), view_distance), ("en_us", 6));
    assert_eq!(entity_id.0, server_entity_id);

    // The retained packet trace.
    let folded = trace.folded();
    let server_trace = server_trace.lock().unwrap().clone();
    println!("VMC-ATP-003 client packet trace:");
    for entry in &folded {
        println!("  {entry}");
    }
    println!("VMC-ATP-003 server dispatch trace:");
    for entry in &server_trace {
        println!("  {entry}");
    }

    let configuration_end = folded
        .iter()
        .position(|entry| entry == "configuration C->S finish_configuration")
        .expect("configuration finished");
    let mut expected = vec![
        "handshake C->S intention".to_string(),
        "login C->S hello".to_string(),
        "login S->C login_finished".to_string(),
        "login C->S login_acknowledged".to_string(),
        "configuration C->S client_information".to_string(),
        "configuration S->C select_known_packs".to_string(),
        "configuration C->S select_known_packs".to_string(),
    ];
    expected.push(match registry_ids.len() {
        1 => "configuration S->C registry_data".to_string(),
        count => format!("configuration S->C registry_data x{count}"),
    });
    expected.extend([
        "configuration S->C update_tags".to_string(),
        "configuration S->C finish_configuration".to_string(),
        "configuration C->S finish_configuration".to_string(),
    ]);
    assert_eq!(
        folded[..=configuration_end],
        expected,
        "handshake, login and configuration trace"
    );

    let play = &folded[configuration_end + 1..];
    assert_eq!(
        play.first().map(String::as_str),
        Some("play S->C login"),
        "Login opens the play state"
    );
    let milestones = [
        "play S->C login",
        "play S->C commands",
        "play S->C level_chunk_with_light",
        "play S->C player_position",
        "play C->S accept_teleportation",
        "play C->S move_player_pos_rot",
        "play C->S player_loaded",
        "play S->C player_info_update",
    ];
    let mut rest = play.iter();
    for milestone in milestones {
        assert!(
            rest.any(|entry| entry == milestone || entry.starts_with(&format!("{milestone} x"))),
            "play trace reaches {milestone} after the previous milestones: {play:#?}"
        );
    }
    assert!(
        !play
            .iter()
            .take_while(|entry| *entry != "play C->S player_loaded")
            .any(|entry| entry == "play S->C player_info_update"),
        "the player is listed only once it is loaded: {play:#?}"
    );

    assert_eq!(
        server_trace,
        [
            "Handshake handshake",
            "Login login_start",
            "Login login_acknowledged",
            "Configuration client_information",
            "Configuration known_packs",
            "Configuration finish_configuration_acknowledged",
            "Play player_loaded",
        ],
        "the server dispatched each serverbound packet in its state"
    );

    drop(connection);
    server.stop().await;
}

/// The server stores the client settings from the Client Information sent in
/// configuration, then from one sent while playing.
#[tokio::test]
async fn client_settings_are_stored_from_configuration_and_from_play() {
    let server = TestServer::start().await;
    let alice = server
        .join_with_information(
            "Alice",
            ClientInformation {
                language: "fr_fr".to_string(),
                view_distance: 5,
                chat_visibility: ChatVisibility::System,
                ..Default::default()
            },
        )
        .await;
    let uuid = alice.uuid();
    let stored = move |world: &mut World| {
        world
            .query::<(&PlayerUuid, &ClientSettings)>()
            .iter(world)
            .find(|(id, _)| id.0 == uuid)
            .map(|(_, settings)| {
                (
                    settings.locale.clone(),
                    settings.view_distance,
                    settings.chat_mode,
                )
            })
    };
    assert_eq!(
        server.with_world(stored).await,
        Some(("fr_fr".to_string(), 5, ChatMode::CommandOnly)),
        "the settings sent in configuration"
    );

    alice
        .send(PlayClientInformation {
            client_information: ClientInformation {
                language: "de_de".to_string(),
                view_distance: 3,
                chat_visibility: ChatVisibility::Hidden,
                ..Default::default()
            },
        })
        .await;
    alice.sync().await;
    assert_eq!(
        server.with_world(stored).await,
        Some(("de_de".to_string(), 3, ChatMode::Hidden)),
        "the settings sent while playing"
    );

    alice.assert_healthy();
    server.stop().await;
}
