//! VMC-ATP-007: chat typed by one player is shown once to every connected
//! player whose chat is shown, still loading the world or not, with the
//! sender's name and the text as typed, and chat never costs anyone their
//! connection.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use azalea_crypto::signing::MessageSignature;
use bevy_ecs::prelude::{Has, World};
use voidmc::WorldMessages;
use voidmc::components::{ConnectionState, PlayerReady, PlayerUuid};
use voidmc::events::ChatMessageEvent;
use voidmc::{Command, CommandBuilder, MessageArg, On};
use voidmc_e2e::core::bitset::FixedBitSet;
use voidmc_e2e::protocol::common::client_information::{ChatVisibility, ClientInformation};
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::packets::game::s_chat::{LastSeenMessagesUpdate, ServerboundChat};
use voidmc_e2e::protocol::packets::game::s_chat_ack::ServerboundChatAck;
use voidmc_e2e::protocol::packets::game::s_chat_command_signed::{
    ArgumentSignature, ServerboundChatCommandSigned,
};
use voidmc_e2e::protocol::packets::game::s_chat_session_update::{
    ProfilePublicKeyData, RemoteChatSessionData, ServerboundChatSessionUpdate,
};
use voidmc_e2e::protocol::packets::game::s_client_information::ServerboundClientInformation;
use voidmc_e2e::{Bot, TestServer};
use voidmc_protocol::State;

/// The line a vanilla client adds to its chat box for this packet, as plain
/// text. An overlay system message goes to the action bar, not the chat box.
fn chat_line(packet: &ClientboundGamePacket) -> Option<String> {
    match packet {
        ClientboundGamePacket::SystemChat(chat) if !chat.overlay => Some(chat.content.to_string()),
        ClientboundGamePacket::PlayerChat(chat) => Some(chat.message().to_string()),
        ClientboundGamePacket::DisguisedChat(chat) => Some(chat.message().to_string()),
        _ => None,
    }
}

async fn join_ready(server: &TestServer, name: &str) -> Bot {
    let bot = server.join(name).await;
    until_ready(server, &bot).await;
    bot
}

async fn join_ready_with_chat(
    server: &TestServer,
    name: &str,
    chat_visibility: ChatVisibility,
) -> Bot {
    let bot = server
        .join_with_information(name, chat_setting(chat_visibility))
        .await;
    until_ready(server, &bot).await;
    bot
}

async fn until_ready(server: &TestServer, bot: &Bot) {
    let uuid = bot.uuid();
    server
        .wait_for("the player to finish loading", move |world| {
            world
                .query::<(&PlayerUuid, Option<&PlayerReady>)>()
                .iter(world)
                .any(|(id, ready)| id.0 == uuid && ready.is_some())
                .then_some(())
        })
        .await;
}

/// The default client settings with this chat visibility ("Shown",
/// "Commands only" or "Hidden" in the vanilla chat settings).
fn chat_setting(chat_visibility: ChatVisibility) -> ClientInformation {
    ClientInformation {
        chat_visibility,
        ..Default::default()
    }
}

/// Changes the chat visibility like a player editing their chat settings
/// while playing, and returns once the server has handled the change.
async fn set_chat_visibility(bot: &Bot, chat_visibility: ChatVisibility) {
    bot.send(ServerboundClientInformation {
        client_information: chat_setting(chat_visibility),
    })
    .await;
    bot.sync().await;
}

/// Waits for `line` in `bot`'s chat box, then checks, once every packet the
/// server sent it so far has arrived, that it was not shown a second time.
async fn sees_once(bot: &Bot, line: &str) {
    bot.expect(&format!("the chat line {line:?}"), |packet| {
        (chat_line(packet)? == line).then_some(())
    })
    .await;
    bot.sync().await;
    let again = bot.queued(|packet| chat_line(packet).filter(|seen| seen == line));
    assert!(again.is_empty(), "{} saw {line:?} again", bot.name());
}

/// Checks that no chat line mentioning `text` reached `bot`.
async fn never_sees(bot: &Bot, text: &str) {
    bot.sync().await;
    let seen = bot.queued(|packet| chat_line(packet).filter(|line| line.contains(text)));
    assert!(seen.is_empty(), "{} saw {seen:?}", bot.name());
}

#[tokio::test]
async fn each_line_is_shown_once_to_both_players_with_its_sender() {
    let server = TestServer::start().await;
    let alice = join_ready(&server, "Alice").await;
    let bob = join_ready(&server, "Bob").await;

    alice.chat("hello from Alice 7f3a").await;
    for bot in [&alice, &bob] {
        sees_once(bot, "<Alice> hello from Alice 7f3a").await;
    }

    bob.chat("hi Alice, Bob here c91e").await;
    for bot in [&alice, &bob] {
        sees_once(bot, "<Bob> hi Alice, Bob here c91e").await;
    }

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn chat_text_is_shown_literally_and_senders_stay_distinguishable() {
    let server = TestServer::start().await;
    let alice = join_ready(&server, "Alice").await;
    let lookalike = join_ready(&server, "Alice_").await;

    let longest_ascii = format!("max{}", "x".repeat(253));
    let longest_emoji = "😀".repeat(128);
    let lines = [
        "plain ASCII, 1 2 3 !?".to_string(),
        "Grüße, ça va? 你好 Привет 😀👍🏽".to_string(),
        "  leading and trailing  ".to_string(),
        "multiple     spaces".to_string(),
        String::new(),
        "   ".to_string(),
        longest_ascii,
        longest_emoji,
        r#"{"text":"x","color":"red"}"#.to_string(),
        r#"["",{"translate":"chat.type.text","with":["Bob","hi"]}]"#.to_string(),
        "<Bob> hi".to_string(),
        "%s %1$s chat.type.text".to_string(),
        r"\n<Bob> not a line break".to_string(),
    ];
    for message in &lines {
        alice.chat(message).await;
        let expected = format!("<Alice> {message}");
        for bot in [&alice, &lookalike] {
            let shown = bot
                .expect(&format!("the chat line {expected:?}"), chat_line)
                .await;
            assert_eq!(shown, expected, "{} was shown another line", bot.name());
        }
    }

    lookalike.chat("<Alice> I am Alice").await;
    for bot in [&alice, &lookalike] {
        let shown = bot.expect("the look-alike's line", chat_line).await;
        assert_eq!(shown, "<Alice_> <Alice> I am Alice");
    }

    for bot in [&alice, &lookalike] {
        bot.sync().await;
        assert!(
            bot.queued(chat_line).is_empty(),
            "{} saw extra lines",
            bot.name()
        );
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn lines_a_vanilla_server_refuses_reach_nobody_and_disconnect_nobody() {
    let delivered = Arc::new(AtomicUsize::new(0));
    let observed = delivered.clone();
    let server = TestServer::builder()
        .plugin(move |app| {
            app.add_observer(move |_: On<ChatMessageEvent>| {
                observed.fetch_add(1, Ordering::Relaxed);
            });
        })
        .start()
        .await;
    let alice = join_ready(&server, "Alice").await;
    let bob = join_ready(&server, "Bob").await;

    let illegal = "Illegal characters in chat";
    let too_long = "Chat message too long";
    let refused = [
        ("§cred §kobfuscated".to_string(), illegal),
        ("first\n<Bob> forged second line".to_string(), illegal),
        ("carriage\rreturn".to_string(), illegal),
        ("tab\tstop".to_string(), illegal),
        ("bell\u{7}".to_string(), illegal),
        ("delete\u{7f}".to_string(), illegal),
        (format!("long{}", "y".repeat(253)), too_long),
        (format!("{}a", "😀".repeat(128)), too_long),
        (format!("huge{}", "z".repeat(5000)), too_long),
    ];
    for (message, reason) in &refused {
        alice.chat(message).await;
        let shown = alice
            .expect(&format!("the refusal of {message:?}"), chat_line)
            .await;
        assert_eq!(shown, *reason, "refusal of {message:?}");
    }
    for bot in [&alice, &bob] {
        bot.sync().await;
        let seen = bot.queued(chat_line);
        assert!(seen.is_empty(), "{} saw {seen:?}", bot.name());
    }
    assert_eq!(
        delivered.load(Ordering::Relaxed),
        0,
        "a refused line reached plugins"
    );

    alice.chat("still connected").await;
    bob.chat("me too").await;
    for bot in [&alice, &bob] {
        sees_once(bot, "<Alice> still connected").await;
        sees_once(bot, "<Bob> me too").await;
        bot.assert_healthy();
    }
    assert_eq!(delivered.load(Ordering::Relaxed), 2);
    server.stop().await;
}

fn shout_command() -> Command {
    CommandBuilder::new("shout")
        .arg_variadic_required("message", Arc::new(MessageArg))
        .handler(|ctx| {
            let message = ctx.get::<String>("message").cloned().unwrap_or_default();
            ctx.reply(&format!("shouted {message}"));
        })
        .build()
}

/// What a signed-in vanilla client sends around chat: its chat session
/// right after joining, signed chat with the last-seen window and checksum,
/// signed commands and acknowledgements.
#[tokio::test]
async fn chat_packets_of_a_signed_in_client_are_understood() {
    let server = TestServer::builder().command(shout_command).start().await;
    let alice = join_ready(&server, "Alice").await;
    let bob = join_ready(&server, "Bob").await;

    alice
        .send(ServerboundChatSessionUpdate {
            chat_session: RemoteChatSessionData {
                session_id: uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
                profile_public_key: ProfilePublicKeyData {
                    expires_at: 1_900_000_000_000,
                    key: vec![0x30; 294],
                    key_signature: vec![0xa5; 512],
                },
            },
        })
        .await;
    let mut acknowledged = FixedBitSet::<20>::new();
    acknowledged.set(0);
    acknowledged.set(19);
    alice
        .send(ServerboundChat {
            message: "signed hello".to_string(),
            timestamp: 1_763_000_000_000,
            salt: 0x1234_5678_9abc_def0,
            signature: Some(MessageSignature { bytes: [0x5c; 256] }),
            last_seen_messages: LastSeenMessagesUpdate {
                offset: 3,
                acknowledged: acknowledged.clone(),
                checksum: 0x7e,
            },
        })
        .await;
    alice
        .send(ServerboundChat {
            message: "unsigned hello".to_string(),
            timestamp: 1_763_000_000_001,
            salt: 42,
            signature: None,
            last_seen_messages: LastSeenMessagesUpdate::default(),
        })
        .await;
    alice.send(ServerboundChatAck { messages: 3 }).await;
    alice
        .send(ServerboundChatCommandSigned {
            command: "shout loud".to_string(),
            timestamp: 1_763_000_000_002,
            salt: 7,
            argument_signatures: vec![ArgumentSignature {
                name: "message".to_string(),
                signature: MessageSignature { bytes: [0x33; 256] },
            }],
            last_seen_messages: LastSeenMessagesUpdate {
                offset: 0,
                acknowledged,
                checksum: 1,
            },
        })
        .await;

    for bot in [&alice, &bob] {
        sees_once(bot, "<Alice> signed hello").await;
        sees_once(bot, "<Alice> unsigned hello").await;
    }
    sees_once(&alice, "shouted loud").await;
    never_sees(&bob, "shouted").await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

async fn burst(bot: &Bot, lines: usize) {
    for i in 0..lines {
        bot.chat(&format!("burst {i}")).await;
    }
}

#[tokio::test]
async fn a_burst_from_both_players_arrives_whole_and_in_one_order() {
    const PER_PLAYER: usize = 40;
    let server = TestServer::start().await;
    let alice = join_ready(&server, "Alice").await;
    let bob = join_ready(&server, "Bob").await;

    tokio::join!(burst(&alice, PER_PLAYER), burst(&bob, PER_PLAYER));

    let mut orders = Vec::new();
    for bot in [&alice, &bob] {
        let mut received = Vec::new();
        for _ in 0..2 * PER_PLAYER {
            let line = bot
                .expect("a burst line", |packet| {
                    chat_line(packet).filter(|line| line.contains("> burst "))
                })
                .await;
            received.push(line);
        }
        bot.sync().await;
        assert!(
            bot.queued(chat_line).is_empty(),
            "{} saw extra lines",
            bot.name()
        );
        for sender in ["Alice", "Bob"] {
            let from: Vec<&String> = received
                .iter()
                .filter(|line| line.starts_with(&format!("<{sender}> ")))
                .collect();
            let sent: Vec<String> = (0..PER_PLAYER)
                .map(|i| format!("<{sender}> burst {i}"))
                .collect();
            assert_eq!(
                from,
                sent.iter().collect::<Vec<_>>(),
                "{sender}'s lines at {}",
                bot.name()
            );
        }
        orders.push(received);
        bot.assert_healthy();
    }
    assert_eq!(
        orders[0], orders[1],
        "Alice and Bob saw the burst in different orders"
    );
    server.stop().await;
}

#[tokio::test]
async fn lines_reach_players_present_when_sent_and_nobody_else() {
    let server = TestServer::start().await;
    let alice = join_ready(&server, "Alice").await;
    let bob = join_ready(&server, "Bob").await;

    alice.chat("before Carol").await;
    sees_once(&bob, "<Alice> before Carol").await;

    let carol = join_ready(&server, "Carol").await;
    alice.chat("after Carol").await;
    for bot in [&alice, &bob, &carol] {
        sees_once(bot, "<Alice> after Carol").await;
    }
    never_sees(&carol, "before Carol").await;

    let bob_uuid = bob.uuid();
    bob.disconnect().await;
    server
        .wait_for("Bob's player entity to be despawned", move |world| {
            let mut players = world.query::<&PlayerUuid>();
            (!players.iter(world).any(|uuid| uuid.0 == bob_uuid)).then_some(())
        })
        .await;
    carol.chat("after Bob left").await;
    for bot in [&alice, &carol] {
        sees_once(bot, "<Carol> after Bob left").await;
        bot.assert_healthy();
    }
    server.stop().await;
}

/// Like a vanilla server, only a client whose chat is shown ("Full") is sent
/// other players' chat; one on "Commands only" or "Hidden" still gets its
/// command feedback and the server's own messages.
#[tokio::test]
async fn players_who_hide_chat_are_sent_no_player_chat_but_still_command_feedback() {
    let server = TestServer::builder().command(shout_command).start().await;
    let alice = join_ready(&server, "Alice").await;
    let bob = join_ready_with_chat(&server, "Bob", ChatVisibility::System).await;
    let carol = join_ready_with_chat(&server, "Carol", ChatVisibility::Hidden).await;

    alice.chat("for shown chat only 5d2b").await;
    sees_once(&alice, "<Alice> for shown chat only 5d2b").await;
    for bot in [&bob, &carol] {
        never_sees(bot, "for shown chat only 5d2b").await;
    }

    for bot in [&bob, &carol] {
        bot.command(&format!("shout from {}", bot.name())).await;
        sees_once(bot, &format!("shouted from {}", bot.name())).await;
    }
    never_sees(&alice, "shouted").await;

    server
        .with_world(|world| {
            WorldMessages::new(world)
                .broadcast("server notice 8a1f")
                .send();
        })
        .await;
    for bot in [&alice, &bob, &carol] {
        sees_once(bot, "server notice 8a1f").await;
        bot.assert_healthy();
    }
    server.stop().await;
}

/// A chat setting changed while playing decides the next lines, in both
/// directions.
#[tokio::test]
async fn a_chat_setting_changed_while_playing_applies_to_the_next_line() {
    let server = TestServer::start().await;
    let alice = join_ready(&server, "Alice").await;
    let bob = join_ready(&server, "Bob").await;

    set_chat_visibility(&bob, ChatVisibility::Hidden).await;
    alice.chat("while Bob hides chat").await;
    sees_once(&alice, "<Alice> while Bob hides chat").await;
    never_sees(&bob, "while Bob hides chat").await;

    set_chat_visibility(&bob, ChatVisibility::Full).await;
    alice.chat("Bob shows chat again").await;
    for bot in [&alice, &bob] {
        sees_once(bot, "<Alice> Bob shows chat again").await;
    }

    set_chat_visibility(&bob, ChatVisibility::System).await;
    alice.chat("while Bob shows commands only").await;
    sees_once(&alice, "<Alice> while Bob shows commands only").await;
    never_sees(&bob, "while Bob shows commands only").await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// A player in the play state is sent chat before its client reports the
/// world loaded, like on a vanilla server.
#[tokio::test]
async fn a_player_still_loading_the_world_is_sent_chat() {
    let server = TestServer::start().await;
    let alice = join_ready(&server, "Alice").await;
    let mut dave = server.join_silent("Dave").await;
    dave.expect("the play Login", |packet| {
        matches!(packet, ClientboundGamePacket::Login(_)).then_some(())
    })
    .await;
    let uuid = dave.uuid();
    let loading = move |world: &mut World| {
        world
            .query::<(&PlayerUuid, &ConnectionState, Has<PlayerReady>)>()
            .iter(world)
            .find(|(id, ..)| id.0 == uuid)
            .map(|(_, state, ready)| (state.0, ready))
    };
    assert_eq!(
        server.with_world(loading).await,
        Some((State::Play, false)),
        "Dave is in the play state and not ready"
    );

    let line = "<Alice> while Dave loads 2e4c";
    alice.chat("while Dave loads 2e4c").await;
    sees_once(&alice, line).await;
    dave.expect(&format!("the chat line {line:?}"), |packet| {
        (chat_line(packet)? == line).then_some(())
    })
    .await;
    assert_eq!(
        server.with_world(loading).await,
        Some((State::Play, false)),
        "Dave is still loading when the line arrives"
    );

    alice.assert_healthy();
    server.stop().await;
}
