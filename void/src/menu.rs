//! Server-side GUI menus: build a [`Menu`], open it for a player with
//! [`Menus::open`], react to clicks through [`MenuClickEvent`] observers or
//! the menu's own `on_click` handler. A menu is read-only unless marked
//! `editable`: every click is answered by restoring the true contents.

use std::collections::VecDeque;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use ussr_nbt::owned::Nbt;
use voidmc_data::Version;
pub use voidmc_protocol::serverbound::ContainerInput;

use crate::item::ItemStack;
use crate::messages::TextColor;
use crate::players::WorldPlayers;
use crate::window::{DragState, Layout};

const VERSION: Version = Version::V26_1_2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuType {
    Generic9x1,
    Generic9x2,
    Generic9x3,
    Generic9x4,
    Generic9x5,
    Generic9x6,
    Generic3x3,
    Crafter3x3,
    Anvil,
    Beacon,
    BlastFurnace,
    BrewingStand,
    Crafting,
    Enchantment,
    Furnace,
    Grindstone,
    Hopper,
    Lectern,
    Loom,
    Merchant,
    ShulkerBox,
    Smithing,
    Smoker,
    CartographyTable,
    Stonecutter,
}

impl MenuType {
    pub const ALL: [MenuType; 25] = [
        MenuType::Generic9x1,
        MenuType::Generic9x2,
        MenuType::Generic9x3,
        MenuType::Generic9x4,
        MenuType::Generic9x5,
        MenuType::Generic9x6,
        MenuType::Generic3x3,
        MenuType::Crafter3x3,
        MenuType::Anvil,
        MenuType::Beacon,
        MenuType::BlastFurnace,
        MenuType::BrewingStand,
        MenuType::Crafting,
        MenuType::Enchantment,
        MenuType::Furnace,
        MenuType::Grindstone,
        MenuType::Hopper,
        MenuType::Lectern,
        MenuType::Loom,
        MenuType::Merchant,
        MenuType::ShulkerBox,
        MenuType::Smithing,
        MenuType::Smoker,
        MenuType::CartographyTable,
        MenuType::Stonecutter,
    ];

    pub fn chest(rows: u8) -> Option<MenuType> {
        Some(match rows {
            1 => MenuType::Generic9x1,
            2 => MenuType::Generic9x2,
            3 => MenuType::Generic9x3,
            4 => MenuType::Generic9x4,
            5 => MenuType::Generic9x5,
            6 => MenuType::Generic9x6,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            MenuType::Generic9x1 => "minecraft:generic_9x1",
            MenuType::Generic9x2 => "minecraft:generic_9x2",
            MenuType::Generic9x3 => "minecraft:generic_9x3",
            MenuType::Generic9x4 => "minecraft:generic_9x4",
            MenuType::Generic9x5 => "minecraft:generic_9x5",
            MenuType::Generic9x6 => "minecraft:generic_9x6",
            MenuType::Generic3x3 => "minecraft:generic_3x3",
            MenuType::Crafter3x3 => "minecraft:crafter_3x3",
            MenuType::Anvil => "minecraft:anvil",
            MenuType::Beacon => "minecraft:beacon",
            MenuType::BlastFurnace => "minecraft:blast_furnace",
            MenuType::BrewingStand => "minecraft:brewing_stand",
            MenuType::Crafting => "minecraft:crafting",
            MenuType::Enchantment => "minecraft:enchantment",
            MenuType::Furnace => "minecraft:furnace",
            MenuType::Grindstone => "minecraft:grindstone",
            MenuType::Hopper => "minecraft:hopper",
            MenuType::Lectern => "minecraft:lectern",
            MenuType::Loom => "minecraft:loom",
            MenuType::Merchant => "minecraft:merchant",
            MenuType::ShulkerBox => "minecraft:shulker_box",
            MenuType::Smithing => "minecraft:smithing",
            MenuType::Smoker => "minecraft:smoker",
            MenuType::CartographyTable => "minecraft:cartography_table",
            MenuType::Stonecutter => "minecraft:stonecutter",
        }
    }

    /// Slots owned by the menu itself; the player's main inventory and hotbar
    /// follow them in the window (except for the lectern, which shows none).
    pub fn slot_count(self) -> usize {
        match self {
            MenuType::Generic9x1 => 9,
            MenuType::Generic9x2 => 18,
            MenuType::Generic9x3 | MenuType::ShulkerBox => 27,
            MenuType::Generic9x4 => 36,
            MenuType::Generic9x5 => 45,
            MenuType::Generic9x6 => 54,
            MenuType::Generic3x3 | MenuType::Crafter3x3 => 9,
            MenuType::Anvil
            | MenuType::BlastFurnace
            | MenuType::Furnace
            | MenuType::Smoker
            | MenuType::Grindstone
            | MenuType::Merchant
            | MenuType::CartographyTable => 3,
            MenuType::Beacon | MenuType::Lectern => 1,
            MenuType::BrewingStand | MenuType::Hopper => 5,
            MenuType::Crafting => 10,
            MenuType::Enchantment | MenuType::Stonecutter => 2,
            MenuType::Loom | MenuType::Smithing => 4,
        }
    }

    pub fn shows_player_inventory(self) -> bool {
        self != MenuType::Lectern
    }

    /// The `minecraft:menu` registry id sent in Open Screen.
    pub fn registry_id(self) -> i32 {
        voidmc_data::menu_id(VERSION, self.name()).unwrap_or_else(|| {
            panic!(
                "{} is missing from the minecraft:menu registry",
                self.name()
            )
        })
    }

    pub(crate) fn layout(self) -> Layout {
        Layout::menu(self.slot_count(), self.shows_player_inventory())
    }
}

pub type MenuClickHandler = Arc<dyn Fn(&mut MenuClickContext) + Send + Sync>;

#[derive(Clone)]
pub struct Menu {
    kind: MenuType,
    title: String,
    slots: Vec<ItemStack>,
    editable: bool,
    handler: Option<MenuClickHandler>,
    dirty_slots: u64,
    title_dirty: bool,
}

impl fmt::Debug for Menu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Menu")
            .field("kind", &self.kind)
            .field("title", &self.title)
            .field("editable", &self.editable)
            .field("has_handler", &self.handler.is_some())
            .finish_non_exhaustive()
    }
}

impl Menu {
    pub fn new(kind: MenuType, title: impl Into<String>) -> Self {
        Menu {
            kind,
            title: title.into(),
            slots: vec![ItemStack::EMPTY; kind.slot_count()],
            editable: false,
            handler: None,
            dirty_slots: 0,
            title_dirty: false,
        }
    }

    /// A `rows`-row chest menu; rows are clamped to 1..=6.
    pub fn chest(rows: u8, title: impl Into<String>) -> Self {
        Menu::new(MenuType::chest(rows.clamp(1, 6)).unwrap(), title)
    }

    pub fn hopper(title: impl Into<String>) -> Self {
        Menu::new(MenuType::Hopper, title)
    }

    pub fn dispenser(title: impl Into<String>) -> Self {
        Menu::new(MenuType::Generic3x3, title)
    }

    /// Builder form of [`Menu::set_slot`]; out-of-range indices are ignored.
    pub fn slot(mut self, index: usize, stack: ItemStack) -> Self {
        self.set_slot(index, stack);
        self
    }

    pub fn fill(mut self, stack: ItemStack) -> Self {
        for i in 0..self.slots.len() {
            self.set_slot(i, stack.clone());
        }
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&mut MenuClickContext) + Send + Sync + 'static,
    ) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Lets the player move items between the menu and their inventory.
    pub fn editable(mut self) -> Self {
        self.editable = true;
        self
    }

    pub fn kind(&self) -> MenuType {
        self.kind
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        let title = title.into();
        if self.title != title {
            self.title = title;
            self.title_dirty = true;
        }
    }

    pub fn is_editable(&self) -> bool {
        self.editable
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn slots(&self) -> &[ItemStack] {
        &self.slots
    }

    /// Out-of-range indices read as empty.
    pub fn get(&self, index: usize) -> &ItemStack {
        static EMPTY: ItemStack = ItemStack::EMPTY;
        self.slots.get(index).unwrap_or(&EMPTY)
    }

    /// Out-of-range indices are ignored; an unchanged value sends nothing.
    pub fn set_slot(&mut self, index: usize, stack: ItemStack) {
        if let Some(slot) = self.slots.get_mut(index)
            && *slot != stack
        {
            *slot = stack;
            self.dirty_slots |= 1 << index;
        }
    }

    pub(crate) fn title_nbt(&self) -> Nbt {
        crate::messages::plain_text_component(&self.title)
    }

    pub(crate) fn layout(&self) -> Layout {
        self.kind.layout()
    }

    pub(crate) fn handler(&self) -> Option<MenuClickHandler> {
        self.handler.clone()
    }

    pub(crate) fn mark_slot(&mut self, index: usize) {
        self.dirty_slots |= 1 << index;
    }

    pub(crate) fn take_dirty(&mut self) -> (u64, bool) {
        (
            std::mem::take(&mut self.dirty_slots),
            std::mem::take(&mut self.title_dirty),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCloseReason {
    /// The player pressed escape or clicked outside the window.
    Client,
    /// `Menus::close` or removing the `OpenMenu` component.
    Server,
    /// Another menu was opened over this one.
    Replaced,
    Disconnect,
}

/// Per-player counter for menu container ids; window 0 is never handed out.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct ContainerIds {
    last: i32,
}

impl ContainerIds {
    pub fn allocate(&mut self) -> i32 {
        self.last = self.last % 100 + 1;
        self.last
    }
}

/// The menu a player currently has open. Mutate the menu through `Deref`;
/// the inventory plugin sends the matching slot updates each tick.
#[derive(Component)]
pub struct OpenMenu {
    menu: Menu,
    pub(crate) container_id: Option<i32>,
    pub(crate) state_id: i32,
    pub(crate) drag: Option<DragState>,
    pub(crate) full_resync: bool,
    pub(crate) closing: Option<MenuCloseReason>,
}

impl OpenMenu {
    pub fn new(menu: Menu) -> Self {
        OpenMenu {
            menu,
            container_id: None,
            state_id: 0,
            drag: None,
            full_resync: true,
            closing: None,
        }
    }

    /// `None` until the Open Screen packet has been sent (the tick after insertion).
    pub fn container_id(&self) -> Option<i32> {
        self.container_id
    }

    pub fn menu(&self) -> &Menu {
        &self.menu
    }

    pub fn menu_mut(&mut self) -> &mut Menu {
        &mut self.menu
    }

    pub(crate) fn next_state_id(&mut self) -> i32 {
        self.state_id = (self.state_id + 1) & 32767;
        self.state_id
    }
}

impl Deref for OpenMenu {
    type Target = Menu;

    fn deref(&self) -> &Menu {
        &self.menu
    }
}

impl DerefMut for OpenMenu {
    fn deref_mut(&mut self) -> &mut Menu {
        &mut self.menu
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuSlot {
    Menu(usize),
    /// A player inventory index (9..=44), as `Inventory` numbers it.
    Inventory(usize),
    Outside,
}

impl MenuSlot {
    pub(crate) fn from_window(layout: &Layout, window_slot: i16) -> Option<MenuSlot> {
        if window_slot == voidmc_protocol::serverbound::ClickContainer::SLOT_OUTSIDE {
            return Some(MenuSlot::Outside);
        }
        let index = usize::try_from(window_slot).ok()?;
        if layout.container.contains(&index) {
            Some(MenuSlot::Menu(index))
        } else if (layout.main.start..layout.hotbar.end).contains(&index) {
            Some(MenuSlot::Inventory(
                crate::inventory::Inventory::MAIN_START + index - layout.main.start,
            ))
        } else {
            None
        }
    }

    pub fn menu_index(self) -> Option<usize> {
        match self {
            MenuSlot::Menu(index) => Some(index),
            _ => None,
        }
    }
}

#[derive(Event, Debug, Clone)]
pub struct MenuClickEvent {
    pub player: Entity,
    pub container_id: i32,
    pub slot: MenuSlot,
    pub button: i8,
    pub input: ContainerInput,
    /// What the clicked slot held before the click.
    pub item: ItemStack,
}

#[derive(Event, Debug, Clone)]
pub struct MenuClosedEvent {
    pub player: Entity,
    pub container_id: i32,
    pub reason: MenuCloseReason,
}

#[derive(Resource, Default)]
pub(crate) struct MenuClickQueue(pub VecDeque<MenuClickEvent>);

pub struct MenuClickContext<'a> {
    world: &'a mut World,
    pub player: Entity,
    pub container_id: i32,
    pub slot: MenuSlot,
    pub button: i8,
    pub input: ContainerInput,
    pub item: ItemStack,
}

impl<'a> MenuClickContext<'a> {
    pub(crate) fn new(world: &'a mut World, click: &MenuClickEvent) -> Self {
        MenuClickContext {
            world,
            player: click.player,
            container_id: click.container_id,
            slot: click.slot,
            button: click.button,
            input: click.input,
            item: click.item.clone(),
        }
    }

    pub fn is_left_click(&self) -> bool {
        self.input == ContainerInput::Pickup && self.button == 0
    }

    pub fn is_right_click(&self) -> bool {
        self.input == ContainerInput::Pickup && self.button == 1
    }

    pub fn reply(&self, message: &str) {
        crate::commands::send_system_chat(self.world, self.player, message, TextColor::White);
    }

    pub fn players(&self) -> WorldPlayers<'_> {
        WorldPlayers::new(self.world)
    }

    pub fn menu(&self) -> Option<&Menu> {
        self.world.get::<OpenMenu>(self.player).map(OpenMenu::menu)
    }

    pub fn menu_mut(&mut self) -> Option<Mut<'_, OpenMenu>> {
        self.world.get_mut::<OpenMenu>(self.player)
    }

    pub fn close(&mut self) {
        WorldMenus::new(self.world).close(self.player);
    }

    pub fn open(&mut self, menu: Menu) {
        WorldMenus::new(self.world).open(self.player, menu);
    }

    pub fn with_world<R>(&self, f: impl FnOnce(&World) -> R) -> R {
        f(self.world)
    }

    pub fn with_world_mut<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> R {
        f(self.world)
    }
}

fn open_in_world(world: &mut World, player: Entity, menu: Menu) -> bool {
    let Ok(mut entity) = world.get_entity_mut(player) else {
        tracing::warn!(?player, "Cannot open menu: entity no longer exists");
        return false;
    };
    if entity.get::<crate::inventory::Inventory>().is_none() {
        tracing::warn!(?player, "Cannot open menu: entity has no Inventory");
        return false;
    }
    if let Some(mut open) = entity.get_mut::<OpenMenu>() {
        open.closing = Some(MenuCloseReason::Replaced);
        entity.remove::<OpenMenu>();
    }
    entity.insert(OpenMenu::new(menu));
    true
}

fn close_in_world(world: &mut World, player: Entity) -> bool {
    let Ok(mut entity) = world.get_entity_mut(player) else {
        return false;
    };
    let Some(mut open) = entity.get_mut::<OpenMenu>() else {
        return false;
    };
    open.closing = Some(MenuCloseReason::Server);
    entity.remove::<OpenMenu>();
    true
}

#[derive(SystemParam)]
pub struct Menus<'w, 's> {
    commands: Commands<'w, 's>,
    open: Query<'w, 's, &'static OpenMenu>,
}

impl Menus<'_, '_> {
    /// Opens `menu` for `player` at the end of the current stage, replacing
    /// any menu already open.
    pub fn open(&mut self, player: Entity, menu: Menu) {
        self.commands.queue(move |world: &mut World| {
            open_in_world(world, player, menu);
        });
    }

    pub fn close(&mut self, player: Entity) {
        self.commands.queue(move |world: &mut World| {
            close_in_world(world, player);
        });
    }

    pub fn is_open(&self, player: Entity) -> bool {
        self.open.contains(player)
    }

    pub fn current(&self, player: Entity) -> Option<&Menu> {
        self.open.get(player).ok().map(OpenMenu::menu)
    }
}

pub struct WorldMenus<'w> {
    world: &'w mut World,
}

impl<'w> WorldMenus<'w> {
    pub fn new(world: &'w mut World) -> Self {
        Self { world }
    }

    /// Opens `menu` immediately; returns `false` if `player` has no inventory.
    pub fn open(&mut self, player: Entity, menu: Menu) -> bool {
        open_in_world(self.world, player, menu)
    }

    pub fn close(&mut self, player: Entity) -> bool {
        close_in_world(self.world, player)
    }

    pub fn is_open(&self, player: Entity) -> bool {
        self.world.get::<OpenMenu>(player).is_some()
    }

    pub fn current(&self, player: Entity) -> Option<&Menu> {
        self.world.get::<OpenMenu>(player).map(OpenMenu::menu)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn oversized_title_round_trips_below_the_nbt_limit() {
        let text = "😀".repeat(11000);
        crate::messages::assert_guarded(&Menu::hopper(text.clone()).title_nbt(), &text);
    }

    #[test]
    fn every_menu_type_resolves_in_the_registry() {
        let mut ids: Vec<i32> = MenuType::ALL
            .iter()
            .map(|kind| kind.registry_id())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MenuType::ALL.len());
        assert_eq!(MenuType::Generic9x1.registry_id(), 0);
        assert_eq!(MenuType::Hopper.registry_id(), 16);
    }

    #[test]
    fn chest_rows_map_to_generic_types() {
        assert_eq!(MenuType::chest(3), Some(MenuType::Generic9x3));
        assert_eq!(MenuType::chest(0), None);
        assert_eq!(MenuType::chest(7), None);
        assert_eq!(Menu::chest(9, "x").kind(), MenuType::Generic9x6);
        assert_eq!(Menu::chest(0, "x").kind(), MenuType::Generic9x1);
        assert_eq!(Menu::chest(2, "x").slot_count(), 18);
    }

    #[test]
    fn builder_ignores_out_of_range_and_tracks_changes() {
        let stone = ItemStack::of("minecraft:stone", 1).unwrap();
        let mut menu = Menu::hopper("Hop")
            .slot(1, stone.clone())
            .slot(99, stone.clone())
            .slot(1, stone.clone());
        assert_eq!(menu.get(1), &stone);
        assert!(menu.get(99).is_empty());
        assert_eq!(menu.take_dirty(), (1 << 1, false));
        assert_eq!(menu.take_dirty(), (0, false));

        menu.set_title("Hop");
        menu.set_title("Hop 2");
        assert_eq!(menu.take_dirty(), (0, true));
        assert!(!menu.is_editable());
        assert!(menu.clone().editable().is_editable());
        assert_eq!(Menu::dispenser("d").fill(stone).slot_count(), 9);
    }

    #[test]
    fn container_ids_cycle_and_skip_zero() {
        let mut ids = ContainerIds::default();
        let first: Vec<i32> = (0..100).map(|_| ids.allocate()).collect();
        assert_eq!(first[0], 1);
        assert_eq!(first[99], 100);
        assert!(first.iter().all(|id| *id != 0));
        assert_eq!(ids.allocate(), 1);
    }

    #[test]
    fn menu_slot_classifies_window_indices() {
        let layout = MenuType::Generic9x1.layout();
        assert_eq!(MenuSlot::from_window(&layout, 4), Some(MenuSlot::Menu(4)));
        assert_eq!(
            MenuSlot::from_window(&layout, 9),
            Some(MenuSlot::Inventory(9))
        );
        assert_eq!(
            MenuSlot::from_window(&layout, 44),
            Some(MenuSlot::Inventory(44))
        );
        assert_eq!(MenuSlot::from_window(&layout, 45), None);
        assert_eq!(
            MenuSlot::from_window(&layout, -999),
            Some(MenuSlot::Outside)
        );
        assert_eq!(MenuSlot::from_window(&layout, -1), None);
        let lectern = MenuType::Lectern.layout();
        assert_eq!(MenuSlot::from_window(&lectern, 1), None);
    }
}
