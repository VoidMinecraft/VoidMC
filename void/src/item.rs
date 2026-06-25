//! Domain item types: [`ItemId`] and [`ItemStack`].
//!
//! These are the ergonomic, ECS/developer-facing counterparts to the protocol
//! [`Slot`](voidmc_protocol::slot::Slot) wire type. [`ItemStack::to_slot`] and
//! [`ItemStack::from_slot`] are the only points where the two representations
//! cross; everything else in the server works with [`ItemStack`].

use voidmc_data::Version;
use voidmc_protocol::slot::{DataComponent, Slot};

/// The data version the server's item/registry tables are generated from.
const VERSION: Version = Version::V26_1_2;

/// A registry id from `minecraft:item`, wrapping its protocol id.
///
/// Construct from a namespaced name with [`ItemId::from_name`]; the bare protocol
/// id is also available as the public field for hot paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemId(pub i32);

impl ItemId {
    /// `minecraft:air` (protocol id 0) — the canonical "no item" id.
    pub const AIR: ItemId = ItemId(0);

    /// Resolves a name like `"minecraft:stone"` (or the unqualified `"stone"`) to
    /// its protocol id, or `None` if it is not a known item.
    pub fn from_name(name: &str) -> Option<ItemId> {
        let owned;
        let full = if name.contains(':') {
            name
        } else {
            owned = format!("minecraft:{name}");
            &owned
        };
        voidmc_data::item_id(VERSION, full).map(ItemId)
    }

    /// The namespaced name (e.g. `"minecraft:stone"`), or `None` if unknown.
    pub fn name(self) -> Option<&'static str> {
        voidmc_data::item_name(VERSION, self.0)
    }

    /// The default block-state id this item places, or `None` if it is not a
    /// placeable block item.
    pub fn default_block_state(self) -> Option<i32> {
        voidmc_data::item_default_block_state(VERSION, self.0)
    }

    /// Whether this item places a block when used on one.
    pub fn is_block_item(self) -> bool {
        self.default_block_state().is_some()
    }
}

/// A stack of items: an [`ItemId`], a count, and any data components.
///
/// An empty stack has `count == 0` (and conventionally [`ItemId::AIR`]); use
/// [`ItemStack::is_empty`] rather than testing the fields directly.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStack {
    pub item: ItemId,
    pub count: u8,
    pub components: Vec<DataComponent>,
}

impl ItemStack {
    /// The empty stack.
    pub const EMPTY: ItemStack = ItemStack {
        item: ItemId::AIR,
        count: 0,
        components: Vec::new(),
    };

    /// A stack of `count` of `item` with no components.
    pub fn new(item: ItemId, count: u8) -> Self {
        ItemStack {
            item,
            count,
            components: Vec::new(),
        }
    }

    /// A stack built from an item name, or `None` if the name is unknown.
    pub fn of(name: &str, count: u8) -> Option<Self> {
        Some(ItemStack::new(ItemId::from_name(name)?, count))
    }

    /// Whether the stack holds nothing.
    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.item == ItemId::AIR
    }

    /// Adds an arbitrary data component (builder style).
    pub fn with_component(mut self, component: DataComponent) -> Self {
        self.components.push(component);
        self
    }

    /// Overrides the stack's maximum stack size (builder style).
    pub fn with_max_stack_size(self, max: i32) -> Self {
        self.with_component(DataComponent::MaxStackSize(max))
    }

    /// Sets the item's durability damage (builder style).
    pub fn with_damage(self, damage: i32) -> Self {
        self.with_component(DataComponent::Damage(damage))
    }

    /// Converts this stack to its protocol [`Slot`] wire form.
    pub fn to_slot(&self) -> Slot {
        if self.is_empty() {
            return Slot::EMPTY;
        }
        Slot {
            count: self.count as i32,
            item_id: self.item.0,
            components_to_add: self.components.clone(),
            components_to_remove: Vec::new(),
        }
    }

    /// Builds a stack from a decoded protocol [`Slot`].
    pub fn from_slot(slot: &Slot) -> Self {
        if slot.is_empty() {
            return ItemStack::EMPTY;
        }
        ItemStack {
            item: ItemId(slot.item_id),
            count: slot.count.clamp(0, u8::MAX as i32) as u8,
            components: slot.components_to_add.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_id_name_roundtrip() {
        let stone = ItemId::from_name("minecraft:stone").unwrap();
        assert_eq!(stone.name(), Some("minecraft:stone"));
        // Unqualified names default to the minecraft namespace.
        assert_eq!(ItemId::from_name("stone"), Some(stone));
        assert_eq!(ItemId::from_name("definitely_not_an_item"), None);
    }

    #[test]
    fn block_items_report_their_state() {
        let stone = ItemId::from_name("minecraft:stone").unwrap();
        assert!(stone.is_block_item());
        assert_eq!(
            stone.default_block_state(),
            Some(voidmc_data::v26_1_2::blocks::STONE)
        );

        let sword = ItemId::from_name("minecraft:diamond_sword").unwrap();
        assert!(!sword.is_block_item());
    }

    #[test]
    fn stack_slot_roundtrip() {
        let stack = ItemStack::of("minecraft:diamond_block", 5).unwrap();
        let slot = stack.to_slot();
        assert_eq!(slot.count, 5);
        assert_eq!(slot.item_id, stack.item.0);
        assert_eq!(ItemStack::from_slot(&slot), stack);
    }

    #[test]
    fn empty_stack_maps_to_empty_slot() {
        assert!(ItemStack::EMPTY.to_slot().is_empty());
        assert!(ItemStack::from_slot(&Slot::EMPTY).is_empty());
    }
}
