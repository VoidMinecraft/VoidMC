//! VMC-ATP-030: a lever or button triggers a note block through basic
//! redstone, as seen by real clients.

use bevy_app::First;
use bevy_ecs::prelude::*;
use voidmc::components::PlayerUuid;
use voidmc::events::BlockChangeEvent;
use voidmc::plugins::redstone::RedstonePlugin;
use voidmc::world::set_block_state;
use voidmc::{BlockPosition, DimensionId};
use voidmc_e2e::block::properties::{
    Face, FacingCardinal, Note, Power, Sound, WireEast, WireNorth, WireSouth, WireWest,
};
use voidmc_e2e::block::{BlockState, BlockTrait, blocks};
use voidmc_e2e::core::direction::Direction;
use voidmc_e2e::core::position::{BlockPos, Vec3};
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket,
    c_sound::SoundSource,
    s_interact::InteractionHand,
    s_player_action::{Action, ServerboundPlayerAction},
    s_player_input::ServerboundPlayerInput,
    s_set_creative_mode_slot::ServerboundSetCreativeModeSlot,
    s_use_item_on::{BlockHit, ServerboundUseItemOn},
};
use voidmc_e2e::registry::Holder;
use voidmc_e2e::registry::builtin::{BlockKind, ItemKind, SoundEvent};
use voidmc_e2e::{Bot, ClientView, TestServer};

/// High above the terrain, inside the chunks around spawn.
const Y: i32 = 150;

fn at(x: i32, z: i32) -> BlockPos {
    BlockPos::new(x, Y, z)
}

fn lever(powered: bool) -> BlockState {
    blocks::Lever {
        face: Face::Floor,
        facing: FacingCardinal::East,
        powered,
    }
    .as_block_state()
}

fn stone_button() -> BlockState {
    blocks::StoneButton {
        face: Face::Floor,
        facing: FacingCardinal::East,
        powered: false,
    }
    .as_block_state()
}

/// Unpowered dust shaped as an east-west line.
fn dust() -> BlockState {
    blocks::RedstoneWire {
        east: WireEast::Side,
        north: WireNorth::None,
        power: Power::_0,
        south: WireSouth::None,
        west: WireWest::Side,
    }
    .as_block_state()
}

fn note_block(note: Note) -> BlockState {
    blocks::NoteBlock {
        instrument: Sound::Harp,
        note,
        powered: false,
    }
    .as_block_state()
}

fn stone() -> BlockState {
    blocks::Stone {}.as_block_state()
}

/// A property of the block a client sees at `pos`, decoded by Azalea.
fn property(view: &ClientView, pos: BlockPos, name: &str) -> Option<&'static str> {
    view.block_at(pos)
        .and_then(|state| Box::<dyn BlockTrait>::from(state).get_property(name))
}

fn powered(view: &ClientView, pos: BlockPos) -> bool {
    property(view, pos, "powered") == Some("true")
}

fn powers(view: &ClientView, line: &[BlockPos]) -> Vec<String> {
    line.iter()
        .map(|&pos| property(view, pos, "power").unwrap_or("-").to_string())
        .collect()
}

fn expected(powers: &[u8]) -> Vec<String> {
    powers.iter().map(u8::to_string).collect()
}

async fn start() -> TestServer {
    TestServer::builder()
        .plugin(|app| {
            app.add_plugins(RedstonePlugin);
        })
        .start()
        .await
}

/// Builds blocks in the overworld through the server API, with a stone floor
/// under each and air above it.
async fn build(server: &TestServer, blocks: Vec<(BlockPos, BlockState)>) {
    let mut changes = Vec::new();
    for (pos, state) in blocks {
        changes.push((pos.down(1), stone()));
        changes.push((pos.up(1), BlockState::AIR));
        changes.push((pos, state));
    }
    set(server, changes).await;
}

/// Sets blocks in the overworld through the server API.
async fn set(server: &TestServer, changes: Vec<(BlockPos, BlockState)>) {
    server
        .with_world(move |world| {
            for (pos, state) in changes {
                let position = BlockPosition {
                    x: pos.x,
                    y: pos.y as i16,
                    z: pos.z,
                };
                set_block_state(
                    world,
                    DimensionId::Overworld,
                    position,
                    i32::from(state.id()),
                    None,
                );
            }
        })
        .await;
}

/// A use of `hand` on the top of `pos`. A click only reaches blocks a few
/// blocks from the player's eyes, so the bot first stands two blocks west of
/// `pos`, at its height, out of the way of the circuits.
async fn use_on(bot: &Bot, pos: BlockPos, hand: InteractionHand, seq: u32) {
    let beside = Vec3::new(
        f64::from(pos.x) - 1.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    );
    bot.move_to(beside).await;
    bot.send(ServerboundUseItemOn {
        hand,
        block_hit: BlockHit {
            block_pos: pos,
            direction: Direction::Up,
            location: Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.1,
                f64::from(pos.z) + 0.5,
            ),
            inside: false,
            world_border: false,
        },
        seq,
    })
    .await;
}

/// A main-hand right-click on `pos`, returned once acknowledged.
async fn click(bot: &Bot, pos: BlockPos, seq: u32) {
    use_on(bot, pos, InteractionHand::MainHand, seq).await;
    expect_ack(bot, seq).await;
}

async fn expect_ack(bot: &Bot, seq: u32) {
    bot.expect("the use acknowledgement", |packet| match packet {
        ClientboundGamePacket::BlockChangedAck(ack) if ack.seq == seq => Some(()),
        _ => None,
    })
    .await;
}

/// Waits for one note played at `pos`: the block event and the harp sound.
async fn expect_note(bot: &Bot, pos: BlockPos, pitch: f32) {
    let (action, parameter, block) = bot
        .expect("the note block event", |packet| match packet {
            ClientboundGamePacket::BlockEvent(event) if event.pos == pos => {
                Some((event.action_id, event.action_parameter, event.block))
            }
            _ => None,
        })
        .await;
    assert_eq!((action, parameter, block), (0, 0, BlockKind::NoteBlock));
    let sound = bot
        .expect("the harp sound", |packet| match packet {
            ClientboundGamePacket::Sound(sound)
                if sound.sound == Holder::Reference(SoundEvent::BlockNoteBlockHarp) =>
            {
                Some(sound.clone())
            }
            _ => None,
        })
        .await;
    assert_eq!(sound.source, SoundSource::Records);
    assert_eq!(
        (sound.x, sound.y, sound.z),
        (pos.x * 8 + 4, pos.y * 8 + 4, pos.z * 8 + 4),
        "the sound comes from the note block's centre"
    );
    assert_eq!(sound.volume, 3.0);
    assert!(
        (sound.pitch - pitch).abs() < 1e-4,
        "pitch {} for the stored note, expected {pitch}",
        sound.pitch
    );
}

/// Notes still queued: block events at `pos`, and harp sounds.
fn queued_notes(bot: &Bot, pos: BlockPos) -> usize {
    bot.queued(|packet| match packet {
        ClientboundGamePacket::BlockEvent(event) if event.pos == pos => Some(()),
        ClientboundGamePacket::Sound(sound)
            if sound.sound == Holder::Reference(SoundEvent::BlockNoteBlockHarp) =>
        {
            Some(())
        }
        _ => None,
    })
    .len()
}

async fn assert_silent(bots: &[&Bot], pos: BlockPos, why: &str) {
    for bot in bots {
        bot.sync().await;
        assert_eq!(queued_notes(bot, pos), 0, "{}: {why}", bot.name());
    }
}

struct Circuit {
    lever: BlockPos,
    line: Vec<BlockPos>,
    note: BlockPos,
}

/// A lever, `dust` dust east of it, then a note block at note 12 (pitch 1).
fn circuit(z: i32, dust_count: i32) -> (Circuit, Vec<(BlockPos, BlockState)>) {
    let circuit = Circuit {
        lever: at(2, z),
        line: (3..3 + dust_count).map(|x| at(x, z)).collect(),
        note: at(3 + dust_count, z),
    };
    let mut blocks = vec![(circuit.lever, lever(false))];
    blocks.extend(circuit.line.iter().map(|&pos| (pos, dust())));
    blocks.push((circuit.note, note_block(Note::_12)));
    (circuit, blocks)
}

async fn wait_for_circuit(bot: &Bot, circuit: &Circuit) {
    let (lever, note) = (circuit.lever, circuit.note);
    bot.wait_until("the prepared circuit", |view| {
        view.block_at(lever).map(BlockKind::from) == Some(BlockKind::Lever)
            && view.block_at(note).map(BlockKind::from) == Some(BlockKind::NoteBlock)
    })
    .await;
}

async fn wait_for_state(bot: &Bot, circuit: &Circuit, on: bool, line: &[u8]) {
    let line_powers = expected(line);
    bot.wait_until(
        &format!("the lever, dust {line:?} and note block powered={on}"),
        |view| {
            powered(view, circuit.lever) == on
                && powers(view, &circuit.line) == line_powers
                && powered(view, circuit.note) == on
        },
    )
    .await;
}

#[tokio::test]
async fn lever_dust_and_note_block_stay_in_sync_for_every_player_and_after_a_rejoin() {
    let server = start().await;
    let (circuit, blocks) = circuit(3, 3);
    build(&server, blocks).await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    // Step 2: both see the prepared circuit, unpowered.
    for bot in [&alice, &bob] {
        wait_for_circuit(bot, &circuit).await;
        wait_for_state(bot, &circuit, false, &[0, 0, 0]).await;
    }

    // Step 3: the lever powers the dust, which triggers the note block once.
    click(&alice, circuit.lever, 1).await;
    for bot in [&alice, &bob] {
        wait_for_state(bot, &circuit, true, &[15, 14, 13]).await;
        expect_note(bot, circuit.note, 1.0).await;
    }
    server.wait_ticks(10).await;
    assert_silent(
        &[&alice, &bob],
        circuit.note,
        "nothing replays while it stays on",
    )
    .await;

    // Step 4: off, then on again: one more note.
    click(&alice, circuit.lever, 2).await;
    for bot in [&alice, &bob] {
        wait_for_state(bot, &circuit, false, &[0, 0, 0]).await;
    }
    assert_silent(
        &[&alice, &bob],
        circuit.note,
        "nothing plays when it turns off",
    )
    .await;
    click(&alice, circuit.lever, 3).await;
    for bot in [&alice, &bob] {
        wait_for_state(bot, &circuit, true, &[15, 14, 13]).await;
        expect_note(bot, circuit.note, 1.0).await;
    }
    assert_silent(
        &[&alice, &bob],
        circuit.note,
        "exactly one note per rising edge",
    )
    .await;

    // Step 5: Alice rejoins; the chunk she receives has the current states.
    let alice_uuid = alice.uuid();
    alice.disconnect().await;
    server
        .wait_for("Alice's player entity to be despawned", move |world| {
            let mut players = world.query::<&PlayerUuid>();
            (!players.iter(world).any(|uuid| uuid.0 == alice_uuid)).then_some(())
        })
        .await;
    let alice = server.join("Alice").await;
    wait_for_circuit(&alice, &circuit).await;
    alice.sync().await;
    alice.view(|view| {
        assert!(powered(view, circuit.lever), "the lever is still on");
        assert_eq!(powers(view, &circuit.line), expected(&[15, 14, 13]));
        assert!(
            powered(view, circuit.note),
            "the note block is still powered"
        );
    });

    // The circuit still works after the rejoin, for both players.
    click(&alice, circuit.lever, 1).await;
    for bot in [&alice, &bob] {
        wait_for_state(bot, &circuit, false, &[0, 0, 0]).await;
    }
    click(&alice, circuit.lever, 2).await;
    for bot in [&alice, &bob] {
        wait_for_state(bot, &circuit, true, &[15, 14, 13]).await;
        expect_note(bot, circuit.note, 1.0).await;
    }
    assert_silent(
        &[&alice, &bob],
        circuit.note,
        "exactly one note per rising edge",
    )
    .await;

    let carol = server.join("Carol").await;
    wait_for_circuit(&carol, &circuit).await;
    carol.sync().await;
    carol.view(|view| {
        assert!(powered(view, circuit.lever) && powered(view, circuit.note));
        assert_eq!(powers(view, &circuit.line), expected(&[15, 14, 13]));
    });

    for bot in [&alice, &bob, &carol] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn the_signal_reaches_fifteen_dust_and_dies_on_the_sixteenth() {
    let server = start().await;
    let (fifteen, mut blocks) = circuit(3, 15);
    let (sixteen, more) = circuit(6, 16);
    blocks.extend(more);
    build(&server, blocks).await;
    let alice = server.join("Alice").await;
    wait_for_circuit(&alice, &fifteen).await;
    wait_for_circuit(&alice, &sixteen).await;

    click(&alice, fifteen.lever, 1).await;
    let reached: Vec<u8> = (1..=15).rev().collect();
    wait_for_state(&alice, &fifteen, true, &reached).await;
    expect_note(&alice, fifteen.note, 1.0).await;

    click(&alice, sixteen.lever, 2).await;
    let mut dead = reached.clone();
    dead.push(0);
    let (lever, line, note) = (sixteen.lever, sixteen.line.clone(), sixteen.note);
    let dead_powers = expected(&dead);
    alice
        .wait_until("the sixteenth dust unpowered", |view| {
            powered(view, lever) && powers(view, &line) == dead_powers
        })
        .await;
    alice.sync().await;
    assert!(!alice.view(|view| powered(view, note)));
    assert_silent(&[&alice], note, "the signal died before the note block").await;

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn a_covered_note_block_is_powered_but_silent() {
    let server = start().await;
    let (circuit, blocks) = circuit(3, 1);
    build(&server, blocks).await;
    set(&server, vec![(circuit.note.up(1), stone())]).await;
    let alice = server.join("Alice").await;
    wait_for_circuit(&alice, &circuit).await;
    let cover = circuit.note.up(1);
    alice
        .wait_until("the stone on the note block", |view| {
            view.block_at(cover) == Some(stone())
        })
        .await;

    click(&alice, circuit.lever, 1).await;
    wait_for_state(&alice, &circuit, true, &[15]).await;
    assert_silent(&[&alice], circuit.note, "a block above silences it").await;

    alice.assert_healthy();
    server.stop().await;
}

#[tokio::test]
async fn breaking_the_line_or_the_lever_depowers_what_is_downstream() {
    let server = start().await;
    let (circuit, blocks) = circuit(3, 5);
    build(&server, blocks).await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    wait_for_circuit(&alice, &circuit).await;
    wait_for_circuit(&bob, &circuit).await;

    click(&alice, circuit.lever, 1).await;
    for bot in [&alice, &bob] {
        wait_for_state(bot, &circuit, true, &[15, 14, 13, 12, 11]).await;
        expect_note(bot, circuit.note, 1.0).await;
    }

    // Alice (creative) breaks the middle dust.
    alice
        .send(ServerboundPlayerAction {
            action: Action::StartDestroyBlock,
            pos: circuit.line[2],
            direction: Direction::Up,
            seq: 2,
        })
        .await;
    expect_ack(&alice, 2).await;
    let (line, note) = (circuit.line.clone(), circuit.note);
    for bot in [&alice, &bob] {
        bot.wait_until("the line cut in the middle", |view| {
            view.block_at(line[2]) == Some(BlockState::AIR)
                && property(view, line[0], "power") == Some("15")
                && property(view, line[1], "power") == Some("14")
                && property(view, line[3], "power") == Some("0")
                && property(view, line[4], "power") == Some("0")
                && !powered(view, note)
        })
        .await;
    }

    // Breaking the lever depowers the rest.
    alice
        .send(ServerboundPlayerAction {
            action: Action::StartDestroyBlock,
            pos: circuit.lever,
            direction: Direction::Up,
            seq: 3,
        })
        .await;
    expect_ack(&alice, 3).await;
    for bot in [&alice, &bob] {
        bot.wait_until("the line unpowered", |view| {
            property(view, line[0], "power") == Some("0")
                && property(view, line[1], "power") == Some("0")
        })
        .await;
    }
    assert_silent(&[&alice, &bob], circuit.note, "no note while depowering").await;

    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

/// Server ticks, counted from `First`, and every block change with its tick.
#[derive(Resource, Default)]
struct ChangeLog {
    tick: u64,
    changes: Vec<(u64, BlockPosition, i32)>,
}

#[tokio::test]
async fn a_stone_button_releases_itself_twenty_ticks_after_the_press() {
    let server = TestServer::builder()
        .plugin(|app| {
            app.add_plugins(RedstonePlugin)
                .init_resource::<ChangeLog>()
                .add_systems(First, |mut log: ResMut<ChangeLog>| log.tick += 1)
                .add_observer(|event: On<BlockChangeEvent>, mut log: ResMut<ChangeLog>| {
                    let tick = log.tick;
                    log.changes.push((tick, event.position, event.new_state));
                });
        })
        .start()
        .await;
    let button = at(2, 9);
    let note = at(3, 9);
    build(
        &server,
        vec![(button, stone_button()), (note, note_block(Note::_6))],
    )
    .await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;
    for bot in [&alice, &bob] {
        bot.wait_until("the button and note block", |view| {
            view.block_at(button) == Some(stone_button())
                && view.block_at(note).map(BlockKind::from) == Some(BlockKind::NoteBlock)
        })
        .await;
    }

    click(&alice, button, 1).await;
    for bot in [&alice, &bob] {
        bot.wait_until("the button pressed and the note block powered", |view| {
            powered(view, button) && powered(view, note)
        })
        .await;
        expect_note(bot, note, 2f32.powf(-0.5)).await;
    }
    bob.expect("the button click", |packet| match packet {
        ClientboundGamePacket::Sound(sound)
            if sound.sound == Holder::Reference(SoundEvent::BlockStoneButtonClickOn) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;

    for bot in [&alice, &bob] {
        bot.wait_until("the button released and the note block unpowered", |view| {
            !powered(view, button) && !powered(view, note) && view.block_at(button).is_some()
        })
        .await;
        bot.expect("the release click", |packet| match packet {
            ClientboundGamePacket::Sound(sound)
                if sound.sound == Holder::Reference(SoundEvent::BlockStoneButtonClickOff) =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
    }
    assert_silent(&[&alice, &bob], note, "the release plays nothing").await;

    let pressed = i32::from(
        blocks::StoneButton {
            face: Face::Floor,
            facing: FacingCardinal::East,
            powered: true,
        }
        .as_block_state()
        .id(),
    );
    let released = i32::from(stone_button().id());
    let position = BlockPosition {
        x: button.x,
        y: button.y as i16,
        z: button.z,
    };
    let changes = server
        .with_world(move |world| {
            world
                .resource::<ChangeLog>()
                .changes
                .iter()
                .filter(|(_, at, _)| *at == position)
                .map(|&(tick, _, state)| (tick, state))
                .collect::<Vec<_>>()
        })
        .await;
    let press = changes.iter().rev().find(|(_, s)| *s == pressed).unwrap().0;
    let release = changes
        .iter()
        .rev()
        .find(|(_, s)| *s == released)
        .unwrap()
        .0;
    assert_eq!(release - press, 20, "stone buttons stay pressed 20 ticks");

    for bot in [&alice, &bob] {
        bot.assert_healthy();
    }
    server.stop().await;
}

#[tokio::test]
async fn one_click_toggles_once_and_sneaking_with_a_block_places_it() {
    let server = start().await;
    let lever_at = at(2, 12);
    let note = at(3, 12);
    build(
        &server,
        vec![(lever_at, lever(false)), (note, note_block(Note::_12))],
    )
    .await;
    let alice = server.join("Alice").await;
    alice
        .wait_until("the lever", |view| {
            view.block_at(lever_at) == Some(lever(false))
        })
        .await;

    // A click the client did not consume sends the main hand, then the off
    // hand: vanilla only lets the main hand use the lever.
    use_on(&alice, lever_at, InteractionHand::MainHand, 1).await;
    use_on(&alice, lever_at, InteractionHand::OffHand, 2).await;
    expect_ack(&alice, 1).await;
    expect_ack(&alice, 2).await;
    alice
        .wait_until("the lever on", |view| {
            powered(view, lever_at) && powered(view, note)
        })
        .await;
    expect_note(&alice, note, 1.0).await;
    alice.sync().await;
    assert_eq!(alice.block_at(lever_at), Some(lever(true)), "toggled once");
    assert_silent(&[&alice], note, "one click plays one note").await;

    // Holding stone, a click toggles the lever and places nothing.
    alice
        .send(ServerboundSetCreativeModeSlot {
            slot_num: 36,
            item_stack: ItemStack::Present(ItemStackData::new(ItemKind::Stone, 1)),
        })
        .await;
    click(&alice, lever_at, 3).await;
    alice
        .wait_until("the lever off", |view| !powered(view, lever_at))
        .await;
    alice.sync().await;
    assert_eq!(alice.block_at(lever_at.up(1)), Some(BlockState::AIR));

    // Sneaking with stone in hand places it on the lever instead.
    alice
        .send(ServerboundPlayerInput {
            shift: true,
            ..Default::default()
        })
        .await;
    click(&alice, lever_at, 4).await;
    alice
        .wait_until("the stone placed against the lever", |view| {
            view.block_at(lever_at.up(1)) == Some(stone())
        })
        .await;
    alice.sync().await;
    assert_eq!(alice.block_at(lever_at), Some(lever(false)), "not toggled");

    alice.assert_healthy();
    server.stop().await;
}
