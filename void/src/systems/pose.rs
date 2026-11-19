//! A player's shared flags and pose as other players see them: the complete
//! shared flags byte (sneaking, invisible, glowing, from
//! [`crate::entity::metadata::flags_byte`]) and the crouching pose derived
//! from its [`Sneaking`] state. Players have no `EntityMetadata`, so this is
//! the only system that sends a player's shared flags.

use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound::entity_metadata::{entity_index, pose};
use voidmc_protocol::clientbound::{EntityMetadataValue as Value, SetEntityData};

use crate::components::{MinecraftEntityId, PlayerReady};
use crate::entity::metadata::{FlagSources, flags_byte};
use crate::players::Players;
use crate::plugins::abilities::PlayerAbilities;

/// The shared flags byte and pose of a player, as last sent to the other
/// ready players.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SharedPlayerData {
    flags: i8,
    pose: i32,
}

impl SharedPlayerData {
    /// What a client assumes for a player it just spawned.
    const SPAWNED: Self = Self {
        flags: 0,
        pose: pose::STANDING,
    };

    /// The flags from every source; the pose crouches while the player
    /// sneaks without flying, as vanilla does.
    fn of(sources: FlagSources, flying: bool) -> Self {
        let sneaking = sources.3.is_some_and(|sneaking| sneaking.0);
        Self {
            flags: flags_byte(sources),
            pose: if sneaking && !flying {
                pose::CROUCHING
            } else {
                pose::STANDING
            },
        }
    }

    fn packet(self, entity_id: i32) -> SetEntityData {
        SetEntityData::new(entity_id)
            .with(entity_index::FLAGS, Value::Byte(self.flags))
            .with(entity_index::POSE, Value::Pose(self.pose))
    }
}

/// Sends a player's complete shared flags and pose to the other ready
/// players whenever any of their sources changes, and to players who just
/// spawned it when it differs from a freshly spawned player. The player's
/// own client is never sent its own flags: it owns its crouch.
#[instrument(level = "info", skip_all)]
pub(crate) fn broadcast_shared_flags(
    mut commands: Commands,
    players: Players,
    joined: Query<Entity, Added<PlayerReady>>,
    mut ready_players: Query<(
        Entity,
        &MinecraftEntityId,
        FlagSources,
        Option<&PlayerAbilities>,
        Ref<PlayerReady>,
        Option<&mut SharedPlayerData>,
    )>,
) {
    let ready = players.ready();
    for (player, id, sources, abilities, is_ready, sent) in ready_players.iter_mut() {
        let flying = abilities.is_some_and(|abilities| abilities.flying);
        let current = SharedPlayerData::of(sources, flying);
        let known = match &sent {
            Some(sent) if !is_ready.is_added() => **sent,
            _ => SharedPlayerData::SPAWNED,
        };
        if current != known {
            ready.send_except(player, current.packet(id.0));
        } else if current != SharedPlayerData::SPAWNED {
            for viewer in joined.iter().filter(|viewer| *viewer != player) {
                players.send(viewer, current.packet(id.0));
            }
        }
        match sent {
            Some(mut sent) => {
                sent.set_if_neq(current);
            }
            None => {
                commands.entity(player).insert(current);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, PostUpdate};
    use voidmc_protocol::clientbound::entity_metadata::entity_flag;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, Sneaking};
    use crate::entity::{Effect, EffectDuration, Glowing, Invisible, StatusEffects};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

    fn test_app() -> (App, flume::Receiver<OutgoingPacket>) {
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
        .add_systems(PostUpdate, broadcast_shared_flags);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut()
            .spawn((ClientId(id), MinecraftEntityId(id as i32), PlayerReady))
            .id()
    }

    /// `(receiver, entity, shared flags, pose)` of every metadata packet sent.
    fn sent(rx: &flume::Receiver<OutgoingPacket>) -> Vec<(u32, i32, i8, i32)> {
        let mut sent: Vec<_> = rx
            .try_iter()
            .map(|out| {
                let ClientboundPacket::Play(PlayPacket::SetEntityData(data)) = out.packet else {
                    panic!("unexpected packet {:?}", out.packet);
                };
                let [flags, pose] = data.entries.as_slice() else {
                    panic!("expected flags and pose, got {:?}", data.entries);
                };
                let (Value::Byte(flags), Value::Pose(pose)) = (&flags.value, &pose.value) else {
                    panic!("unexpected values {:?}", data.entries);
                };
                (out.client_id, data.entity_id, *flags, *pose)
            })
            .collect();
        sent.sort();
        sent
    }

    const SNEAKING: u8 = entity_flag::SNEAKING;
    const GLOWING: u8 = entity_flag::GLOWING;
    const INVISIBLE: u8 = entity_flag::INVISIBLE;
    const CROUCHING: (i8, i32) = (SNEAKING as i8, pose::CROUCHING);
    const STANDING: (i8, i32) = (0, pose::STANDING);

    fn crouching(flags: u8) -> (i8, i32) {
        ((flags | SNEAKING) as i8, pose::CROUCHING)
    }

    fn standing(flags: u8) -> (i8, i32) {
        (flags as i8, pose::STANDING)
    }

    fn to(receiver: u32, entity: i32, (flags, pose): (i8, i32)) -> (u32, i32, i8, i32) {
        (receiver, entity, flags, pose)
    }

    #[test]
    fn crouching_and_standing_up_reach_the_other_players_only() {
        let (mut app, rx) = test_app();
        let alpha = player(&mut app, 1);
        player(&mut app, 2);
        player(&mut app, 3);
        app.update();
        assert!(sent(&rx).is_empty());

        app.world_mut().entity_mut(alpha).insert(Sneaking(true));
        app.update();
        assert_eq!(sent(&rx), [to(2, 1, CROUCHING), to(3, 1, CROUCHING)]);

        app.update();
        assert!(sent(&rx).is_empty(), "nothing new while crouching");

        app.world_mut().get_mut::<Sneaking>(alpha).unwrap().0 = false;
        app.update();
        assert_eq!(sent(&rx), [to(2, 1, STANDING), to(3, 1, STANDING)]);
    }

    #[test]
    fn a_flying_player_holding_shift_keeps_standing() {
        let (mut app, rx) = test_app();
        let alpha = player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut()
            .entity_mut(alpha)
            .insert(PlayerAbilities::new().flying(true));
        app.update();

        app.world_mut().entity_mut(alpha).insert(Sneaking(true));
        app.update();
        let flag_only = (entity_flag::SNEAKING as i8, pose::STANDING);
        assert_eq!(sent(&rx), [to(2, 1, flag_only)]);

        app.world_mut()
            .get_mut::<PlayerAbilities>(alpha)
            .unwrap()
            .flying = false;
        app.update();
        assert_eq!(sent(&rx), [to(2, 1, CROUCHING)]);
    }

    #[test]
    fn a_player_who_joins_sees_who_already_crouches() {
        let (mut app, rx) = test_app();
        let alpha = player(&mut app, 1);
        let beta = player(&mut app, 2);
        app.world_mut().entity_mut(alpha).insert(Sneaking(true));
        app.world_mut().entity_mut(beta).insert(Sneaking(false));
        app.update();
        sent(&rx);

        player(&mut app, 3);
        app.update();
        assert_eq!(sent(&rx), [to(3, 1, CROUCHING)]);
    }

    #[test]
    fn a_player_crouching_before_it_is_ready_is_announced_once_ready() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let late = app
            .world_mut()
            .spawn((ClientId(2), MinecraftEntityId(2), Sneaking(true)))
            .id();
        app.update();
        assert!(sent(&rx).is_empty());

        app.world_mut().entity_mut(late).insert(PlayerReady);
        app.update();
        assert_eq!(sent(&rx), [to(1, 2, CROUCHING)]);
    }

    #[test]
    fn crouching_keeps_glowing_and_invisibility() {
        let (mut app, rx) = test_app();
        let alpha = player(&mut app, 1);
        let beta = player(&mut app, 2);
        app.world_mut().entity_mut(alpha).insert(Glowing);
        app.world_mut()
            .entity_mut(beta)
            .insert(StatusEffects::new().with(Effect::Invisibility, 0, EffectDuration::Infinite));
        app.update();
        assert_eq!(
            sent(&rx),
            [to(1, 2, standing(INVISIBLE)), to(2, 1, standing(GLOWING))]
        );

        app.world_mut().entity_mut(alpha).insert(Sneaking(true));
        app.world_mut().entity_mut(beta).insert(Sneaking(true));
        app.update();
        assert_eq!(
            sent(&rx),
            [to(1, 2, crouching(INVISIBLE)), to(2, 1, crouching(GLOWING))]
        );

        app.world_mut().get_mut::<Sneaking>(alpha).unwrap().0 = false;
        app.world_mut().get_mut::<Sneaking>(beta).unwrap().0 = false;
        app.update();
        assert_eq!(
            sent(&rx),
            [to(1, 2, standing(INVISIBLE)), to(2, 1, standing(GLOWING))]
        );
    }

    #[test]
    fn glowing_and_invisibility_changing_while_crouching_keep_the_crouch() {
        let (mut app, rx) = test_app();
        let alpha = player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut().entity_mut(alpha).insert(Sneaking(true));
        app.update();
        assert_eq!(sent(&rx), [to(2, 1, CROUCHING)]);

        let mut world_step = |change: &dyn Fn(&mut EntityWorldMut)| {
            change(&mut app.world_mut().entity_mut(alpha));
            app.update();
            sent(&rx)
        };
        assert_eq!(
            world_step(&|alpha| {
                alpha.insert(Glowing);
            }),
            [to(2, 1, crouching(GLOWING))]
        );
        assert_eq!(
            world_step(&|alpha| {
                alpha.insert(StatusEffects::new().with(
                    Effect::Invisibility,
                    0,
                    EffectDuration::Infinite,
                ));
            }),
            [to(2, 1, crouching(GLOWING | INVISIBLE))]
        );
        assert_eq!(
            world_step(&|alpha| {
                alpha.remove::<Glowing>();
            }),
            [to(2, 1, crouching(INVISIBLE))]
        );
        assert_eq!(
            world_step(&|alpha| {
                alpha.insert(Invisible);
            }),
            [],
            "a second source of a bit that is already set changes nothing"
        );
        assert_eq!(
            world_step(&|alpha| {
                alpha.remove::<StatusEffects>();
            }),
            [],
            "still invisible through the marker"
        );
        assert_eq!(
            world_step(&|alpha| {
                alpha.remove::<Invisible>();
            }),
            [to(2, 1, CROUCHING)]
        );
    }

    #[test]
    fn effects_that_leave_the_flags_alone_send_nothing() {
        let (mut app, rx) = test_app();
        let alpha = player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut()
            .entity_mut(alpha)
            .insert((Sneaking(true), StatusEffects::new()));
        app.update();
        assert_eq!(sent(&rx), [to(2, 1, CROUCHING)]);

        app.world_mut()
            .get_mut::<StatusEffects>(alpha)
            .unwrap()
            .add(Effect::Speed, 1, EffectDuration::Infinite);
        app.update();
        assert!(sent(&rx).is_empty());
    }

    #[test]
    fn a_player_who_joins_gets_the_complete_flags_and_pose() {
        let (mut app, rx) = test_app();
        let alpha = player(&mut app, 1);
        let beta = player(&mut app, 2);
        app.world_mut()
            .entity_mut(alpha)
            .insert((Sneaking(true), Glowing));
        app.world_mut().entity_mut(beta).insert(Invisible);
        app.update();
        sent(&rx);

        player(&mut app, 3);
        app.update();
        assert_eq!(
            sent(&rx),
            [to(3, 1, crouching(GLOWING)), to(3, 2, standing(INVISIBLE))]
        );
    }

    #[test]
    fn a_glowing_player_is_announced_with_its_flags_once_ready() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let late = app
            .world_mut()
            .spawn((ClientId(2), MinecraftEntityId(2), Glowing))
            .id();
        app.update();
        assert!(sent(&rx).is_empty());

        app.world_mut().entity_mut(late).insert(PlayerReady);
        app.update();
        assert_eq!(sent(&rx), [to(1, 2, standing(GLOWING))]);
    }
}
