//! Dropped-item entities: spawning, client rendering (metadata), and pickup.
//!
//! A drop is requested with an [`ItemDropEvent`] (emitted by inventory throws,
//! creative-inventory drops and the drop key); this plugin spawns a
//! `minecraft:item` entity carrying an [`ItemEntity`], thrown from the
//! dropper's eyes in the direction they look, as in vanilla. The entity tracker
//! replicates it and `ItemEntity` projects the item into the entity metadata so
//! the client shows the actual item. Nearby players pick drops up once the
//! pickup delay has run out.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use tracing::instrument;
use voidmc_protocol::clientbound;

use crate::components::{
    EntityDimension, EntityViewers, Grounded, ItemEntity, MinecraftEntityId, PickupDelay,
    PlayerDimension, PlayerReady, Position, Rotation, SpawnedEntity, Velocity,
};
use crate::entity::{EntityBuilder, EntityKind};
use crate::events::{EntityDespawnEvent, ItemDropEvent, PlayerDropItemEvent};
use crate::inventory::Inventory;
use crate::item::ItemStack;
use crate::players::Players;
use crate::schedule::VoidSystems;
use crate::systems::physics::apply_spawned_entity_physics;
use crate::world::DimensionId;

/// Ticks before a player-thrown item can be picked up (vanilla's 40, two
/// seconds at 20 TPS). An item thrown during tick `T` can be collected from
/// tick `T + PICKUP_DELAY_TICKS` on.
pub const PICKUP_DELAY_TICKS: u8 = 40;
/// Distance in blocks from a player's feet to an item's position within which
/// the player collects it.
pub const PICKUP_RADIUS: f64 = 1.5;
const PICKUP_RADIUS_SQ: f64 = PICKUP_RADIUS * PICKUP_RADIUS;
/// Height of the throw above the dropper's feet: a standing player's eyes
/// (1.62) less vanilla's 0.3.
const THROW_HEIGHT: f64 = 1.62 - 0.3;
/// Speed of a throw along the look direction, in blocks per tick.
const THROW_SPEED: f64 = 0.3;
/// Upward speed added to every throw, in blocks per tick.
const THROW_LIFT: f64 = 0.1;
/// Horizontal speed kept per tick in the air.
const AIR_DRAG: f64 = 0.98;
/// Horizontal speed kept per tick on the ground (block friction 0.6 × 0.98).
const GROUND_DRAG: f64 = 0.6 * 0.98;
/// A grounded item slower than this (squared, blocks per tick) stops.
const REST_SPEED_SQ: f64 = 1.0e-5;

pub struct ItemDropsPlugin;

impl Plugin for ItemDropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_item_drop)
            .add_observer(on_player_drop_item)
            .configure_sets(
                Update,
                VoidSystems::ItemPickup.after(VoidSystems::EntitySimulation),
            )
            .add_systems(
                Update,
                (
                    slow_dropped_items
                        .after(apply_spawned_entity_physics)
                        .in_set(VoidSystems::EntitySimulation),
                    // Pickup first: a delay that reaches zero in this tick
                    // takes effect in the next one.
                    (pickup_items, tick_pickup_delay)
                        .chain()
                        .in_set(VoidSystems::ItemPickup),
                ),
            );
    }
}

/// The vanilla throw (without its random spread): `THROW_SPEED` along the look
/// direction plus `THROW_LIFT` upwards.
fn throw_velocity(rotation: &Rotation) -> Velocity {
    let yaw = f64::from(rotation.yaw).to_radians();
    let pitch = f64::from(rotation.pitch).to_radians();
    Velocity {
        x: -yaw.sin() * pitch.cos() * THROW_SPEED,
        y: -pitch.sin() * THROW_SPEED + THROW_LIFT,
        z: yaw.cos() * pitch.cos() * THROW_SPEED,
    }
}

fn spawn_drop(
    commands: &mut Commands,
    dimension: DimensionId,
    position: Position,
    velocity: Velocity,
    stack: ItemStack,
) {
    EntityBuilder::new(EntityKind::Item)
        .position(position)
        .in_dimension(dimension)
        .velocity(velocity)
        .gravity(true)
        .block_collision(true)
        // A thrown item follows its arc; it is never snapped to the ground.
        .settle_ticks(0)
        .spawn(commands)
        .insert((ItemEntity { stack }, PickupDelay(PICKUP_DELAY_TICKS)));
}

/// Observer: throws a drop from the dropper's eyes along their look direction.
fn on_item_drop(
    event: On<ItemDropEvent>,
    mut commands: Commands,
    players: Query<(&Position, &Rotation, &PlayerDimension)>,
) {
    if event.stack.is_empty() {
        return;
    }
    let Ok((pos, rotation, dim)) = players.get(event.dropper) else {
        return;
    };
    spawn_drop(
        &mut commands,
        dim.0,
        Position {
            x: pos.x,
            y: pos.y + THROW_HEIGHT,
            z: pos.z,
        },
        throw_velocity(rotation),
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

/// `Update`, after the physics step: air and ground friction on a dropped
/// item's horizontal motion, so a throw comes to rest.
fn slow_dropped_items(
    mut items: Query<(&Grounded, &mut Velocity), (With<ItemEntity>, With<SpawnedEntity>)>,
) {
    for (grounded, mut velocity) in items.iter_mut() {
        if velocity.x == 0.0 && velocity.z == 0.0 {
            continue;
        }
        let drag = if grounded.0 { GROUND_DRAG } else { AIR_DRAG };
        let (mut x, mut z) = (velocity.x * drag, velocity.z * drag);
        if grounded.0 && x * x + z * z < REST_SPEED_SQ {
            (x, z) = (0.0, 0.0);
        }
        velocity.x = x;
        velocity.z = z;
    }
}

fn tick_pickup_delay(mut delays: Query<&mut PickupDelay>) {
    for mut delay in delays.iter_mut() {
        if delay.0 > 0 {
            delay.0 -= 1;
        }
    }
}

/// `Update`: nearby players collect dropped items. Each player in range takes
/// what fits in their inventory, in turn, until the item is gone; every
/// viewer of the item is sent the pickup animation with the amount taken.
#[instrument(
    name = "item_pickup",
    level = "info",
    skip(commands, sender, players, items)
)]
fn pickup_items(
    mut commands: Commands,
    sender: Players,
    mut players: Query<
        (
            &MinecraftEntityId,
            &Position,
            &PlayerDimension,
            &mut Inventory,
        ),
        With<PlayerReady>,
    >,
    mut items: Query<
        (
            Entity,
            &MinecraftEntityId,
            &Position,
            &EntityDimension,
            &EntityViewers,
            &mut ItemEntity,
            Option<&PickupDelay>,
        ),
        With<SpawnedEntity>,
    >,
) {
    for (item_entity, item_id, item_pos, item_dim, viewers, mut item, delay) in items.iter_mut() {
        if delay.is_some_and(|d| d.0 > 0) || item.stack.is_empty() {
            continue;
        }
        for (player_id, player_pos, player_dim, mut inventory) in players.iter_mut() {
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
            let taken = item.stack.count - leftover.count;
            if taken == 0 {
                // A full inventory leaves the item to the next player.
                continue;
            }
            sender.send_to(
                viewers.iter(),
                clientbound::TakeItemEntity {
                    collected_entity_id: item_id.0,
                    collector_entity_id: player_id.0,
                    amount: i32::from(taken),
                },
            );
            item.stack = leftover;
            if item.stack.is_empty() {
                commands.trigger(EntityDespawnEvent {
                    entity: item_entity,
                });
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;

    use super::*;
    use crate::item::ItemId;
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

    /// An app that can send packets, with the outgoing queue.
    fn packet_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx));
        (app, outgoing_rx)
    }

    fn assert_close(actual: Velocity, expected: (f64, f64, f64)) {
        let off = [
            actual.x - expected.0,
            actual.y - expected.1,
            actual.z - expected.2,
        ];
        assert!(
            off.iter().all(|d| d.abs() < 1.0e-9),
            "{actual:?} is not {expected:?}"
        );
    }

    #[test]
    fn throw_goes_where_the_player_looks() {
        let throw = |yaw, pitch| throw_velocity(&Rotation { yaw, pitch });
        // Vanilla yaw: 0 faces south (+z), 90 west (-x), -90 east (+x).
        assert_close(throw(0.0, 0.0), (0.0, 0.1, 0.3));
        assert_close(throw(90.0, 0.0), (-0.3, 0.1, 0.0));
        assert_close(throw(-90.0, 0.0), (0.3, 0.1, 0.0));
        assert_close(throw(180.0, 0.0), (0.0, 0.1, -0.3));
        // Straight down drops it at the feet; straight up throws it upwards.
        assert_close(throw(0.0, 90.0), (0.0, -0.2, 0.0));
        assert_close(throw(0.0, -90.0), (0.0, 0.4, 0.0));
    }

    fn diamonds(count: u8) -> ItemStack {
        ItemStack::new(ItemId::from_name("minecraft:diamond").unwrap(), count)
    }

    fn full_of_stone() -> Inventory {
        let mut inventory = Inventory::new();
        let stone = ItemStack::of("minecraft:stone", 64).unwrap();
        for slot in Inventory::MAIN_START..Inventory::OFFHAND {
            inventory.set(slot, stone.clone());
        }
        inventory
    }

    fn total(inventory: &Inventory, item: ItemId) -> u32 {
        (0..Inventory::SIZE)
            .map(|slot| inventory.get(slot))
            .filter(|stack| stack.item == item)
            .map(|stack| u32::from(stack.count))
            .sum()
    }

    #[test]
    fn a_full_inventory_leaves_the_item_to_the_next_player_in_range() {
        let diamond = ItemId::from_name("minecraft:diamond").unwrap();
        // Query order follows spawn order here: the full player is met first
        // in one run and last in the other.
        for full_first in [true, false] {
            let (mut app, _outgoing) = packet_app();
            app.add_systems(Update, pickup_items);
            let spawn_player = |app: &mut App, id: i32, inventory: Inventory| {
                app.world_mut()
                    .spawn((
                        MinecraftEntityId(id),
                        Position::default(),
                        PlayerDimension(DimensionId::Overworld),
                        inventory,
                        PlayerReady,
                    ))
                    .id()
            };
            let (full, roomy) = if full_first {
                let full = spawn_player(&mut app, 1, full_of_stone());
                (full, spawn_player(&mut app, 2, Inventory::new()))
            } else {
                let roomy = spawn_player(&mut app, 2, Inventory::new());
                (spawn_player(&mut app, 1, full_of_stone()), roomy)
            };
            let item = EntityBuilder::new(EntityKind::Item)
                .at(0.5, 0.0, 0.5)
                .spawn_in(app.world_mut())
                .insert(ItemEntity { stack: diamonds(3) })
                .id();

            app.update();

            let world = app.world();
            assert_eq!(total(world.get::<Inventory>(full).unwrap(), diamond), 0);
            assert_eq!(total(world.get::<Inventory>(roomy).unwrap(), diamond), 3);
            assert!(
                world.get::<ItemEntity>(item).unwrap().stack.is_empty(),
                "full player first: {full_first}"
            );
        }
    }

    #[test]
    fn items_rest_after_a_throw() {
        let mut app = App::new();
        app.add_systems(Update, slow_dropped_items);
        let item = EntityBuilder::new(EntityKind::Item)
            .gravity(true)
            .velocity(Velocity {
                x: 0.3,
                y: 0.0,
                z: -0.2,
            })
            .spawn_in(app.world_mut())
            .insert(ItemEntity { stack: diamonds(1) })
            .id();

        app.update();
        let velocity = *app.world().get::<Velocity>(item).unwrap();
        assert_close(velocity, (0.3 * AIR_DRAG, 0.0, -0.2 * AIR_DRAG));

        app.world_mut().get_mut::<Grounded>(item).unwrap().0 = true;
        for _ in 0..30 {
            app.update();
        }
        assert_close(*app.world().get::<Velocity>(item).unwrap(), (0.0, 0.0, 0.0));
    }
}
