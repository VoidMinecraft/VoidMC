//! VMC-ATP-010: `/gamemode` changes the game mode a player is shown, not the
//! rules the server applies.

use voidmc::TabEntry;
use voidmc::components::PlayerUuid;
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::game_type::GameMode;
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::protocol::packets::game::c_commands::{BrigadierParser, NodeType};
use voidmc_e2e::protocol::packets::game::c_game_event::EventType;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_player_action::{Action, ServerboundPlayerAction},
};
use voidmc_e2e::{Bot, TestServer};

use crate::ground;

const USAGE: &str = "Usage: /gamemode <mode:gamemode>";

/// The mode values of the queued Game Event packets that change the game mode.
fn mode_events(bot: &Bot) -> Vec<f32> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::GameEvent(event) if event.event == EventType::ChangeGameMode => {
            Some(event.param)
        }
        _ => None,
    })
}

fn chat_lines(bot: &Bot) -> Vec<String> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => Some(chat.content.to_string()),
        _ => None,
    })
}

/// Every tab-list game-mode change queued for this bot, as (uuid, mode) rows.
/// Rows that add a player also carry its mode; they are not changes.
fn mode_updates(bot: &Bot) -> Vec<(uuid::Uuid, GameMode)> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::PlayerInfoUpdate(update)
            if update.actions.update_game_mode && !update.actions.add_player =>
        {
            Some(
                update
                    .entries
                    .iter()
                    .map(|entry| (entry.profile.uuid, entry.game_mode))
                    .collect::<Vec<_>>(),
            )
        }
        _ => None,
    })
    .into_iter()
    .flatten()
    .collect()
}

/// The game mode the latest queued ADD_PLAYER row for `uuid` carries.
fn listed_mode(bot: &Bot, uuid: uuid::Uuid) -> Option<GameMode> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::PlayerInfoUpdate(update) if update.actions.add_player => update
            .entries
            .iter()
            .find(|entry| entry.profile.uuid == uuid)
            .map(|entry| entry.game_mode),
        _ => None,
    })
    .pop()
}

fn login_mode(bot: &Bot) -> (GameMode, Option<GameMode>) {
    let modes = bot.queued(|packet| match packet {
        ClientboundGamePacket::Login(login) => Some((
            login.common.game_type,
            login.common.previous_game_type.into(),
        )),
        _ => None,
    });
    assert_eq!(modes.len(), 1, "{} logged in once", bot.name());
    modes[0]
}

fn respawns_or_teleports(bot: &Bot) -> Vec<&'static str> {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::Login(_) => Some("login"),
        ClientboundGamePacket::Respawn(_) => Some("respawn"),
        ClientboundGamePacket::PlayerPosition(_) => Some("teleport"),
        _ => None,
    })
}

async fn server_mode(server: &TestServer, bot: &Bot) -> Option<u8> {
    let player = server.player_entity(bot.uuid()).await;
    server
        .with_world(move |world| {
            world
                .get::<TabEntry>(player)
                .expect("ready players have a tab entry")
                .game_mode
        })
        .await
}

#[tokio::test]
async fn every_valid_mode_reaches_the_client_and_every_tab_list() {
    // The default configuration, like the example server: Creative.
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    alice.sync().await;
    bob.sync().await;

    assert_eq!(login_mode(&alice), (GameMode::Creative, None));
    assert_eq!(listed_mode(&alice, alice.uuid()), Some(GameMode::Creative));
    assert_eq!(listed_mode(&bob, alice.uuid()), Some(GameMode::Creative));
    assert_eq!(server_mode(&server, &alice).await, None);

    let steps = [
        ("gamemode 0", GameMode::Survival, "survival"),
        ("gamemode 1", GameMode::Creative, "creative"),
        ("gamemode 2", GameMode::Adventure, "adventure"),
        ("gamemode 3", GameMode::Spectator, "spectator"),
        ("gm survival", GameMode::Survival, "survival"),
        ("gamemode Creative", GameMode::Creative, "creative"),
    ];
    for (command, mode, name) in steps {
        let confirmation = format!("Game mode set to {name} ({})", mode.to_id());
        let position = alice.position();
        alice.clear();
        bob.clear();
        alice.command(command).await;
        alice.sync().await;
        bob.sync().await;

        let id = f32::from(mode.to_id());
        assert_eq!(mode_events(&alice), [id], "/{command}: the game event");
        assert_eq!(chat_lines(&alice), [confirmation], "/{command}: the reply");
        assert_eq!(
            mode_updates(&alice),
            [(alice.uuid(), mode)],
            "/{command}: Alice's own tab list"
        );
        assert_eq!(
            mode_updates(&bob),
            [(alice.uuid(), mode)],
            "/{command}: Bob's tab list shows Alice's new mode"
        );
        assert!(
            mode_events(&bob).is_empty(),
            "/{command}: Bob keeps his mode"
        );
        assert!(chat_lines(&bob).is_empty(), "/{command}: Bob gets no reply");
        assert!(
            respawns_or_teleports(&alice).is_empty(),
            "/{command} must not respawn or move Alice: {:?}",
            respawns_or_teleports(&alice)
        );
        assert_eq!(alice.position(), position);
        assert_eq!(server_mode(&server, &alice).await, Some(mode.to_id()));
    }

    let destination = alice.position() + Vec3::new(2.0, 0.0, 1.5);
    alice.move_to(destination).await;
    bob.wait_until("Alice to walk after her mode changes", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(destination) < 1e-3)
    })
    .await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn invalid_modes_are_rejected_by_parsing_and_change_nothing() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    alice.sync().await;
    bob.sync().await;

    let (nodes, root) = alice
        .expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    for name in ["gamemode", "gm"] {
        let literal = nodes[root]
            .children
            .iter()
            .map(|child| &nodes[*child as usize])
            .find(|node| node.name() == Some(name))
            .unwrap_or_else(|| panic!("/{name} in the command tree"));
        assert!(!literal.is_executable, "/{name} needs a mode");
        let [argument] = literal.children[..] else {
            panic!("/{name} takes one argument: {literal:?}");
        };
        let argument = &nodes[argument as usize];
        assert!(
            matches!(
                &argument.node_type,
                NodeType::Argument { name, parser: BrigadierParser::GameMode, .. } if name == "mode"
            ),
            "the vanilla client validates /{name}'s mode itself: {argument:?}"
        );
        assert!(argument.is_executable && argument.children.is_empty());
    }

    let invalid = |value: &str| {
        format!(
            "Invalid value '{value}' for <mode>: expected gamemode ('{value}' is not a valid game mode (expected survival, creative, adventure or spectator))"
        )
    };
    let cases = [
        ("gamemode 4", invalid("4")),
        ("gamemode -1", invalid("-1")),
        ("gamemode 99", invalid("99")),
        ("gamemode 256", invalid("256")),
        (
            "gamemode 99999999999999999999",
            invalid("99999999999999999999"),
        ),
        ("gamemode hardcore", invalid("hardcore")),
        ("gm 1.0", invalid("1.0")),
        (
            "gamemode",
            "Missing required argument <mode> (expected gamemode)".to_string(),
        ),
        (
            "gamemode 1 extra",
            "Too many arguments: expected 1, got 2".to_string(),
        ),
    ];
    for (command, error) in cases {
        alice.clear();
        bob.clear();
        alice.command(command).await;
        alice.sync().await;
        bob.sync().await;

        assert_eq!(
            chat_lines(&alice),
            [error.as_str(), USAGE],
            "/{command}: the parse error"
        );
        assert!(mode_events(&alice).is_empty(), "/{command}: no game event");
        assert_eq!(mode_updates(&alice), [], "/{command}: no tab update");
        assert_eq!(mode_updates(&bob), [], "/{command}: no tab update");
        assert!(chat_lines(&bob).is_empty(), "/{command}: Bob gets no reply");
        assert_eq!(server_mode(&server, &alice).await, None, "/{command}");
    }

    alice.clear();
    alice.command("gamemode 2").await;
    alice.sync().await;
    assert_eq!(mode_events(&alice), [2.0], "commands still run afterwards");

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn the_server_keeps_its_configured_mode_and_rejoins_reset_to_it() {
    let server = TestServer::builder()
        .config(|config| config.game_mode(0))
        .start()
        .await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    alice.sync().await;
    bob.sync().await;
    assert_eq!(login_mode(&alice), (GameMode::Survival, None));
    assert_eq!(listed_mode(&alice, alice.uuid()), Some(GameMode::Survival));
    assert_eq!(listed_mode(&bob, alice.uuid()), Some(GameMode::Survival));

    alice.command("gamemode 1").await;
    alice.sync().await;
    assert_eq!(mode_events(&alice), [1.0]);

    // A creative client removes the block when it starts digging and sends
    // nothing more. The server still applies the configured Survival rules,
    // where a start alone breaks nothing.
    let block = ground(&alice);
    alice
        .send(ServerboundPlayerAction {
            action: Action::StartDestroyBlock,
            pos: block,
            direction: Direction::Up,
            seq: 1,
        })
        .await;
    alice
        .expect("the dig acknowledgement", |packet| match packet {
            ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == 1 => Some(()),
            _ => None,
        })
        .await;
    alice.sync().await;
    bob.sync().await;
    assert!(alice.block_at(block).is_some_and(|state| !state.is_air()));
    assert!(bob.block_at(block).is_some_and(|state| !state.is_air()));

    let alice_uuid = alice.uuid();
    alice.disconnect().await;
    bob.wait_until("Alice gone from the tab list", |view| {
        !view.players.contains_key(&alice_uuid)
    })
    .await;
    server
        .wait_for("Alice's player entity to be despawned", move |world| {
            let mut players = world.query::<&PlayerUuid>();
            (!players.iter(world).any(|uuid| uuid.0 == alice_uuid)).then_some(())
        })
        .await;
    bob.clear();

    let alice = server.join("Alice").await;
    alice.sync().await;
    bob.sync().await;
    assert_eq!(
        login_mode(&alice),
        (GameMode::Survival, None),
        "a rejoin starts in the configured mode, not the one /gamemode showed"
    );
    assert_eq!(listed_mode(&alice, alice_uuid), Some(GameMode::Survival));
    assert_eq!(listed_mode(&bob, alice_uuid), Some(GameMode::Survival));
    assert_eq!(server_mode(&server, &alice).await, None);

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}
