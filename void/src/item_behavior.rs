//! The overridable item-behaviour API.
//!
//! Developers implement [`ItemBehavior`] for a unit struct and register it for an
//! item; the server then dispatches right-clicks (on a block or in the air) to it,
//! falling back to a built-in default (a block item places its block) when the
//! behaviour returns [`UseResult::Pass`] or none is registered.
//!
//! ```no_run
//! use voidmc::item_behavior::{ItemBehavior, ItemUseContext, UseResult, ItemBehaviorRegistry};
//!
//! struct Wand;
//! impl ItemBehavior for Wand {
//!     fn on_use_on_block(&self, ctx: &mut ItemUseContext) -> UseResult {
//!         ctx.reply("zap!");
//!         ctx.place_block(voidmc_data::v26_1_2::blocks::GLOWSTONE);
//!         UseResult::Handled
//!     }
//! }
//! # fn register(reg: &mut ItemBehaviorRegistry) {
//! reg.register_for("minecraft:stick", Wand);
//! # }
//! ```
//!
//! Behaviours run in an exclusive system with `&mut World`, so [`ItemUseContext`]
//! exposes full world access — observers cannot take `&mut World`, hence the
//! queue + drain pattern below (mirroring the command system).
//!
//! Blocks that react to a right-click themselves (a lever) implement
//! [`BlockInteraction`] instead; it runs after the item's behaviour passed and
//! before the default placement.
//!
//! The default placement follows vanilla: the block goes into the clicked
//! block when that one is replaceable (air, water, short grass, ...), else
//! across the clicked face if the block there is replaceable; it is refused
//! when an entity that blocks building is in the way; it takes its
//! orientation from the clicked face, the cursor and the player's rotation;
//! and outside Creative it uses up one item. A refused placement resends the
//! real blocks to the player. [`ItemUseContext::place_block`] does none of
//! this: it puts the given state across the clicked face, whatever is there.
//!
//! Before any of this, a click on a block out of the player's reach (its
//! block interaction range plus one, from its eyes) is refused without an
//! event, for placing as for breaking.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound;
use voidmc_protocol::types::{BlockFace, BlockPosition, Hand};

use crate::components::{
    ClientId, EntityCollider, EntityDimension, EntityType, LoadedChunks, PlayerDimension, Position,
    Rotation, Sneaking, SpawnedEntity,
};
use crate::config::ServerConfigResource;
use crate::inventory::Inventory;
use crate::item::{ItemId, ItemStack};
use crate::messages::TextColor;
use crate::placement::{self, Click, Placement};
use crate::players::WorldPlayers;
use crate::plugins::abilities::PlayerAbilities;
use crate::world::mutation::send_ack;
use crate::world::{
    BlockMutation, ChunkData, ChunkIndex, ChunkPos, DimensionId, mutate_block, offset_position,
};

/// The block a "use item on block" action targeted.
#[derive(Clone, Copy, Debug)]
pub struct BlockUseTarget {
    /// The clicked block (a block is placed adjacent to it across `face`).
    pub position: BlockPosition,
    pub face: BlockFace,
    /// Cursor position on the clicked face, each component in 0.0..=1.0.
    pub cursor: (f32, f32, f32),
    pub inside_block: bool,
}

/// The outcome of an item behaviour. `Handled` suppresses the default action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseResult {
    Handled,
    Pass,
}

/// Context passed to an [`ItemBehavior`], with full world access.
pub struct ItemUseContext<'a> {
    world: &'a mut World,
    pub player: Entity,
    pub client_id: u32,
    pub hand: Hand,
    /// Snapshot of the item in `hand` when the action was queued.
    pub held: ItemStack,
    /// The targeted block, or `None` for a use-in-air.
    pub block: Option<BlockUseTarget>,
    /// The inventory slot `held` was taken from.
    held_slot: Option<usize>,
}

impl ItemUseContext<'_> {
    /// Places `state_id` adjacent to the clicked block face, whatever is
    /// there: unlike the default placement, it replaces any block, ignores
    /// the entities in the way and uses up nothing. No-op for use-in-air.
    pub fn place_block(&mut self, state_id: i32) -> bool {
        let Some(target) = self.block else {
            return false;
        };
        let Some(dimension) = self.world.get::<PlayerDimension>(self.player).map(|d| d.0) else {
            return false;
        };
        let pos = offset_position(target.position, target.face);
        mutate_block(
            self.world,
            self.player,
            dimension,
            pos,
            state_id,
            target.face,
            BlockMutation::Place,
        )
        .is_some()
    }

    /// Sets an arbitrary block in the player's dimension.
    pub fn set_block(&mut self, position: BlockPosition, state_id: i32) -> bool {
        let Some(dimension) = self.world.get::<PlayerDimension>(self.player).map(|d| d.0) else {
            return false;
        };
        mutate_block(
            self.world,
            self.player,
            dimension,
            position,
            state_id,
            BlockFace::Top,
            BlockMutation::Place,
        )
        .is_some()
    }

    /// Removes `n` from the stack in the slot the item was used from
    /// (server-authoritative).
    pub fn consume(&mut self, n: u8) {
        let Some(idx) = self.held_slot else {
            return;
        };
        if let Some(mut inv) = self.world.get_mut::<Inventory>(self.player) {
            let mut stack = inv.get(idx).clone();
            if !stack.is_empty() {
                stack.count = stack.count.saturating_sub(n);
                inv.set(
                    idx,
                    if stack.count == 0 {
                        ItemStack::EMPTY
                    } else {
                        stack
                    },
                );
            }
        }
    }

    /// Gives the player an item stack (stacking).
    pub fn give(&mut self, stack: ItemStack) {
        if let Some(mut inv) = self.world.get_mut::<Inventory>(self.player) {
            inv.give(stack);
        }
    }

    /// Sends a chat message to the acting player.
    pub fn reply(&self, message: &str) {
        crate::commands::send_system_chat(self.world, self.player, message, TextColor::White);
    }

    /// Read-only world access for advanced behaviours.
    pub fn with_world<R>(&self, f: impl FnOnce(&World) -> R) -> R {
        f(self.world)
    }

    /// Mutable world access for advanced behaviours.
    pub fn with_world_mut<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> R {
        f(self.world)
    }

    /// The default placement of the held block item, `item_state` being its
    /// block: vanilla's rules for the target, the entities in the way, the
    /// orientation and the item used up (see the module documentation).
    fn place_held_block(&mut self, target: BlockUseTarget, item_state: i32) {
        let Some(item_block) = voidmc_data::block_type_of_state(BLOCK_DATA_VERSION, item_state)
        else {
            return;
        };
        let Some(dimension) = self.world.get::<PlayerDimension>(self.player).map(|d| d.0) else {
            return;
        };
        let rotation = self
            .world
            .get::<Rotation>(self.player)
            .copied()
            .unwrap_or_default();
        let click = Click {
            clicked: target.position,
            face: target.face,
            cursor: target.cursor,
            yaw: rotation.yaw,
            pitch: rotation.pitch,
        };
        let world = &*self.world;
        let outcome = placement::place(item_block, &click, |position| {
            block_state(world, dimension, position)
        });
        let (filled, placed) = match outcome {
            Placement::Place { position, state } => {
                let placed = !entity_in_the_way(self.world, dimension, position, state)
                    && mutate_block(
                        self.world,
                        self.player,
                        dimension,
                        position,
                        state,
                        target.face,
                        BlockMutation::Place,
                    )
                    .is_some();
                (position, placed)
            }
            Placement::Refused { target } => (target, false),
        };
        if placed {
            if !placement::creative(self.world) {
                self.use_up_one();
            }
        } else {
            resync_blocks(self.world, self.player, &[target.position, filled]);
            resend_hand(self.world, self.player, self.held_slot, &self.held);
        }
    }

    /// Takes one item from the slot the item was used from, as its client
    /// predicted when it placed the block: the slot is not sent back.
    fn use_up_one(&mut self) {
        let Some(index) = self.held_slot else {
            return;
        };
        if let Some(mut inventory) = self.world.get_mut::<Inventory>(self.player) {
            let mut stack = inventory.get(index).clone();
            if stack.is_empty() || stack.item != self.held.item {
                return;
            }
            stack.count -= 1;
            let stack = if stack.count == 0 {
                ItemStack::EMPTY
            } else {
                stack
            };
            inventory.set_predicted(index, stack);
        }
    }
}

/// Whether an entity that keeps blocks from being placed into it (a player,
/// a mob, ...; not an item) intersects `state` placed at `position`.
fn entity_in_the_way(
    world: &mut World,
    dimension: DimensionId,
    position: BlockPosition,
    state: i32,
) -> bool {
    let boxes = voidmc_data::v26_1_2::shapes::for_state(state);
    if boxes.is_empty() {
        return false;
    }
    let origin = [
        f64::from(position.x),
        f64::from(position.y),
        f64::from(position.z),
    ];
    let intersects = |feet: &Position, half_width: f64, height: f64| {
        let min = [feet.x - half_width, feet.y, feet.z - half_width];
        let max = [feet.x + half_width, feet.y + height, feet.z + half_width];
        boxes.iter().any(|b| {
            let lo = [b.x0, b.y0, b.z0].map(f64::from);
            let hi = [b.x1, b.y1, b.z1].map(f64::from);
            (0..3).all(|axis| {
                min[axis] < origin[axis] + hi[axis] && max[axis] > origin[axis] + lo[axis]
            })
        })
    };

    // Spectators never block, and a spectator server has only spectators.
    let spectators = world
        .get_resource::<ServerConfigResource>()
        .is_some_and(|config| config.game_mode == SPECTATOR);
    if !spectators {
        let mut players = world.query::<(
            &Position,
            &PlayerDimension,
            Option<&Sneaking>,
            Option<&PlayerAbilities>,
        )>();
        for (feet, in_dimension, sneaking, abilities) in players.iter(world) {
            let crouching = sneaking.is_some_and(|s| s.0)
                && !abilities.is_some_and(|abilities| abilities.flying);
            let height = if crouching {
                PLAYER_CROUCHING_HEIGHT
            } else {
                PLAYER_HEIGHT
            };
            if in_dimension.0 == dimension && intersects(feet, PLAYER_HALF_WIDTH, height) {
                return true;
            }
        }
    }
    let mut entities = world.query_filtered::<(
        &Position,
        &EntityDimension,
        &EntityCollider,
        &EntityType,
    ), With<SpawnedEntity>>();
    entities
        .iter(world)
        .any(|(feet, in_dimension, collider, kind)| {
            in_dimension.0 == dimension
                && crate::entity::EntityKind::from_id(kind.0)
                    .is_some_and(placement::blocks_building)
                && intersects(feet, collider.half_width, collider.height)
        })
}

/// The configured game mode of a spectator server.
const SPECTATOR: u8 = 3;

/// A player's collision box: 0.6 wide, 1.8 tall, 1.5 crouching.
const PLAYER_HALF_WIDTH: f64 = 0.3;
const PLAYER_HEIGHT: f64 = 1.8;
const PLAYER_CROUCHING_HEIGHT: f64 = 1.5;

/// Sends `player` the real blocks at `positions` (each once), as a vanilla
/// server does to undo a refused click the client predicted. Positions
/// outside the world or the chunks the player holds are skipped.
fn resync_blocks(world: &World, player: Entity, positions: &[BlockPosition]) {
    let Some(dimension) = world.get::<PlayerDimension>(player).map(|d| d.0) else {
        return;
    };
    let Some(loaded) = world.get::<LoadedChunks>(player) else {
        return;
    };
    let players = WorldPlayers::new(world);
    for (index, &position) in positions.iter().enumerate() {
        let chunk = ChunkPos::new(position.x.div_euclid(16), position.z.div_euclid(16));
        if positions[..index].contains(&position) || !loaded.0.contains(&chunk) {
            continue;
        }
        if let Some(state) = block_state(world, dimension, position) {
            players.send(
                player,
                clientbound::BlockUpdate {
                    position,
                    block_state_id: state,
                },
            );
        }
    }
}

/// Resends the slot a refused placement was made from, outside Creative:
/// the client may have predicted using the item up.
fn resend_hand(world: &mut World, player: Entity, slot: Option<usize>, held: &ItemStack) {
    if held.is_empty() || placement::creative(world) {
        return;
    }
    if let (Some(slot), Some(mut inventory)) = (slot, world.get_mut::<Inventory>(player)) {
        inventory.mark_slot(slot);
    }
}

/// Refuses a click on a block before anything handles it (out of reach):
/// `player` is sent the real blocks at `positions` and, for a use with a
/// `hand`, that hand's slot outside Creative, then its `sequence` is
/// acknowledged, in order with its other queued actions.
pub(crate) fn refuse_click(
    world: &mut World,
    player: Entity,
    sequence: i32,
    positions: &[BlockPosition],
    hand: Option<Hand>,
) {
    let client_id = world.get::<ClientId>(player).map_or(0, |id| id.0);
    let held = hand.and_then(|hand| held_in(world.get::<Inventory>(player), hand));
    let queued = QueuedUse {
        player,
        client_id,
        hand: hand.unwrap_or(Hand::MainHand),
        sequence,
        action: UseAction::Refused {
            positions: positions.to_vec(),
        },
        held,
    };
    match world.get_resource_mut::<ItemUseQueue>() {
        Some(mut queue) => queue.0.push_back(queued),
        None => {
            resync_blocks(world, player, positions);
            if let Some((slot, held)) = &queued.held {
                resend_hand(world, player, Some(*slot), held);
            }
            send_ack(world, player, sequence);
        }
    }
}

/// Implemented by developers to customise what an item does when right-clicked.
///
/// All methods default to [`UseResult::Pass`], which lets the built-in default
/// (block item → place its block) run.
pub trait ItemBehavior: Send + Sync + 'static {
    /// Right-click while pointing at a block.
    fn on_use_on_block(&self, _ctx: &mut ItemUseContext) -> UseResult {
        UseResult::Pass
    }
    /// Right-click while not pointing at a block. A vanilla client also sends
    /// one right after a right-click on a block that it expects the item to do
    /// nothing with, so this can follow [`Self::on_use_on_block`] for the same
    /// click.
    fn on_use(&self, _ctx: &mut ItemUseContext) -> UseResult {
        UseResult::Pass
    }
    /// Called when the player starts breaking a block while holding this item,
    /// and in survival once more right before the mined block is removed
    /// (`broken_state` is the block still in place). `Handled` keeps the block
    /// and resyncs it to the player, so tools like a selection wand can use
    /// left-click without destroying anything.
    fn before_break_block(&self, _ctx: &mut BlockBreakContext) -> UseResult {
        UseResult::Pass
    }
    /// Called when the player finishes breaking a block while holding this item
    /// (after the block is removed). Use it for tool side effects — custom drops,
    /// durability, messages. The return value is reserved for suppressing default
    /// loot once loot tables exist.
    fn on_break_block(&self, _ctx: &mut BlockBreakContext) -> UseResult {
        UseResult::Pass
    }
}

/// Context passed to [`ItemBehavior::on_break_block`], with full world access.
pub struct BlockBreakContext<'a> {
    world: &'a mut World,
    pub player: Entity,
    pub client_id: u32,
    /// The item that was in hand when the block broke.
    pub held: ItemStack,
    pub position: BlockPosition,
    /// The block-state id that was removed.
    pub broken_state: i32,
}

impl BlockBreakContext<'_> {
    /// Gives the player an item stack (e.g. a tool-specific drop) and resyncs.
    pub fn give(&mut self, stack: ItemStack) {
        if let Some(mut inv) = self.world.get_mut::<Inventory>(self.player) {
            inv.give(stack);
        }
    }

    /// Sends a chat message to the breaking player.
    pub fn reply(&self, message: &str) {
        crate::commands::send_system_chat(self.world, self.player, message, TextColor::White);
    }

    /// Read-only world access.
    pub fn with_world<R>(&self, f: impl FnOnce(&World) -> R) -> R {
        f(self.world)
    }

    /// Mutable world access.
    pub fn with_world_mut<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> R {
        f(self.world)
    }
}

/// Registry mapping items to their custom behaviours. Developers populate this
/// (e.g. in a startup plugin) via [`register`](Self::register) /
/// [`register_for`](Self::register_for).
#[derive(Resource, Default)]
pub struct ItemBehaviorRegistry {
    behaviors: HashMap<ItemId, Arc<dyn ItemBehavior>>,
}

impl ItemBehaviorRegistry {
    /// Registers a behaviour for an item id, replacing any earlier one.
    pub fn register(&mut self, item: ItemId, behavior: impl ItemBehavior) {
        self.behaviors.insert(item, Arc::new(behavior));
    }

    /// Registers a behaviour by item name, e.g. `"minecraft:stick"`, replacing
    /// any earlier one. Logs a warning and registers nothing for an unknown
    /// name.
    pub fn register_for(&mut self, name: &str, behavior: impl ItemBehavior) {
        match ItemId::from_name(name) {
            Some(item) => self.register(item, behavior),
            None => tracing::warn!("ItemBehaviorRegistry::register_for: unknown item '{name}'"),
        }
    }

    pub fn contains(&self, item: ItemId) -> bool {
        self.behaviors.contains_key(&item)
    }

    fn get(&self, item: ItemId) -> Option<Arc<dyn ItemBehavior>> {
        self.behaviors.get(&item).cloned()
    }
}

/// Implemented for blocks that react to a right-click, such as a lever or a
/// button (see [`BlockInteractionRegistry`]).
///
/// It runs for a main-hand right-click on the block after the held item's
/// [`ItemBehavior`] (if any) returned [`UseResult::Pass`], and before the
/// default block placement. Like vanilla, it is skipped for an off-hand use
/// and when the player is sneaking ([`Sneaking`]) with an item in either
/// hand. Returning [`UseResult::Handled`] consumes the click: nothing is
/// placed.
pub trait BlockInteraction: Send + Sync + 'static {
    /// The player right-clicked `state`, the block at `ctx.block.position`.
    fn on_use(&self, ctx: &mut ItemUseContext, state: i32) -> UseResult;
}

/// Registry mapping blocks (every state of a block type) to their
/// [`BlockInteraction`]. Empty by default: without a registration, a
/// right-click on a block uses the held item.
#[derive(Resource, Default)]
pub struct BlockInteractionRegistry {
    interactions: HashMap<&'static str, Arc<dyn BlockInteraction>>,
}

impl BlockInteractionRegistry {
    /// Registers an interaction for a block by name, e.g. `"minecraft:lever"`.
    /// Logs and ignores unknown names.
    pub fn register_for(&mut self, block: &str, interaction: impl BlockInteraction) {
        match voidmc_data::block_type(BLOCK_DATA_VERSION, block) {
            Some(block) => {
                self.interactions.insert(block.name, Arc::new(interaction));
            }
            None => {
                tracing::warn!("BlockInteractionRegistry::register_for: unknown block '{block}'")
            }
        }
    }

    /// Whether a block, by name, has an interaction.
    pub fn contains(&self, block: &str) -> bool {
        self.interactions.contains_key(block)
    }

    fn get(&self, state: i32) -> Option<Arc<dyn BlockInteraction>> {
        let block = voidmc_data::block_type_of_state(BLOCK_DATA_VERSION, state)?;
        self.interactions.get(block.name).cloned()
    }
}

const BLOCK_DATA_VERSION: voidmc_data::Version = voidmc_data::Version::V26_1_2;

/// Runs the clicked block's [`BlockInteraction`], if the vanilla rules let
/// the block react to this click.
fn interact_with_block(ctx: &mut ItemUseContext, target: BlockUseTarget, hand: Hand) -> UseResult {
    if hand != Hand::MainHand {
        return UseResult::Pass;
    }
    let player = ctx.player;
    // A player that has not sent any input yet has no `Sneaking`: not sneaking.
    let sneaking = ctx.world.get::<Sneaking>(player).is_some_and(|s| s.0);
    let secondary_use = sneaking
        && ctx.world.get::<Inventory>(player).is_some_and(|inventory| {
            !inventory.held().is_empty() || !inventory.get(Inventory::OFFHAND).is_empty()
        });
    if secondary_use {
        return UseResult::Pass;
    }
    let Some(dimension) = ctx.world.get::<PlayerDimension>(player).map(|d| d.0) else {
        return UseResult::Pass;
    };
    let Some(state) = block_state(ctx.world, dimension, target.position) else {
        return UseResult::Pass;
    };
    let Some(interaction) = ctx
        .world
        .get_resource::<BlockInteractionRegistry>()
        .and_then(|registry| registry.get(state))
    else {
        return UseResult::Pass;
    };
    interaction.on_use(ctx, state)
}

/// A queued player action needing exclusive world access.
enum UseAction {
    /// Right-click on a block.
    UseOnBlock(BlockUseTarget),
    /// Right-click in the air.
    UseInAir,
    /// Finished breaking a block (turns it to air).
    BreakBlock {
        position: BlockPosition,
        face: BlockFace,
    },
    /// Started mining a block that is only removed once mining finishes.
    StartBreak { position: BlockPosition },
    /// A click refused before anything handled it: resend these blocks.
    Refused { positions: Vec<BlockPosition> },
}

/// A queued action awaiting processing in the exclusive drain system.
struct QueuedUse {
    player: Entity,
    client_id: u32,
    hand: Hand,
    sequence: i32,
    action: UseAction,
    /// The slot and stack in `hand` when the packet arrived (the main hand for
    /// a break). Later packets of the same tick, such as a hotbar change, must
    /// not alter what this action uses.
    held: Option<(usize, ItemStack)>,
}

/// The slot and stack in `hand`: the selected hotbar slot or the off hand.
fn held_in(inventory: Option<&Inventory>, hand: Hand) -> Option<(usize, ItemStack)> {
    let inventory = inventory?;
    let slot = match hand {
        Hand::MainHand => Inventory::hotbar_slot_index(inventory.selected_hotbar()),
        Hand::OffHand => Inventory::OFFHAND,
    };
    Some((slot, inventory.get(slot).clone()))
}

/// Pending item/block actions, drained each tick with `&mut World`.
#[derive(Resource, Default)]
pub struct ItemUseQueue(VecDeque<QueuedUse>);

#[cfg(test)]
impl ItemUseQueue {
    pub(crate) fn pending_len(&self) -> usize {
        self.0.len()
    }
}

/// Enqueues a "use item on block" action with the item `inventory` holds in
/// `hand` now.
pub fn enqueue_use_on_block(
    queue: &mut ItemUseQueue,
    player: Entity,
    client_id: u32,
    hand: Hand,
    sequence: i32,
    target: BlockUseTarget,
    inventory: Option<&Inventory>,
) {
    queue.0.push_back(QueuedUse {
        player,
        client_id,
        hand,
        sequence,
        action: UseAction::UseOnBlock(target),
        held: held_in(inventory, hand),
    });
}

/// Enqueues a "use item in air" action with the item `inventory` holds in
/// `hand` now.
pub fn enqueue_use_in_air(
    queue: &mut ItemUseQueue,
    player: Entity,
    client_id: u32,
    hand: Hand,
    sequence: i32,
    inventory: Option<&Inventory>,
) {
    queue.0.push_back(QueuedUse {
        player,
        client_id,
        hand,
        sequence,
        action: UseAction::UseInAir,
        held: held_in(inventory, hand),
    });
}

/// Enqueues a block-break (dig-finished) action with the item `inventory`
/// holds in the main hand now.
pub fn enqueue_break(
    queue: &mut ItemUseQueue,
    player: Entity,
    client_id: u32,
    sequence: i32,
    position: BlockPosition,
    face: BlockFace,
    inventory: Option<&Inventory>,
) {
    queue.0.push_back(QueuedUse {
        player,
        client_id,
        hand: Hand::MainHand,
        sequence,
        action: UseAction::BreakBlock { position, face },
        held: held_in(inventory, Hand::MainHand),
    });
}

fn break_cancelled(
    world: &mut World,
    player: Entity,
    client_id: u32,
    held: &ItemStack,
    dimension: DimensionId,
    position: BlockPosition,
) -> bool {
    let held = held.clone();
    let Some(behavior) = world.resource::<ItemBehaviorRegistry>().get(held.item) else {
        return false;
    };
    let Some(current) = block_state(world, dimension, position) else {
        return false;
    };
    let mut ctx = BlockBreakContext {
        world,
        player,
        client_id,
        held,
        position,
        broken_state: current,
    };
    if behavior.before_break_block(&mut ctx) == UseResult::Pass {
        return false;
    }
    let state = block_state(world, dimension, position).unwrap_or(current);
    WorldPlayers::new(world).send(
        player,
        clientbound::BlockUpdate {
            position,
            block_state_id: state,
        },
    );
    true
}

fn block_state(world: &World, dimension: DimensionId, position: BlockPosition) -> Option<i32> {
    let chunk = ChunkPos::new(position.x.div_euclid(16), position.z.div_euclid(16));
    let entity = *world.resource::<ChunkIndex>().0.get(&(dimension, chunk))?;
    world.get::<ChunkData>(entity)?.get_block(
        position.x.rem_euclid(16) as u8,
        i32::from(position.y),
        position.z.rem_euclid(16) as u8,
    )
}

/// Enqueues the start of a survival dig, which only consults
/// [`ItemBehavior::before_break_block`], with the item `inventory` holds in the
/// main hand now.
pub fn enqueue_start_break(
    queue: &mut ItemUseQueue,
    player: Entity,
    client_id: u32,
    sequence: i32,
    position: BlockPosition,
    inventory: Option<&Inventory>,
) {
    queue.0.push_back(QueuedUse {
        player,
        client_id,
        hand: Hand::MainHand,
        sequence,
        action: UseAction::StartBreak { position },
        held: held_in(inventory, Hand::MainHand),
    });
}

/// Exclusive system: process each queued action — break, or dispatch a use to
/// its behaviour (or the built-in default) — then acknowledge the prediction
/// sequence exactly once.
pub fn drain_item_use_queue(world: &mut World) {
    while let Some(queued) = world.resource_mut::<ItemUseQueue>().0.pop_front() {
        if !world.entities().contains(queued.player) {
            continue;
        }

        let (held_slot, held) = match queued.held {
            Some((slot, stack)) => (Some(slot), stack),
            None => (None, ItemStack::EMPTY),
        };
        match queued.action {
            UseAction::Refused { positions } => {
                resync_blocks(world, queued.player, &positions);
                resend_hand(world, queued.player, held_slot, &held);
            }
            UseAction::StartBreak { position } => {
                if let Some(dimension) = world.get::<PlayerDimension>(queued.player).map(|d| d.0) {
                    break_cancelled(
                        world,
                        queued.player,
                        queued.client_id,
                        &held,
                        dimension,
                        position,
                    );
                }
            }
            UseAction::BreakBlock { position, face } => {
                if let Some(dimension) = world.get::<PlayerDimension>(queued.player).map(|d| d.0)
                    && !break_cancelled(
                        world,
                        queued.player,
                        queued.client_id,
                        &held,
                        dimension,
                        position,
                    )
                    && let Some(broken_state) = mutate_block(
                        world,
                        queued.player,
                        dimension,
                        position,
                        0,
                        face,
                        BlockMutation::Break,
                    )
                {
                    if let Some(behavior) = world.resource::<ItemBehaviorRegistry>().get(held.item)
                    {
                        let mut ctx = BlockBreakContext {
                            world,
                            player: queued.player,
                            client_id: queued.client_id,
                            held,
                            position,
                            broken_state,
                        };
                        behavior.on_break_block(&mut ctx);
                    }
                }
            }
            UseAction::UseOnBlock(_) | UseAction::UseInAir => {
                let block = match queued.action {
                    UseAction::UseOnBlock(target) => Some(target),
                    _ => None,
                };
                let behavior = world.resource::<ItemBehaviorRegistry>().get(held.item);

                let mut ctx = ItemUseContext {
                    world,
                    player: queued.player,
                    client_id: queued.client_id,
                    hand: queued.hand,
                    held: held.clone(),
                    block,
                    held_slot,
                };

                let result = match (&behavior, block.is_some()) {
                    (Some(b), true) => b.on_use_on_block(&mut ctx),
                    (Some(b), false) => b.on_use(&mut ctx),
                    (None, _) => UseResult::Pass,
                };

                // Then the clicked block's own interaction (a lever toggles).
                let result = match block {
                    Some(target) if result == UseResult::Pass => {
                        interact_with_block(&mut ctx, target, queued.hand)
                    }
                    _ => result,
                };

                // Default: a block item places its block when used on a block.
                // An empty hand places nothing (the air item maps to the air
                // block, which would erase what is there).
                if result == UseResult::Pass
                    && let Some(target) = block
                    && !held.is_empty()
                    && let Some(state) = held.item.default_block_state()
                {
                    ctx.place_held_block(target, state);
                }
            }
        }

        // Acknowledge the client's predicted sequence exactly once.
        send_ack(world, queued.player, queued.sequence);
        world.flush();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

    type Calls = Arc<Mutex<Vec<(&'static str, &'static str)>>>;

    /// Records which item's break hooks ran.
    struct Tag(&'static str, Calls);

    impl ItemBehavior for Tag {
        fn before_break_block(&self, _ctx: &mut BlockBreakContext) -> UseResult {
            self.1.lock().unwrap().push((self.0, "before"));
            UseResult::Pass
        }
        fn on_break_block(&self, _ctx: &mut BlockBreakContext) -> UseResult {
            self.1.lock().unwrap().push((self.0, "after"));
            UseResult::Pass
        }
    }

    const POSITION: BlockPosition = BlockPosition { x: 1, y: 64, z: 1 };

    /// A world with one stone block and a player holding a stick in the
    /// selected first hotbar slot and a blaze rod in the second.
    fn world() -> (World, Entity, Calls, flume::Receiver<OutgoingPacket>) {
        let mut world = World::new();
        let (_incoming_tx, incoming) = flume::unbounded::<IncomingPacket>();
        let (outgoing, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect) = flume::unbounded::<u32>();
        let (kick, _kick_rx) = flume::unbounded::<u32>();
        world.insert_resource(NetworkChannels {
            incoming,
            outgoing,
            disconnect,
            kick,
        });
        world.init_resource::<ItemUseQueue>();

        let calls = Calls::default();
        let mut registry = ItemBehaviorRegistry::default();
        registry.register_for("minecraft:stick", Tag("stick", calls.clone()));
        registry.register_for("minecraft:blaze_rod", Tag("blaze_rod", calls.clone()));
        world.insert_resource(registry);

        let mut chunk = ChunkData::new(
            (0..24).map(|_| ChunkSection::empty()).collect(),
            ChunkHeightmaps::empty(),
            LightData::empty(),
        );
        chunk.set_block(1, 64, 1, 1);
        let chunk = world.spawn(chunk).id();
        let mut index = ChunkIndex::default();
        index
            .0
            .insert((DimensionId::Overworld, ChunkPos::new(0, 0)), chunk);
        world.insert_resource(index);

        let mut inventory = Inventory::new();
        inventory.set(
            Inventory::hotbar_slot_index(0),
            ItemStack::of("minecraft:stick", 1).unwrap(),
        );
        inventory.set(
            Inventory::hotbar_slot_index(1),
            ItemStack::of("minecraft:blaze_rod", 1).unwrap(),
        );
        let player = world
            .spawn((
                ClientId(1),
                PlayerDimension(DimensionId::Overworld),
                LoadedChunks(HashSet::from([ChunkPos::new(0, 0)])),
                PlayerReady,
                inventory,
            ))
            .id();
        (world, player, calls, outgoing_rx)
    }

    /// Queues an action with the player's inventory as it is, then selects the
    /// second hotbar slot before the queue drains, as a hotbar change arriving
    /// later in the same tick does.
    fn queue_then_switch(
        world: &mut World,
        player: Entity,
        enqueue: impl FnOnce(&mut ItemUseQueue, Option<&Inventory>),
    ) {
        let inventory = world.get::<Inventory>(player).unwrap().clone();
        enqueue(&mut world.resource_mut::<ItemUseQueue>(), Some(&inventory));
        world
            .get_mut::<Inventory>(player)
            .unwrap()
            .set_selected_hotbar(1);
        drain_item_use_queue(world);
    }

    #[test]
    fn break_hooks_run_for_the_item_held_when_the_break_arrived() {
        let (mut world, player, calls, _outgoing) = world();
        queue_then_switch(&mut world, player, |queue, inventory| {
            enqueue_break(queue, player, 1, 7, POSITION, BlockFace::Top, inventory);
        });

        assert_eq!(
            block_state(&world, DimensionId::Overworld, POSITION),
            Some(0)
        );
        assert_eq!(
            *calls.lock().unwrap(),
            [("stick", "before"), ("stick", "after")]
        );
    }

    #[test]
    fn a_survival_dig_start_consults_the_item_held_when_it_arrived() {
        let (mut world, player, calls, _outgoing) = world();
        queue_then_switch(&mut world, player, |queue, inventory| {
            enqueue_start_break(queue, player, 1, 7, POSITION, inventory);
        });

        assert_eq!(
            block_state(&world, DimensionId::Overworld, POSITION),
            Some(1)
        );
        assert_eq!(*calls.lock().unwrap(), [("stick", "before")]);
    }
}
