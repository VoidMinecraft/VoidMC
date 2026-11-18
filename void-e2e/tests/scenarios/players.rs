use voidmc::components::PlayerUuid;
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::registry::builtin::EntityKind;
use voidmc_e2e::{Bot, TestServer};

fn sees(viewer: &Bot, target: &Bot) -> Option<Vec3> {
    viewer.view(|view| {
        view.entity_by_uuid(target.uuid())
            .filter(|(_, entity)| entity.kind == EntityKind::Player)
            .map(|(_, entity)| entity.position)
    })
}

#[tokio::test]
async fn players_see_each_other_move_and_leave() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    for (viewer, target) in [(&alice, &bob), (&bob, &alice)] {
        viewer
            .wait_until("the other player listed and spawned", |view| {
                view.players.get(&target.uuid()).map(String::as_str) == Some(target.name())
                    && view.entity_by_uuid(target.uuid()).is_some()
            })
            .await;
        assert_eq!(sees(viewer, target), Some(target.position()));
    }
    assert_eq!(
        bob.view(|view| view.entity_by_uuid(alice.uuid()).map(|(id, _)| id)),
        Some(alice.entity_id())
    );

    let destination = alice.position() + Vec3::new(3.0, 0.0, -2.5);
    alice.move_to(destination).await;
    bob.wait_until("Alice at her new position", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(destination) < 1e-3)
    })
    .await;

    let alice_uuid = alice.uuid();
    alice.disconnect().await;
    bob.wait_until("Alice gone from the world and the tab list", |view| {
        view.entity_by_uuid(alice_uuid).is_none() && !view.players.contains_key(&alice_uuid)
    })
    .await;
    server
        .wait_for("Alice's player entity to be despawned", move |world| {
            let mut players = world.query::<&PlayerUuid>();
            (!players.iter(world).any(|uuid| uuid.0 == alice_uuid)).then_some(())
        })
        .await;

    let carol = server.join("Carol").await;
    carol.sync().await;
    assert!(
        carol.view(|view| view.entity_by_uuid(bob.uuid()).is_some()
            && view.players.contains_key(&bob.uuid())),
        "a player who joins later sees the players still online"
    );
    assert!(
        carol.view(|view| view.entity_by_uuid(alice_uuid).is_none()
            && !view.players.contains_key(&alice_uuid)),
        "a player who joins later never hears of Alice"
    );
    assert!(
        carol
            .queued(|packet| match packet {
                ClientboundGamePacket::AddEntity(add) if add.uuid == alice_uuid => Some(()),
                _ => None,
            })
            .is_empty()
    );

    bob.assert_healthy();
    carol.assert_healthy();
    server.stop().await;
}
