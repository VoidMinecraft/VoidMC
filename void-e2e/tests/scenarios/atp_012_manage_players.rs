//! VMC-ATP-012: the built-in `/list`, `/tell` (`/msg`), `/broadcast`, `/say`
//! and `/kick` commands affect the connected players as documented.
//!
//! The default commands carry no permission requirement, so the harness
//! server (which registers them) lets every player run them. Each scenario
//! checks what every connected client receives, not only the sender: a reply
//! that reaches the wrong player, or a line that reaches nobody, fails it.
//!
//! The kicked player is a client driven by hand ([`Raw`]) that ignores the
//! `Disconnect` packet and keeps reading: the test only passes if the server
//! itself closes the connection.

use std::collections::BTreeSet;
use std::future::Future;
use std::time::Duration;

use uuid::Uuid;
use voidmc_e2e::protocol::common::client_information::ClientInformation;
use voidmc_e2e::protocol::common::movements::MoveFlags;
use voidmc_e2e::protocol::connect::Connection;
use voidmc_e2e::protocol::packets::config::{
    ClientboundConfigPacket, s_client_information::ServerboundClientInformation,
    s_finish_configuration::ServerboundFinishConfiguration,
    s_keep_alive::ServerboundKeepAlive as ConfigKeepAlive,
    s_select_known_packs::ServerboundSelectKnownPacks,
};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, ServerboundGamePacket,
    s_accept_teleportation::ServerboundAcceptTeleportation, s_chat_command::ServerboundChatCommand,
    s_keep_alive::ServerboundKeepAlive, s_move_player_pos_rot::ServerboundMovePlayerPosRot,
    s_player_loaded::ServerboundPlayerLoaded, s_pong::ServerboundPong,
};
use voidmc_e2e::protocol::packets::handshake::s_intention::ServerboundIntention;
use voidmc_e2e::protocol::packets::login::{
    ClientboundLoginPacket, s_hello::ServerboundHello,
    s_login_acknowledged::ServerboundLoginAcknowledged,
};
use voidmc_e2e::protocol::packets::{ClientIntention, PROTOCOL_VERSION, Packet};
use voidmc_e2e::protocol::read::ReadPacketError;
use voidmc_e2e::{Bot, TestServer};

// -------------------------------------------------------------------------
// Reading what a client was shown
// -------------------------------------------------------------------------

/// A chat-box line (`SystemChat` without `overlay`) as the client shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    /// The text as displayed; legacy `§` codes are already applied (removed).
    text: String,
    color: Option<String>,
    /// More than one component: part of the line has its own style.
    styled: bool,
}

fn line(packet: &ClientboundGamePacket) -> Option<Line> {
    match packet {
        ClientboundGamePacket::SystemChat(chat) if !chat.overlay => {
            let base = chat.content.get_base();
            Some(Line {
                text: chat.content.to_string(),
                color: base.style.color.as_ref().map(|color| color.to_string()),
                styled: !base.siblings.is_empty(),
            })
        }
        _ => None,
    }
}

fn plain(text: &str, color: &str) -> Line {
    Line {
        text: text.to_string(),
        color: Some(color.to_string()),
        styled: false,
    }
}

/// Every queued chat-box line, without removing them.
fn lines(bot: &Bot) -> Vec<Line> {
    bot.queued(line)
}

/// Syncs every bot, so each has received what the server sent it for the
/// commands handled so far, then returns and clears their queued lines.
async fn take_lines<const N: usize>(bots: [&Bot; N]) -> [Vec<Line>; N] {
    for bot in bots {
        bot.sync().await;
    }
    bots.map(|bot| {
        let seen = lines(bot);
        bot.clear();
        seen
    })
}

/// Runs a command, waits until the server has handled it and every bot has
/// received what it was sent, and returns the lines each bot got.
async fn run<const N: usize>(sender: &Bot, command: &str, bots: [&Bot; N]) -> [Vec<Line>; N] {
    for bot in bots {
        bot.sync().await;
        bot.clear();
    }
    sender.command(command).await;
    take_lines(bots).await
}

/// The player names in a `/list` reply, which must have the documented
/// format `There are N player(s) online: A, B`.
fn listed(reply: &[Line]) -> BTreeSet<String> {
    assert_eq!(reply.len(), 1, "one /list reply: {reply:?}");
    let reply = &reply[0];
    assert_eq!(reply.color.as_deref(), Some("white"), "{reply:?}");
    assert!(!reply.styled, "{reply:?}");
    if reply.text == "There are 0 player(s) online." {
        return BTreeSet::new();
    }
    let (count, names) = reply
        .text
        .strip_prefix("There are ")
        .and_then(|rest| rest.split_once(" player(s) online: "))
        .unwrap_or_else(|| panic!("unexpected /list reply {:?}", reply.text));
    let names: Vec<String> = names.split(", ").map(str::to_string).collect();
    assert_eq!(count, names.len().to_string(), "{:?}", reply.text);
    let set: BTreeSet<String> = names.iter().cloned().collect();
    assert_eq!(
        set.len(),
        names.len(),
        "a name listed twice: {:?}",
        reply.text
    );
    set
}

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

/// The names in a bot's tab list.
fn tab_list(bot: &Bot) -> BTreeSet<String> {
    bot.view(|view| view.players.values().cloned().collect())
}

/// Waits (a bounded number of ticks) until the server-list status reports
/// `expected` players online.
async fn assert_status_online(server: &TestServer, expected: i32) {
    let mut online = server.status().await.players.online;
    for _ in 0..100 {
        if online == expected {
            return;
        }
        server.wait_ticks(1).await;
        online = server.status().await.players.online;
    }
    assert_eq!(online, expected, "players online in the server-list status");
}

/// The error a command gives for a bad argument: a red line, then the gray
/// usage line.
fn assert_refused(reply: &[Line], error: &str, usage: &str) {
    assert_eq!(
        reply,
        [
            plain(error, "red"),
            plain(&format!("Usage: {usage}"), "gray")
        ],
    );
}

// -------------------------------------------------------------------------
// A client driven by hand
// -------------------------------------------------------------------------

const BARRIER_BIT: u32 = 0x8000_0000;

/// A play-state client that only does what the test tells it to. Unlike the
/// harness [`Bot`], it can stay not ready (never send `PlayerLoaded`), and it
/// tells a connection the server closed apart from a `Disconnect` packet.
struct Raw {
    name: String,
    uuid: Uuid,
    entity_id: Option<i32>,
    connection: Connection<ClientboundGamePacket, ServerboundGamePacket>,
    next_barrier: u32,
}

async fn within<T>(who: &str, what: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(15), future)
        .await
        .unwrap_or_else(|_| panic!("{who}: timed out waiting for {what}"))
}

impl Raw {
    /// Logs in and configures like a vanilla client; with `ready`, also
    /// accepts the spawn teleport and reports the world loaded, as the
    /// vanilla client does once it can render, so the player becomes ready.
    async fn join(server: &TestServer, name: &str, ready: bool) -> Self {
        let uuid = azalea_crypto::offline::generate_uuid(name);
        let address = server.address();
        let mut connection = Connection::new(&address)
            .await
            .unwrap_or_else(|error| panic!("{name} could not connect: {error}"));
        connection
            .write(ServerboundIntention {
                protocol_version: PROTOCOL_VERSION,
                hostname: address.ip().to_string(),
                port: address.port(),
                intention: ClientIntention::Login,
            })
            .await
            .expect("handshake");
        let mut connection = connection.login();
        connection
            .write(ServerboundHello {
                name: name.to_string(),
                profile_id: uuid,
            })
            .await
            .expect("login start");
        loop {
            match within(name, "a login packet", connection.read()).await {
                Ok(ClientboundLoginPacket::LoginCompression(packet)) => {
                    connection.set_compression_threshold(packet.compression_threshold);
                }
                Ok(ClientboundLoginPacket::LoginFinished(_)) => break,
                other => panic!("{name}: unexpected login answer {other:?}"),
            }
        }
        connection
            .write(ServerboundLoginAcknowledged {})
            .await
            .expect("login acknowledged");
        let mut connection = connection.config();
        connection
            .write(ServerboundClientInformation {
                information: ClientInformation::default(),
            })
            .await
            .expect("client information");
        loop {
            match within(name, "a configuration packet", connection.read()).await {
                Ok(ClientboundConfigPacket::SelectKnownPacks(packet)) => connection
                    .write(ServerboundSelectKnownPacks {
                        known_packs: packet.known_packs,
                    })
                    .await
                    .expect("select known packs"),
                Ok(ClientboundConfigPacket::KeepAlive(packet)) => connection
                    .write(ConfigKeepAlive { id: packet.id })
                    .await
                    .expect("configuration keep-alive"),
                Ok(ClientboundConfigPacket::FinishConfiguration(_)) => break,
                Ok(_) => {}
                Err(error) => panic!("{name}: configuration read failed: {error}"),
            }
        }
        connection
            .write(ServerboundFinishConfiguration {})
            .await
            .expect("finish configuration");

        let mut raw = Raw {
            name: name.to_string(),
            uuid,
            entity_id: None,
            connection: connection.game(),
            next_barrier: 1,
        };
        let teleport = raw
            .expect("the spawn teleport", |packet| match packet {
                ClientboundGamePacket::PlayerPosition(teleport) => Some(teleport.clone()),
                _ => None,
            })
            .await;
        if ready {
            raw.send(ServerboundAcceptTeleportation { id: teleport.id })
                .await;
            raw.send(ServerboundMovePlayerPosRot {
                pos: teleport.change.pos,
                look_direction: teleport.change.look_direction,
                flags: MoveFlags {
                    on_ground: true,
                    horizontal_collision: false,
                },
            })
            .await;
            raw.send(ServerboundPlayerLoaded {}).await;
        }
        raw.sync().await;
        assert!(raw.entity_id.is_some(), "{name} got no Login packet");
        raw
    }

    /// Reads one play packet, answering keep-alives and pings like any
    /// client; `Err` once the server has closed the connection.
    async fn read(&mut self) -> Result<ClientboundGamePacket, String> {
        let packet = within(&self.name, "a play packet", self.connection.read())
            .await
            .map_err(|error| match *error {
                ReadPacketError::ConnectionClosed => "closed".to_string(),
                error => format!("read failed: {error}"),
            })?;
        match &packet {
            ClientboundGamePacket::Login(login) => self.entity_id = Some(login.player_id.0),
            ClientboundGamePacket::KeepAlive(keep_alive) => {
                self.send(ServerboundKeepAlive { id: keep_alive.id }).await;
            }
            ClientboundGamePacket::Ping(ping) if ping.id & BARRIER_BIT == 0 => {
                self.send(ServerboundPong { id: ping.id }).await;
            }
            _ => {}
        }
        Ok(packet)
    }

    /// Reads until `f` matches, skipping (and dropping) other packets.
    async fn expect<T>(
        &mut self,
        what: &str,
        mut f: impl FnMut(&ClientboundGamePacket) -> Option<T>,
    ) -> T {
        loop {
            let packet = self
                .read()
                .await
                .unwrap_or_else(|error| panic!("{} waiting for {what}: {error}", self.name));
            if let Some(found) = f(&packet) {
                return found;
            }
        }
    }

    async fn send(&mut self, packet: impl Packet<ServerboundGamePacket>) {
        let name = self.name.clone();
        self.connection
            .write(packet.into_variant())
            .await
            .unwrap_or_else(|error| panic!("{name} could not send: {error}"));
    }

    async fn command(&mut self, command: &str) {
        self.send(ServerboundChatCommand {
            command: command.to_string(),
        })
        .await;
    }

    /// Returns once the server has handled every packet sent before and
    /// finished that tick, with the chat lines received meanwhile.
    async fn sync(&mut self) -> Vec<Line> {
        let id = BARRIER_BIT | self.next_barrier;
        self.next_barrier += 1;
        self.send(ServerboundPong { id }).await;
        let mut seen = Vec::new();
        self.expect("the sync barrier", |packet| match packet {
            ClientboundGamePacket::Ping(ping) if ping.id == id => Some(()),
            packet => {
                seen.extend(line(packet));
                None
            }
        })
        .await;
        seen
    }

    /// Reads the kick's `Disconnect` and returns its reason, then ignores it
    /// like a misbehaving client: keeps reading and answering keep-alives
    /// until the server closes the connection.
    async fn kicked(mut self) -> String {
        let reason = self
            .expect("the kick", |packet| match packet {
                ClientboundGamePacket::Disconnect(disconnect) => {
                    let base = disconnect.reason.get_base();
                    Some(plain_reason(
                        disconnect.reason.to_string(),
                        base.style.color.as_ref().map(|color| color.to_string()),
                    ))
                }
                _ => None,
            })
            .await;
        // Keep-alives would keep a per-read timeout from expiring: bound the
        // whole wait. The server gives a kicked connection 2 s to flush.
        let mut after = Vec::new();
        let name = self.name.clone();
        within(&name, "the server to close the connection", async {
            loop {
                match self.read().await {
                    Ok(packet) => after.push(format!("{packet:?}")),
                    Err(error) if error == "closed" => break,
                    Err(error) => panic!("{name}: {error} after the kick"),
                }
            }
        })
        .await;
        // The kicked client is sent nothing after its Disconnect.
        assert!(
            after.is_empty(),
            "{name} received packets after its Disconnect: {after:?}"
        );
        reason
    }
}

fn plain_reason(text: String, color: Option<String>) -> String {
    assert_eq!(
        color.as_deref(),
        Some("red"),
        "kick reasons are red: {text:?}"
    );
    text
}

// -------------------------------------------------------------------------
// Scenarios
// -------------------------------------------------------------------------

/// `/list` names every ready player once, with the right count, to its sender
/// only; it follows joins and leaves, leaves out a player still loading the
/// world, and agrees with the tab list and the server-list player count.
#[tokio::test]
async fn atp_012_list_follows_the_ready_players() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    alice
        .wait_until("Bob in the tab list", |view| {
            view.players.contains_key(&bob.uuid())
        })
        .await;

    let [to_alice, to_bob] = run(&alice, "list", [&alice, &bob]).await;
    assert_eq!(listed(&to_alice), names(&["Alice", "Bob"]));
    assert!(
        to_bob.is_empty(),
        "the reply goes to the sender only: {to_bob:?}"
    );
    assert_eq!(tab_list(&alice), names(&["Alice", "Bob"]));
    assert_status_online(&server, 2).await;
    // Both players can use it and get the same answer.
    let [to_alice, to_bob] = run(&bob, "list", [&alice, &bob]).await;
    assert!(to_alice.is_empty(), "{to_alice:?}");
    assert_eq!(listed(&to_bob), names(&["Alice", "Bob"]));

    // A player still loading the world (no PlayerLoaded yet) is not online
    // yet: not listed, not in the tab list, not counted.
    let mut loading = Raw::join(&server, "Loading", false).await;
    let [to_alice, to_bob] = run(&alice, "list", [&alice, &bob]).await;
    assert_eq!(listed(&to_alice), names(&["Alice", "Bob"]));
    assert!(to_bob.is_empty(), "{to_bob:?}");
    let heard = loading.sync().await;
    assert!(
        heard.is_empty(),
        "the loading player hears nothing: {heard:?}"
    );
    assert_eq!(tab_list(&alice), names(&["Alice", "Bob"]));
    assert_status_online(&server, 2).await;
    drop(loading);

    let carol = server.join("Carol").await;
    alice
        .wait_until("Carol in the tab list", |view| {
            view.players.contains_key(&carol.uuid())
        })
        .await;
    let [to_alice, to_bob, to_carol] = run(&carol, "list", [&alice, &bob, &carol]).await;
    assert_eq!(listed(&to_carol), names(&["Alice", "Bob", "Carol"]));
    assert_eq!((to_alice, to_bob), (vec![], vec![]));
    assert_eq!(tab_list(&alice), names(&["Alice", "Bob", "Carol"]));
    assert_status_online(&server, 3).await;

    let bob_uuid = bob.uuid();
    bob.disconnect().await;
    alice
        .wait_until("Bob gone from the tab list", |view| {
            !view.players.contains_key(&bob_uuid)
        })
        .await;
    let [to_alice, to_carol] = run(&alice, "list", [&alice, &carol]).await;
    assert_eq!(listed(&to_alice), names(&["Alice", "Carol"]));
    assert!(to_carol.is_empty(), "{to_carol:?}");
    assert_eq!(tab_list(&alice), names(&["Alice", "Carol"]));
    assert_status_online(&server, 2).await;

    alice.assert_healthy();
    carol.assert_healthy();
    server.stop().await;
}

/// `/tell` and its alias `/msg` deliver the text, whole, to the recipient
/// only, and echo it to the sender; every misuse is answered to the sender
/// alone.
#[tokio::test]
async fn atp_012_tell_reaches_only_its_recipient() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let carol = server.join("Carol").await;
    let everyone = [&alice, &bob, &carol];

    for (command, recipient) in [("tell", "Bob"), ("msg", "Bob"), ("tell", "bOB")] {
        let text = "Grüße aus Köln, 世界 😀 — meet at /spawn {\"text\":\"x\"} <Carol> 100%";
        let [to_alice, to_bob, to_carol] =
            run(&alice, &format!("{command} {recipient} {text}"), everyone).await;
        assert_eq!(
            to_bob,
            [plain(&format!("Alice whispers to you: {text}"), "gray")],
            "/{command} {recipient}"
        );
        assert_eq!(
            to_alice,
            [plain(&format!("You whisper to Bob: {text}"), "white")],
            "/{command} {recipient}: the sender's echo names the recipient as listed"
        );
        assert!(
            to_carol.is_empty(),
            "/{command}: a third player receives nothing: {to_carol:?}"
        );
    }

    // The other direction, and a selector the docs list.
    let [to_alice, to_bob, to_carol] = run(&bob, "tell Alice ok", everyone).await;
    assert_eq!(to_alice, [plain("Bob whispers to you: ok", "gray")]);
    assert_eq!(to_bob, [plain("You whisper to Alice: ok", "white")]);
    assert!(to_carol.is_empty(), "{to_carol:?}");

    // Telling yourself shows both lines to you and nothing to anyone else.
    for target in ["Carol", "@s"] {
        let [to_alice, to_bob, to_carol] =
            run(&carol, &format!("tell {target} note to self"), everyone).await;
        assert_eq!(
            to_carol,
            [
                plain("Carol whispers to you: note to self", "gray"),
                plain("You whisper to Carol: note to self", "white"),
            ]
        );
        assert_eq!((to_alice, to_bob), (vec![], vec![]));
    }

    // Misuse: answered to the sender only, nobody else hears anything.
    let usage = "/tell <player:player> <message:text>...";
    for (command, error) in [
        (
            "tell Nobody hi",
            "Invalid value 'Nobody' for <player>: expected player (player 'Nobody' not found)",
        ),
        (
            "tell Bob",
            "Missing required argument <message> (expected text)",
        ),
        (
            "tell",
            "Missing required argument <player> (expected player)",
        ),
        (
            "tell @a hi",
            "Invalid value '@a' for <player>: expected player ('@a' selects several players, expected one)",
        ),
    ] {
        let [to_alice, to_bob, to_carol] = run(&alice, command, everyone).await;
        let expected: &[Line] = if command == "tell" {
            // Both arguments are missing: one error each, then the usage.
            &[
                plain(error, "red"),
                plain("Missing required argument <message> (expected text)", "red"),
                plain(&format!("Usage: {usage}"), "gray"),
            ]
        } else {
            &[
                plain(error, "red"),
                plain(&format!("Usage: {usage}"), "gray"),
            ]
        };
        assert_eq!(to_alice, expected, "/{command}");
        assert_eq!((to_bob, to_carol), (vec![], vec![]), "/{command}");
    }

    // A formatting code would restyle the recipient's line: refused.
    let [to_alice, to_bob, to_carol] = run(&alice, "tell Bob §4§lurgent", everyone).await;
    assert_eq!(to_alice, [plain("Illegal characters in chat", "red")]);
    assert_eq!((to_bob, to_carol), (vec![], vec![]));

    // A recipient who just left is unknown again.
    let bob_uuid = bob.uuid();
    bob.disconnect().await;
    alice
        .wait_until("Bob gone from the tab list", |view| {
            !view.players.contains_key(&bob_uuid)
        })
        .await;
    let [to_alice, to_carol] = run(&alice, "tell Bob are you there?", [&alice, &carol]).await;
    assert_refused(
        &to_alice,
        "Invalid value 'Bob' for <player>: expected player (player 'Bob' not found)",
        usage,
    );
    assert!(to_carol.is_empty(), "{to_carol:?}");

    alice.assert_healthy();
    carol.assert_healthy();
    server.stop().await;
}

/// `/broadcast` shows its text once to every connected player, the sender
/// included, and to nobody who joins later; it cannot restyle the line or
/// start a new one.
#[tokio::test]
async fn atp_012_broadcast_reaches_every_player_once() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let carol = server.join("Carol").await;
    let everyone = [&alice, &bob, &carol];

    let text = "Server restart at 18:00 — ça va? 再见 😀 <Alice> {\"text\":\"x\"}";
    let [to_alice, to_bob, to_carol] = run(&bob, &format!("broadcast {text}"), everyone).await;
    let expected = [plain(&format!("[Broadcast] {text}"), "white")];
    assert_eq!(to_alice, expected);
    assert_eq!(to_bob, expected, "the sender sees it too");
    assert_eq!(to_carol, expected);

    // Someone who joins afterwards gets no old broadcast.
    let dave = server.join("Dave").await;
    dave.sync().await;
    let old: Vec<Line> = lines(&dave)
        .into_iter()
        .filter(|line| line.text.contains("[Broadcast]"))
        .collect();
    assert!(old.is_empty(), "{old:?}");
    let everyone = [&alice, &bob, &carol, &dave];
    let [to_alice, to_bob, to_carol, to_dave] = run(&dave, "broadcast hi all", everyone).await;
    for seen in [to_alice, to_bob, to_carol, to_dave] {
        assert_eq!(seen, [plain("[Broadcast] hi all", "white")]);
    }

    // No text: refused, to the sender only.
    let [to_alice, to_bob, to_carol, to_dave] = run(&alice, "broadcast", everyone).await;
    assert_refused(
        &to_alice,
        "Missing required argument <message> (expected text)",
        "/broadcast <message:text>...",
    );
    assert_eq!((to_bob, to_carol, to_dave), (vec![], vec![], vec![]));

    // A line break cannot start a second line that looks like someone else's.
    let [to_alice, to_bob, to_carol, to_dave] =
        run(&alice, "broadcast first\n<Bob> forged", everyone).await;
    for seen in [to_alice, to_bob, to_carol, to_dave] {
        assert_eq!(seen, [plain("[Broadcast] first <Bob> forged", "white")]);
    }

    // Formatting codes and other control characters would restyle or hide
    // part of the line: refused, to the sender only.
    for text in ["§cred §kobfuscated", "§r", "bell\u{7}", "nul\u{0}"] {
        let [to_alice, to_bob, to_carol, to_dave] =
            run(&alice, &format!("broadcast {text}"), everyone).await;
        assert_eq!(
            to_alice,
            [plain("Illegal characters in chat", "red")],
            "{text:?}"
        );
        assert_eq!(
            (to_bob, to_carol, to_dave),
            (vec![], vec![], vec![]),
            "{text:?}"
        );
    }

    for bot in everyone {
        bot.assert_healthy();
    }
    server.stop().await;
}

/// `/say` shows its text after the sender's name to every connected player;
/// like `/broadcast`, it refuses formatting codes and control characters, and
/// a refused line reaches nobody.
#[tokio::test]
async fn atp_012_say_refuses_text_that_would_restyle_the_line() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    let everyone = [&alice, &bob];

    for text in ["§cred §kobfuscated", "§r", "bell\u{7}", "nul\u{0}"] {
        let [to_alice, to_bob] = run(&alice, &format!("say {text}"), everyone).await;
        assert_eq!(
            to_alice,
            [plain("Illegal characters in chat", "red")],
            "{text:?}"
        );
        assert!(to_bob.is_empty(), "{text:?}: {to_bob:?}");
    }

    // Clean text still reaches everyone, the sender included.
    let text = "Grüße aus Köln 😀 <Bob> {\"text\":\"x\"} 100%";
    let [to_alice, to_bob] = run(&alice, &format!("say {text}"), everyone).await;
    let expected = [plain(&format!("[Alice] {text}"), "white")];
    assert_eq!(to_alice, expected);
    assert_eq!(to_bob, expected);

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

/// A kick reason is shown to the kicked player and to the kicker, so one
/// holding formatting codes or control characters is refused like `/tell`
/// text: nobody is kicked and only the kicker is answered. A clean reason
/// still kicks and is shown.
#[tokio::test]
async fn atp_012_kick_refuses_a_reason_that_would_restyle_it() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let mut bob = Raw::join(&server, "Bob", true).await;
    let bob_uuid = bob.uuid;
    alice
        .wait_until("Bob listed", |view| view.players.contains_key(&bob_uuid))
        .await;

    for reason in ["§4§lBanned §kforever", "§r", "bell\u{7}", "nul\u{0}"] {
        let [to_alice] = run(&alice, &format!("kick Bob {reason}"), [&alice]).await;
        assert_eq!(
            to_alice,
            [plain("Illegal characters in chat", "red")],
            "{reason:?}"
        );
        let heard = bob.sync().await;
        assert!(heard.is_empty(), "{reason:?}: {heard:?}");
    }
    let [to_alice] = run(&alice, "list", [&alice]).await;
    assert_eq!(listed(&to_alice), names(&["Alice", "Bob"]));
    assert_status_online(&server, 2).await;

    let reason = "Spamming, ça va? 100% <Alice>";
    let [to_alice] = run(&alice, &format!("kick Bob {reason}"), [&alice]).await;
    assert_eq!(
        to_alice,
        [plain(&format!("Kicked Bob (reason: {reason})"), "white")]
    );
    assert_eq!(bob.kicked().await, reason);
    assert_status_online(&server, 1).await;

    alice.assert_healthy();
    server.stop().await;
}

/// `/kick` sends the kicked player the reason and the server closes its
/// connection even though the client ignores the packet; the other players
/// see the player leave the world, the tab list, `/list` and the server-list
/// count. The kicker is told, misuse disconnects nobody, and the kicked
/// player can come back as a normal player.
#[tokio::test]
async fn atp_012_kick_disconnects_the_player_and_others_see_it_leave() {
    let server = TestServer::builder().operators().start().await;
    let alice = server.join("Alice").await;
    let carol = server.join("Carol").await;
    let mut bob = Raw::join(&server, "Bob", true).await;
    let bob_uuid = bob.uuid;
    let bob_id = bob.entity_id.unwrap();
    for viewer in [&alice, &carol] {
        viewer
            .wait_until("Bob listed and spawned", |view| {
                view.players.contains_key(&bob_uuid) && view.entity_by_uuid(bob_uuid).is_some()
            })
            .await;
    }
    let carol_position = carol.position();
    assert_status_online(&server, 3).await;

    // Misuse disconnects nobody and is answered to the kicker only.
    let usage = "/kick <player:player> [reason:text]...";
    for (command, error) in [
        (
            "kick Nobody",
            "Invalid value 'Nobody' for <player>: expected player (player 'Nobody' not found)",
        ),
        (
            "kick",
            "Missing required argument <player> (expected player)",
        ),
    ] {
        let [to_alice, to_carol] = run(&alice, command, [&alice, &carol]).await;
        assert_refused(&to_alice, error, usage);
        assert!(to_carol.is_empty(), "/{command}: {to_carol:?}");
        let heard = bob.sync().await;
        assert!(heard.is_empty(), "/{command}: {heard:?}");
    }

    // The default reason. The name is matched without case.
    let [to_alice, to_carol] = run(&alice, "kick bob", [&alice, &carol]).await;
    assert_eq!(
        to_alice,
        [plain("Kicked Bob (reason: Kicked by an operator)", "white")]
    );
    assert!(
        to_carol.is_empty(),
        "the confirmation goes to the kicker only: {to_carol:?}"
    );
    assert_eq!(bob.kicked().await, "Kicked by an operator");

    // The client view drops an entity only on a RemoveEntities naming it (and
    // records an anomaly for an unknown id), a tab-list entry only on a
    // PlayerInfoRemove.
    for viewer in [&alice, &carol] {
        viewer
            .wait_until("Bob gone from the world and the tab list", |view| {
                !view.players.contains_key(&bob_uuid) && !view.entities.contains_key(&bob_id)
            })
            .await;
    }
    let [to_alice, to_carol] = run(&carol, "list", [&alice, &carol]).await;
    assert_eq!(listed(&to_carol), names(&["Alice", "Carol"]));
    assert!(to_alice.is_empty(), "{to_alice:?}");
    assert_status_online(&server, 2).await;

    // The kick did not disturb the others.
    assert_eq!(carol.position(), carol_position);
    assert!(carol.view(|view| view.entity_by_uuid(alice.uuid()).is_some()));
    assert!(alice.view(|view| view.entity_by_uuid(carol.uuid()).is_some()));

    // Bob comes back under the same name and is a normal player again.
    let mut bob = Raw::join(&server, "Bob", true).await;
    let bob_id = bob.entity_id.unwrap();
    for viewer in [&alice, &carol] {
        viewer
            .wait_until("Bob listed and spawned again", |view| {
                view.players.get(&bob_uuid).map(String::as_str) == Some("Bob")
                    && view.entity_by_uuid(bob_uuid).map(|(id, _)| id) == Some(bob_id)
            })
            .await;
    }
    let [to_alice, to_carol] = run(&alice, "list", [&alice, &carol]).await;
    assert_eq!(listed(&to_alice), names(&["Alice", "Bob", "Carol"]));
    assert!(to_carol.is_empty(), "{to_carol:?}");
    assert_status_online(&server, 3).await;
    let [to_alice, to_carol] = run(&alice, "tell Bob welcome back", [&alice, &carol]).await;
    assert_eq!(
        to_alice,
        [plain("You whisper to Bob: welcome back", "white")]
    );
    assert!(to_carol.is_empty(), "{to_carol:?}");
    assert_eq!(
        bob.sync().await,
        [plain("Alice whispers to you: welcome back", "gray")]
    );
    bob.command("list").await;
    assert_eq!(listed(&bob.sync().await), names(&["Alice", "Bob", "Carol"]));

    // A custom reason of several words, kicked by Carol this time.
    let [to_alice, to_carol] = run(
        &carol,
        "kick Bob Spamming the chat, see the rules (section 2)",
        [&alice, &carol],
    )
    .await;
    assert!(to_alice.is_empty(), "{to_alice:?}");
    assert_eq!(
        to_carol,
        [plain(
            "Kicked Bob (reason: Spamming the chat, see the rules (section 2))",
            "white"
        )]
    );
    assert_eq!(
        bob.kicked().await,
        "Spamming the chat, see the rules (section 2)"
    );
    alice
        .wait_until("Bob gone again", |view| {
            !view.players.contains_key(&bob_uuid) && view.entity_by_uuid(bob_uuid).is_none()
        })
        .await;
    assert_status_online(&server, 2).await;

    // Kicking yourself is allowed: Dave is disconnected like anyone else.
    let mut dave = Raw::join(&server, "Dave", true).await;
    let dave_uuid = dave.uuid;
    alice
        .wait_until("Dave listed", |view| view.players.contains_key(&dave_uuid))
        .await;
    dave.command("kick @s Leaving now").await;
    assert_eq!(dave.kicked().await, "Leaving now");
    for viewer in [&alice, &carol] {
        viewer
            .wait_until("Dave gone", |view| {
                !view.players.contains_key(&dave_uuid) && view.entity_by_uuid(dave_uuid).is_none()
            })
            .await;
    }
    let [to_alice, to_carol] = run(&alice, "list", [&alice, &carol]).await;
    assert_eq!(listed(&to_alice), names(&["Alice", "Carol"]));
    assert!(to_carol.is_empty(), "{to_carol:?}");

    alice.assert_healthy();
    carol.assert_healthy();
    server.stop().await;
}
