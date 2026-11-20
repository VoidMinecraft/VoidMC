//! What other players are shown a player holding and wearing (Set
//! Equipment): the selected hotbar slot as the main hand, the off-hand and
//! the four armor slots. Like vanilla, a slot is sent to the other ready
//! players (the audience of the player's spawn) when it changes, and every
//! non-empty slot to a player who just joined. A player is never sent its
//! own equipment: its inventory window already shows it.

use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::{EquipmentSlot, SetEquipment};
use voidmc_protocol::slot::Slot;

use crate::components::{MinecraftEntityId, PlayerReady};
use crate::inventory::Inventory;
use crate::item::ItemStack;
use crate::players::Players;

/// The slots a player is shown with, in Set Equipment order.
const SLOTS: [EquipmentSlot; 6] = [
    EquipmentSlot::MainHand,
    EquipmentSlot::OffHand,
    EquipmentSlot::Feet,
    EquipmentSlot::Legs,
    EquipmentSlot::Chest,
    EquipmentSlot::Head,
];

/// What a client assumes for a player it just spawned.
const NOTHING: [ItemStack; 6] = [const { ItemStack::EMPTY }; 6];

/// A player's equipment as last sent to the other ready players, in
/// [`SLOTS`] order.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct ShownEquipment([ItemStack; 6]);

fn current(inventory: &Inventory) -> [&ItemStack; 6] {
    [
        inventory.held(),
        inventory.get(Inventory::OFFHAND),
        inventory.get(Inventory::ARMOR_FEET),
        inventory.get(Inventory::ARMOR_LEGS),
        inventory.get(Inventory::ARMOR_CHEST),
        inventory.get(Inventory::ARMOR_HEAD),
    ]
}

fn same(a: &ItemStack, b: &ItemStack) -> bool {
    (a.is_empty() && b.is_empty()) || a == b
}

fn packet(entity_id: i32, equipment: Vec<(EquipmentSlot, Slot)>) -> SetEquipment {
    SetEquipment {
        entity_id,
        equipment,
    }
}

/// Sends each ready player's changed equipment slots to the other ready
/// players, and its non-empty slots to players who became ready this tick.
pub(crate) fn broadcast_equipment(
    mut commands: Commands,
    players: Players,
    joined: Query<Entity, Added<PlayerReady>>,
    mut ready: Query<(
        Entity,
        &MinecraftEntityId,
        &Inventory,
        Ref<PlayerReady>,
        Option<&mut ShownEquipment>,
    )>,
) {
    let joined: Vec<Entity> = joined.iter().collect();
    let recipients = players.ready();
    for (player, id, inventory, is_ready, shown) in ready.iter_mut() {
        let now = current(inventory);
        let known = match &shown {
            Some(shown) if !is_ready.is_added() => &shown.0,
            _ => &NOTHING,
        };
        let changed: Vec<(EquipmentSlot, Slot)> = SLOTS
            .iter()
            .zip(now)
            .zip(known)
            .filter(|((_, now), known)| !same(now, known))
            .map(|((slot, now), _)| (*slot, now.to_slot()))
            .collect();
        if !changed.is_empty() {
            recipients.send_where(
                |recipient| recipient.entity() != player && !joined.contains(&recipient.entity()),
                packet(id.0, changed),
            );
        }

        let viewers: Vec<Entity> = joined
            .iter()
            .copied()
            .filter(|viewer| *viewer != player)
            .collect();
        let worn: Vec<(EquipmentSlot, Slot)> = SLOTS
            .iter()
            .zip(now)
            .filter(|(_, now)| !now.is_empty())
            .map(|(slot, now)| (*slot, now.to_slot()))
            .collect();
        if !viewers.is_empty() && !worn.is_empty() {
            players.send_to(viewers, packet(id.0, worn));
        }

        let up_to_date = shown
            .as_ref()
            .is_some_and(|shown| shown.0.iter().zip(now).all(|(a, b)| same(a, b)));
        if !up_to_date {
            let snapshot = ShownEquipment(now.map(|stack| {
                if stack.is_empty() {
                    ItemStack::EMPTY
                } else {
                    stack.clone()
                }
            }));
            match shown {
                Some(mut shown) => *shown = snapshot,
                None => {
                    commands.entity(player).try_insert(snapshot);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, PostUpdate};
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::ClientId;
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
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
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx))
        .add_systems(PostUpdate, broadcast_equipment);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(id),
                MinecraftEntityId(id as i32),
                PlayerReady,
                Inventory::new(),
            ))
            .id()
    }

    /// `(receiver, entity id, [(slot ordinal, item id, count)])` per packet.
    type Sent = (u32, i32, Vec<(u8, i32, i32)>);

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent: Vec<Sent> = rx
            .try_iter()
            .filter_map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::SetEquipment(p)) => Some((
                    out.client_id,
                    p.entity_id,
                    p.equipment
                        .iter()
                        .map(|(slot, item)| (slot.ordinal(), item.item_id, item.count))
                        .collect(),
                )),
                _ => None,
            })
            .collect();
        sent.sort();
        sent
    }

    fn stack(name: &str, count: u8) -> ItemStack {
        ItemStack::of(name, count).unwrap()
    }

    fn id(name: &str) -> i32 {
        stack(name, 1).item.0
    }

    #[test]
    fn changes_go_to_the_others_and_a_joiner_gets_every_worn_slot() {
        let (mut app, rx) = test_app();
        let alice = player(&mut app, 1);
        player(&mut app, 2);
        app.update();
        assert!(drain(&rx).is_empty(), "nothing is worn yet");

        {
            let mut inventory = app.world_mut().get_mut::<Inventory>(alice).unwrap();
            inventory.set(Inventory::HOTBAR_START + 1, stack("minecraft:stone", 3));
            inventory.set(Inventory::ARMOR_HEAD, stack("minecraft:diamond_helmet", 1));
        }
        app.update();
        let helmet = id("minecraft:diamond_helmet");
        assert_eq!(drain(&rx), vec![(2, 1, vec![(5, helmet, 1)])]);

        // Selecting the stone, without change detection (as the client's
        // own selection is tracked), still shows it in the main hand.
        app.world_mut()
            .get_mut::<Inventory>(alice)
            .unwrap()
            .bypass_change_detection()
            .accept_selected_hotbar(1);
        app.update();
        let stone = id("minecraft:stone");
        assert_eq!(drain(&rx), vec![(2, 1, vec![(0, stone, 3)])]);
        app.update();
        assert!(drain(&rx).is_empty());

        // A joiner whose arrival coincides with a change gets the full set
        // once; the player already online gets only the change.
        app.world_mut()
            .get_mut::<Inventory>(alice)
            .unwrap()
            .set(Inventory::OFFHAND, stack("minecraft:shield", 1));
        player(&mut app, 3);
        app.update();
        let shield = id("minecraft:shield");
        assert_eq!(
            drain(&rx),
            vec![
                (2, 1, vec![(1, shield, 1)]),
                (3, 1, vec![(0, stone, 3), (1, shield, 1), (5, helmet, 1)]),
            ]
        );

        app.world_mut().get_mut::<Inventory>(alice).unwrap().clear();
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                (2, 1, vec![(0, 0, 0), (1, 0, 0), (5, 0, 0)]),
                (3, 1, vec![(0, 0, 0), (1, 0, 0), (5, 0, 0)]),
            ]
        );
    }

    #[test]
    fn a_player_who_joins_wearing_something_is_shown_to_those_online() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        app.update();
        let late = app
            .world_mut()
            .spawn((ClientId(2), MinecraftEntityId(2), Inventory::new()))
            .id();
        app.world_mut()
            .get_mut::<Inventory>(late)
            .unwrap()
            .set(Inventory::ARMOR_FEET, stack("minecraft:diamond_boots", 1));
        app.update();
        assert!(drain(&rx).is_empty(), "not ready, not shown");

        app.world_mut().entity_mut(late).insert(PlayerReady);
        app.update();
        let boots = id("minecraft:diamond_boots");
        assert_eq!(drain(&rx), vec![(1, 2, vec![(2, boots, 1)])]);
    }
}
