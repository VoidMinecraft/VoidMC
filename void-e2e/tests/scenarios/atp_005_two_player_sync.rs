//! VMC-ATP-005: two players see each other's movement, look, jumps and
//! crouch in both directions, keep one converging entity each, and the one
//! who leaves disappears.
//!
//! Spawning and listing each other (including a player already online), one
//! walk seen by the second player, and the first player's departure are
//! covered by `players::players_see_each_other_move_and_leave`.
//!
//! The crouch shares the shared-flags byte with glowing and invisibility:
//! the last scenarios check that every update of that byte carries all of
//! them, whichever one changed.

use azalea_entity::{EntityDataValue, LookDirection, Pose};
use bevy_ecs::world::EntityWorldMut;
use voidmc::{Effect, EffectDuration, Glowing, Invisible, StatusEffects};
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::protocol::common::movements::MoveFlags;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, s_move_player_pos::ServerboundMovePlayerPos,
    s_move_player_rot::ServerboundMovePlayerRot, s_player_input::ServerboundPlayerInput,
};
use voidmc_e2e::{Bot, TestServer};

/// Shared entity flags (byte) and pose metadata indices of every entity.
const SHARED_FLAGS: u8 = 0;
const POSE: u8 = 6;
/// Bits of the shared flags byte.
const CROUCHING_FLAG: u8 = 0x02;
const INVISIBLE_FLAG: u8 = 0x20;
const GLOWING_FLAG: u8 = 0x40;

/// The look sent in the scenarios, and its protocol angle bytes
/// (`floor(degrees * 256 / 360)`): 135° is 96, -30° is -22.
const YAW: f32 = 135.0;
const PITCH: f32 = -30.0;
const YAW_BYTE: i8 = 96;
const PITCH_BYTE: i8 = -22;

/// Delta-encoded moves carry 1/4096 of a block: a client that follows them
/// is never further than one unit from the real position.
const DELTA_PRECISION: f64 = 1.0 / 4096.0 + 1e-9;

/// What a movement packet tells a client about entity `id`.
#[derive(Debug)]
struct Moved {
    look: Option<(i8, i8)>,
    dy: f64,
    on_ground: bool,
    absolute: bool,
}

fn movement_of(id: i32, packet: &ClientboundGamePacket) -> Option<Moved> {
    match packet {
        ClientboundGamePacket::MoveEntityPos(p) if p.entity_id.0 == id => Some(Moved {
            look: None,
            dy: f64::from(p.delta.ya) / 4096.0,
            on_ground: p.on_ground,
            absolute: false,
        }),
        ClientboundGamePacket::MoveEntityPosRot(p) if p.entity_id.0 == id => Some(Moved {
            look: Some((p.look_direction.y_rot, p.look_direction.x_rot)),
            dy: f64::from(p.delta.ya) / 4096.0,
            on_ground: p.on_ground,
            absolute: false,
        }),
        ClientboundGamePacket::MoveEntityRot(p) if p.entity_id.0 == id => Some(Moved {
            look: Some((p.look_direction.y_rot, p.look_direction.x_rot)),
            dy: 0.0,
            on_ground: p.on_ground,
            absolute: false,
        }),
        ClientboundGamePacket::EntityPositionSync(p) if p.id.0 == id => Some(Moved {
            look: Some((
                angle_byte(p.values.look_direction.y_rot()),
                angle_byte(p.values.look_direction.x_rot()),
            )),
            dy: f64::NAN,
            on_ground: p.on_ground,
            absolute: true,
        }),
        _ => None,
    }
}

fn angle_byte(degrees: f32) -> i8 {
    (degrees * 256.0 / 360.0).floor() as i32 as i8
}

/// The shared flags and pose a metadata update for entity `id` carries, if
/// it carries either.
fn crouch_metadata_of(
    id: i32,
    packet: &ClientboundGamePacket,
) -> Option<(Option<u8>, Option<Pose>)> {
    let ClientboundGamePacket::SetEntityData(data) = packet else {
        return None;
    };
    if data.id.0 != id {
        return None;
    }
    let mut found = (None, None);
    for item in &data.packed_items.0 {
        match (item.index, &item.value) {
            (SHARED_FLAGS, EntityDataValue::Byte(flags)) => found.0 = Some(*flags),
            (POSE, EntityDataValue::Pose(pose)) => found.1 = Some(*pose),
            _ => {}
        }
    }
    (found != (None, None)).then_some(found)
}

/// Consumes `actor`'s metadata updates on `observer` until both the shift
/// key flag and the pose say `crouching`, as the vanilla client needs both:
/// the pose for the crouched model, the flag for the dimmed name tag.
async fn wait_for_crouch(observer: &Bot, actor: &Bot, crouching: bool) {
    let what = if crouching {
        "crouching"
    } else {
        "standing up"
    };
    let wanted_pose = if crouching {
        Pose::Crouching
    } else {
        Pose::Standing
    };
    let (mut flags, mut pose) = (None, None);
    while flags.map(|flags: u8| flags & CROUCHING_FLAG != 0) != Some(crouching)
        || pose != Some(wanted_pose)
    {
        let (new_flags, new_pose) = observer
            .expect(&format!("{} {what}", actor.name()), |packet| {
                crouch_metadata_of(actor.entity_id(), packet)
            })
            .await;
        flags = new_flags.or(flags);
        pose = new_pose.or(pose);
    }
}

/// The shared flags and pose one client knows for one player, accumulated
/// from the metadata updates it received (an update may carry either).
#[derive(Default)]
struct SharedData {
    flags: Option<u8>,
    pose: Option<Pose>,
}

/// Consumes `actor`'s metadata updates on `observer` into `seen` until the
/// shared flags are exactly `flags` and the pose is `pose`. Every flags
/// update on the way must keep the `kept` bits: a byte that drops one of them,
/// even for one update, makes the vanilla client stop rendering it.
async fn wait_for_shared_data(
    observer: &Bot,
    actor: &Bot,
    seen: &mut SharedData,
    (flags, pose): (u8, Pose),
    kept: u8,
) {
    let what = format!("{}'s flags {flags:#04x} and pose {pose:?}", actor.name());
    while seen.flags != Some(flags) || seen.pose != Some(pose) {
        let (new_flags, new_pose) = observer
            .expect(&what, |packet| {
                crouch_metadata_of(actor.entity_id(), packet)
            })
            .await;
        if let Some(new_flags) = new_flags {
            assert_eq!(
                new_flags & kept,
                kept,
                "{} got flags {new_flags:#04x} for {}, which drop bits of {kept:#04x}, \
                 while waiting for {what}",
                observer.name(),
                actor.name()
            );
        }
        seen.flags = new_flags.or(seen.flags);
        seen.pose = new_pose.or(seen.pose);
    }
}

/// Lets the server finish everything `actor` and `observer` sent, then
/// checks that `observer` holds no other shared-flags update for `actor`
/// (one that would undo the state just reached), and that no client was
/// sent its own player's shared flags (vanilla clients own their crouch).
async fn assert_settled(actor: &Bot, observer: &Bot) {
    actor.sync().await;
    observer.sync().await;
    let pending = observer.queued(|packet| crouch_metadata_of(actor.entity_id(), packet));
    assert!(
        pending.is_empty(),
        "{} has unexpected updates for {}: {pending:?}",
        observer.name(),
        actor.name()
    );
    for bot in [actor, observer] {
        let own = bot.queued(|packet| crouch_metadata_of(bot.entity_id(), packet));
        assert!(own.is_empty(), "{} got its own flags: {own:?}", bot.name());
    }
}

fn seen_position(viewer: &Bot, target: &Bot) -> Option<Vec3> {
    viewer.view(|view| {
        view.entity_by_uuid(target.uuid())
            .map(|(_, entity)| entity.position)
    })
}

async fn wait_until_seen_at(observer: &Bot, actor: &Bot, at: Vec3, tolerance: f64) {
    observer
        .wait_until(&format!("{} at {at:?}", actor.name()), |view| {
            view.entity_by_uuid(actor.uuid())
                .is_some_and(|(_, entity)| entity.position.distance_to(at) <= tolerance)
        })
        .await;
}

async fn step(actor: &Bot, to: Vec3, on_ground: bool) {
    actor
        .send(ServerboundMovePlayerPos {
            pos: to,
            flags: MoveFlags {
                on_ground,
                horizontal_collision: false,
            },
        })
        .await;
}

async fn sneak(actor: &Bot, shift: bool) {
    actor
        .send(ServerboundPlayerInput {
            shift,
            ..Default::default()
        })
        .await;
}

async fn look(actor: &Bot, yaw: f32, pitch: f32) {
    actor
        .send(ServerboundMovePlayerRot {
            look_direction: LookDirection::new(yaw, pitch),
            flags: MoveFlags {
                on_ground: true,
                horizontal_collision: false,
            },
        })
        .await;
}

/// Walks, turns, jumps and crouches with `actor`, checking after each action
/// that `observer` received what a vanilla client needs to render it.
async fn act_and_observe(actor: &Bot, observer: &Bot) {
    let id = actor.entity_id();
    let start = actor.position();

    let walked = start + Vec3::new(2.0, 0.0, -1.5);
    actor.move_to(walked).await;
    wait_until_seen_at(observer, actor, walked, DELTA_PRECISION).await;

    look(actor, YAW, PITCH).await;
    observer
        .expect(&format!("{}'s body turned", actor.name()), |packet| {
            movement_of(id, packet).filter(|moved| moved.look == Some((YAW_BYTE, PITCH_BYTE)))
        })
        .await;
    observer
        .expect(
            &format!("{}'s head turned", actor.name()),
            |packet| match packet {
                ClientboundGamePacket::RotateHead(head)
                    if head.entity_id.0 == id && head.y_head_rot == YAW_BYTE =>
                {
                    Some(())
                }
                _ => None,
            },
        )
        .await;

    let apex = walked + Vec3::new(0.0, 1.25, 0.0);
    step(actor, apex, false).await;
    observer
        .expect(&format!("{} leaving the ground", actor.name()), |packet| {
            movement_of(id, packet).filter(|moved| !moved.on_ground && moved.dy > 1.0)
        })
        .await;
    wait_until_seen_at(observer, actor, apex, DELTA_PRECISION).await;
    actor.move_to(walked).await;
    observer
        .expect(&format!("{} landing", actor.name()), |packet| {
            movement_of(id, packet).filter(|moved| moved.on_ground && moved.dy < -1.0)
        })
        .await;
    wait_until_seen_at(observer, actor, walked, DELTA_PRECISION).await;

    sneak(actor, true).await;
    wait_for_crouch(observer, actor, true).await;
    sneak(actor, false).await;
    wait_for_crouch(observer, actor, false).await;
}

#[tokio::test]
async fn players_see_each_other_walk_turn_jump_and_crouch() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    for (viewer, target) in [(&alice, &bob), (&bob, &alice)] {
        viewer
            .wait_until("the other player spawned", |view| {
                view.entity_by_uuid(target.uuid()).is_some()
            })
            .await;
    }

    act_and_observe(&alice, &bob).await;
    act_and_observe(&bob, &alice).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_joining_player_sees_the_look_and_crouch_of_one_already_online() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    look(&alice, YAW, PITCH).await;
    sneak(&alice, true).await;
    alice.sync().await;

    let bob = server.join("Bob").await;
    let spawn = bob
        .expect("Alice spawned", |packet| match packet {
            ClientboundGamePacket::AddEntity(add) if add.id.0 == alice.entity_id() => {
                Some(add.clone())
            }
            _ => None,
        })
        .await;
    assert_eq!(
        (spawn.y_rot, spawn.x_rot, spawn.y_head_rot),
        (YAW_BYTE, PITCH_BYTE, YAW_BYTE),
        "Alice spawns facing where she looks"
    );
    wait_for_crouch(&bob, &alice, true).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn each_player_stays_one_entity_that_follows_many_moves_and_long_hops() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    for (actor, observer) in [(&alice, &bob), (&bob, &alice)] {
        wait_until_seen_at(observer, actor, actor.position(), 0.0).await;
        let id = actor.entity_id();
        let mut at = actor.position();

        // 0.3 blocks is 1228.8 delta units: each step's delta must come from
        // the encoded positions, or the remote copy drifts away step by step.
        for _ in 0..30 {
            at += Vec3::new(0.3, 0.0, -0.3);
            actor.move_to(at).await;
            wait_until_seen_at(observer, actor, at, DELTA_PRECISION).await;
        }

        // Beyond 8 blocks a delta cannot encode the move.
        at += Vec3::new(20.0, 0.0, 12.0);
        actor.move_to(at).await;
        observer
            .expect(&format!("{}'s absolute position", actor.name()), |packet| {
                movement_of(id, packet).filter(|moved| moved.absolute)
            })
            .await;
        wait_until_seen_at(observer, actor, at, 1e-9).await;

        for _ in 0..5 {
            at += Vec3::new(-0.3, 0.0, 0.3);
            actor.move_to(at).await;
            wait_until_seen_at(observer, actor, at, DELTA_PRECISION).await;
        }

        actor.sync().await;
        observer.sync().await;
        let copies = observer.view(|view| {
            view.entities
                .iter()
                .filter(|(_, entity)| entity.uuid == actor.uuid())
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            copies,
            [id],
            "{} sees {} as exactly one entity",
            observer.name(),
            actor.name()
        );
        let seen = seen_position(observer, actor).expect("still seen");
        assert!(
            seen.distance_to(at) <= DELTA_PRECISION,
            "{seen:?} vs {at:?}"
        );
    }

    let bob_uuid = bob.uuid();
    bob.disconnect().await;
    alice
        .wait_until("Bob gone from the world and the tab list", |view| {
            view.entity_by_uuid(bob_uuid).is_none() && !view.players.contains_key(&bob_uuid)
        })
        .await;

    alice.assert_healthy();
    server.stop().await;
}

/// Runs `f` on the server-side entity of `bot`'s player, the way an
/// application changes a player's components.
async fn on_player(
    server: &TestServer,
    bot: &Bot,
    f: impl FnOnce(&mut EntityWorldMut) + Send + 'static,
) {
    let entity = server.player_entity(bot.uuid()).await;
    server
        .with_world(move |world| f(&mut world.entity_mut(entity)))
        .await;
}

#[tokio::test]
async fn glowing_and_invisible_players_keep_those_flags_while_they_crouch() {
    use Pose::{Crouching, Standing};

    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    for (viewer, target) in [(&alice, &bob), (&bob, &alice)] {
        viewer
            .wait_until("the other player spawned", |view| {
                view.entity_by_uuid(target.uuid()).is_some()
            })
            .await;
    }
    let mut bob_sees_alice = SharedData::default();
    let mut alice_sees_bob = SharedData::default();

    // Both public ways to set the bits: a marker component and an effect.
    on_player(&server, &alice, |alice| {
        alice.insert(Glowing);
    })
    .await;
    on_player(&server, &bob, |bob| {
        bob.insert(StatusEffects::new().with(Effect::Invisibility, 0, EffectDuration::Infinite));
    })
    .await;
    let glowing = (GLOWING_FLAG, Standing);
    wait_for_shared_data(&bob, &alice, &mut bob_sees_alice, glowing, 0).await;
    let invisible = (INVISIBLE_FLAG, Standing);
    wait_for_shared_data(&alice, &bob, &mut alice_sees_bob, invisible, 0).await;

    // Crouching and standing up keep glowing and invisibility.
    for (actor, observer, seen, bit) in [
        (&alice, &bob, &mut bob_sees_alice, GLOWING_FLAG),
        (&bob, &alice, &mut alice_sees_bob, INVISIBLE_FLAG),
    ] {
        sneak(actor, true).await;
        wait_for_shared_data(
            observer,
            actor,
            seen,
            (bit | CROUCHING_FLAG, Crouching),
            bit,
        )
        .await;
        sneak(actor, false).await;
        wait_for_shared_data(observer, actor, seen, (bit, Standing), bit).await;
        assert_settled(actor, observer).await;
    }

    // Glowing and invisibility changing while crouching keep the crouch.
    sneak(&alice, true).await;
    let glowing_crouch = (GLOWING_FLAG | CROUCHING_FLAG, Crouching);
    wait_for_shared_data(
        &bob,
        &alice,
        &mut bob_sees_alice,
        glowing_crouch,
        GLOWING_FLAG,
    )
    .await;
    on_player(&server, &alice, |alice| {
        alice.remove::<Glowing>();
    })
    .await;
    let crouch = (CROUCHING_FLAG, Crouching);
    wait_for_shared_data(&bob, &alice, &mut bob_sees_alice, crouch, CROUCHING_FLAG).await;
    on_player(&server, &alice, |alice| {
        alice.insert(StatusEffects::new().with(Effect::Glowing, 0, EffectDuration::Infinite));
    })
    .await;
    wait_for_shared_data(
        &bob,
        &alice,
        &mut bob_sees_alice,
        glowing_crouch,
        CROUCHING_FLAG,
    )
    .await;
    on_player(&server, &alice, |alice| {
        alice.insert(Invisible);
    })
    .await;
    let everything = (GLOWING_FLAG | INVISIBLE_FLAG | CROUCHING_FLAG, Crouching);
    let kept = glowing_crouch.0;
    wait_for_shared_data(&bob, &alice, &mut bob_sees_alice, everything, kept).await;
    on_player(&server, &alice, |alice| {
        alice.remove::<StatusEffects>();
    })
    .await;
    let invisible_crouch = (INVISIBLE_FLAG | CROUCHING_FLAG, Crouching);
    let kept = invisible_crouch.0;
    wait_for_shared_data(&bob, &alice, &mut bob_sees_alice, invisible_crouch, kept).await;
    assert_settled(&alice, &bob).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_joining_player_sees_the_complete_flags_of_a_glowing_crouching_player() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    on_player(&server, &alice, |alice| {
        alice.insert(Glowing);
    })
    .await;
    sneak(&alice, true).await;
    alice.sync().await;

    let bob = server.join("Bob").await;
    bob.wait_until("Alice spawned", |view| {
        view.entity_by_uuid(alice.uuid()).is_some()
    })
    .await;
    let mut seen = SharedData::default();
    let everything = (GLOWING_FLAG | CROUCHING_FLAG, Pose::Crouching);
    wait_for_shared_data(&bob, &alice, &mut seen, everything, 0).await;
    assert_settled(&alice, &bob).await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}
