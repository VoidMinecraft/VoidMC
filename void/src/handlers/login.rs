use bevy_ecs::prelude::*;
use void_protocol::{clientbound, serverbound};

use crate::components::{ConnectionState, PlayerName, PlayerUuid};
use crate::events::LoginPacketEvent;
use crate::network::{NetworkChannels, OutgoingPacket};

pub fn handle_login(
    mut events: MessageReader<LoginPacketEvent>,
    channels: Res<NetworkChannels>,
    mut commands: Commands,
) {
    for event in events.read() {
        match &event.packet {
            serverbound::LoginPacket::LoginStart(login_start) => {
                tracing::info!(
                    "Player {} ({}) logging in",
                    login_start.name,
                    login_start.uuid
                );

                commands.entity(event.entity).insert((
                    PlayerName(login_start.name.clone()),
                    PlayerUuid(login_start.uuid),
                ));

                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Login(
                        clientbound::LoginPacket::LoginSuccess(clientbound::LoginSuccess {
                            uuid: login_start.uuid,
                            username: login_start.name.clone(),
                            properties: vec![],
                        }),
                    ),
                });
            }
            serverbound::LoginPacket::LoginAcknowledged(_) => {
                tracing::debug!("Client {} acknowledged login", event.client_id);

                commands
                    .entity(event.entity)
                    .insert(ConnectionState(void_protocol::State::Configuration));

                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: event.client_id,
                    packet: clientbound::ClientboundPacket::Configuration(
                        clientbound::ConfigurationPacket::KnownPacks(clientbound::KnownPacks {
                            known_packs: vec![clientbound::KnownPack {
                                namespace: "minecraft".to_string(),
                                id: "core".to_string(),
                                version: "1.21.4".to_string(),
                            }],
                        }),
                    ),
                });
            }
        }
    }
}
