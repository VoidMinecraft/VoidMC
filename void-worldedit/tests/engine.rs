#![cfg(feature = "engine")]

use std::collections::HashSet;

use bevy_app::App;
use bevy_ecs::prelude::*;
use voidmc::commands::dispatch_command;
use voidmc::components::{
    ClientId, LoadedChunks, Operator, PlayerDimension, PlayerReady, Position, Rotation,
};
use voidmc::network::PacketEvent;
use voidmc::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
use voidmc::plugins::interaction::InteractionPlugin;
use voidmc::plugins::item_use::ItemUsePlugin;
use voidmc::{
    ChunkData, ChunkIndex, ChunkPos, DimensionId, Inventory, ItemStack, ServerConfig,
    ServerConfigResource,
};
use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
use voidmc_protocol::clientbound::{ClientboundPacket, ManualPlayPacket, PlayPacket};
use voidmc_protocol::serverbound::{PlayerAction, UseItem, UseItemOn};
use voidmc_protocol::types::{BlockFace, BlockPosition, Hand, PlayerActionStatus};
use voidmc_worldedit::engine::{EditQueue, EditSession, Permission};
use voidmc_worldedit::{BlockPos, BlockState, WorldEditPlugin};

struct Server {
    app: App,
    outgoing: flume::Receiver<OutgoingPacket>,
    player: Entity,
}

impl Server {
    fn new(plugin: WorldEditPlugin) -> Self {
        let mut app = App::new();
        let (_incoming_tx, incoming) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect) = flume::unbounded::<u32>();
        let (kick, _kick_rx) = flume::unbounded::<u32>();
        app.insert_resource(NetworkChannels {
            incoming,
            outgoing: outgoing_tx,
            disconnect,
            kick,
        })
        .init_resource::<ChunkIndex>()
        .insert_resource(ServerConfigResource::from(&ServerConfig::default()))
        .add_plugins(plugin)
        .add_plugins((InteractionPlugin, ItemUsePlugin));

        let mut loaded = HashSet::new();
        for x in -2..=2 {
            for z in -2..=2 {
                let pos = ChunkPos::new(x, z);
                let chunk = ChunkData::new(
                    (0..24).map(|_| ChunkSection::empty()).collect(),
                    ChunkHeightmaps::empty(),
                    LightData::empty(),
                );
                let entity = app.world_mut().spawn(chunk).id();
                app.world_mut()
                    .resource_mut::<ChunkIndex>()
                    .0
                    .insert((DimensionId::Overworld, pos), entity);
                loaded.insert(pos);
            }
        }
        let player = app
            .world_mut()
            .spawn((
                ClientId(1),
                PlayerReady,
                Operator,
                LoadedChunks(loaded),
                PlayerDimension(DimensionId::Overworld),
                Position {
                    x: 0.5,
                    y: 64.0,
                    z: 0.5,
                },
                Rotation {
                    yaw: -90.0,
                    pitch: 0.0,
                },
                Inventory::new(),
            ))
            .id();
        Self {
            app,
            outgoing,
            player,
        }
    }

    fn command(&mut self, line: &str) {
        let mut tokens = line.split_whitespace().map(str::to_string);
        let name = tokens.next().unwrap();
        let name = name.strip_prefix('/').unwrap();
        dispatch_command(self.app.world_mut(), 1, self.player, name, tokens.collect());
    }

    fn tick(&mut self) {
        self.app.update();
    }

    fn settle(&mut self) {
        for _ in 0..2000 {
            self.tick();
            if self.app.world().resource::<EditQueue>().is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("edit queue never drained");
    }

    fn block(&self, x: i32, y: i32, z: i32) -> BlockState {
        let world = self.app.world();
        let chunk = ChunkPos::new(x.div_euclid(16), z.div_euclid(16));
        let entity = world.resource::<ChunkIndex>().0[&(DimensionId::Overworld, chunk)];
        let state = world
            .get::<ChunkData>(entity)
            .unwrap()
            .get_block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8)
            .unwrap();
        BlockState(state as u32)
    }

    fn hold(&mut self, item: &str) -> ItemStack {
        let stack = ItemStack::of(item, 1).unwrap();
        self.app
            .world_mut()
            .get_mut::<Inventory>(self.player)
            .unwrap()
            .set(Inventory::HOTBAR_START, stack.clone());
        stack
    }

    fn packet<T: Send + Sync + 'static>(&mut self, packet: T) {
        let player = self.player;
        self.app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity: player,
            packet,
        });
        self.tick();
    }

    fn packets(&self) -> Vec<ClientboundPacket> {
        self.outgoing
            .try_iter()
            .map(|outgoing| outgoing.packet)
            .collect()
    }

    fn chat(packets: &[ClientboundPacket]) -> Vec<String> {
        packets
            .iter()
            .filter_map(|packet| match packet {
                ClientboundPacket::Play(PlayPacket::SystemChat(chat)) => {
                    Some(format!("{:?}", chat.content))
                }
                _ => None,
            })
            .collect()
    }
}

fn stone() -> BlockState {
    BlockState::parse("stone").unwrap()
}

#[test]
fn set_writes_chunks_sends_section_updates_and_undo_restores() {
    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//pos1 0 64 0");
    server.command("//pos2 3 65 2");
    server.packets();
    server.command("//set stone");
    server.settle();

    for (x, y, z) in [(0, 64, 0), (3, 65, 2), (1, 64, 1)] {
        assert_eq!(server.block(x, y, z), stone());
    }
    assert_eq!(server.block(4, 64, 0), BlockState::AIR);

    let packets = server.packets();
    let updates: Vec<_> = packets
        .iter()
        .filter_map(|packet| match packet {
            ClientboundPacket::Play(PlayPacket::SectionBlocksUpdate(update)) => Some(update),
            _ => None,
        })
        .collect();
    assert_eq!(updates.len(), 1);
    assert_eq!(
        (
            updates[0].section_x,
            updates[0].section_y,
            updates[0].section_z
        ),
        (0, 4, 0)
    );
    assert_eq!(updates[0].blocks.len(), 4 * 2 * 3);
    assert!(
        Server::chat(&packets)
            .iter()
            .any(|line| line.contains("24 blocks changed"))
    );

    server.command("//undo");
    server.settle();
    assert_eq!(server.block(1, 64, 1), BlockState::AIR);
    server.command("//redo");
    server.settle();
    assert_eq!(server.block(1, 64, 1), stone());
}

#[test]
fn large_edits_spread_over_ticks_and_resend_whole_chunks() {
    let mut server = Server::new(WorldEditPlugin::default().blocks_per_tick(4096));
    server.command("//pos1 0 -64 0");
    server.command("//pos2 15 63 15");
    server.command("//set stone");
    server.packets();

    server.tick();
    assert!(!server.app.world().resource::<EditQueue>().is_empty());
    let mut ticks = 1;
    while !server.app.world().resource::<EditQueue>().is_empty() {
        server.tick();
        ticks += 1;
    }
    assert!(ticks >= 8, "8 sections at one per tick, took {ticks}");
    assert_eq!(server.block(7, -64, 7), stone());
    assert_eq!(server.block(15, 63, 15), stone());

    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//pos1 0 -64 0");
    server.command("//pos2 15 63 15");
    server.command("//set stone");
    server.packets();
    server.settle();
    let packets = server.packets();
    assert!(packets.iter().any(|packet| matches!(
        packet,
        ClientboundPacket::ManualPlay(ManualPlayPacket::ChunkDataAndLight(chunk))
            if chunk.chunk_x == 0 && chunk.chunk_z == 0
    )));
    assert!(!packets.iter().any(|packet| matches!(
        packet,
        ClientboundPacket::Play(PlayPacket::SectionBlocksUpdate(_))
    )));
}

#[test]
fn copy_rotate_paste_and_move() {
    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//pos1 0 64 0");
    server.command("//pos2 2 64 0");
    server.command("//set oak_stairs[facing=east]");
    server.settle();
    server.command("//copy");
    server.settle();
    server.packets();
    server.command("//rotate 90");
    assert!(
        server
            .app
            .world()
            .resource::<EditQueue>()
            .is_busy(server.player)
    );
    server.command("//paste");
    assert!(
        Server::chat(&server.packets())
            .iter()
            .any(|line| line.contains("still running"))
    );
    server.settle();

    server
        .app
        .world_mut()
        .get_mut::<Position>(server.player)
        .unwrap()
        .z = 10.5;
    server.command("//paste");
    server.settle();
    for z in 10..=12 {
        assert_eq!(
            server.block(0, 64, z).property("facing"),
            Some("south"),
            "z={z}"
        );
    }

    server.command("//move 5 up");
    server.settle();
    assert_eq!(server.block(1, 64, 0), BlockState::AIR);
    assert_eq!(server.block(1, 69, 0).property("facing"), Some("east"));
}

#[test]
fn stack_repeats_along_the_facing_direction() {
    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//pos1 0 64 0");
    server.command("//pos2 1 64 0");
    server.command("//set glass");
    server.settle();
    server.command("//stack 3");
    server.settle();
    for x in 0..8 {
        assert_eq!(
            server.block(x, 64, 0),
            BlockState::parse("glass").unwrap(),
            "x={x}"
        );
    }
    assert_eq!(server.block(8, 64, 0), BlockState::AIR);
}

#[test]
fn selection_outline_is_sent_to_its_owner_only() {
    let mut server = Server::new(WorldEditPlugin::default());
    let other = server
        .app
        .world_mut()
        .spawn((
            ClientId(2),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
        ))
        .id();
    server.command("//pos1 0 64 0");
    server.tick();
    server.tick();
    server.command("//pos2 4 66 4");
    server.tick();
    let outgoing: Vec<_> = server.outgoing.try_iter().collect();
    let spawns = outgoing
        .iter()
        .filter(|o| {
            matches!(
                o.packet,
                ClientboundPacket::Play(PlayPacket::SpawnEntity(_))
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(spawns.len(), 14);
    assert!(spawns.iter().all(|o| o.client_id == 1));
    let _ = other;

    server.command("//expand 2 up");
    server.tick();
    let packets = server.packets();
    assert!(
        packets
            .iter()
            .all(|p| !matches!(p, ClientboundPacket::Play(PlayPacket::SpawnEntity(_))))
    );
    assert!(
        packets
            .iter()
            .any(|p| matches!(p, ClientboundPacket::Play(PlayPacket::SetEntityData(_))))
    );

    server.command("//desel");
    server.tick();
    let packets = server.packets();
    assert!(packets.iter().any(|p| matches!(
        p,
        ClientboundPacket::ManualPlay(ManualPlayPacket::RemoveEntities(remove)) if remove.entity_ids.len() == 14
    )));
}

#[test]
fn non_operators_are_refused_unless_everyone_may_edit() {
    let mut server = Server::new(WorldEditPlugin::default());
    server
        .app
        .world_mut()
        .entity_mut(server.player)
        .remove::<Operator>();
    server.command("//pos1 0 64 0");
    assert!(
        server
            .app
            .world()
            .get::<EditSession>(server.player)
            .is_none()
    );

    let mut server = Server::new(WorldEditPlugin::default().permission(Permission::Everyone));
    server
        .app
        .world_mut()
        .entity_mut(server.player)
        .remove::<Operator>();
    server.command("//pos1 0 64 0");
    let session = server
        .app
        .world()
        .get::<EditSession>(server.player)
        .unwrap();
    assert_eq!(session.selection.pos1(), Some(BlockPos::new(0, 64, 0)));
}

#[test]
fn schematics_save_and_load_through_the_clipboard() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = Server::new(WorldEditPlugin::default().schematic_dir(dir.path()));
    server.command("//pos1 0 64 0");
    server.command("//pos2 1 65 1");
    server.command("//set dirt");
    server.settle();
    server.command("//copy");
    server.settle();
    server.command("/schem save hut");
    server.settle();
    assert!(dir.path().join("hut.schem").is_file());
    server.command("/schem save ../escape");
    server.settle();
    assert!(!dir.path().join("../escape.schem").exists());

    server.packets();
    server.command("/schem load missing");
    server.settle();
    assert!(
        Server::chat(&server.packets())
            .iter()
            .any(|line| line.contains("No schematic named 'missing'"))
    );
    server.command("/schem load hut");
    server.settle();
    server
        .app
        .world_mut()
        .get_mut::<Position>(server.player)
        .unwrap()
        .x = 20.5;
    server.command("//paste");
    server.settle();
    assert_eq!(server.block(21, 65, 1), BlockState::parse("dirt").unwrap());
}

#[test]
fn brushes_bind_to_the_held_item_and_edit_where_the_player_looks() {
    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//pos1 -20 60 -20");
    server.command("//pos2 20 63 20");
    server.command("//set stone");
    server.settle();

    let stick = voidmc::ItemStack::of("minecraft:stick", 1).unwrap();
    server
        .app
        .world_mut()
        .get_mut::<Inventory>(server.player)
        .unwrap()
        .set(Inventory::HOTBAR_START, stick.clone());
    server.command("/brush sphere glass 2");
    let world = server.app.world();
    let session = world.get::<EditSession>(server.player).unwrap();
    assert!(session.brush(stick.item).is_some());
    assert!(
        world
            .resource::<voidmc::ItemBehaviorRegistry>()
            .contains(stick.item)
    );

    server.command("/brush sphere nope 2");
    server.command("/brush none");
    assert!(
        server
            .app
            .world()
            .get::<EditSession>(server.player)
            .unwrap()
            .brush(stick.item)
            .is_none()
    );
}

#[test]
fn wand_left_click_sets_the_first_corner_and_keeps_the_block() {
    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//pos1 2 64 2");
    server.command("//set stone");
    server.command("//desel");
    server.hold("minecraft:wooden_axe");
    server.packet(PlayerAction {
        status: PlayerActionStatus::StartedDigging,
        position: BlockPosition { x: 2, y: 64, z: 2 },
        face: BlockFace::Top,
        sequence: 3,
    });
    assert_eq!(server.block(2, 64, 2), BlockState::AIR);

    server.command("//pos1 2 64 2");
    server.command("//pos2 2 64 2");
    server.command("//set stone");
    server.settle();
    server.command("//desel");
    server.packet(PlayerAction {
        status: PlayerActionStatus::StartedDigging,
        position: BlockPosition { x: 2, y: 64, z: 2 },
        face: BlockFace::Top,
        sequence: 4,
    });
    assert_eq!(server.block(2, 64, 2), stone());
    let session = server
        .app
        .world()
        .get::<EditSession>(server.player)
        .unwrap();
    assert_eq!(session.selection.pos1(), Some(BlockPos::new(2, 64, 2)));
}

#[test]
fn edits_larger_than_the_history_budget_are_not_recorded() {
    let mut server = Server::new(WorldEditPlugin::default().history(25, 1024));
    server.command("//pos1 0 64 0");
    server.command("//pos2 15 79 15");
    server.packets();
    server.command("//set stone,dirt,gravel,sand");
    server.settle();
    assert!(
        Server::chat(&server.packets())
            .iter()
            .any(|line| line.contains("cannot be undone"))
    );
    let session = server
        .app
        .world()
        .get::<EditSession>(server.player)
        .unwrap();
    assert_eq!(session.history.undo_len(), 0);
    assert!(session.history.memory_bytes() <= 1024);
}

#[test]
fn the_wand_checks_permission_before_claiming_the_off_hand() {
    let mut server = Server::new(WorldEditPlugin::default().wand("minecraft:stone"));
    server
        .app
        .world_mut()
        .entity_mut(server.player)
        .remove::<Operator>();
    server.hold("minecraft:stone");
    server.packet(UseItemOn {
        hand: Hand::OffHand,
        location: BlockPosition { x: 3, y: 63, z: 3 },
        face: BlockFace::Top,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 1,
    });
    server.tick();
    assert_eq!(server.block(3, 64, 3), stone());
}

#[test]
fn brush_fires_at_the_block_in_sight() {
    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//pos1 -10 60 -10");
    server.command("//pos2 10 63 10");
    server.command("//set stone");
    server.settle();
    server.hold("minecraft:stick");
    server.command("/brush sphere glass 2");
    {
        let world = server.app.world_mut();
        let mut position = world.get_mut::<Position>(server.player).unwrap();
        position.y = 70.0;
        let mut rotation = world.get_mut::<Rotation>(server.player).unwrap();
        rotation.pitch = 90.0;
    }
    server.packet(UseItem {
        hand: Hand::MainHand,
        sequence: 1,
        yaw: 0.0,
        pitch: 90.0,
    });
    server.settle();
    let glass = BlockState::parse("glass").unwrap();
    assert_eq!(server.block(0, 63, 0), glass);
    assert_eq!(server.block(0, 61, 0), glass);
    assert_eq!(server.block(0, 65, 0), glass);
    assert_eq!(server.block(0, 60, 0), stone());
    assert_eq!(server.block(0, 66, 0), BlockState::AIR);
}

#[test]
fn clipboard_and_edit_commands_wait_for_the_running_edit() {
    let mut server = Server::new(WorldEditPlugin::default().blocks_per_tick(4096));
    server.command("//pos1 0 0 0");
    server.command("//pos2 40 40 40");
    server.command("//copy");
    server.tick();
    server.packets();
    server.command("//paste");
    server.command("//set stone");
    let refusals = Server::chat(&server.packets())
        .iter()
        .filter(|line| line.contains("still running"))
        .count();
    assert_eq!(refusals, 2);
    server.settle();
    assert!(
        server
            .app
            .world()
            .get::<EditSession>(server.player)
            .unwrap()
            .clipboard()
            .is_some()
    );
}

#[test]
fn tall_sparse_edits_resend_the_chunk_once() {
    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//pos1 3 -64 3");
    server.command("//pos2 3 319 3");
    server.command("//set stone");
    server.packets();
    server.settle();
    let packets = server.packets();
    let chunks = packets
        .iter()
        .filter(|p| {
            matches!(
                p,
                ClientboundPacket::ManualPlay(ManualPlayPacket::ChunkDataAndLight(_))
            )
        })
        .count();
    let sections = packets
        .iter()
        .filter(|p| {
            matches!(
                p,
                ClientboundPacket::Play(PlayPacket::SectionBlocksUpdate(_))
            )
        })
        .count();
    assert_eq!((chunks, sections), (1, 0));
}

#[test]
fn absurd_coordinates_neither_panic_nor_pass_the_volume_limit() {
    let mut server = Server::new(WorldEditPlugin::default());
    server.command("//sel sphere");
    server.command("//pos1 1000000000 64 0");
    server.command("//pos2 -1000000000 64 0");
    server.tick();
    server.command("//size");
    server.packets();
    server.command("//set stone");
    assert!(
        Server::chat(&server.packets())
            .iter()
            .any(|line| line.contains("the limit is"))
    );
    assert!(server.app.world().resource::<EditQueue>().is_empty());

    server.command("//sel cuboid");
    server.command("//pos1 0 0 0");
    server.command("//pos2 1073741823 1073741823 15");
    server.command("//expand 2147483647 east");
    server.command("//stack 10000 east");
    server.tick();
    assert!(server.app.world().resource::<EditQueue>().is_empty());
}
