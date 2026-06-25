//! Item-use plugin: turns player right-clicks and dig-finishes into queued
//! actions processed by the exclusive item-behaviour drain system, tracks the
//! selected hotbar slot, and applies creative-mode slot changes.
//!
//! This replaces the old hardcoded `HotbarBlocks` placement: what a right-click
//! does is now driven by the [`ItemStack`] the player is actually holding and the
//! [`ItemBehaviorRegistry`](crate::item_behavior::ItemBehaviorRegistry).

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use voidmc_protocol::serverbound::SetCreativeModeSlot;

use crate::components::{ClientId, HotbarSlot};
use crate::events::{
    PlayerChangeSlotEvent, PlayerFinishDiggingEvent, PlayerUseItemEvent, PlayerUseItemOnBlockEvent,
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
