//! Dropped-item entities: spawning, client rendering (metadata), and pickup.
//!
//! A drop is requested with an [`ItemDropEvent`] (emitted by inventory throws and
//! the drop key); this plugin spawns a `minecraft:item` entity carrying an
//! [`ItemEntity`], which the standard entity broadcast renders, plus a
//! `SetEntityData` packet so the client shows the actual item. Nearby players
//! pick drops up after a short delay.

use bevy_app::{App, Plugin, PostUpdate, Update};
use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;

use crate::components::{
    ClientId, EntityCollider, EntityDimension, EntityIdCounter, EntityType, EntityUuid, Grounded,
    ItemEntity, MinecraftEntityId, MovementConfig, PickupDelay, PlayerDimension, PlayerReady,
    Position, PreviousPosition, RecentlySpawned, Rotation, SpawnedEntity, Velocity,
    VerticalVelocity,
};
use crate::events::{EntityDespawnEvent, ItemDropEvent, PlayerDropItemEvent, PlayerReadyEvent};
use crate::inventory::Inventory;
use crate::item::ItemStack;
use crate::network::{NetworkChannels, OutgoingPacket};
use crate::plugins::inventory::InventoryDirty;
use crate::systems::entities::broadcast_entity_spawns;
use crate::world::DimensionId;

/// Protocol entity-type id of `minecraft:item` in 26.1.2.
const ITEM_ENTITY_TYPE: i32 = 71;
/// Ticks before a freshly dropped item can be picked up.
const PICKUP_DELAY_TICKS: u8 = 10;
/// Squared pickup radius in blocks.
const PICKUP_RADIUS_SQ: f64 = 1.5 * 1.5;

pub struct ItemDropsPlugin;

impl Plugin for ItemDropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_item_drop)
            .add_observer(on_player_drop_item)
            .add_observer(send_item_data_on_join)
            .add_systems(
                PostUpdate,
                broadcast_item_data.after(broadcast_entity_spawns),
            )
            .add_systems(Update, (tick_pickup_delay, pickup_items));
    }
}

fn entity_visible(
    item_dim: Option<&EntityDimension>,
    player_dim: Option<&PlayerDimension>,
) -> bool {
    match (item_dim, player_dim) {
        (Some(a), Some(b)) => a.0 == b.0,
        _ => true,
    }
}

fn item_data_packet(entity_id: i32, stack: &ItemStack) -> clientbound::ClientboundPacket {
    clientbound::ClientboundPacket::Play(clientbound::PlayPacket::SetEntityData(
        clientbound::SetEntityData::item(entity_id, stack.to_slot()),
    ))
}

/// Spawns a floating item entity. The `Added<SpawnedEntity>` broadcast sends the
/// `SpawnEntity` packet; [`broadcast_item_data`] sends its metadata.
#[allow(clippy::too_many_arguments)]
fn spawn_drop(
    commands: &mut Commands,
    id_counter: &mut EntityIdCounter,
    dimension: DimensionId,
    x: f64,
    y: f64,
    z: f64,
    velocity: Velocity,
    stack: ItemStack,
) {
    let entity_id = id_counter.0;
    id_counter.0 += 1;
    let vy = velocity.y;
    commands.spawn((
        MinecraftEntityId(entity_id),
        EntityUuid(uuid::Uuid::new_v4()),
        Position { x, y, z },
        PreviousPosition { x, y, z },
        Rotation {
            yaw: 0.0,
            pitch: 0.0,
        },
        velocity,
        EntityType(ITEM_ENTITY_TYPE),
        EntityDimension(dimension),
        SpawnedEntity,
        EntityCollider::for_entity_name("minecraft:item"),
        MovementConfig {
            wander: false,
            gravity_enabled: true,
            block_collision_enabled: true,
        },
        VerticalVelocity(vy),
        Grounded(false),
        RecentlySpawned(5),
        (ItemEntity { stack }, PickupDelay(PICKUP_DELAY_TICKS)),
    ));
}

/// Observer: spawns a drop at the dropper's position.
fn on_item_drop(
    event: On<ItemDropEvent>,
    mut commands: Commands,
    mut id_counter: ResMut<EntityIdCounter>,
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
        &mut id_counter,
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
    commands.entity(event.entity).insert(InventoryDirty);
}

/// `PostUpdate`: sends metadata for newly spawned item entities so the client
/// renders the actual item (runs after the `SpawnEntity` broadcast).
#[instrument(
    name = "item_metadata_broadcast",
    level = "info",
    skip(channels, new_items, ready_players)
)]
fn broadcast_item_data(
    channels: Res<NetworkChannels>,
    new_items: Query<
        (&MinecraftEntityId, &ItemEntity, Option<&EntityDimension>),
        Added<SpawnedEntity>,
    >,
    ready_players: Query<(&ClientId, Option<&PlayerDimension>), With<PlayerReady>>,
) {
    for (entity_id, item, dim) in new_items.iter() {
        let packet = item_data_packet(entity_id.0, &item.stack);
        for (client_id, player_dim) in ready_players.iter() {
            if entity_visible(dim, player_dim) {
                let _ = channels.outgoing.send(OutgoingPacket {
                    client_id: client_id.0,
                    packet: packet.clone(),
                });
            }
        }
    }
}

/// Observer: sends existing item-entity metadata to a joining player (the
/// `SpawnEntity` packets are sent by the generic entity-spawn handler).
#[instrument(
    name = "item_metadata_join_sync",
    level = "info",
    skip(event, channels, joiner, items)
)]
fn send_item_data_on_join(
    event: On<PlayerReadyEvent>,
    channels: Res<NetworkChannels>,
    joiner: Query<(&ClientId, Option<&PlayerDimension>)>,
    items: Query<(&MinecraftEntityId, &ItemEntity, Option<&EntityDimension>), With<SpawnedEntity>>,
) {
    let Ok((client_id, player_dim)) = joiner.get(event.entity) else {
        return;
    };
    for (entity_id, item, dim) in items.iter() {
        if entity_visible(dim, player_dim) {
            let _ = channels.outgoing.send(OutgoingPacket {
                client_id: client_id.0,
                packet: item_data_packet(entity_id.0, &item.stack),
            });
        }
    }
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
    mut players: Query<(Entity, &Position, &PlayerDimension, &mut Inventory), With<PlayerReady>>,
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
        for (player, player_pos, player_dim, mut inventory) in players.iter_mut() {
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
            commands.entity(player).insert(InventoryDirty);
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
