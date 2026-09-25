//! Keeps each client's container windows in sync: the player inventory
//! (window 0) and whatever [`OpenMenu`] the player has. Setters record what
//! changed; `sync_containers` turns that into Set Container Slot / Content,
//! Set Cursor Item and Set Held Slot once per tick. Clicks are applied
//! server-side (or cancelled for read-only menus) and answered the same way.

use bevy_app::{App, Plugin, PostUpdate, Update};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound;
use voidmc_protocol::serverbound::{ClickContainer, CloseContainer};
use voidmc_protocol::slot::Slot;

use crate::config::ServerConfigResource;
use crate::events::{ItemDropEvent, PlayerChangeSlotEvent, PlayerQuitEvent, PlayerReadyEvent};
use crate::inventory::Inventory;
use crate::item::ItemStack;
use crate::menu::{
    ContainerIds, MenuClickContext, MenuClickEvent, MenuClickQueue, MenuCloseReason,
    MenuClosedEvent, MenuSlot, OpenMenu,
};
use crate::network::PacketEvent;
use crate::players::Players;
use crate::schedule::VoidSystems;
use crate::window::{Layout, Window};

/// Window id of the player's own inventory.
pub const PLAYER_WINDOW_ID: i32 = 0;

const CREATIVE_GAME_MODE: u8 = 1;

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuClickQueue>()
            .add_observer(sync_inventory_on_ready)
            .add_observer(track_selected_slot)
            .add_observer(handle_container_click)
            .add_observer(handle_close_container)
            .add_observer(mark_menu_closed_on_quit)
            .add_observer(on_menu_removed)
            .add_systems(
                Update,
                drain_menu_clicks.in_set(VoidSystems::MenuClickDrain),
            )
            .add_systems(
                PostUpdate,
                sync_containers.in_set(VoidSystems::InventorySync),
            );
    }
}

fn sync_inventory_on_ready(
    event: On<PlayerReadyEvent>,
    players: Players,
    mut inventories: Query<&mut Inventory>,
) {
    let Ok(mut inv) = inventories.get_mut(event.entity) else {
        return;
    };
    let inv = inv.bypass_change_detection();
    inv.take_dirty();
    let state_id = inv.next_state_id();
    players.send(
        event.entity,
        clientbound::SetContainerContent {
            container_id: PLAYER_WINDOW_ID,
            state_id,
            items: inv.to_slots(),
            carried: inv.cursor().to_slot(),
        },
    );
    players.send(
        event.entity,
        clientbound::SetHeldSlot {
            slot: inv.selected_hotbar() as i32,
        },
    );
}

fn track_selected_slot(event: On<PlayerChangeSlotEvent>, mut inventories: Query<&mut Inventory>) {
    if let Ok(mut inv) = inventories.get_mut(event.entity) {
        inv.bypass_change_detection()
            .accept_selected_hotbar(event.slot.clamp(0, 8) as u8);
    }
}

fn handle_container_click(
    event: On<PacketEvent<ClickContainer>>,
    mut players: Query<(&mut Inventory, Option<&mut OpenMenu>)>,
    config: Option<Res<ServerConfigResource>>,
    mut queue: ResMut<MenuClickQueue>,
    mut commands: Commands,
) {
    let packet = &event.packet;
    let creative = config.is_some_and(|config| config.game_mode == CREATIVE_GAME_MODE);
    let Ok((mut inv, open)) = players.get_mut(event.entity) else {
        return;
    };
    if packet.slot < -1 && packet.slot != ClickContainer::SLOT_OUTSIDE {
        return;
    }

    if packet.container_id == PLAYER_WINDOW_ID {
        if open.is_some() {
            return;
        }
        if packet.state_id != inv.state_id() {
            inv.mark_full();
        }
        let dropped = inv.apply_click(packet.slot, packet.button, packet.input, creative);
        drop_all(&mut commands, event.entity, dropped);
        return;
    }

    let Some(mut open) = open else {
        return;
    };
    if open.container_id != Some(packet.container_id) {
        return;
    }
    let layout = open.layout();
    let Some(slot) = MenuSlot::from_window(&layout, packet.slot) else {
        return;
    };
    let item = match slot {
        MenuSlot::Menu(index) => open.get(index).clone(),
        MenuSlot::Inventory(index) => inv.get(index).clone(),
        MenuSlot::Outside => ItemStack::EMPTY,
    };
    let stale = packet.state_id != open.state_id;

    if open.is_editable() {
        let dropped = apply_menu_click(&mut inv, &mut open, &layout, packet, creative);
        drop_all(&mut commands, event.entity, dropped);
    } else {
        for changed in &packet.changed_slots {
            match MenuSlot::from_window(&layout, changed.slot) {
                Some(MenuSlot::Menu(index)) => open.mark_slot(index),
                Some(MenuSlot::Inventory(index)) => inv.mark_slot(index),
                _ => {}
            }
        }
        inv.mark_cursor();
    }
    if stale {
        open.full_resync = true;
    }

    queue.0.push_back(MenuClickEvent {
        player: event.entity,
        container_id: packet.container_id,
        slot,
        button: packet.button,
        input: packet.input,
        item,
    });
}

fn apply_menu_click(
    inv: &mut Inventory,
    open: &mut OpenMenu,
    layout: &Layout,
    packet: &ClickContainer,
    creative: bool,
) -> Vec<ItemStack> {
    let mut slots: Vec<ItemStack> = open
        .slots()
        .iter()
        .chain(&inv.slots()[visible_inventory(layout)])
        .cloned()
        .collect();
    let before = slots.clone();
    let mut cursor = inv.cursor().clone();
    let dropped = Window {
        slots: &mut slots,
        cursor: &mut cursor,
        drag: &mut open.drag,
        layout,
        creative,
    }
    .apply_click(packet.slot, packet.button, packet.input);

    for (index, (old, new)) in before.iter().zip(slots).enumerate() {
        if *old == new {
            continue;
        }
        if layout.container.contains(&index) {
            open.set_slot(index, new);
        } else {
            inv.set(Inventory::MAIN_START + index - layout.main.start, new);
        }
    }
    inv.set_cursor(cursor);
    dropped
}

fn drop_all(commands: &mut Commands, dropper: Entity, dropped: Vec<ItemStack>) {
    for stack in dropped {
        commands.trigger(ItemDropEvent { dropper, stack });
    }
}

fn handle_close_container(
    event: On<PacketEvent<CloseContainer>>,
    mut open: Query<&mut OpenMenu>,
    mut commands: Commands,
) {
    let Ok(mut open) = open.get_mut(event.entity) else {
        return;
    };
    if open.container_id != Some(event.packet.container_id) {
        return;
    }
    open.closing = Some(MenuCloseReason::Client);
    commands.entity(event.entity).remove::<OpenMenu>();
}

fn mark_menu_closed_on_quit(event: On<PlayerQuitEvent>, mut open: Query<&mut OpenMenu>) {
    if let Ok(mut open) = open.get_mut(event.entity) {
        open.closing = Some(MenuCloseReason::Disconnect);
    }
}

fn on_menu_removed(
    event: On<Remove, OpenMenu>,
    players: Players,
    mut removed: Query<(&OpenMenu, &mut Inventory)>,
    mut commands: Commands,
) {
    let Ok((open, mut inv)) = removed.get_mut(event.entity) else {
        return;
    };
    let Some(container_id) = open.container_id else {
        return;
    };
    let reason = open.closing.unwrap_or(MenuCloseReason::Server);
    if reason != MenuCloseReason::Client && reason != MenuCloseReason::Disconnect {
        players.send(event.entity, clientbound::CloseContainer { container_id });
    }
    let carried = std::mem::replace(inv.cursor_mut(), ItemStack::EMPTY);
    inv.mark_cursor();
    let leftover = inv.give(carried);
    if !leftover.is_empty() {
        commands.trigger(ItemDropEvent {
            dropper: event.entity,
            stack: leftover,
        });
    }
    inv.mark_full();
    commands.trigger(MenuClosedEvent {
        player: event.entity,
        container_id,
        reason,
    });
}

fn drain_menu_clicks(world: &mut World) {
    while let Some(click) = world.resource_mut::<MenuClickQueue>().0.pop_front() {
        let Some(open) = world.get::<OpenMenu>(click.player) else {
            continue;
        };
        if open.container_id != Some(click.container_id) {
            continue;
        }
        let handler = open.handler();
        world.trigger(click.clone());
        if let Some(handler) = handler {
            handler(&mut MenuClickContext::new(world, &click));
        }
    }
}

/// The inventory indices a menu window shows after its own slots.
fn visible_inventory(layout: &Layout) -> std::ops::Range<usize> {
    Inventory::MAIN_START..Inventory::MAIN_START + layout.hotbar.end - layout.main.start
}

fn range_mask(range: std::ops::Range<usize>) -> u64 {
    range.fold(0, |mask, index| mask | (1 << index))
}

fn window_slots(inv: &Inventory, open: &OpenMenu, layout: &Layout) -> Vec<Slot> {
    let mut items: Vec<Slot> = open.slots().iter().map(ItemStack::to_slot).collect();
    items.extend(visible_inventory(layout).map(|index| inv.get(index).to_slot()));
    items
}

fn sync_containers(
    players: Players,
    mut changed: Query<
        (
            Entity,
            &mut Inventory,
            Option<&mut OpenMenu>,
            &mut ContainerIds,
        ),
        Or<(Changed<Inventory>, Changed<OpenMenu>)>,
    >,
) {
    for (entity, mut inv, open, mut ids) in changed.iter_mut() {
        let inv = inv.bypass_change_detection();
        let dirty = inv.take_dirty();

        if let Some(mut open) = open {
            let open = open.bypass_change_detection();
            let (menu_dirty, title_dirty) = open.take_dirty();
            let container_id = match open.container_id {
                Some(id) => {
                    if title_dirty {
                        players.send(entity, open_screen(id, open));
                        open.full_resync = true;
                    }
                    id
                }
                None => {
                    let id = ids.allocate();
                    open.container_id = Some(id);
                    open.full_resync = true;
                    players.send(entity, open_screen(id, open));
                    id
                }
            };
            let layout = open.layout();
            let visible = visible_inventory(&layout);
            let hidden = range_mask(0..Inventory::SIZE) & !range_mask(visible.clone());
            inv.retain_dirty(if dirty.full {
                hidden
            } else {
                dirty.slots & hidden
            });
            if std::mem::take(&mut open.full_resync) || dirty.full {
                let state_id = open.next_state_id();
                players.send(
                    entity,
                    clientbound::SetContainerContent {
                        container_id,
                        state_id,
                        items: window_slots(inv, open, &layout),
                        carried: inv.cursor().to_slot(),
                    },
                );
            } else {
                for index in (0..open.slot_count()).filter(|i| menu_dirty & (1 << i) != 0) {
                    let state_id = open.next_state_id();
                    players.send(
                        entity,
                        clientbound::SetContainerSlot {
                            container_id,
                            state_id,
                            slot: index as i16,
                            data: open.get(index).to_slot(),
                        },
                    );
                }
                for index in dirty.slot_indices().filter(|i| visible.contains(i)) {
                    let state_id = open.next_state_id();
                    players.send(
                        entity,
                        clientbound::SetContainerSlot {
                            container_id,
                            state_id,
                            slot: (layout.main.start + index - Inventory::MAIN_START) as i16,
                            data: inv.get(index).to_slot(),
                        },
                    );
                }
                if dirty.cursor {
                    players.send(
                        entity,
                        clientbound::SetCursorItem {
                            contents: inv.cursor().to_slot(),
                        },
                    );
                }
            }
        } else if dirty.full || dirty.slot_count() as usize > Inventory::SIZE / 2 {
            let state_id = inv.next_state_id();
            players.send(
                entity,
                clientbound::SetContainerContent {
                    container_id: PLAYER_WINDOW_ID,
                    state_id,
                    items: inv.to_slots(),
                    carried: inv.cursor().to_slot(),
                },
            );
        } else {
            for index in dirty.slot_indices() {
                let state_id = inv.next_state_id();
                players.send(
                    entity,
                    clientbound::SetContainerSlot {
                        container_id: PLAYER_WINDOW_ID,
                        state_id,
                        slot: index as i16,
                        data: inv.get(index).to_slot(),
                    },
                );
            }
            if dirty.cursor {
                players.send(
                    entity,
                    clientbound::SetCursorItem {
                        contents: inv.cursor().to_slot(),
                    },
                );
            }
        }

        if dirty.held {
            players.send(
                entity,
                clientbound::SetHeldSlot {
                    slot: inv.selected_hotbar() as i32,
                },
            );
        }
    }
}

fn open_screen(container_id: i32, open: &OpenMenu) -> clientbound::OpenScreen {
    clientbound::OpenScreen {
        container_id,
        menu_type: open.kind().registry_id(),
        title: open.title_nbt(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};
    use voidmc_protocol::serverbound::{ChangedSlot, ContainerInput};

    use super::*;
    use crate::components::{ClientId, PlayerReady};
    use crate::inventory::Inventories;
    use crate::menu::{Menu, MenuType, Menus, WorldMenus};
    use crate::network::{NetworkChannels, OutgoingPacket};

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
        .add_plugins(InventoryPlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut()
            .spawn((ClientId(id), PlayerReady, Inventory::new()))
            .id()
    }

    #[derive(Debug, PartialEq, Clone)]
    enum Sent {
        Open(i32, i32),
        Close(i32),
        Content(i32, i32, usize, Slot),
        SlotSet(i32, i32, i16, Slot),
        Cursor(Slot),
        Held(i32),
        Cooldown(String, i32),
        Other,
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        rx.try_iter()
            .map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::OpenScreen(p)) => {
                    Sent::Open(p.container_id, p.menu_type)
                }
                ClientboundPacket::Play(PlayPacket::CloseContainer(p)) => {
                    Sent::Close(p.container_id)
                }
                ClientboundPacket::Play(PlayPacket::SetContainerContent(p)) => {
                    Sent::Content(p.container_id, p.state_id, p.items.len(), p.carried)
                }
                ClientboundPacket::Play(PlayPacket::SetContainerSlot(p)) => {
                    Sent::SlotSet(p.container_id, p.state_id, p.slot, p.data)
                }
                ClientboundPacket::Play(PlayPacket::SetCursorItem(p)) => Sent::Cursor(p.contents),
                ClientboundPacket::Play(PlayPacket::SetHeldSlot(p)) => Sent::Held(p.slot),
                ClientboundPacket::Play(PlayPacket::SetCooldown(p)) => {
                    Sent::Cooldown(p.cooldown_group, p.duration)
                }
                _ => Sent::Other,
            })
            .collect()
    }

    fn stone(n: u8) -> ItemStack {
        ItemStack::of("minecraft:stone", n).unwrap()
    }

    fn click(app: &mut App, entity: Entity, container_id: i32, state_id: i32, slot: i16) {
        click_with(
            app,
            entity,
            container_id,
            state_id,
            slot,
            ContainerInput::Pickup,
            vec![ChangedSlot { slot, item: None }],
        );
    }

    fn click_with(
        app: &mut App,
        entity: Entity,
        container_id: i32,
        state_id: i32,
        slot: i16,
        input: ContainerInput,
        changed_slots: Vec<ChangedSlot>,
    ) {
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity,
            packet: ClickContainer {
                container_id,
                state_id,
                slot,
                button: 0,
                input,
                changed_slots,
                carried: None,
            },
        });
    }

    fn open_id(app: &App, entity: Entity) -> i32 {
        app.world()
            .get::<OpenMenu>(entity)
            .unwrap()
            .container_id()
            .unwrap()
    }

    #[test]
    fn ready_sends_full_inventory_and_held_slot() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .give(stone(3));
        app.world_mut().trigger(PlayerReadyEvent {
            client_id: 1,
            entity: p,
        });
        assert_eq!(
            drain(&rx),
            vec![Sent::Content(0, 1, 46, Slot::EMPTY), Sent::Held(0)]
        );
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn single_slot_change_sends_one_set_slot() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .set(Inventory::MAIN_START, stone(4));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::SlotSet(0, 1, 9, stone(4).to_slot())]);

        app.world_mut().get_mut::<Inventory>(p).unwrap();
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().get_mut::<Inventory>(p).unwrap().clear();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Content(0, 2, 46, Slot::EMPTY)]);
    }

    #[test]
    fn many_slot_changes_collapse_into_full_content() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        {
            let mut inv = app.world_mut().get_mut::<Inventory>(p).unwrap();
            for i in Inventory::MAIN_START..Inventory::MAIN_START + 30 {
                inv.set(i, stone(1));
            }
        }
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Content(0, 1, 46, Slot::EMPTY)]);
    }

    #[test]
    fn held_slot_is_sent_only_for_server_changes() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        app.world_mut()
            .trigger(PlayerChangeSlotEvent { entity: p, slot: 5 });
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world().get::<Inventory>(p).unwrap().selected_hotbar(),
            5
        );

        let set = |app: &mut App, entity: Entity, slot: u8| {
            let mut state = bevy_ecs::system::SystemState::<Inventories>::new(app.world_mut());
            let mut inventories = state.get_mut(app.world_mut());
            let ok = inventories.set_held_slot(entity, slot);
            state.apply(app.world_mut());
            ok
        };
        assert!(set(&mut app, p, 7));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Held(7)]);
        assert!(set(&mut app, p, 7));
        app.update();
        assert!(drain(&rx).is_empty());
        let bare = app.world_mut().spawn(ClientId(2)).id();
        assert!(!set(&mut app, bare, 1));
    }

    #[test]
    fn cooldown_sends_the_packet() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        let mut state = bevy_ecs::system::SystemState::<Inventories>::new(app.world_mut());
        let inventories = state.get_mut(app.world_mut());
        let pearl = crate::item::ItemId::from_name("minecraft:ender_pearl").unwrap();
        assert!(inventories.cooldown(p, crate::inventory::Cooldown::item(pearl).ticks(20)));
        assert_eq!(
            drain(&rx),
            vec![Sent::Cooldown("minecraft:ender_pearl".into(), 20)]
        );
    }

    #[test]
    fn player_window_click_answers_with_changed_slots_and_cursor() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .set(Inventory::HOTBAR_START, stone(5));
        app.update();
        drain(&rx);

        click(&mut app, p, 0, 1, Inventory::HOTBAR_START as i16);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::SlotSet(0, 2, 36, Slot::EMPTY),
                Sent::Cursor(stone(5).to_slot()),
            ]
        );

        click(&mut app, p, 0, 99, Inventory::MAIN_START as i16);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Content(0, 3, 46, Slot::EMPTY)]);
        assert_eq!(
            app.world()
                .get::<Inventory>(p)
                .unwrap()
                .get(Inventory::MAIN_START),
            &stone(5)
        );
    }

    #[test]
    fn open_allocates_an_id_and_sends_screen_then_content() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .set(Inventory::HOTBAR_START, stone(2));
        app.update();
        drain(&rx);

        let mut state = bevy_ecs::system::SystemState::<Menus>::new(app.world_mut());
        let mut menus = state.get_mut(app.world_mut());
        assert!(!menus.is_open(p));
        menus.open(p, Menu::chest(3, "Shop").slot(0, stone(1)));
        state.apply(app.world_mut());
        assert!(
            app.world()
                .get::<OpenMenu>(p)
                .unwrap()
                .container_id()
                .is_none()
        );

        app.update();
        let sent = drain(&rx);
        assert_eq!(sent[0], Sent::Open(1, MenuType::Generic9x3.registry_id()));
        assert_eq!(sent[1], Sent::Content(1, 1, 63, Slot::EMPTY));
        assert_eq!(sent.len(), 2);
        assert_eq!(open_id(&app, p), 1);

        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn menu_and_inventory_edits_route_through_the_open_window() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        WorldMenus::new(app.world_mut()).open(p, Menu::hopper("H"));
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<OpenMenu>(p)
            .unwrap()
            .set_slot(2, stone(1));
        {
            let mut inv = app.world_mut().get_mut::<Inventory>(p).unwrap();
            inv.set(Inventory::MAIN_START, stone(2));
            inv.set(Inventory::HOTBAR_START + 8, stone(3));
            inv.set(Inventory::ARMOR_HEAD, stone(4));
        }
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::SlotSet(1, 2, 2, stone(1).to_slot()),
                Sent::SlotSet(1, 3, 5, stone(2).to_slot()),
                Sent::SlotSet(1, 4, 40, stone(3).to_slot()),
            ]
        );
    }

    #[test]
    fn read_only_click_is_cancelled_and_queued() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        let seen: Arc<Mutex<Vec<(MenuSlot, ItemStack)>>> = Arc::default();
        let sink = seen.clone();
        WorldMenus::new(app.world_mut()).open(
            p,
            Menu::chest(1, "M")
                .slot(3, stone(7))
                .on_click(move |ctx| sink.lock().unwrap().push((ctx.slot, ctx.item.clone()))),
        );
        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .set(Inventory::MAIN_START, stone(2));
        app.update();
        drain(&rx);

        click(&mut app, p, 1, 1, 3);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::SlotSet(1, 2, 3, stone(7).to_slot()),
                Sent::Cursor(Slot::EMPTY),
            ]
        );
        assert_eq!(app.world().get::<OpenMenu>(p).unwrap().get(3), &stone(7));

        click(&mut app, p, 1, 2, 9);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::SlotSet(1, 3, 9, stone(2).to_slot()),
                Sent::Cursor(Slot::EMPTY),
            ]
        );

        click(&mut app, p, 1, 0, 4);
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Content(1, 4, 45, Slot::EMPTY)]);

        click(&mut app, p, 7, 4, 4);
        click(&mut app, p, 0, 4, 4);
        app.update();
        assert!(drain(&rx).is_empty());

        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                (MenuSlot::Menu(3), stone(7)),
                (MenuSlot::Inventory(9), stone(2)),
                (MenuSlot::Menu(4), ItemStack::EMPTY),
            ]
        );
    }

    #[test]
    fn click_event_reaches_observers_with_the_payload() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        #[derive(Resource, Default)]
        struct Seen(Vec<MenuClickEvent>);
        app.init_resource::<Seen>();
        app.add_observer(|event: On<MenuClickEvent>, mut seen: ResMut<Seen>| {
            seen.0.push(event.event().clone());
        });
        WorldMenus::new(app.world_mut()).open(p, Menu::chest(1, "M").slot(0, stone(1)));
        app.update();
        drain(&rx);

        click_with(&mut app, p, 1, 1, -999, ContainerInput::Throw, vec![]);
        app.update();
        let seen = app.world().resource::<Seen>();
        assert_eq!(seen.0.len(), 1);
        assert_eq!(seen.0[0].player, p);
        assert_eq!(seen.0[0].container_id, 1);
        assert_eq!(seen.0[0].slot, MenuSlot::Outside);
        assert_eq!(seen.0[0].input, ContainerInput::Throw);
        assert!(seen.0[0].item.is_empty());
    }

    #[test]
    fn editable_menu_moves_items_and_rejects_bad_slots() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        WorldMenus::new(app.world_mut()).open(p, Menu::chest(1, "E").slot(0, stone(5)).editable());
        app.update();
        drain(&rx);

        click(&mut app, p, 1, 1, 0);
        click(&mut app, p, 1, 1, 9);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::SlotSet(1, 2, 0, Slot::EMPTY),
                Sent::SlotSet(1, 3, 9, stone(5).to_slot()),
                Sent::Cursor(Slot::EMPTY),
            ]
        );
        assert!(app.world().get::<OpenMenu>(p).unwrap().get(0).is_empty());
        assert_eq!(
            app.world()
                .get::<Inventory>(p)
                .unwrap()
                .get(Inventory::MAIN_START),
            &stone(5)
        );

        click(&mut app, p, 1, 3, 45);
        click(&mut app, p, 1, 3, -5);
        app.update();
        assert!(drain(&rx).is_empty());

        click_with(&mut app, p, 1, 3, 9, ContainerInput::QuickMove, vec![]);
        app.update();
        assert_eq!(app.world().get::<OpenMenu>(p).unwrap().get(0), &stone(5));
    }

    #[test]
    fn close_from_both_sides_and_replacement() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        #[derive(Resource, Default)]
        struct Closed(Vec<(i32, MenuCloseReason)>);
        app.init_resource::<Closed>();
        app.add_observer(|event: On<MenuClosedEvent>, mut closed: ResMut<Closed>| {
            closed.0.push((event.container_id, event.reason));
        });

        WorldMenus::new(app.world_mut()).open(p, Menu::chest(1, "A"));
        app.update();
        drain(&rx);
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity: p,
            packet: CloseContainer { container_id: 5 },
        });
        assert!(app.world().get::<OpenMenu>(p).is_some());
        app.world_mut().trigger(PacketEvent {
            client_id: 1,
            entity: p,
            packet: CloseContainer { container_id: 1 },
        });
        app.update();
        assert!(app.world().get::<OpenMenu>(p).is_none());
        assert_eq!(drain(&rx), vec![Sent::Content(0, 1, 46, Slot::EMPTY)]);

        WorldMenus::new(app.world_mut()).open(p, Menu::chest(1, "B"));
        app.update();
        assert_eq!(drain(&rx)[0], Sent::Open(2, 0));
        WorldMenus::new(app.world_mut()).open(p, Menu::chest(2, "C"));
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Close(2),
                Sent::Open(3, 1),
                Sent::Content(3, 1, 54, Slot::EMPTY),
            ]
        );

        let mut state = bevy_ecs::system::SystemState::<Menus>::new(app.world_mut());
        state.get_mut(app.world_mut()).close(p);
        state.apply(app.world_mut());
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::Close(3), Sent::Content(0, 2, 46, Slot::EMPTY)]
        );
        assert_eq!(
            app.world().resource::<Closed>().0,
            vec![
                (1, MenuCloseReason::Client),
                (2, MenuCloseReason::Replaced),
                (3, MenuCloseReason::Server),
            ]
        );
    }

    #[test]
    fn closing_an_editable_menu_returns_the_cursor_item() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        WorldMenus::new(app.world_mut()).open(p, Menu::chest(1, "E").slot(0, stone(5)).editable());
        app.update();
        click(&mut app, p, 1, 1, 0);
        app.update();
        drain(&rx);
        assert_eq!(app.world().get::<Inventory>(p).unwrap().cursor(), &stone(5));

        WorldMenus::new(app.world_mut()).close(p);
        let inv = app.world().get::<Inventory>(p).unwrap();
        assert!(inv.cursor().is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START), &stone(5));
    }

    #[test]
    fn disconnect_cleans_up_without_packets() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        #[derive(Resource, Default)]
        struct Closed(Vec<MenuCloseReason>);
        app.init_resource::<Closed>();
        app.add_observer(|event: On<MenuClosedEvent>, mut closed: ResMut<Closed>| {
            closed.0.push(event.reason);
        });
        WorldMenus::new(app.world_mut()).open(p, Menu::chest(1, "A").on_click(|_| panic!()));
        app.update();
        drain(&rx);
        click(&mut app, p, 1, 1, 0);

        app.world_mut().trigger(PlayerQuitEvent {
            client_id: 1,
            entity: p,
        });
        app.world_mut().despawn(p);
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world().resource::<Closed>().0,
            vec![MenuCloseReason::Disconnect]
        );
        assert!(app.world().resource::<MenuClickQueue>().0.is_empty());
    }

    #[test]
    fn container_ids_never_reuse_zero_across_many_opens() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        let mut ids = Vec::new();
        for _ in 0..101 {
            WorldMenus::new(app.world_mut()).open(p, Menu::chest(1, "A"));
            app.update();
            ids.push(open_id(&app, p));
        }
        drain(&rx);
        assert!(ids.iter().all(|id| *id != 0));
        assert_eq!(ids[0], 1);
        assert_eq!(ids[100], 1);
    }

    #[test]
    fn open_refuses_entities_without_inventory() {
        let (mut app, _rx) = test_app();
        let e = app.world_mut().spawn(ClientId(9)).id();
        assert!(!WorldMenus::new(app.world_mut()).open(e, Menu::chest(1, "A")));
        assert!(!WorldMenus::new(app.world_mut()).is_open(e));
    }

    #[test]
    fn title_change_resends_screen_and_full_contents() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        WorldMenus::new(app.world_mut()).open(p, Menu::hopper("H").slot(0, stone(1)));
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<OpenMenu>(p)
            .unwrap()
            .set_title("H2");
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Open(1, MenuType::Hopper.registry_id()),
                Sent::Content(1, 2, 41, Slot::EMPTY),
            ]
        );
    }

    #[test]
    fn hidden_inventory_changes_survive_an_open_menu() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        WorldMenus::new(app.world_mut()).open(p, Menu::hopper("H"));
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .set(Inventory::ARMOR_HEAD, stone(1));
        app.update();
        assert!(drain(&rx).is_empty());
        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .set(Inventory::MAIN_START, stone(2));
        app.update();
        assert_eq!(drain(&rx), vec![Sent::SlotSet(1, 2, 5, stone(2).to_slot())]);

        WorldMenus::new(app.world_mut()).close(p);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::Close(1), Sent::Content(0, 1, 46, Slot::EMPTY)]
        );

        WorldMenus::new(app.world_mut()).open(p, Menu::hopper("H"));
        app.update();
        drain(&rx);
        app.world_mut().get_mut::<Inventory>(p).unwrap().clear();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Content(2, 2, 41, Slot::EMPTY)]);
        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .set(Inventory::HOTBAR_START, stone(3));
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::SlotSet(2, 3, 32, stone(3).to_slot())]
        );
        let inv = app.world().get::<Inventory>(p).unwrap();
        assert_eq!(
            inv.dirty().slot_indices().collect::<Vec<_>>(),
            (0..9).chain([45]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn disconnect_returns_the_cursor_item_to_the_inventory() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        WorldMenus::new(app.world_mut()).open(p, Menu::chest(1, "E").slot(0, stone(5)).editable());
        app.update();
        click(&mut app, p, 1, 1, 0);
        app.update();
        drain(&rx);
        assert_eq!(app.world().get::<Inventory>(p).unwrap().cursor(), &stone(5));

        app.world_mut().trigger(PlayerQuitEvent {
            client_id: 1,
            entity: p,
        });
        app.world_mut().entity_mut(p).remove::<OpenMenu>();
        let inv = app.world().get::<Inventory>(p).unwrap();
        assert!(inv.cursor().is_empty());
        assert_eq!(inv.get(Inventory::HOTBAR_START), &stone(5));
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn clone_is_a_no_op_in_survival_and_works_in_creative() {
        let (mut app, rx) = test_app();
        let p = player(&mut app, 1);
        app.world_mut()
            .get_mut::<Inventory>(p)
            .unwrap()
            .set(Inventory::HOTBAR_START, stone(1));
        app.update();
        drain(&rx);

        let clone = |app: &mut App| {
            app.world_mut().trigger(PacketEvent {
                client_id: 1,
                entity: p,
                packet: ClickContainer {
                    container_id: 0,
                    state_id: 1,
                    slot: Inventory::HOTBAR_START as i16,
                    button: 2,
                    input: ContainerInput::Clone,
                    changed_slots: vec![],
                    carried: None,
                },
            });
        };
        clone(&mut app);
        assert!(app.world().get::<Inventory>(p).unwrap().cursor().is_empty());

        let config = crate::config::ServerConfig {
            game_mode: 1,
            ..Default::default()
        };
        app.insert_resource(ServerConfigResource::from(&config));
        clone(&mut app);
        assert_eq!(app.world().get::<Inventory>(p).unwrap().cursor().count, 64);
    }
}
