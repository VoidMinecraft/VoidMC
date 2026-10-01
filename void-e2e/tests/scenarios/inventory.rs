use voidmc::{Inventory, ItemId, ItemStack as ServerItemStack};
use voidmc_e2e::TestServer;
use voidmc_e2e::inventory::operations::ClickType;
use voidmc_e2e::inventory::{ItemStack, ItemStackData};
use voidmc_e2e::protocol::packets::game::s_container_click::{
    HashedActualItem, HashedPatchMap, HashedStack, ServerboundContainerClick,
};
use voidmc_e2e::registry::builtin::ItemKind;

fn hashed(kind: ItemKind, count: i32) -> HashedStack {
    HashedStack(Some(HashedActualItem {
        kind,
        count,
        components: HashedPatchMap {
            added_components: Vec::new(),
            removed_components: Vec::new(),
        },
    }))
}

#[tokio::test]
async fn clicks_move_items_between_slots() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let entity = server.player_entity(alice.uuid()).await;

    alice.command("give diamond 5").await;
    alice
        .wait_until("the diamonds in the first hotbar slot", |view| {
            view.inventory[36] == ItemStack::Present(ItemStackData::new(ItemKind::Diamond, 5))
        })
        .await;

    let state_id = alice.view(|view| view.inventory_state_id);
    alice
        .send(ServerboundContainerClick {
            container_id: 0,
            state_id,
            slot_num: 36,
            button_num: 0,
            click_type: ClickType::Pickup,
            changed_slots: [(36, HashedStack(None))].into_iter().collect(),
            carried_item: hashed(ItemKind::Diamond, 5),
        })
        .await;
    alice
        .send(ServerboundContainerClick {
            container_id: 0,
            state_id,
            slot_num: 9,
            button_num: 0,
            click_type: ClickType::Pickup,
            changed_slots: [(9, hashed(ItemKind::Diamond, 5))].into_iter().collect(),
            carried_item: HashedStack(None),
        })
        .await;
    alice.sync().await;

    let (top_left, hotbar, cursor) = server
        .with_world(move |world| {
            let inventory = world.get::<Inventory>(entity).unwrap();
            (
                inventory.get(9).clone(),
                inventory.get(36).clone(),
                inventory.cursor().clone(),
            )
        })
        .await;
    assert_eq!(
        top_left,
        ServerItemStack::new(ItemId::from_name("minecraft:diamond").unwrap(), 5)
    );
    assert!(hotbar.is_empty());
    assert!(cursor.is_empty());

    alice.assert_healthy();
    server.stop().await;
}
