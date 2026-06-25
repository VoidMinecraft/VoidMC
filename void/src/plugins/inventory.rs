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

use crate::components::{ClientId, ContainerSync, HotbarSlot};
use crate::events::PlayerReadyEvent;
use crate::inventory::Inventory;
use crate::network::{NetworkChannels, OutgoingPacket, PacketEvent};

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
            .add_systems(PostUpdate, resync_dirty_inventories);
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
fn send_full_inventory(client_id: u32, inv: &Inventory, state_id: i32, channels: &NetworkChannels) {
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SetContainerContent(
            clientbound::SetContainerContent {
                container_id: PLAYER_WINDOW_ID,
                state_id,
                items: inv.to_slots(),
                carried: inv.cursor().to_slot(),
            },
        )),
    });
}

/// Observer: on join, send the full inventory and the selected hotbar slot.
fn sync_inventory_on_ready(
    event: On<PlayerReadyEvent>,
    channels: Res<NetworkChannels>,
    mut players: Query<(&ClientId, &Inventory, &mut ContainerSync, &HotbarSlot)>,
) {
    let Ok((client_id, inv, mut sync, hotbar)) = players.get_mut(event.entity) else {
        return;
    };
    let state_id = sync.advance();
    send_full_inventory(client_id.0, inv, state_id, &channels);
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id: client_id.0,
        packet: clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SetHeldSlot(
            clientbound::SetHeldSlot {
                slot: hotbar.0.clamp(0, 8) as i32,
            },
        )),
    });
}

/// `PostUpdate`: re-sync inventories flagged [`InventoryDirty`] and clear the flag.
fn resync_dirty_inventories(
    mut commands: Commands,
    channels: Res<NetworkChannels>,
    mut dirty: Query<(Entity, &ClientId, &Inventory, &mut ContainerSync), With<InventoryDirty>>,
) {
    for (entity, client_id, inv, mut sync) in dirty.iter_mut() {
        let state_id = sync.advance();
        send_full_inventory(client_id.0, inv, state_id, &channels);
        commands.entity(entity).remove::<InventoryDirty>();
    }
}
