//! Inventory synchronisation plugin.
//!
//! Sends the player their inventory contents when they become ready and keeps
//! the client in sync afterwards. A change is signalled by inserting the
//! [`InventoryDirty`] marker; a `PostUpdate` system then re-sends the window and
//! clears the marker. (M2 re-sends the whole window for simplicity; per-slot
//! `SetContainerSlot` updates are a later optimisation.)

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound;
use voidmc_protocol::serverbound::ClickContainer;

use crate::components::{ContainerSync, HotbarSlot};
use crate::events::PlayerReadyEvent;
use crate::inventory::Inventory;
use crate::network::PacketEvent;
use crate::players::Players;
use crate::schedule::VoidSystems;

/// Marker: this player's inventory changed and must be re-synced to the client.
/// Insert it after mutating an [`Inventory`]; the resync system removes it.
#[derive(Component)]
pub struct InventoryDirty;

/// Window id of the player's own inventory.
pub const PLAYER_WINDOW_ID: i32 = 0;

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(sync_inventory_on_ready)
            .add_observer(handle_container_click)
            .add_systems(
                PostUpdate,
                resync_dirty_inventories.in_set(VoidSystems::InventorySync),
            );
    }
}

/// Observer: applies a container click to the player's own inventory (window 0)
/// authoritatively, then re-syncs. Items dropped by the click are spawned in M5.
fn handle_container_click(
    event: On<PacketEvent<ClickContainer>>,
    mut inventories: Query<&mut Inventory>,
    mut commands: Commands,
) {
    if event.packet.container_id != PLAYER_WINDOW_ID {
        // Foreign containers (chests, etc.) are not implemented yet.
        return;
    }
    if let Ok(mut inv) = inventories.get_mut(event.entity) {
        let dropped = inv.apply_click(event.packet.slot, event.packet.button, event.packet.mode);
        for stack in dropped {
            commands.trigger(crate::events::ItemDropEvent {
                dropper: event.entity,
                stack,
            });
        }
        commands.entity(event.entity).insert(InventoryDirty);
    }
}

/// Sends a full `SetContainerContent` for the player window.
fn send_full_inventory(players: &Players, player: Entity, inv: &Inventory, state_id: i32) {
    players.send(
        player,
        clientbound::SetContainerContent {
            container_id: PLAYER_WINDOW_ID,
            state_id,
            items: inv.to_slots(),
            carried: inv.cursor().to_slot(),
        },
    );
}

/// Observer: on join, send the full inventory and the selected hotbar slot.
fn sync_inventory_on_ready(
    event: On<PlayerReadyEvent>,
    players: Players,
    mut inventories: Query<(&Inventory, &mut ContainerSync, &HotbarSlot)>,
) {
    let Ok((inv, mut sync, hotbar)) = inventories.get_mut(event.entity) else {
        return;
    };
    let state_id = sync.advance();
    send_full_inventory(&players, event.entity, inv, state_id);
    players.send(
        event.entity,
        clientbound::SetHeldSlot {
            slot: hotbar.0.clamp(0, 8) as i32,
        },
    );
}

/// `PostUpdate`: re-sync inventories flagged [`InventoryDirty`] and clear the flag.
fn resync_dirty_inventories(
    mut commands: Commands,
    players: Players,
    mut dirty: Query<(Entity, &Inventory, &mut ContainerSync), With<InventoryDirty>>,
) {
    for (entity, inv, mut sync) in dirty.iter_mut() {
        let state_id = sync.advance();
        send_full_inventory(&players, entity, inv, state_id);
        commands.entity(entity).remove::<InventoryDirty>();
    }
}
