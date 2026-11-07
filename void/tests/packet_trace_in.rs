#![cfg(feature = "packet-trace")]

use bevy_app::App;
use tokio::io::AsyncWriteExt;
use voidmc::components::ConnectionState;
use voidmc::config::{ServerConfigBuilder, ServerConfigResource};
use voidmc::network::{ClientConnected, ClientToEntityMap, IncomingPacket, NetworkPlugin};
use voidmc::packet_trace::PacketTracePlugin;
use voidmc_codec::{Encode, VarI32};
use voidmc_net::socket::{Packet, ServerSocket};
use voidmc_protocol::serverbound::{
    Handshake, HandshakePacket, LoginAcknowledged, LoginPacket, LoginStart,
};
use voidmc_protocol::{PROTOCOL_VERSION, State};

fn received(body: Vec<u8>) -> Packet {
    let mut frame = Vec::new();
    VarI32(body.len() as i32).encode(&mut frame);
    frame.extend(body);
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

fn encoded(packet: impl Encode) -> Vec<u8> {
    let mut bytes = Vec::new();
    packet.encode(&mut bytes);
    bytes
}

#[test]
fn incoming_packets_are_named_and_the_login_names_the_player() {
    let path = std::env::temp_dir().join(format!("void-in-{}.jsonl", std::process::id()));
    // SAFETY: this file holds one test, and nothing else runs yet.
    unsafe { std::env::set_var("VOID_TRACE", &path) };

    let (incoming_tx, incoming_rx) = flume::unbounded();
    let (outgoing_tx, _outgoing_rx) = flume::unbounded();
    let (_disconnect_tx, disconnect_rx) = flume::unbounded();
    let (kick_tx, _kick_rx) = flume::unbounded();
    let (connected_tx, connected_rx) = flume::unbounded();
    let mut app = App::new();
    app.add_plugins(NetworkPlugin::new(
        incoming_rx,
        outgoing_tx,
        disconnect_rx,
        kick_tx,
        connected_rx,
    ))
    .add_plugins(PacketTracePlugin)
    .insert_resource(ServerConfigResource::from(
        &ServerConfigBuilder::new().build(),
    ));
    let (client_tx, _client_rx) = flume::unbounded();
    connected_tx
        .send(ClientConnected {
            client_id: 4,
            outgoing: client_tx,
        })
        .unwrap();
    let send = |body| {
        let packet = received(body);
        incoming_tx
            .send(IncomingPacket {
                client_id: 4,
                packet,
            })
            .unwrap();
    };

    let handshake = encoded(HandshakePacket::Handshake(Handshake {
        protocol_version: PROTOCOL_VERSION,
        server_address: "localhost".to_string(),
        server_port: 25565,
        next_state: State::Login,
    }));
    let handshake_bytes = handshake.len();
    send(handshake);
    send(vec![0x7f]);
    app.update();

    let entity = app.world().resource::<ClientToEntityMap>().0[&4];
    app.world_mut()
        .entity_mut(entity)
        .insert(ConnectionState(State::Login));
    send(encoded(LoginPacket::LoginStart(LoginStart {
        name: "sonde".to_string(),
        uuid: uuid::Uuid::nil(),
    })));
    send(encoded(LoginPacket::LoginAcknowledged(
        LoginAcknowledged {},
    )));
    app.update();

    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let packets: Vec<_> = lines.iter().map(|line| line["packet"].as_str()).collect();
    assert_eq!(
        packets,
        [
            Some("Handshake/Handshake"),
            Some("Handshake/?"),
            Some("Login/LoginStart"),
            Some("Login/LoginAcknowledged"),
        ]
    );
    assert!(lines.iter().all(|line| line["dir"] == "in"));
    assert_eq!(lines[0]["bytes"], handshake_bytes);
    assert_eq!(lines[1]["bytes"], 1);
    assert!(lines[2].get("player").is_none());
    assert_eq!(lines[3]["player"], "sonde");
}
