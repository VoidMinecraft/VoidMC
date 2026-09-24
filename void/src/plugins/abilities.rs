//! Player abilities as a component: insert or mutate [`PlayerAbilities`] on a
//! player and the client receives the matching Player Abilities packet in
//! `PostUpdate`. The client's own fly toggles are folded back into the
//! component without a round trip, unless flight is not allowed.

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use voidmc_protocol::{clientbound, serverbound};

use crate::components::PlayerReady;
use crate::network::PacketEvent;
use crate::players::Players;
use crate::schedule::VoidSystems;

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PlayerAbilities {
    pub invulnerable: bool,
    pub flying: bool,
    pub allow_flight: bool,
    pub instant_build: bool,
    pub flying_speed: f32,
    pub walking_speed: f32,
}

impl Default for PlayerAbilities {
    fn default() -> Self {
        Self {
            invulnerable: false,
            flying: false,
            allow_flight: false,
            instant_build: false,
            flying_speed: Self::DEFAULT_FLYING_SPEED,
            walking_speed: Self::DEFAULT_WALKING_SPEED,
        }
    }
}

impl PlayerAbilities {
    pub const DEFAULT_FLYING_SPEED: f32 = 0.05;
    pub const DEFAULT_WALKING_SPEED: f32 = 0.1;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn invulnerable(mut self, invulnerable: bool) -> Self {
        self.invulnerable = invulnerable;
        self
    }

    /// Whether the player may toggle flight (double-tap jump).
    pub fn allow_flight(mut self, allow: bool) -> Self {
        self.allow_flight = allow;
        if !allow {
            self.flying = false;
        }
        self
    }

    /// Starts or stops flying; `true` also allows flight.
    pub fn flying(mut self, flying: bool) -> Self {
        self.flying = flying;
        if flying {
            self.allow_flight = true;
        }
        self
    }

    pub fn instant_build(mut self, instant_build: bool) -> Self {
        self.instant_build = instant_build;
        self
    }

    pub fn flying_speed(mut self, speed: f32) -> Self {
        self.flying_speed = speed;
        self
    }

    pub fn walking_speed(mut self, speed: f32) -> Self {
        self.walking_speed = speed;
        self
    }

    pub fn flags(&self) -> u8 {
        let mut flags = 0;
        if self.invulnerable {
            flags |= clientbound::PlayerAbilities::INVULNERABLE;
        }
        if self.flying {
            flags |= clientbound::PlayerAbilities::FLYING;
        }
        if self.allow_flight {
            flags |= clientbound::PlayerAbilities::ALLOW_FLYING;
        }
        if self.instant_build {
            flags |= clientbound::PlayerAbilities::INSTANT_BUILD;
        }
        flags
    }

    pub fn packet(&self) -> clientbound::PlayerAbilities {
        clientbound::PlayerAbilities {
            flags: self.flags(),
            flying_speed: self.flying_speed,
            walking_speed: self.walking_speed,
        }
    }
}

pub struct AbilitiesPlugin;

impl Plugin for AbilitiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(track_client_fly_toggle).add_systems(
            PostUpdate,
            sync_player_abilities.in_set(VoidSystems::AbilitiesSync),
        );
    }
}

fn sync_player_abilities(
    players: Players,
    changed: Query<
        (Entity, &PlayerAbilities),
        (
            With<PlayerReady>,
            Or<(Changed<PlayerAbilities>, Added<PlayerReady>)>,
        ),
    >,
) {
    for (player, abilities) in changed.iter() {
        players.send(player, abilities.packet());
    }
}

fn track_client_fly_toggle(
    event: On<PacketEvent<serverbound::PlayerAbilities>>,
    mut abilities: Query<&mut PlayerAbilities>,
) {
    let Ok(mut abilities) = abilities.get_mut(event.entity) else {
        return;
    };
    let flying = event.packet.flags & clientbound::PlayerAbilities::FLYING != 0;
    if flying && !abilities.allow_flight {
        abilities.flying = false;
    } else {
        abilities.bypass_change_detection().flying = flying;
    }
}

#[cfg(test)]
mod tests {
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::ClientId;
    use crate::network::{NetworkChannels, OutgoingPacket};

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
        .add_plugins(AbilitiesPlugin);
        (app, outgoing_rx)
    }

    fn abilities_packets(receiver: &Receiver<OutgoingPacket>) -> Vec<clientbound::PlayerAbilities> {
        receiver
            .drain()
            .filter_map(|p| match p.packet {
                ClientboundPacket::Play(PlayPacket::PlayerAbilities(a)) => Some(a),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn builder_sets_protocol_flags() {
        let abilities = PlayerAbilities::new()
            .flying(true)
            .invulnerable(true)
            .flying_speed(0.2);
        assert!(abilities.allow_flight);
        assert_eq!(abilities.flags(), 0x07);
        assert_eq!(abilities.packet().flying_speed, 0.2);
        assert_eq!(abilities.allow_flight(false).flags(), 0x01);
        assert_eq!(PlayerAbilities::new().instant_build(true).flags(), 0x08);
    }

    #[test]
    fn changes_are_sent_once_and_only_to_ready_players() {
        let (mut app, receiver) = test_app();
        let player = app
            .world_mut()
            .spawn((ClientId(1), PlayerAbilities::new().flying(true)))
            .id();
        app.update();
        assert!(abilities_packets(&receiver).is_empty());

        app.world_mut().entity_mut(player).insert(PlayerReady);
        app.update();
        let sent = abilities_packets(&receiver);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].flags, 0x06);
        app.update();
        assert!(abilities_packets(&receiver).is_empty());

        app.world_mut()
            .get_mut::<PlayerAbilities>(player)
            .unwrap()
            .walking_speed = 0.3;
        app.update();
        let sent = abilities_packets(&receiver);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].walking_speed, 0.3);
    }

    #[test]
    fn client_toggle_is_tracked_silently_but_corrected_when_flight_is_not_allowed() {
        let (mut app, receiver) = test_app();
        let player = app
            .world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                PlayerAbilities::new().allow_flight(true),
            ))
            .id();
        app.update();
        receiver.drain();

        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity: player,
            packet: serverbound::PlayerAbilities { flags: 0x02 },
        });
        app.update();
        assert!(app.world().get::<PlayerAbilities>(player).unwrap().flying);
        assert!(abilities_packets(&receiver).is_empty());

        app.world_mut()
            .get_mut::<PlayerAbilities>(player)
            .unwrap()
            .allow_flight = false;
        app.update();
        receiver.drain();
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity: player,
            packet: serverbound::PlayerAbilities { flags: 0x02 },
        });
        app.update();
        assert!(!app.world().get::<PlayerAbilities>(player).unwrap().flying);
        let sent = abilities_packets(&receiver);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].flags, 0);
    }
}
