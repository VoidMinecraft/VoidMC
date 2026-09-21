//! Dropped-item entities: spawning, client rendering (metadata), and pickup.
//!
//! A drop is requested with an [`ItemDropEvent`] (emitted by inventory throws and
//! the drop key); this plugin spawns a `minecraft:item` entity carrying an
//! [`ItemEntity`]; the entity tracker replicates it and `ItemEntity` projects
//! the item into the entity metadata so the client shows the actual item.
//! Nearby players pick drops up after a short delay.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use tracing::instrument;

use crate::components::{
    EntityDimension, ItemEntity, PickupDelay, PlayerDimension, PlayerReady, Position,
    SpawnedEntity, Velocity,
};
use crate::entity::{EntityBuilder, EntityKind};
use crate::events::{EntityDespawnEvent, ItemDropEvent, PlayerDropItemEvent};
use crate::inventory::Inventory;
use crate::item::ItemStack;
use crate::players::Players;
use crate::plugins::inventory::InventoryDirty;
use crate::schedule::VoidSystems;
use crate::world::DimensionId;

/// Ticks before a freshly dropped item can be picked up.
const PICKUP_DELAY_TICKS: u8 = 10;
/// Squared pickup radius in blocks.
const PICKUP_RADIUS_SQ: f64 = 1.5 * 1.5;

pub struct ItemDropsPlugin;

impl Plugin for ItemDropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_item_drop)
            .add_observer(on_player_drop_item)
            .add_systems(
                Update,
                (tick_pickup_delay, pickup_items).in_set(VoidSystems::ItemPickup),
            );
    }
}

fn spawn_drop(
    commands: &mut Commands,
    dimension: DimensionId,
    x: f64,
    y: f64,
    z: f64,
    velocity: Velocity,
    stack: ItemStack,
) {
    EntityBuilder::new(EntityKind::Item)
        .at(x, y, z)
        .in_dimension(dimension)
        .velocity(velocity)
        .gravity(true)
        .block_collision(true)
        .settle_ticks(5)
        .spawn(commands)
        .insert((ItemEntity { stack }, PickupDelay(PICKUP_DELAY_TICKS)));
}

/// Observer: spawns a drop at the dropper's position.
fn on_item_drop(
    event: On<ItemDropEvent>,
    mut commands: Commands,
    players: Query<(&Position, &PlayerDimension)>,
) {
    if event.stack.is_empty() {
        return;
    }
    let Ok((pos, dim)) = players.get(event.dropper) else {
        return;
    };
    spawn_drop(
        &mut commands,
        dim.0,
        pos.x,
        pos.y + 1.0,
        pos.z,
        Velocity {
            x: 0.0,
            y: 0.2,
            z: 0.0,
        },
        event.stack.clone(),
    );
}

/// Observer: the drop key (Q) drops one or the whole held stack.
fn on_player_drop_item(
    event: On<PlayerDropItemEvent>,
    mut players: Query<&mut Inventory>,
    mut commands: Commands,
) {
    let Ok(mut inv) = players.get_mut(event.entity) else {
        return;
    };
    let idx = Inventory::hotbar_slot_index(inv.selected_hotbar());
    let held = inv.get(idx).clone();
    if held.is_empty() {
        return;
    }

    let dropped = if event.drop_stack {
        inv.set(idx, ItemStack::EMPTY);
        held
    } else {
        let mut one = held.clone();
        one.count = 1;
        let mut remaining = held;
        remaining.count -= 1;
        inv.set(
            idx,
            if remaining.count == 0 {
                ItemStack::EMPTY
            } else {
                remaining
            },
        );
        one
    };

    commands.trigger(ItemDropEvent {
        dropper: event.entity,
        stack: dropped,
    });
}

fn tick_pickup_delay(mut delays: Query<&mut PickupDelay>) {
    for mut delay in delays.iter_mut() {
        delay.0 = delay.0.saturating_sub(1);
    }
}

/// `Update`: nearby players collect dropped items. Item entities do not render a
/// count, so a partial pickup just lowers the stored stack without re-sending
/// metadata.
#[instrument(name = "item_pickup", level = "info", skip(commands, players, items))]
fn pickup_items(
    mut commands: Commands,
    mut players: Query<(&Position, &PlayerDimension, &mut Inventory), With<PlayerReady>>,
    mut items: Query<
        (
            Entity,
            &Position,
            &EntityDimension,
            &mut ItemEntity,
            Option<&PickupDelay>,
        ),
        With<SpawnedEntity>,
    >,
) {
    for (item_entity, item_pos, item_dim, mut item, delay) in items.iter_mut() {
        if delay.is_some_and(|d| d.0 > 0) || item.stack.is_empty() {
            continue;
        }
        for (player_pos, player_dim, mut inventory) in players.iter_mut() {
            if player_dim.0 != item_dim.0 {
                continue;
            }
            let dx = player_pos.x - item_pos.x;
            let dy = player_pos.y - item_pos.y;
            let dz = player_pos.z - item_pos.z;
            if dx * dx + dy * dy + dz * dz > PICKUP_RADIUS_SQ {
                continue;
            }

            let leftover = inventory.give(item.stack.clone());
            if leftover.is_empty() {
                commands.trigger(EntityDespawnEvent {
                    entity: item_entity,
                });
            } else {
                item.stack = leftover;
            }
            break;
        }
    }
}
