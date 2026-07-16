//! Item-use plugin: turns player right-clicks and block breaks into queued actions
//! processed by the exclusive item-behaviour drain system, tracks the selected
//! hotbar slot, and applies creative-mode slot changes.
//!
//! This replaces the old hardcoded `HotbarBlocks` placement: what a right-click
//! does is now driven by the [`ItemStack`] the player is actually holding and the
//! [`ItemBehaviorRegistry`](crate::item_behavior::ItemBehaviorRegistry).

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use voidmc_protocol::serverbound::SetCreativeModeSlot;

use crate::components::{ClientId, HotbarSlot};
use crate::config::ServerConfigResource;
use crate::events::{
    PlayerChangeSlotEvent, PlayerFinishDiggingEvent, PlayerStartDiggingEvent, PlayerUseItemEvent,
    PlayerUseItemOnBlockEvent,
};
use crate::inventory::Inventory;
use crate::item::ItemStack;
use crate::item_behavior::{
    BlockUseTarget, ItemBehaviorRegistry, ItemUseQueue, drain_item_use_queue, enqueue_break,
    enqueue_use_in_air, enqueue_use_on_block,
};
use crate::network::PacketEvent;

pub struct ItemUsePlugin;

impl Plugin for ItemUsePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemBehaviorRegistry>()
            .init_resource::<ItemUseQueue>()
            .add_observer(queue_use_on_block)
            .add_observer(queue_use_in_air)
            .add_observer(queue_creative_break)
            .add_observer(queue_break)
            .add_observer(handle_creative_slot)
            .add_observer(track_selected_slot)
            .add_systems(Update, drain_item_use_queue);
    }
}

fn queue_use_on_block(
    event: On<PlayerUseItemOnBlockEvent>,
    mut queue: ResMut<ItemUseQueue>,
    clients: Query<&ClientId>,
) {
    let Ok(client_id) = clients.get(event.entity) else {
        return;
    };
    enqueue_use_on_block(
        &mut queue,
        event.entity,
        client_id.0,
        event.hand,
        event.sequence,
        BlockUseTarget {
            position: event.position,
            face: event.face,
            cursor: (event.cursor_x, event.cursor_y, event.cursor_z),
            inside_block: event.inside_block,
        },
    );
}

fn queue_use_in_air(
    event: On<PlayerUseItemEvent>,
    mut queue: ResMut<ItemUseQueue>,
    clients: Query<&ClientId>,
) {
    let Ok(client_id) = clients.get(event.entity) else {
        return;
    };
    enqueue_use_in_air(
        &mut queue,
        event.entity,
        client_id.0,
        event.hand,
        event.sequence,
    );
}

fn queue_creative_break(
    event: On<PlayerStartDiggingEvent>,
    config: Res<ServerConfigResource>,
    mut queue: ResMut<ItemUseQueue>,
    clients: Query<&ClientId>,
) {
    // Creative clients remove the block on start and do not send FinishedDigging.
    if config.game_mode != 1 {
        return;
    }
    let Ok(client_id) = clients.get(event.entity) else {
        return;
    };
    enqueue_break(
        &mut queue,
        event.entity,
        client_id.0,
        event.sequence,
        event.position,
        event.face,
    );
}

fn queue_break(
    event: On<PlayerFinishDiggingEvent>,
    mut queue: ResMut<ItemUseQueue>,
    clients: Query<&ClientId>,
) {
    let Ok(client_id) = clients.get(event.entity) else {
        return;
    };
    enqueue_break(
        &mut queue,
        event.entity,
        client_id.0,
        event.sequence,
        event.position,
        event.face,
    );
}

/// Applies a creative-mode slot change to the server-side inventory. The client
/// already shows the change, so no resync is sent.
fn handle_creative_slot(
    event: On<PacketEvent<SetCreativeModeSlot>>,
    mut inventories: Query<&mut Inventory>,
) {
    let slot = event.packet.slot;
    if slot < 0 {
        // Negative slot = dropping into the world; handled with item drops later.
        return;
    }
    if let Ok(mut inv) = inventories.get_mut(event.entity) {
        inv.set(slot as usize, ItemStack::from_slot(&event.packet.item));
    }
}

/// Keeps the selected hotbar slot in sync on both `HotbarSlot` and `Inventory`.
fn track_selected_slot(
    event: On<PlayerChangeSlotEvent>,
    mut players: Query<(&mut HotbarSlot, &mut Inventory)>,
) {
    if let Ok((mut hotbar, mut inv)) = players.get_mut(event.entity) {
        hotbar.0 = event.slot;
        inv.set_selected_hotbar(event.slot.clamp(0, 8) as u8);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::ServerConfig;
    use crate::components::{LoadedChunks, PlayerDimension, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::plugins::interaction::InteractionPlugin;
    use crate::world::{ChunkData, ChunkIndex, ChunkPos, DimensionId};
    use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
    use voidmc_protocol::serverbound::{PlayerAction, UseItemOn};
    use voidmc_protocol::types::{BlockFace, BlockPosition, Hand, PlayerActionStatus};

    fn app_with_game_mode(game_mode: u8) -> App {
        let config = ServerConfig {
            game_mode,
            ..Default::default()
        };

        let mut app = App::new();
        app.insert_resource(ItemUseQueue::default())
            .insert_resource(ServerConfigResource::from(&config))
            .add_observer(queue_creative_break);
        app
    }

    #[test]
    fn creative_start_digging_queues_block_break() {
        let mut app = app_with_game_mode(1);
        let player = app.world_mut().spawn(ClientId(7)).id();

        app.world_mut().trigger(PlayerStartDiggingEvent {
            entity: player,
            position: BlockPosition { x: 1, y: 64, z: 2 },
            face: BlockFace::Top,
            sequence: 12,
        });

        assert_eq!(app.world().resource::<ItemUseQueue>().pending_len(), 1);
    }

    #[test]
    fn survival_start_digging_waits_for_finish() {
        let mut app = app_with_game_mode(0);
        let player = app.world_mut().spawn(ClientId(7)).id();

        app.world_mut().trigger(PlayerStartDiggingEvent {
            entity: player,
            position: BlockPosition { x: 1, y: 64, z: 2 },
            face: BlockFace::Top,
            sequence: 12,
        });

        assert_eq!(app.world().resource::<ItemUseQueue>().pending_len(), 0);
    }

    #[test]
    fn first_prediction_sequences_are_acknowledged_across_players() {
        let mut server = MultiplayerHarness::new();
        let position = BlockPosition { x: 1, y: 64, z: 2 };
        server.place(0, position);
        server.break_block(1, position);
    }

    #[test]
    fn players_can_alternate_placing_and_breaking_the_same_block() {
        let mut server = MultiplayerHarness::new();
        let position = BlockPosition { x: 3, y: 64, z: 5 };

        server.place(0, position);
        server.break_block(1, position);
        server.place(1, position);
        server.break_block(0, position);
        server.place(0, position);
        server.break_block(1, position);
    }

    #[test]
    fn one_player_can_break_a_two_by_two_placed_by_another() {
        let mut server = MultiplayerHarness::new();
        let positions = [
            BlockPosition { x: 4, y: 64, z: 4 },
            BlockPosition { x: 5, y: 64, z: 4 },
            BlockPosition { x: 4, y: 64, z: 5 },
            BlockPosition { x: 5, y: 64, z: 5 },
        ];

        for position in positions {
            server.place(0, position);
        }
        for position in positions.into_iter().rev() {
            server.break_block(1, position);
        }
        for position in positions {
            server.assert_block(position, 0);
        }
    }

    struct MultiplayerHarness {
        app: App,
        outgoing: flume::Receiver<OutgoingPacket>,
        chunk_entity: Entity,
        players: [Entity; 2],
        next_sequence: [i32; 2],
    }

    impl MultiplayerHarness {
        fn new() -> Self {
            let mut app = App::new();
            let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
            let (outgoing_tx, outgoing) = flume::unbounded::<OutgoingPacket>();
            let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
            let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
            app.insert_resource(NetworkChannels {
                incoming: incoming_rx,
                outgoing: outgoing_tx,
                disconnect: disconnect_rx,
                kick: kick_tx,
            })
            .insert_resource(ServerConfigResource::from(&ServerConfig::default()))
            .insert_resource(ChunkIndex::default())
            .add_plugins((InteractionPlugin, ItemUsePlugin));

            let chunk_pos = ChunkPos::new(0, 0);
            let chunk = ChunkData::new(
                (0..24).map(|_| ChunkSection::empty()).collect(),
                ChunkHeightmaps::empty(),
                LightData::empty(),
            );
            let chunk_entity = app.world_mut().spawn(chunk).id();
            app.world_mut()
                .resource_mut::<ChunkIndex>()
                .0
                .insert((DimensionId::Overworld, chunk_pos), chunk_entity);

            let loaded = LoadedChunks(HashSet::from([chunk_pos]));
            let mut inventory_one = Inventory::new();
            inventory_one.set(
                Inventory::HOTBAR_START,
                ItemStack::of("minecraft:stone", 64).unwrap(),
            );
            let player_one = app
                .world_mut()
                .spawn((
                    ClientId(1),
                    PlayerDimension(DimensionId::Overworld),
                    LoadedChunks(loaded.0.clone()),
                    PlayerReady,
                    inventory_one,
                ))
                .id();
            let mut inventory_two = Inventory::new();
            inventory_two.set(
                Inventory::HOTBAR_START,
                ItemStack::of("minecraft:stone", 64).unwrap(),
            );
            let player_two = app
                .world_mut()
                .spawn((
                    ClientId(2),
                    PlayerDimension(DimensionId::Overworld),
                    loaded,
                    PlayerReady,
                    inventory_two,
                ))
                .id();

            Self {
                app,
                outgoing,
                chunk_entity,
                players: [player_one, player_two],
                next_sequence: [1; 2],
            }
        }

        fn place(&mut self, player: usize, position: BlockPosition) {
            let sequence = self.next_sequence[player];
            let client_id = player as u32 + 1;
            self.next_sequence[player] += 1;
            self.app.world_mut().trigger(PacketEvent {
                client_id,
                entity: self.players[player],
                packet: UseItemOn {
                    hand: Hand::MainHand,
                    location: BlockPosition {
                        y: position.y - 1,
                        ..position
                    },
                    face: BlockFace::Top,
                    cursor_x: 0.5,
                    cursor_y: 1.0,
                    cursor_z: 0.5,
                    inside_block: false,
                    world_border_hit: false,
                    sequence,
                },
            });
            self.app.update();

            self.assert_block(position, 1);
            self.assert_updates_and_ack(position, 1, client_id, sequence);
        }

        fn break_block(&mut self, player: usize, position: BlockPosition) {
            let client_id = player as u32 + 1;
            let sequence = self.next_sequence[player];
            self.next_sequence[player] += 1;
            self.app.world_mut().trigger(PacketEvent {
                client_id,
                entity: self.players[player],
                packet: PlayerAction {
                    status: PlayerActionStatus::StartedDigging,
                    position,
                    face: BlockFace::Top,
                    sequence,
                },
            });
            self.app.update();

            self.assert_block(position, 0);
            self.assert_updates_and_ack(position, 0, client_id, sequence);
        }

        fn assert_block(&self, position: BlockPosition, expected: i32) {
            let chunk = self
                .app
                .world()
                .get::<ChunkData>(self.chunk_entity)
                .unwrap();
            assert_eq!(
                chunk.get_block(
                    position.x.rem_euclid(16) as u8,
                    position.y as i32,
                    position.z.rem_euclid(16) as u8,
                ),
                Some(expected),
                "unexpected server block state at {position:?}"
            );
        }

        fn assert_updates_and_ack(
            &self,
            position: BlockPosition,
            expected_state: i32,
            actor_client_id: u32,
            sequence: i32,
        ) {
            let packets: Vec<_> = self.outgoing.try_iter().collect();
            let mut recipients: Vec<u32> = packets
                .iter()
                .filter_map(|outgoing| match &outgoing.packet {
                    voidmc_protocol::clientbound::ClientboundPacket::Play(
                        voidmc_protocol::clientbound::PlayPacket::BlockUpdate(update),
                    ) if update.position == position && update.block_state_id == expected_state => {
                        Some(outgoing.client_id)
                    }
                    _ => None,
                })
                .collect();
            recipients.sort_unstable();
            assert_eq!(recipients, [1, 2]);

            let actor_update = packets
                .iter()
                .position(|outgoing| {
                    outgoing.client_id == actor_client_id
                        && matches!(
                            &outgoing.packet,
                            voidmc_protocol::clientbound::ClientboundPacket::Play(
                                voidmc_protocol::clientbound::PlayPacket::BlockUpdate(update)
                            ) if update.position == position
                                && update.block_state_id == expected_state
                        )
                })
                .expect("actor block update");
            let ack = packets
                .iter()
                .position(|outgoing| {
                    outgoing.client_id == actor_client_id
                        && matches!(
                            &outgoing.packet,
                            voidmc_protocol::clientbound::ClientboundPacket::Play(
                                voidmc_protocol::clientbound::PlayPacket::BlockChangedAck(ack)
                            ) if ack.sequence == sequence
                        )
                })
                .unwrap_or_else(|| panic!("missing block change ack for sequence {sequence}"));
            assert!(
                actor_update < ack,
                "block update must precede its acknowledgement"
            );
        }
    }
}
