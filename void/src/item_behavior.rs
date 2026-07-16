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

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc_protocol::types::{BlockFace, BlockPosition, Hand};

use crate::components::PlayerDimension;
use crate::inventory::Inventory;
use crate::item::{ItemId, ItemStack};
use crate::plugins::inventory::InventoryDirty;
use crate::world::mutation::send_ack;
use crate::world::{BlockMutation, mutate_block, offset_position};

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
    /// Snapshot of the item in hand when the action was queued.
    pub held: ItemStack,
    /// The targeted block, or `None` for a use-in-air.
    pub block: Option<BlockUseTarget>,
}

impl ItemUseContext<'_> {
    /// Places `state_id` adjacent to the clicked block face. No-op for use-in-air.
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

    /// Removes `n` from the held stack (server-authoritative) and resyncs.
    pub fn consume(&mut self, n: u8) {
        if let Some(mut inv) = self.world.get_mut::<Inventory>(self.player) {
            let idx = Inventory::hotbar_slot_index(inv.selected_hotbar());
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
        self.mark_inventory_dirty();
    }

    /// Gives the player an item stack (stacking) and resyncs.
    pub fn give(&mut self, stack: ItemStack) {
        if let Some(mut inv) = self.world.get_mut::<Inventory>(self.player) {
            inv.give(stack);
        }
        self.mark_inventory_dirty();
    }

    /// Sends a chat message to the acting player.
    pub fn reply(&self, message: &str) {
        crate::commands::send_system_chat(self.world, self.client_id, message, "white");
    }

    /// Read-only world access for advanced behaviours.
    pub fn with_world<R>(&self, f: impl FnOnce(&World) -> R) -> R {
        f(self.world)
    }

    /// Mutable world access for advanced behaviours.
    pub fn with_world_mut<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> R {
        f(self.world)
    }

    fn mark_inventory_dirty(&mut self) {
        self.world.entity_mut(self.player).insert(InventoryDirty);
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
    /// Right-click while not pointing at a block.
    fn on_use(&self, _ctx: &mut ItemUseContext) -> UseResult {
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
        self.world.entity_mut(self.player).insert(InventoryDirty);
    }

    /// Sends a chat message to the breaking player.
    pub fn reply(&self, message: &str) {
        crate::commands::send_system_chat(self.world, self.client_id, message, "white");
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
    /// Registers a behaviour for an item id.
    pub fn register(&mut self, item: ItemId, behavior: impl ItemBehavior) {
        self.behaviors.insert(item, Arc::new(behavior));
    }

    /// Registers a behaviour by item name, e.g. `"minecraft:stick"`. Logs and
    /// ignores unknown names.
    pub fn register_for(&mut self, name: &str, behavior: impl ItemBehavior) {
        match ItemId::from_name(name) {
            Some(item) => self.register(item, behavior),
            None => tracing::warn!("ItemBehaviorRegistry::register_for: unknown item '{name}'"),
        }
    }

    fn get(&self, item: ItemId) -> Option<Arc<dyn ItemBehavior>> {
        self.behaviors.get(&item).cloned()
    }
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
}

/// A queued action awaiting processing in the exclusive drain system.
struct QueuedUse {
    player: Entity,
    client_id: u32,
    hand: Hand,
    sequence: i32,
    action: UseAction,
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

/// Enqueues a "use item on block" action.
pub fn enqueue_use_on_block(
    queue: &mut ItemUseQueue,
    player: Entity,
    client_id: u32,
    hand: Hand,
    sequence: i32,
    target: BlockUseTarget,
) {
    queue.0.push_back(QueuedUse {
        player,
        client_id,
        hand,
        sequence,
        action: UseAction::UseOnBlock(target),
    });
}

/// Enqueues a "use item in air" action.
pub fn enqueue_use_in_air(
    queue: &mut ItemUseQueue,
    player: Entity,
    client_id: u32,
    hand: Hand,
    sequence: i32,
) {
    queue.0.push_back(QueuedUse {
        player,
        client_id,
        hand,
        sequence,
        action: UseAction::UseInAir,
    });
}

/// Enqueues a block-break (dig-finished) action.
pub fn enqueue_break(
    queue: &mut ItemUseQueue,
    player: Entity,
    client_id: u32,
    sequence: i32,
    position: BlockPosition,
    face: BlockFace,
) {
    queue.0.push_back(QueuedUse {
        player,
        client_id,
        hand: Hand::MainHand,
        sequence,
        action: UseAction::BreakBlock { position, face },
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

        match queued.action {
            UseAction::BreakBlock { position, face } => {
                if let Some(dimension) = world.get::<PlayerDimension>(queued.player).map(|d| d.0)
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
                    let held = world
                        .get::<Inventory>(queued.player)
                        .map(|inv| inv.held().clone())
                        .unwrap_or(ItemStack::EMPTY);
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
                let held = world
                    .get::<Inventory>(queued.player)
                    .map(|inv| inv.held().clone())
                    .unwrap_or(ItemStack::EMPTY);
                let behavior = world.resource::<ItemBehaviorRegistry>().get(held.item);

                let mut ctx = ItemUseContext {
                    world,
                    player: queued.player,
                    client_id: queued.client_id,
                    hand: queued.hand,
                    held: held.clone(),
                    block,
                };

                let result = match (&behavior, block.is_some()) {
                    (Some(b), true) => b.on_use_on_block(&mut ctx),
                    (Some(b), false) => b.on_use(&mut ctx),
                    (None, _) => UseResult::Pass,
                };

                // Default: a block item places its block when used on a block.
                if result == UseResult::Pass
                    && block.is_some()
                    && let Some(state) = held.item.default_block_state()
                {
                    ctx.place_block(state);
                }
            }
        }

        // Acknowledge the client's predicted sequence exactly once.
        send_ack(world, queued.player, queued.sequence);
        world.flush();
    }
}
