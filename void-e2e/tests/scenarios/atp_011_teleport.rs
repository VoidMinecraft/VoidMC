//! VMC-ATP-011: `/tp` authoritatively moves a player to the supplied
//! coordinates, the client acknowledges the position, movement stays
//! responsive afterwards and no duplicate player entity appears.

use std::collections::HashSet;

use azalea_entity::LookDirection;
use voidmc::components::{Position, Rotation, TeleportState};
use voidmc_e2e::core::position::{ChunkPos, Vec3};
use voidmc_e2e::protocol::common::movements::{MoveFlags, RelativeMovements};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, c_player_position::ClientboundPlayerPosition,
    s_accept_teleportation::ServerboundAcceptTeleportation,
    s_move_player_pos::ServerboundMovePlayerPos,
    s_move_player_pos_rot::ServerboundMovePlayerPosRot,
};
use voidmc_e2e::{Bot, BotOptions, TestServer};

use crate::feet;

const FLAGS: MoveFlags = MoveFlags {
    on_ground: true,
    horizontal_collision: false,
};

/// What the server holds for a player: position, look and unconfirmed
/// teleport id.
#[derive(Debug, PartialEq)]
struct ServerSide {
    position: Vec3,
    look: (f32, f32),
    pending: Option<i32>,
}

async fn server_side(server: &TestServer, bot: &Bot) -> ServerSide {
    let entity = server.player_entity(bot.uuid()).await;
    server
        .with_world(move |world| {
            let position = world.get::<Position>(entity).unwrap();
            let rotation = world.get::<Rotation>(entity).unwrap();
            ServerSide {
                position: Vec3::new(position.x, position.y, position.z),
                look: (rotation.yaw, rotation.pitch),
                pending: world.get::<TeleportState>(entity).unwrap().pending_id,
            }
        })
        .await
}

/// Removes the position sync of the join from the queue, so that later
/// expectations see only the teleports under test.
async fn skip_join_teleport(bot: &Bot) {
    bot.expect("the join teleport", |packet| match packet {
        ClientboundGamePacket::PlayerPosition(teleport) if teleport.id == 0 => Some(()),
        _ => None,
    })
    .await;
}

/// Runs `/tp` and returns the position sync it produced.
async fn teleport(bot: &Bot, destination: Vec3) -> ClientboundPlayerPosition {
    bot.command(&format!(
        "tp {} {} {}",
        destination.x, destination.y, destination.z
    ))
    .await;
    bot.expect("the teleport", |packet| match packet {
        ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.clone()),
        _ => None,
    })
    .await
}

/// Returns once the server has handled the bot's automatic answer to every
/// teleport it received so far: the first barrier is read after the bot
/// answered, the second one after the server handled that answer.
async fn settle(bot: &Bot) {
    bot.sync().await;
    bot.sync().await;
}

async fn send_position(bot: &Bot, pos: Vec3) {
    bot.send(ServerboundMovePlayerPos { pos, flags: FLAGS })
        .await;
}

async fn accept(bot: &Bot, id: u32) {
    bot.send(ServerboundAcceptTeleportation { id }).await;
}

fn sees_at(viewer: &Bot, target: &Bot, position: Vec3) -> bool {
    viewer.view(|view| {
        view.entity_by_uuid(target.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(position) < 1e-3)
    })
}

fn square(center: ChunkPos, radius: i32) -> HashSet<ChunkPos> {
    (-radius..=radius)
        .flat_map(|dx| {
            (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
        })
        .collect()
}

fn system_chat(bot: &Bot) -> Vec<String> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => Some(chat.content.to_string()),
        _ => None,
    })
}

#[tokio::test]
async fn tp_lands_on_the_exact_coordinates_keeps_the_look_and_the_same_entity() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    skip_join_teleport(&alice).await;
    let bob = server.join("Bob").await;
    let start = alice.position();
    bob.wait_until("Alice spawned", |view| {
        view.entity_by_uuid(alice.uuid()).is_some()
    })
    .await;
    alice
        .send(ServerboundMovePlayerPosRot {
            pos: start,
            look_direction: LookDirection::new(30.0, 5.0),
            flags: FLAGS,
        })
        .await;
    alice.sync().await;
    bob.sync().await;
    bob.clear();
    let alice_tab = alice.view(|view| view.players.clone());
    let bob_tab = bob.view(|view| view.players.clone());

    let below_world = Vec3::new(5.0, -200.0, 5.0);
    let destinations = [
        ("integers", "tp 10 80 10", Vec3::new(10.0, 80.0, 10.0)),
        (
            "decimals",
            "tp 12.25 81.5 -7.75",
            Vec3::new(12.25, 81.5, -7.75),
        ),
        ("negatives", "tp -20 70 -30", Vec3::new(-20.0, 70.0, -30.0)),
        ("below the world", "tp 5 -200 5", below_world),
        ("relative", "tp ~1 ~2.5 ~-3", Vec3::new(6.0, -197.5, 2.0)),
        (
            "above the world",
            "tp 0.5 1000 0.5",
            Vec3::new(0.5, 1000.0, 0.5),
        ),
    ];
    let mut ids = Vec::new();
    for (what, command, expected) in destinations {
        alice.command(command).await;
        let sync = alice
            .expect("the teleport", |packet| match packet {
                ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.clone()),
                _ => None,
            })
            .await;
        assert_eq!(sync.change.pos, expected, "{what}: exact coordinates");
        assert_eq!(
            sync.relative,
            RelativeMovements::all_absolute(),
            "{what}: every field is absolute, so the client does not add it to its own"
        );
        assert_eq!(sync.change.delta, Vec3::ZERO, "{what}: no velocity");
        assert_eq!(
            (
                sync.change.look_direction.y_rot(),
                sync.change.look_direction.x_rot()
            ),
            (30.0, 5.0),
            "{what}: /tp keeps the look direction"
        );
        assert!(
            ids.last().is_none_or(|last| sync.id > *last),
            "{what}: a fresh teleport id, got {} after {ids:?}",
            sync.id
        );
        ids.push(sync.id);
        alice
            .expect("the /tp reply", |packet| match packet {
                ClientboundGamePacket::SystemChat(chat) => chat
                    .content
                    .to_string()
                    .starts_with("Teleported to")
                    .then_some(()),
                _ => None,
            })
            .await;
        assert_eq!(alice.position(), expected);

        settle(&alice).await;
        assert_eq!(
            server_side(&server, &alice).await,
            ServerSide {
                position: expected,
                look: (30.0, 5.0),
                pending: None,
            },
            "{what}: the server holds the destination and the client confirmed it"
        );
        bob.wait_until(&format!("Alice teleported ({what})"), |view| {
            view.entity_by_uuid(alice.uuid())
                .is_some_and(|(id, entity)| {
                    id == alice.entity_id() && entity.position.distance_to(expected) < 1e-3
                })
        })
        .await;
    }

    // Movement after the teleport is accepted from the new position, with no
    // teleport back.
    let mut position = alice.position();
    for _ in 0..40 {
        position = Vec3::new(position.x + 0.25, position.y, position.z - 0.25);
        alice.move_to(position).await;
    }
    alice.sync().await;
    assert_eq!(server_side(&server, &alice).await.position, position);
    bob.wait_until("Alice walking from the destination", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(position) < 1e-3)
    })
    .await;
    bob.sync().await;
    assert!(
        alice
            .queued(|packet| match packet {
                ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.id),
                _ => None,
            })
            .is_empty(),
        "walking after a teleport is not rubber-banded"
    );

    // One entity throughout: Bob never re-adds or removes Alice, Alice is
    // never told about herself, and nobody's tab list changes.
    let alice_id = alice.entity_id();
    assert!(
        bob.queued(|packet| match packet {
            ClientboundGamePacket::AddEntity(add)
                if add.uuid == alice.uuid() || add.id.0 == alice_id =>
                Some(()),
            ClientboundGamePacket::RemoveEntities(remove)
                if remove.entity_ids.iter().any(|id| id.0 == alice_id) =>
                Some(()),
            ClientboundGamePacket::PlayerInfoRemove(_) => Some(()),
            _ => None,
        })
        .is_empty(),
        "Bob saw Alice's entity re-created or removed"
    );
    assert_eq!(
        bob.view(|view| {
            view.entities
                .values()
                .filter(|entity| entity.uuid == alice.uuid())
                .count()
        }),
        1
    );
    assert!(
        alice
            .queued(|packet| match packet {
                ClientboundGamePacket::AddEntity(add)
                    if add.uuid == alice.uuid() || add.id.0 == alice_id =>
                    Some(()),
                _ => None,
            })
            .is_empty(),
        "Alice was sent an entity for herself"
    );
    assert_eq!(alice.view(|view| view.players.clone()), alice_tab);
    assert_eq!(bob.view(|view| view.players.clone()), bob_tab);

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn stale_movement_cannot_undo_an_unconfirmed_teleport() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    skip_join_teleport(&alice).await;
    let bob = server.join("Bob").await;
    alice.sync().await;
    let start = alice.position();
    bob.wait_until("Alice at spawn", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(start) < 1e-3)
    })
    .await;
    alice.hold_teleports(true);

    // The client keeps sending its old position until it processes the
    // teleport: those packets must not move the player back.
    let first_stop = Vec3::new(start.x + 40.0, start.y + 10.0, start.z);
    let first = teleport(&alice, first_stop).await;
    send_position(&alice, start).await;
    alice.sync().await;
    assert_eq!(
        server_side(&server, &alice).await.position,
        first_stop,
        "a position sent before the accept undid the teleport"
    );

    // A wrong id is ignored and confirms nothing.
    accept(&alice, first.id + 100).await;
    send_position(&alice, start).await;
    alice.sync().await;
    let state = server_side(&server, &alice).await;
    assert_eq!(state.position, first_stop);
    assert_eq!(state.pending, Some(first.id as i32));

    // A client that does not answer leaves the server at the destination.
    server.wait_ticks(20).await;
    let state = server_side(&server, &alice).await;
    assert_eq!(state.position, first_stop);
    assert_eq!(state.pending, Some(first.id as i32));
    bob.wait_until("Alice at the first stop", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(first_stop) < 1e-3)
    })
    .await;

    // A second teleport before the first is accepted: only its id confirms,
    // and the late answer to the first one with its echo changes nothing.
    let second_stop = Vec3::new(start.x - 30.0, start.y + 5.0, start.z + 20.0);
    let second = teleport(&alice, second_stop).await;
    assert!(second.id > first.id);
    accept(&alice, first.id).await;
    alice
        .send(ServerboundMovePlayerPosRot {
            pos: first_stop,
            look_direction: LookDirection::new(0.0, 0.0),
            flags: FLAGS,
        })
        .await;
    alice.sync().await;
    let state = server_side(&server, &alice).await;
    assert_eq!(state.position, second_stop);
    assert_eq!(state.pending, Some(second.id as i32));

    accept(&alice, second.id).await;
    send_position(&alice, second_stop).await;
    alice.sync().await;
    let state = server_side(&server, &alice).await;
    assert_eq!(state.position, second_stop);
    assert_eq!(state.pending, None);

    // A duplicate accept is ignored and movement goes on.
    accept(&alice, second.id).await;
    let step = Vec3::new(second_stop.x + 1.0, second_stop.y, second_stop.z);
    alice.move_to(step).await;
    alice.sync().await;
    let state = server_side(&server, &alice).await;
    assert_eq!(state.position, step);
    assert_eq!(state.pending, None);
    bob.wait_until("Alice one step from the second stop", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(step) < 1e-3)
    })
    .await;
    bob.sync().await;

    let alice_id = alice.entity_id();
    let synced_to: Vec<Vec3> = bob.queued(|packet| match packet {
        ClientboundGamePacket::EntityPositionSync(sync) if sync.id.0 == alice_id => {
            Some(sync.values.pos)
        }
        _ => None,
    });
    assert!(
        !synced_to.contains(&start),
        "Bob saw Alice pulled back by a stale position: {synced_to:?}"
    );
    assert!(sees_at(&bob, &alice, step));
    assert!(
        alice
            .queued(|packet| match packet {
                ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.id),
                _ => None,
            })
            .is_empty(),
        "no unsolicited teleport"
    );

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn teleporting_far_streams_the_destination_and_back() {
    let server = TestServer::builder().operators().start().await;
    let alice = server
        .join_with("Alice", BotOptions { view_distance: 2 })
        .await;
    skip_join_teleport(&alice).await;
    let spawn = ChunkPos::from(feet(&alice));
    alice
        .wait_until("the spawn chunks", |view| {
            view.chunks.keys().copied().collect::<HashSet<_>>() == square(spawn, 2)
        })
        .await;
    let start = alice.position();

    for (what, destination) in [
        (
            "beyond the generated area",
            Vec3::new(1000.5, 100.0, -2000.5),
        ),
        (
            "ten million blocks out, above the world",
            Vec3::new(10_000_000.5, 1000.0, -10_000_000.5),
        ),
    ] {
        let sync = teleport(&alice, destination).await;
        assert_eq!(sync.change.pos, destination);
        let target = ChunkPos::from(feet(&alice));
        alice
            .wait_until(&format!("the chunks around {what}"), |view| {
                view.chunk_center == Some(target)
                    && view.chunks.keys().copied().collect::<HashSet<_>>() == square(target, 2)
            })
            .await;
        settle(&alice).await;
        assert_eq!(server_side(&server, &alice).await.position, destination);
    }

    alice.clear();
    teleport(&alice, start).await;
    alice
        .wait_until("the spawn chunks again", |view| {
            view.chunk_center == Some(spawn)
                && view.chunks.keys().copied().collect::<HashSet<_>>() == square(spawn, 2)
        })
        .await;
    settle(&alice).await;
    let resent: Vec<ChunkPos> = alice.queued(|packet| match packet {
        ClientboundGamePacket::LevelChunkWithLight(chunk) => Some(ChunkPos::new(chunk.x, chunk.z)),
        _ => None,
    });
    assert_eq!(resent.len(), 25, "each spawn chunk is sent back once");
    assert_eq!(resent.into_iter().collect::<HashSet<_>>(), square(spawn, 2));
    assert_eq!(server_side(&server, &alice).await.position, start);

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn invalid_tp_input_is_rejected_without_moving_anyone() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    skip_join_teleport(&alice).await;
    let bob = server.join("Bob").await;
    alice.sync().await;
    bob.sync().await;
    let before = server_side(&server, &alice).await;
    let position = alice.position();
    alice.clear();
    bob.clear();

    let parse_errors = [
        "tp",
        "tp 1 2",
        "tp 1 2 3 4",
        "tp a b c",
        "tp 1 NaN 3",
        "tp inf 64 0",
        "tp 1 ~x 3",
        "tp ^1 ~ 3",
    ];
    let out_of_bounds = [
        "tp 30000000 64 0",
        "tp 0 64 -30000001",
        "tp 0 20000000 0",
        "tp 0 -20000001 0",
        "tp 1e300 64 0",
        "tp ~ ~1e308 ~",
    ];
    for command in parse_errors.iter().chain(&out_of_bounds) {
        alice.command(command).await;
        alice.sync().await;
        let chat = system_chat(&alice);
        let expected = if parse_errors.contains(command) {
            "Usage: /tp"
        } else {
            "Invalid position for teleport"
        };
        assert!(
            chat.iter().any(|line| line.starts_with(expected)),
            "/{command} answered {chat:?}, expected {expected:?}"
        );
        assert!(
            alice
                .queued(|packet| match packet {
                    ClientboundGamePacket::PlayerPosition(_) => Some(()),
                    _ => None,
                })
                .is_empty(),
            "/{command} teleported"
        );
        alice.clear();
    }

    assert_eq!(server_side(&server, &alice).await, before);
    assert_eq!(alice.position(), position);
    bob.sync().await;
    assert!(
        bob.queued(|packet| match packet {
            ClientboundGamePacket::EntityPositionSync(_)
            | ClientboundGamePacket::MoveEntityPos(_)
            | ClientboundGamePacket::MoveEntityPosRot(_) => Some(()),
            _ => None,
        })
        .is_empty(),
        "Bob saw Alice move"
    );

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}
