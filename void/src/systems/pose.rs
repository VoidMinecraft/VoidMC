//! Crouching as other players see it: the shift key flag and the crouching
//! pose in a player's synched data, derived from its [`Sneaking`] state.

use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound::entity_metadata::{entity_flag, entity_index, pose};
use voidmc_protocol::clientbound::{EntityMetadataValue as Value, SetEntityData};

use crate::components::{MinecraftEntityId, PlayerReady, Sneaking};
use crate::players::Players;
use crate::plugins::abilities::PlayerAbilities;

/// The shared flags and pose vanilla derives from the shift key: the flag
/// follows the key, the pose crouches unless the player is flying.
fn crouch_metadata(entity_id: i32, sneaking: bool, flying: bool) -> SetEntityData {
    let flags = if sneaking { entity_flag::SNEAKING } else { 0 };
    let pose = if sneaking && !flying {
        pose::CROUCHING
    } else {
        pose::STANDING
    };
    SetEntityData::new(entity_id)
        .with(entity_index::FLAGS, Value::Byte(flags as i8))
        .with(entity_index::POSE, Value::Pose(pose))
}

/// Sends a player's crouch to the other ready players when it changes, and
/// a crouching player's state to players who just spawned it.
#[instrument(level = "info", skip_all)]
pub fn broadcast_crouch(
    players: Players,
    joined: Query<Entity, Added<PlayerReady>>,
    sneakers: Query<(
        Entity,
        &MinecraftEntityId,
        Ref<Sneaking>,
        Option<Ref<PlayerAbilities>>,
        Ref<PlayerReady>,
    )>,
) {
    let ready = players.ready();
    for (player, id, sneaking, abilities, is_ready) in sneakers.iter() {
        let flying = abilities.as_ref().is_some_and(|abilities| abilities.flying);
        let abilities_changed = abilities.is_some_and(|abilities| abilities.is_changed());
        let packet = || crouch_metadata(id.0, sneaking.0, flying);
        if sneaking.is_changed() || (sneaking.0 && (abilities_changed || is_ready.is_added())) {
            ready.send_except(player, packet());
        } else if sneaking.0 {
            for viewer in joined.iter().filter(|viewer| *viewer != player) {
                players.send(viewer, packet());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, PostUpdate};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::ClientId;
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
        .add_systems(PostUpdate, broadcast_crouch);
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

    const CROUCHING: (i8, i32) = (entity_flag::SNEAKING as i8, pose::CROUCHING);
    const STANDING: (i8, i32) = (0, pose::STANDING);

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
}
