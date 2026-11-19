//! VMC-ATP-016: `/summon` validates the entity type and creates a visible
//! non-player entity with a stable identity and position.

use bevy_ecs::prelude::With;
use uuid::Uuid;
use voidmc::EntityKind as VoidKind;
use voidmc::components::{EntityType, EntityUuid, MinecraftEntityId, Position, SpawnedEntity};
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::registry::builtin::EntityKind;
use voidmc_e2e::{Bot, SeenEntity, TestServer};

/// A non-player entity as the server holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ServerEntity {
    id: i32,
    uuid: Uuid,
    kind: i32,
    position: [f64; 3],
}

async fn server_entities(server: &TestServer) -> Vec<ServerEntity> {
    server
        .with_world(|world| {
            let mut entities: Vec<ServerEntity> = world
                .query_filtered::<(&MinecraftEntityId, &EntityUuid, &EntityType, &Position), With<SpawnedEntity>>()
                .iter(world)
                .map(|(id, uuid, kind, position)| ServerEntity {
                    id: id.0,
                    uuid: uuid.0,
                    kind: kind.0,
                    position: [position.x, position.y, position.z],
                })
                .collect();
            entities.sort_by_key(|entity| entity.id);
            entities
        })
        .await
}

/// Waits for the confirmation `/summon` sends its executor.
async fn expect_confirmation(bot: &Bot) -> String {
    bot.expect("the summon confirmation", |packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => {
            let text = chat.content.to_string();
            text.starts_with("Summoned minecraft:pig at ")
                .then_some(text)
        }
        _ => None,
    })
    .await
}

/// Waits for the add-entity packet of the next pig, decoded by Azalea.
async fn expect_pig(bot: &Bot) -> (i32, SeenEntity) {
    bot.expect("a pig added", |packet| match packet {
        ClientboundGamePacket::AddEntity(add) if add.entity_type == EntityKind::Pig => Some((
            add.id.0,
            SeenEntity {
                uuid: add.uuid,
                kind: add.entity_type,
                position: add.position,
            },
        )),
        _ => None,
    })
    .await
}

/// Waits for a red system message containing `needle`: a command error.
async fn expect_error(bot: &Bot, input: &str, needle: &str) {
    let what = format!("the error for `/{input}`");
    bot.expect(&what, |packet| match packet {
        ClientboundGamePacket::SystemChat(chat) if !chat.overlay => {
            let red = chat
                .content
                .get_base()
                .style
                .color
                .as_ref()
                .is_some_and(|color| color.name.as_deref() == Some("red"));
            (red && chat.content.to_string().contains(needle)).then_some(())
        }
        _ => None,
    })
    .await;
}

/// The ids of every queued packet that adds, removes, moves, turns or pushes
/// one of `ids`, tagged with the packet kind.
fn queued_entity_packets(bot: &Bot, ids: &[i32]) -> Vec<(&'static str, i32)> {
    bot.queued(|packet| {
        let hits: Vec<(&'static str, i32)> = match packet {
            ClientboundGamePacket::AddEntity(p) => vec![("add", p.id.0)],
            ClientboundGamePacket::RemoveEntities(p) => {
                p.entity_ids.iter().map(|id| ("remove", id.0)).collect()
            }
            ClientboundGamePacket::MoveEntityPos(p) => vec![("move", p.entity_id.0)],
            ClientboundGamePacket::MoveEntityPosRot(p) => vec![("move", p.entity_id.0)],
            ClientboundGamePacket::MoveEntityRot(p) => vec![("turn", p.entity_id.0)],
            ClientboundGamePacket::RotateHead(p) => vec![("turn", p.entity_id.0)],
            ClientboundGamePacket::EntityPositionSync(p) => vec![("sync", p.id.0)],
            ClientboundGamePacket::TeleportEntity(p) => vec![("teleport", p.id.0)],
            ClientboundGamePacket::SetEntityMotion(p) => vec![("motion", p.id.0)],
            _ => Vec::new(),
        };
        let hits: Vec<_> = hits
            .into_iter()
            .filter(|(_, id)| ids.contains(id))
            .collect();
        (!hits.is_empty()).then_some(hits)
    })
    .into_iter()
    .flatten()
    .collect()
}

fn seen(bot: &Bot, id: i32) -> Option<SeenEntity> {
    bot.view(|view| view.entities.get(&id).cloned())
}

#[tokio::test]
async fn summoned_pig_is_visible_with_a_stable_identity_to_every_player() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let before = server_entities(&server).await;

    // Step 1: `/summon minecraft:pig` at the current position.
    let here = alice.position();
    alice.command("summon minecraft:pig").await;
    let confirmation = expect_confirmation(&alice).await;
    let (first_id, first) = expect_pig(&alice).await;
    assert_eq!(first.kind, EntityKind::Pig, "Azalea resolves the type id");
    assert_eq!(first.position, here, "summoned at the summoner's feet");
    assert!(
        confirmation.contains(&format!("{:.1}, {:.1}, {:.1}", here.x, here.y, here.z)),
        "the confirmation names the position: {confirmation}"
    );

    // Explicit coordinates, near Alice so that Bob holds the chunk too.
    let there = Vec3::new(here.x.floor() + 2.5, here.y + 1.25, here.z.floor() - 1.75);
    alice
        .command(&format!(
            "summon minecraft:pig {} {} {}",
            there.x, there.y, there.z
        ))
        .await;
    expect_confirmation(&alice).await;
    let (second_id, second) = expect_pig(&alice).await;
    assert_eq!(second.position, there, "summoned at the given coordinates");

    // Fresh identities: two distinct pigs, colliding with no player.
    assert_ne!(first_id, second_id, "a second summon is a second entity");
    assert_ne!(first.uuid, second.uuid);
    for (id, uuid) in [(first_id, first.uuid), (second_id, second.uuid)] {
        for player in [&alice, &bob] {
            assert_ne!(
                id,
                player.entity_id(),
                "pig id collides with {}",
                player.name()
            );
            assert_ne!(
                uuid,
                player.uuid(),
                "pig UUID collides with {}",
                player.name()
            );
        }
    }

    // The server holds exactly these two new entities, typed as pigs.
    let pig = VoidKind::Pig.id();
    let expected = vec![
        ServerEntity {
            id: first_id,
            uuid: first.uuid,
            kind: pig,
            position: [here.x, here.y, here.z],
        },
        ServerEntity {
            id: second_id,
            uuid: second.uuid,
            kind: pig,
            position: [there.x, there.y, there.z],
        },
    ];
    let after = server_entities(&server).await;
    let added: Vec<ServerEntity> = after
        .iter()
        .filter(|entity| !before.contains(entity))
        .copied()
        .collect();
    assert_eq!(added, expected);

    // Step 2: another player in range sees the same entities.
    for (id, entity) in [(first_id, &first), (second_id, &second)] {
        bob.wait_until("the pig Alice summoned", |view| {
            view.entities.get(&id) == Some(entity)
        })
        .await;
    }

    // Nothing acts on the pigs: they keep their identity and position.
    server.wait_ticks(40).await;
    for bot in [&alice, &bob] {
        bot.sync().await;
        assert_eq!(seen(bot, first_id).as_ref(), Some(&first));
        assert_eq!(seen(bot, second_id).as_ref(), Some(&second));
    }
    assert_eq!(
        queued_entity_packets(&alice, &[first_id, second_id]),
        [],
        "Alice was sent nothing for the pigs after their add"
    );
    assert_eq!(
        queued_entity_packets(&bob, &[first_id, second_id]),
        [("add", first_id), ("add", second_id)],
        "Bob was sent one add per pig and nothing after it"
    );
    assert_eq!(
        server_entities(&server).await,
        after,
        "server state unchanged"
    );

    // A player who joins later sees each pig exactly once.
    let carol = server.join("Carol").await;
    carol.sync().await;
    assert_eq!(
        queued_entity_packets(&carol, &[first_id, second_id]),
        [("add", first_id), ("add", second_id)],
        "one add per pig for the late joiner"
    );
    assert_eq!(seen(&carol, first_id).as_ref(), Some(&first));
    assert_eq!(seen(&carol, second_id).as_ref(), Some(&second));
    alice.sync().await;
    assert_eq!(
        queued_entity_packets(&alice, &[first_id, second_id]),
        [],
        "a late joiner does not re-send the pigs to Alice"
    );
    carol.clear();

    // A player who walks out of range and back sees each pig once again.
    let home = bob.position();
    bob.sync().await;
    bob.clear();
    bob.command("tp 1000 100 1000").await;
    bob.wait_until("the pigs out of range", |view| {
        !view.entities.contains_key(&first_id) && !view.entities.contains_key(&second_id)
    })
    .await;
    bob.command(&format!("tp {} {} {}", home.x, home.y, home.z))
        .await;
    for (id, entity) in [(first_id, &first), (second_id, &second)] {
        bob.wait_until("the pig back in range", |view| {
            view.entities.get(&id) == Some(entity)
        })
        .await;
    }
    bob.sync().await;
    let mut round_trip = queued_entity_packets(&bob, &[first_id, second_id]);
    round_trip.sort();
    assert_eq!(
        round_trip,
        [
            ("add", first_id.min(second_id)),
            ("add", first_id.max(second_id)),
            ("remove", first_id.min(second_id)),
            ("remove", first_id.max(second_id)),
        ],
        "one remove and one add per pig for the round trip"
    );
    for bot in [&alice, &carol] {
        bot.sync().await;
        assert_eq!(
            queued_entity_packets(bot, &[first_id, second_id]),
            [],
            "Bob's round trip sends {} nothing for the pigs",
            bot.name()
        );
    }

    // The server stays healthy: further commands work.
    alice.command("summon minecraft:pig").await;
    expect_confirmation(&alice).await;
    let (third_id, _) = expect_pig(&alice).await;
    assert!(![first_id, second_id].contains(&third_id));

    for bot in [&alice, &bob, &carol] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn invalid_summons_are_rejected_without_spawning_anything() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    alice.sync().await;
    bob.sync().await;
    alice.clear();
    bob.clear();
    let before = server_entities(&server).await;

    let rejected = [
        ("summon", "Missing required argument <entity>"),
        // `/summon` takes full namespaced ids only (see the commands reference).
        ("summon pig", "Invalid value 'pig' for <entity>"),
        (
            "summon minecraft:Pig",
            "Invalid value 'minecraft:Pig' for <entity>",
        ),
        (
            "summon minecraft:",
            "Invalid value 'minecraft:' for <entity>",
        ),
        ("summon :pig", "Invalid value ':pig' for <entity>"),
        (
            "summon minecraft:pig!",
            "Invalid value 'minecraft:pig!' for <entity>",
        ),
        (
            "summon minecraft:not_a_mob",
            "Unknown entity type: minecraft:not_a_mob",
        ),
        ("summon othermod:pig", "Unknown entity type: othermod:pig"),
        (
            "summon minecraft:player",
            "Entity type is not summonable: minecraft:player",
        ),
        (
            "summon minecraft:fishing_bobber",
            "Entity type is not summonable: minecraft:fishing_bobber",
        ),
        (
            "summon minecraft:pig 1 2",
            "Invalid value '1 2' for <position>",
        ),
        (
            "summon minecraft:pig 1 up 3",
            "Invalid value '1 up 3' for <position>",
        ),
        (
            "summon minecraft:pig NaN 64 0",
            "Invalid value 'NaN 64 0' for <position>",
        ),
        (
            "summon minecraft:pig ~ ~ ^1",
            "cannot mix world and local (^) coordinates",
        ),
        ("summon minecraft:pig 1 2 3 4", "Too many arguments"),
        ("summon minecraft:pig --fly", "Unknown flag: --fly"),
    ];
    for (input, needle) in rejected {
        alice.command(input).await;
        expect_error(&alice, input, needle).await;
    }

    // Step 3 created nothing: no add for anyone, nothing on the server.
    alice.sync().await;
    bob.sync().await;
    for bot in [&alice, &bob] {
        let added = bot.queued(|packet| match packet {
            ClientboundGamePacket::AddEntity(add) => Some((add.id.0, add.entity_type)),
            _ => None,
        });
        assert_eq!(added, [], "{} was sent an entity", bot.name());
        let kinds: Vec<EntityKind> =
            bot.view(|view| view.entities.values().map(|entity| entity.kind).collect());
        assert_eq!(
            kinds,
            [EntityKind::Player],
            "{} sees only the other player",
            bot.name()
        );
    }
    assert_eq!(
        server_entities(&server).await,
        before,
        "no entity was created"
    );
    let confirmations = alice.queued(|packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => {
            let text = chat.content.to_string();
            text.starts_with("Summoned").then_some(text)
        }
        _ => None,
    });
    assert_eq!(confirmations, Vec::<String>::new(), "nothing was confirmed");

    // The server stays healthy: the players are connected and summon works.
    alice.command("summon minecraft:pig").await;
    expect_confirmation(&alice).await;
    let (id, pig) = expect_pig(&alice).await;
    bob.wait_until("the valid pig", |view| view.entities.get(&id) == Some(&pig))
        .await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}
