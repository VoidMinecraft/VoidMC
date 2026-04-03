use bevy_ecs::prelude::*;
use void_protocol::{clientbound, serverbound};

use crate::components::{ConnectionState, PlayerName, PlayerUuid};
use crate::network::{NetworkChannels, OutgoingPacket};

pub fn handle_login_packet(
    world: &mut World,
    client_id: u32,
    entity: Entity,
    packet: serverbound::LoginPacket,
) {
    let sender = world.resource::<NetworkChannels>().outgoing.clone();

    match &packet {
        serverbound::LoginPacket::LoginStart(login_start) => {
            tracing::info!(
                "Player {} ({}) logging in",
                login_start.name,
                login_start.uuid
            );

            world.entity_mut(entity).insert((
                PlayerName(login_start.name.clone()),
                PlayerUuid(login_start.uuid),
            ));

            let _ = sender.send(OutgoingPacket {
                client_id,
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
            tracing::debug!("Client {} acknowledged login", client_id);

            world
                .entity_mut(entity)
                .insert(ConnectionState(void_protocol::State::Configuration));

            let _ = sender.send(OutgoingPacket {
                client_id,
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
