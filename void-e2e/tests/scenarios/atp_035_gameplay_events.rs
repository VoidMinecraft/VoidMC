//! VMC-ATP-035: an application plugin observes movement, chat, block and item
//! events through Bevy observers while real clients play.
//!
//! The plugin below uses public API only. It records every semantic event it
//! observes, naming the acting player through the ECS, and each step asserts
//! the whole log, so a missing, duplicated or unexpected event fails.

use azalea_entity::LookDirection;
use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::components::{ClientId, PlayerName, Position, Rotation, ServerControlledPosition};
use voidmc::events::{
    BlockBreakEvent, BlockChangeEvent, BlockPlaceEvent, ChatCommandEvent, ChatMessageEvent,
    PlayerCancelDiggingEvent, PlayerChangeSlotEvent, PlayerDropItemEvent, PlayerFinishDiggingEvent,
    PlayerInputEvent, PlayerInteractEntityEvent, PlayerMoveEvent, PlayerRotateEvent,
    PlayerSneakEvent, PlayerSprintEvent, PlayerStartDiggingEvent, PlayerSwapHandsEvent,
    PlayerSwingArmEvent, PlayerTeleportEvent, PlayerToggleFlyEvent, PlayerUseItemEvent,
    PlayerUseItemOnBlockEvent,
};
use voidmc::{
    BlockFace, BlockPosition, ChunkData, ChunkIndex, ChunkPos as ServerChunkPos, DimensionId, Hand,
    Inventory, ItemStack as ServerItemStack, On,
};
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::entity_id::MinecraftEntityId;
use voidmc_e2e::core::position::{BlockPos, Vec3};
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::common::movements::MoveFlags;
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    s_interact::InteractionHand,
    s_move_player_pos::ServerboundMovePlayerPos,
    s_move_player_pos_rot::ServerboundMovePlayerPosRot,
    s_move_player_rot::ServerboundMovePlayerRot,
    s_player_action::{Action as DigAction, ServerboundPlayerAction},
    s_player_command::{Action as CommandAction, ServerboundPlayerCommand},
    s_player_input::ServerboundPlayerInput,
    s_set_creative_mode_slot::ServerboundSetCreativeModeSlot,
    s_use_item::ServerboundUseItem,
    s_use_item_on::{BlockHit, ServerboundUseItemOn},
};
use voidmc_e2e::registry::builtin::ItemKind;
use voidmc_e2e::{Bot, TestServer};

use crate::ground;

type Xyz = (f64, f64, f64);

/// One observed event, with the acting player resolved to its name inside
/// the observer and, where the docs promise it, the ECS state at that moment.
#[derive(Clone, Debug, PartialEq)]
enum Seen {
    Move {
        player: String,
        from: Xyz,
        to: Xyz,
        position_now: Option<Xyz>,
    },
    Rotate {
        player: String,
        yaw: f32,
        pitch: f32,
        rotation_now: Option<(f32, f32)>,
    },
    Chat {
        player: String,
        client: String,
        message: String,
    },
    Command {
        player: String,
        client: String,
        command: String,
        args: Vec<String>,
    },
    UseItem {
        player: String,
        hand: Hand,
        sequence: i32,
        held: Option<&'static str>,
    },
    UseItemOnBlock {
        player: String,
        hand: Hand,
        position: BlockPosition,
        face: BlockFace,
        cursor: (f32, f32, f32),
        inside_block: bool,
        sequence: i32,
        held: Option<&'static str>,
    },
    StartDigging {
        player: String,
        position: BlockPosition,
        sequence: i32,
    },
    CancelDigging {
        player: String,
    },
    FinishDigging {
        player: String,
    },
    BlockChange {
        source: Option<String>,
        position: BlockPosition,
        old_state: i32,
        new_state: i32,
        world_now: Option<i32>,
    },
    BlockPlace {
        player: String,
        position: BlockPosition,
        face: BlockFace,
        placed_state: i32,
        world_now: Option<i32>,
    },
    BlockBreak {
        player: String,
        position: BlockPosition,
        broken_state: i32,
        world_now: Option<i32>,
    },
    Input {
        player: String,
        forward: bool,
        sneak: bool,
        sprint: bool,
    },
    Sneak {
        player: String,
        sneaking: bool,
    },
    Sprint {
        player: String,
        sprinting: bool,
    },
    Other {
        player: String,
        event: &'static str,
    },
}

#[derive(Resource, Default)]
struct EventLog {
    recording: bool,
    seen: Vec<Seen>,
}

#[derive(SystemParam)]
struct Recorder<'w, 's> {
    log: ResMut<'w, EventLog>,
    players: Query<'w, 's, (&'static PlayerName, &'static ClientId)>,
}

impl Recorder<'_, '_> {
    fn name(&self, entity: Entity) -> String {
        self.players
            .get(entity)
            .map(|(name, _)| name.0.clone())
            .unwrap_or_else(|_| format!("not a player: {entity}"))
    }

    fn client(&self, client_id: u32) -> String {
        self.players
            .iter()
            .find(|(_, id)| id.0 == client_id)
            .map(|(name, _)| name.0.clone())
            .unwrap_or_else(|| format!("unknown client {client_id}"))
    }

    fn push(&mut self, seen: Seen) {
        if self.log.recording {
            self.log.seen.push(seen);
        }
    }
}

fn held(inventories: &Query<&Inventory>, player: Entity, hand: Hand) -> Option<&'static str> {
    let inventory = inventories.get(player).ok()?;
    let stack = match hand {
        Hand::MainHand => inventory.held(),
        Hand::OffHand => inventory.get(Inventory::OFFHAND),
    };
    if stack.is_empty() {
        None
    } else {
        stack.item.name()
    }
}

fn state_at(
    index: &ChunkIndex,
    chunks: &Query<&ChunkData>,
    dimension: DimensionId,
    position: BlockPosition,
) -> Option<i32> {
    let chunk = ServerChunkPos::new(position.x.div_euclid(16), position.z.div_euclid(16));
    chunks
        .get(*index.0.get(&(dimension, chunk))?)
        .ok()?
        .get_block(
            position.x.rem_euclid(16) as u8,
            i32::from(position.y),
            position.z.rem_euclid(16) as u8,
        )
}

/// The application plugin under test, installed like `VoidServer::add_plugin`.
fn record_events(app: &mut App) {
    app.init_resource::<EventLog>()
        .add_observer(
            |event: On<PlayerMoveEvent>, mut rec: Recorder, positions: Query<&Position>| {
                let seen = Seen::Move {
                    player: rec.name(event.entity),
                    from: (event.old_x, event.old_y, event.old_z),
                    to: (event.new_x, event.new_y, event.new_z),
                    position_now: positions.get(event.entity).ok().map(|p| (p.x, p.y, p.z)),
                };
                rec.push(seen);
            },
        )
        .add_observer(
            |event: On<PlayerRotateEvent>, mut rec: Recorder, rotations: Query<&Rotation>| {
                let seen = Seen::Rotate {
                    player: rec.name(event.entity),
                    yaw: event.yaw,
                    pitch: event.pitch,
                    rotation_now: rotations.get(event.entity).ok().map(|r| (r.yaw, r.pitch)),
                };
                rec.push(seen);
            },
        )
        .add_observer(|event: On<ChatMessageEvent>, mut rec: Recorder| {
            let seen = Seen::Chat {
                player: rec.name(event.entity),
                client: rec.client(event.client_id),
                message: event.message.clone(),
            };
            rec.push(seen);
        })
        .add_observer(|event: On<ChatCommandEvent>, mut rec: Recorder| {
            let seen = Seen::Command {
                player: rec.name(event.entity),
                client: rec.client(event.client_id),
                command: event.command.clone(),
                args: event.args.clone(),
            };
            rec.push(seen);
        })
        .add_observer(
            |event: On<PlayerUseItemEvent>, mut rec: Recorder, inventories: Query<&Inventory>| {
                let seen = Seen::UseItem {
                    player: rec.name(event.entity),
                    hand: event.hand,
                    sequence: event.sequence,
                    held: held(&inventories, event.entity, event.hand),
                };
                rec.push(seen);
            },
        )
        .add_observer(
            |event: On<PlayerUseItemOnBlockEvent>,
             mut rec: Recorder,
             inventories: Query<&Inventory>| {
                let seen = Seen::UseItemOnBlock {
                    player: rec.name(event.entity),
                    hand: event.hand,
                    position: event.position,
                    face: event.face,
                    cursor: (event.cursor_x, event.cursor_y, event.cursor_z),
                    inside_block: event.inside_block,
                    sequence: event.sequence,
                    held: held(&inventories, event.entity, event.hand),
                };
                rec.push(seen);
            },
        )
        .add_observer(|event: On<PlayerStartDiggingEvent>, mut rec: Recorder| {
            let seen = Seen::StartDigging {
                player: rec.name(event.entity),
                position: event.position,
                sequence: event.sequence,
            };
            rec.push(seen);
        })
        .add_observer(|event: On<PlayerCancelDiggingEvent>, mut rec: Recorder| {
            let player = rec.name(event.entity);
            rec.push(Seen::CancelDigging { player });
        })
        .add_observer(|event: On<PlayerFinishDiggingEvent>, mut rec: Recorder| {
            let player = rec.name(event.entity);
            rec.push(Seen::FinishDigging { player });
        })
        .add_observer(
            |event: On<BlockChangeEvent>,
             mut rec: Recorder,
             index: Res<ChunkIndex>,
             chunks: Query<&ChunkData>| {
                let seen = Seen::BlockChange {
                    source: event.source.map(|source| rec.name(source)),
                    position: event.position,
                    old_state: event.old_state,
                    new_state: event.new_state,
                    world_now: state_at(&index, &chunks, event.dimension, event.position),
                };
                rec.push(seen);
            },
        )
        .add_observer(
            |event: On<BlockPlaceEvent>,
             mut rec: Recorder,
             index: Res<ChunkIndex>,
             chunks: Query<&ChunkData>| {
                let seen = Seen::BlockPlace {
                    player: rec.name(event.entity),
                    position: event.position,
                    face: event.face,
                    placed_state: event.placed_state,
                    world_now: state_at(&index, &chunks, event.dimension, event.position),
                };
                rec.push(seen);
            },
        )
        .add_observer(
            |event: On<BlockBreakEvent>,
             mut rec: Recorder,
             index: Res<ChunkIndex>,
             chunks: Query<&ChunkData>| {
                let seen = Seen::BlockBreak {
                    player: rec.name(event.entity),
                    position: event.position,
                    broken_state: event.broken_state,
                    world_now: state_at(&index, &chunks, event.dimension, event.position),
                };
                rec.push(seen);
            },
        )
        .add_observer(|event: On<PlayerInputEvent>, mut rec: Recorder| {
            let seen = Seen::Input {
                player: rec.name(event.entity),
                forward: event.forward,
                sneak: event.sneak,
                sprint: event.sprint,
            };
            rec.push(seen);
        })
        .add_observer(|event: On<PlayerSneakEvent>, mut rec: Recorder| {
            let seen = Seen::Sneak {
                player: rec.name(event.entity),
                sneaking: event.sneaking,
            };
            rec.push(seen);
        })
        .add_observer(|event: On<PlayerSprintEvent>, mut rec: Recorder| {
            let seen = Seen::Sprint {
                player: rec.name(event.entity),
                sprinting: event.sprinting,
            };
            rec.push(seen);
        });

    // Events no step should produce: any of them in the log fails the step.
    fn other<E: Event>(
        event: &'static str,
        entity: impl Fn(&E) -> Entity,
    ) -> impl Fn(On<E>, Recorder) {
        move |trigger: On<E>, mut rec: Recorder| {
            let player = rec.name(entity(&trigger));
            rec.push(Seen::Other { player, event });
        }
    }
    app.add_observer(other("swing arm", |e: &PlayerSwingArmEvent| e.entity))
        .add_observer(other("change slot", |e: &PlayerChangeSlotEvent| e.entity))
        .add_observer(other("interact entity", |e: &PlayerInteractEntityEvent| {
            e.entity
        }))
        .add_observer(other("toggle fly", |e: &PlayerToggleFlyEvent| e.entity))
        .add_observer(other("drop item", |e: &PlayerDropItemEvent| e.entity))
        .add_observer(other("swap hands", |e: &PlayerSwapHandsEvent| e.entity))
        .add_observer(other("teleport", |e: &PlayerTeleportEvent| e.entity));
}

/// A server running the recording plugin, with Alice and Bob in the world and
/// the log recording from now on.
async fn session() -> (TestServer, Bot, Bot) {
    let server = TestServer::builder()
        .operators()
        .plugin(record_events)
        .start()
        .await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    alice.sync().await;
    bob.sync().await;
    server
        .with_world(|world| world.resource_mut::<EventLog>().recording = true)
        .await;
    (server, alice, bob)
}

async fn take(server: &TestServer) -> Vec<Seen> {
    server
        .with_world(|world| std::mem::take(&mut world.resource_mut::<EventLog>().seen))
        .await
}

/// Asserts that exactly `expected` was observed since the last step, in any
/// order: nothing missing, nothing extra, nothing twice.
async fn assert_step(server: &TestServer, step: &str, expected: Vec<Seen>) {
    let mut seen = take(server).await;
    let mut expected = expected;
    let key = |seen: &Seen| format!("{seen:?}");
    seen.sort_by_key(key);
    expected.sort_by_key(key);
    assert_eq!(seen, expected, "events observed for: {step}");
}

async fn server_position(server: &TestServer, bot: &Bot) -> Xyz {
    let entity = server.player_entity(bot.uuid()).await;
    server
        .with_world(move |world| {
            let position = world.get::<Position>(entity).unwrap();
            (position.x, position.y, position.z)
        })
        .await
}

async fn server_block(server: &TestServer, position: BlockPosition) -> i32 {
    server
        .with_world(move |world| {
            let chunk = ServerChunkPos::new(position.x.div_euclid(16), position.z.div_euclid(16));
            let entity = world.resource::<ChunkIndex>().0[&(DimensionId::Overworld, chunk)];
            world
                .get::<ChunkData>(entity)
                .and_then(|chunk| {
                    chunk.get_block(
                        position.x.rem_euclid(16) as u8,
                        i32::from(position.y),
                        position.z.rem_euclid(16) as u8,
                    )
                })
                .expect("the block is loaded on the server")
        })
        .await
}

fn xyz(position: Vec3) -> Xyz {
    (position.x, position.y, position.z)
}

fn server_pos(pos: BlockPos) -> BlockPosition {
    BlockPosition {
        x: pos.x,
        y: pos.y as i16,
        z: pos.z,
    }
}

const STANDING: MoveFlags = MoveFlags {
    on_ground: true,
    horizontal_collision: false,
};

fn mv(player: &str, from: Xyz, to: Xyz) -> Seen {
    Seen::Move {
        player: player.to_string(),
        from,
        to,
        position_now: Some(to),
    }
}

fn rot(player: &str, yaw: f32, pitch: f32) -> Seen {
    Seen::Rotate {
        player: player.to_string(),
        yaw,
        pitch,
        rotation_now: Some((yaw, pitch)),
    }
}

fn command(player: &str, command: &str, args: &[&str]) -> Seen {
    Seen::Command {
        player: player.to_string(),
        client: player.to_string(),
        command: command.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
    }
}

#[tokio::test]
async fn movement_packets_fire_one_move_or_rotate_event_for_the_moving_player() {
    let (server, alice, bob) = session().await;
    let start = server_position(&server, &alice).await;
    let bob_start = server_position(&server, &bob).await;

    let first = Vec3::new(start.0 + 0.5, start.1, start.2 - 0.25);
    alice.move_to(first).await;
    alice.sync().await;
    assert_step(
        &server,
        "a position-only packet",
        vec![mv("Alice", start, xyz(first))],
    )
    .await;

    // Yaw is not normalised: the client's unwrapped value is reported as sent.
    alice
        .send(ServerboundMovePlayerRot {
            look_direction: LookDirection::new(450.5, -45.25),
            flags: STANDING,
        })
        .await;
    alice.sync().await;
    assert_step(
        &server,
        "a rotation-only packet",
        vec![rot("Alice", 450.5, -45.25)],
    )
    .await;

    let second = Vec3::new(first.x - 1.0, first.y + 0.5, first.z + 2.0);
    alice
        .send(ServerboundMovePlayerPosRot {
            pos: second,
            look_direction: LookDirection::new(-90.0, 10.0),
            flags: STANDING,
        })
        .await;
    alice.sync().await;
    assert_step(
        &server,
        "a position-and-rotation packet",
        vec![
            mv("Alice", xyz(first), xyz(second)),
            rot("Alice", -90.0, 10.0),
        ],
    )
    .await;

    // Clients resend an unchanged position; the event still fires, from == to.
    alice.move_to(second).await;
    alice.sync().await;
    assert_step(
        &server,
        "a packet repeating the position",
        vec![mv("Alice", xyz(second), xyz(second))],
    )
    .await;

    let bob_to = Vec3::new(bob_start.0 - 0.75, bob_start.1, bob_start.2 + 0.5);
    bob.move_to(bob_to).await;
    bob.sync().await;
    assert_step(
        &server,
        "Bob moving",
        vec![mv("Bob", bob_start, xyz(bob_to))],
    )
    .await;

    // While the server owns the position, client positions are rejected: no
    // move event, but the rotation still applies.
    let alice_entity = server.player_entity(alice.uuid()).await;
    server
        .with_world(move |world| {
            world
                .entity_mut(alice_entity)
                .insert(ServerControlledPosition);
        })
        .await;
    alice
        .send(ServerboundMovePlayerPos {
            pos: Vec3::new(500.0, 100.0, 500.0),
            flags: STANDING,
        })
        .await;
    alice
        .send(ServerboundMovePlayerPosRot {
            pos: Vec3::new(500.0, 100.0, 500.0),
            look_direction: LookDirection::new(30.0, 5.0),
            flags: STANDING,
        })
        .await;
    alice.sync().await;
    assert_step(
        &server,
        "positions sent while the server controls the position",
        vec![rot("Alice", 30.0, 5.0)],
    )
    .await;
    assert_eq!(server_position(&server, &alice).await, xyz(second));
    server
        .with_world(move |world| {
            world
                .entity_mut(alice_entity)
                .remove::<ServerControlledPosition>();
        })
        .await;

    // A server-side teleport moves the player without a move event; the
    // client's confirmation echo is a position packet like any other.
    let destination = (second.x + 3.0, second.y + 10.0, second.z - 3.0);
    alice
        .command(&format!(
            "tp {} {} {}",
            destination.0, destination.1, destination.2
        ))
        .await;
    let look = alice
        .expect("the teleport", |packet| match packet {
            ClientboundGamePacket::PlayerPosition(teleport)
                if xyz(teleport.change.pos) == destination =>
            {
                Some((
                    teleport.change.look_direction.y_rot(),
                    teleport.change.look_direction.x_rot(),
                ))
            }
            _ => None,
        })
        .await;
    assert_eq!(look, (30.0, 5.0), "/tp keeps the server-side rotation");
    // The first barrier is read after the bot answered the teleport, the
    // second one after the server handled that answer.
    alice.sync().await;
    alice.sync().await;
    let args = [destination.0, destination.1, destination.2].map(|v| v.to_string());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_step(
        &server,
        "a server-side teleport and the client's echo",
        vec![
            command("Alice", "tp", &args),
            mv("Alice", destination, destination),
            rot("Alice", look.0, look.1),
        ],
    )
    .await;

    assert_step(&server, "the end of the session", vec![]).await;
    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn chat_lines_fire_one_chat_event_and_commands_fire_none() {
    let (server, alice, bob) = session().await;

    const LINE: &str = "Grüße, 世界! ✓ <b>not markup</b>";
    alice.chat(LINE).await;
    bob.expect("Alice's chat line", |packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => {
            chat.content.to_string().contains(LINE).then_some(())
        }
        _ => None,
    })
    .await;
    alice.sync().await;
    assert_step(
        &server,
        "a chat line",
        vec![Seen::Chat {
            player: "Alice".into(),
            client: "Alice".into(),
            message: LINE.into(),
        }],
    )
    .await;

    bob.chat("hi Alice").await;
    bob.sync().await;
    assert_step(
        &server,
        "Bob's chat line",
        vec![Seen::Chat {
            player: "Bob".into(),
            client: "Bob".into(),
            message: "hi Alice".into(),
        }],
    )
    .await;

    // A command, sent as a command or typed as a chat line, is not chat.
    alice.command("list").await;
    alice.sync().await;
    assert_step(&server, "a command", vec![command("Alice", "list", &[])]).await;
    bob.chat("/say héllo there").await;
    bob.sync().await;
    assert_step(
        &server,
        "a command typed as a chat line",
        vec![command("Bob", "say", &["héllo", "there"])],
    )
    .await;

    assert_step(&server, "the end of the session", vec![]).await;
    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

async fn give(bot: &Bot, kind: ItemKind) {
    bot.send(ServerboundSetCreativeModeSlot {
        slot_num: 36,
        item_stack: ItemStack::Present(ItemStackData::new(kind, 64)),
    })
    .await;
}

async fn use_on(bot: &Bot, support: BlockPos, seq: u32) {
    bot.send(ServerboundUseItemOn {
        hand: InteractionHand::MainHand,
        block_hit: BlockHit {
            block_pos: support,
            direction: Direction::Up,
            location: Vec3::new(
                f64::from(support.x) + 0.25,
                f64::from(support.y) + 1.0,
                f64::from(support.z) + 0.75,
            ),
            inside: false,
            world_border: false,
        },
        seq,
    })
    .await;
    acknowledged(bot, seq).await;
}

async fn dig(bot: &Bot, pos: BlockPos, seq: u32) {
    bot.send(ServerboundPlayerAction {
        action: DigAction::StartDestroyBlock,
        pos,
        direction: Direction::Up,
        seq,
    })
    .await;
    acknowledged(bot, seq).await;
}

async fn acknowledged(bot: &Bot, seq: u32) {
    bot.expect("the action acknowledgement", |packet| match packet {
        ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == seq => Some(()),
        _ => None,
    })
    .await;
    bot.sync().await;
}

fn used_on(player: &str, support: BlockPos, sequence: i32, held: Option<&'static str>) -> Seen {
    Seen::UseItemOnBlock {
        player: player.into(),
        hand: Hand::MainHand,
        position: server_pos(support),
        face: BlockFace::Top,
        cursor: (0.25, 1.0, 0.75),
        inside_block: false,
        sequence,
        held,
    }
}

fn placed(player: &str, position: BlockPos, old_state: i32, state: i32) -> [Seen; 2] {
    [
        Seen::BlockChange {
            source: Some(player.into()),
            position: server_pos(position),
            old_state,
            new_state: state,
            world_now: Some(state),
        },
        Seen::BlockPlace {
            player: player.into(),
            position: server_pos(position),
            face: BlockFace::Top,
            placed_state: state,
            world_now: Some(state),
        },
    ]
}

fn broken(player: &str, position: BlockPos, sequence: i32, state: i32) -> [Seen; 3] {
    [
        Seen::StartDigging {
            player: player.into(),
            position: server_pos(position),
            sequence,
        },
        Seen::BlockChange {
            source: Some(player.into()),
            position: server_pos(position),
            old_state: state,
            new_state: 0,
            world_now: Some(0),
        },
        Seen::BlockBreak {
            player: player.into(),
            position: server_pos(position),
            broken_state: state,
            world_now: Some(0),
        },
    ]
}

#[tokio::test]
async fn block_and_item_actions_fire_events_for_the_acting_player() {
    let (server, alice, bob) = session().await;
    let stone = ServerItemStack::of("minecraft:stone", 1)
        .unwrap()
        .item
        .default_block_state()
        .unwrap();
    let glass = ServerItemStack::of("minecraft:glass", 1)
        .unwrap()
        .item
        .default_block_state()
        .unwrap();

    let feet = ground(&alice);
    let column = |dx: i32| {
        alice
            .view(|view| view.ground_below(BlockPos::new(feet.x + dx, feet.y + 8, feet.z)))
            .expect("Alice holds the chunks around her")
    };
    let alice_support = column(2);
    let bob_support = column(4);
    let alice_block = alice_support.up(1);
    let bob_block = bob_support.up(1);
    let alice_old = server_block(&server, server_pos(alice_block)).await;
    let bob_old = server_block(&server, server_pos(bob_block)).await;

    give(&alice, ItemKind::Stone).await;
    alice.sync().await;
    assert_step(&server, "a creative inventory change", vec![]).await;

    // Bob's hands are empty: the click is reported, nothing is placed.
    use_on(&bob, bob_support, 1).await;
    assert_step(
        &server,
        "a use on a block with an empty hand",
        vec![used_on("Bob", bob_support, 1, None)],
    )
    .await;

    use_on(&alice, alice_support, 1).await;
    let [change, place] = placed("Alice", alice_block, alice_old, stone);
    assert_step(
        &server,
        "Alice placing stone",
        vec![
            used_on("Alice", alice_support, 1, Some("minecraft:stone")),
            change,
            place,
        ],
    )
    .await;

    give(&bob, ItemKind::Glass).await;
    use_on(&bob, bob_support, 2).await;
    let [change, place] = placed("Bob", bob_block, bob_old, glass);
    assert_step(
        &server,
        "Bob placing glass",
        vec![
            used_on("Bob", bob_support, 2, Some("minecraft:glass")),
            change,
            place,
        ],
    )
    .await;

    // Creative clients break on the first dig packet. Alice breaks Bob's
    // block and Bob breaks Alice's: each event names the breaker.
    dig(&alice, bob_block, 2).await;
    assert_step(
        &server,
        "Alice breaking Bob's glass",
        broken("Alice", bob_block, 2, glass).into(),
    )
    .await;
    dig(&bob, alice_block, 3).await;
    assert_step(
        &server,
        "Bob breaking Alice's stone",
        broken("Bob", alice_block, 3, stone).into(),
    )
    .await;

    // Breaking air changes nothing: the dig is reported, no break.
    dig(&alice, alice_block, 3).await;
    assert_step(
        &server,
        "a dig on air",
        vec![Seen::StartDigging {
            player: "Alice".into(),
            position: server_pos(alice_block),
            sequence: 3,
        }],
    )
    .await;

    for (hand, seq, held) in [
        (InteractionHand::MainHand, 4, Some("minecraft:stone")),
        (InteractionHand::OffHand, 5, None),
    ] {
        alice
            .send(ServerboundUseItem {
                hand,
                seq,
                y_rot: 12.0,
                x_rot: -30.0,
            })
            .await;
        acknowledged(&alice, seq).await;
        assert_step(
            &server,
            &format!("a use in the air with the {hand:?}"),
            vec![Seen::UseItem {
                player: "Alice".into(),
                hand: match hand {
                    InteractionHand::MainHand => Hand::MainHand,
                    InteractionHand::OffHand => Hand::OffHand,
                },
                sequence: seq as i32,
                held,
            }],
        )
        .await;
    }

    assert_step(&server, "the end of the session", vec![]).await;
    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

fn input(player: &str, forward: bool, sneak: bool, sprint: bool) -> Seen {
    Seen::Input {
        player: player.into(),
        forward,
        sneak,
        sprint,
    }
}

#[tokio::test]
async fn sneak_and_sprint_events_follow_what_the_client_reports() {
    let (server, alice, bob) = session().await;

    let keys = |forward: bool, shift: bool, sprint: bool| ServerboundPlayerInput {
        forward,
        shift,
        sprint,
        ..Default::default()
    };

    alice.send(keys(false, true, false)).await;
    alice.sync().await;
    assert_step(
        &server,
        "pressing sneak",
        vec![
            input("Alice", false, true, false),
            Seen::Sneak {
                player: "Alice".into(),
                sneaking: true,
            },
        ],
    )
    .await;

    alice.send(keys(true, true, false)).await;
    alice.sync().await;
    assert_step(
        &server,
        "walking while sneaking",
        vec![input("Alice", true, true, false)],
    )
    .await;

    // A modern client reports sprinting with Player Command action 1.
    bob.send(keys(true, false, true)).await;
    bob.send(ServerboundPlayerCommand {
        id: MinecraftEntityId(bob.entity_id()),
        action: CommandAction::StartSprinting,
        data: 0,
    })
    .await;
    bob.sync().await;
    assert_step(
        &server,
        "Bob starting to sprint",
        vec![
            input("Bob", true, false, true),
            Seen::Sprint {
                player: "Bob".into(),
                sprinting: true,
            },
        ],
    )
    .await;

    bob.send(ServerboundPlayerCommand {
        id: MinecraftEntityId(bob.entity_id()),
        action: CommandAction::StopSprinting,
        data: 0,
    })
    .await;
    alice.send(keys(false, false, false)).await;
    bob.sync().await;
    alice.sync().await;
    assert_step(
        &server,
        "Bob stopping to sprint and Alice releasing sneak",
        vec![
            Seen::Sprint {
                player: "Bob".into(),
                sprinting: false,
            },
            input("Alice", false, false, false),
            Seen::Sneak {
                player: "Alice".into(),
                sneaking: false,
            },
        ],
    )
    .await;

    assert_step(&server, "the end of the session", vec![]).await;
    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}
