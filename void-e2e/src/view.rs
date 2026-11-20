use std::collections::HashMap;
use std::io::Cursor;

use azalea_block::BlockState;
use azalea_core::game_type::GameMode;
use azalea_core::position::{BlockPos, ChunkBlockPos, ChunkPos, Vec3};
use azalea_core::registry_holder::RegistryHolder;
use azalea_inventory::ItemStack;
use azalea_inventory::components::EquipmentSlot;
use azalea_inventory::operations::ClickType;
use azalea_protocol::common::movements::{PositionMoveRotation, RelativeMovements};
use azalea_protocol::packets::game::ClientboundGamePacket;
use azalea_protocol::packets::game::c_game_event::EventType;
use azalea_registry::builtin::EntityKind;
use azalea_world::Chunk;
use uuid::Uuid;

use crate::clicks::{Click, InventoryMenu, from_client, to_client};

const OVERWORLD_MIN_Y: i32 = -64;
const OVERWORLD_HEIGHT: u32 = 384;
const PLAYER_INVENTORY: i32 = 0;

/// An entity as this client was told about it.
#[derive(Clone, Debug, PartialEq)]
pub struct SeenEntity {
    pub uuid: Uuid,
    pub kind: EntityKind,
    pub position: Vec3,
}

/// Everything a vanilla client would remember from the packets it received:
/// its own position, the chunks it holds, the entities and players it knows,
/// and its inventory.
///
/// Like the vanilla client, the inventory also shows the client's own
/// prediction of the clicks it sends ([`Bot::click`](crate::Bot::click)) and
/// of the slots it sets from the creative inventory, until the server
/// corrects them. The server does not resend a slot the client predicted
/// right, so the inventory is what the player sees, not only what the server
/// sent.
#[derive(Debug, Default)]
pub struct ClientView {
    pub entity_id: Option<i32>,
    pub position: Vec3,
    pub chunk_center: Option<ChunkPos>,
    pub chunks: HashMap<ChunkPos, Chunk>,
    pub entities: HashMap<i32, SeenEntity>,
    pub players: HashMap<Uuid, String>,
    /// What the other entities are shown holding and wearing, by entity id,
    /// from Set Equipment. A slot never sent reads as empty.
    pub equipment: HashMap<i32, HashMap<EquipmentSlot, ItemStack>>,
    pub inventory: Vec<ItemStack>,
    pub cursor: ItemStack,
    pub inventory_state_id: u32,
    /// The game mode, from Login, Respawn and Game Event packets.
    pub game_mode: GameMode,
    pub anomalies: Vec<String>,
    /// The drag in progress, which lasts across clicks, as in the client's
    /// own inventory menu.
    menu: InventoryMenu,
    min_y: i32,
    height: u32,
}

impl ClientView {
    pub(crate) fn new(registries: &RegistryHolder) -> Self {
        let (min_y, height) = registries
            .dimension_type
            .map
            .iter()
            .find(|(id, _)| id.to_string() == "minecraft:overworld")
            .map(|(_, dimension)| (dimension.min_y, dimension.height))
            .unwrap_or((OVERWORLD_MIN_Y, OVERWORLD_HEIGHT));
        Self {
            inventory: vec![ItemStack::Empty; 46],
            min_y,
            height,
            ..Default::default()
        }
    }

    pub fn block_at(&self, pos: BlockPos) -> Option<BlockState> {
        self.chunks
            .get(&ChunkPos::from(pos))?
            .get_block_state(&ChunkBlockPos::from(pos), self.min_y)
    }

    /// The first non-air block at or below `pos`, scanning down.
    pub fn ground_below(&self, pos: BlockPos) -> Option<BlockPos> {
        (self.min_y..=pos.y)
            .rev()
            .map(|y| BlockPos::new(pos.x, y, pos.z))
            .find(|pos| self.block_at(*pos).is_some_and(|state| !state.is_air()))
    }

    /// Whether the chunk this client stands in is held, or it stands above or
    /// below the world, where the vanilla client needs no chunk to leave its
    /// loading screen.
    pub(crate) fn stands_in_a_held_chunk(&self) -> bool {
        let feet = BlockPos::from(self.position);
        let outside = feet.y < self.min_y || feet.y >= self.min_y + self.height as i32;
        outside || self.chunks.contains_key(&ChunkPos::from(feet))
    }

    /// What entity `id` is shown holding or wearing in `slot`.
    pub fn equipment_of(&self, id: i32, slot: EquipmentSlot) -> ItemStack {
        self.equipment
            .get(&id)
            .and_then(|slots| slots.get(&slot))
            .cloned()
            .unwrap_or(ItemStack::Empty)
    }

    /// Predicts a click in the player inventory with vanilla's rules, from
    /// what this view shows, as the vanilla client does before sending it.
    pub(crate) fn predict_click(&mut self, slot: i16, button: u8, mode: ClickType) -> Click {
        self.menu.slots = self.inventory.iter().map(from_client).collect();
        self.menu.carried = from_client(&self.cursor);
        self.menu.creative = self.game_mode == GameMode::Creative;
        self.menu.drops.clear();
        let before = self.menu.slots.clone();
        self.menu.click(slot, button, mode);
        let changed = before
            .iter()
            .zip(&self.menu.slots)
            .enumerate()
            .filter(|(_, (before, after))| before != after)
            .map(|(index, (_, after))| (index as u16, *after))
            .collect();
        Click {
            state_id: self.inventory_state_id,
            slot,
            button,
            mode,
            changed,
            carried: self.menu.carried,
            drops: std::mem::take(&mut self.menu.drops),
        }
    }

    /// Shows the prediction of `click` until the server corrects it.
    pub(crate) fn show_prediction(&mut self, click: &Click) {
        for &(slot, stack) in &click.changed {
            if let Some(shown) = self.inventory.get_mut(usize::from(slot)) {
                *shown = to_client(stack, shown);
            }
        }
        self.cursor = to_client(click.carried, &self.cursor);
    }

    pub fn entity_by_uuid(&self, uuid: Uuid) -> Option<(i32, &SeenEntity)> {
        self.entities
            .iter()
            .find(|(_, entity)| entity.uuid == uuid)
            .map(|(id, entity)| (*id, entity))
    }

    pub(crate) fn apply(&mut self, packet: &ClientboundGamePacket) -> Result<(), String> {
        match packet {
            ClientboundGamePacket::Login(login) => {
                self.entity_id = Some(login.player_id.0);
                self.game_mode = login.common.game_type;
            }
            ClientboundGamePacket::Respawn(respawn) => {
                self.game_mode = respawn.common.game_type;
            }
            ClientboundGamePacket::GameEvent(event) if event.event == EventType::ChangeGameMode => {
                if let Some(mode) = GameMode::from_id(event.param as u8) {
                    self.game_mode = mode;
                }
            }
            ClientboundGamePacket::PlayerPosition(teleport) => {
                let mut position = self.position;
                apply_movement(&mut position, &teleport.change, &teleport.relative);
                self.position = position;
            }
            ClientboundGamePacket::SetChunkCacheCenter(center) => {
                self.chunk_center = Some(ChunkPos::new(center.x, center.z));
            }
            ClientboundGamePacket::LevelChunkWithLight(packet) => {
                let pos = ChunkPos::new(packet.x, packet.z);
                let data: &[u8] = &packet.chunk_data.data;
                let mut cursor = Cursor::new(data);
                let chunk = Chunk::read_with_dimension_height(
                    &mut cursor,
                    self.height,
                    self.min_y,
                    &packet.chunk_data.heightmaps,
                )
                .map_err(|error| format!("chunk {pos:?} has undecodable sections: {error}"))?;
                let leftover = data.len() - cursor.position() as usize;
                if leftover != 0 {
                    return Err(format!(
                        "chunk {pos:?} carries {leftover} bytes after its sections"
                    ));
                }
                if self.chunks.insert(pos, chunk).is_some() {
                    self.anomalies.push(format!("chunk {pos:?} sent twice"));
                }
            }
            ClientboundGamePacket::ForgetLevelChunk(packet)
                if self.chunks.remove(&packet.pos).is_none() =>
            {
                self.anomalies
                    .push(format!("forget for unloaded chunk {:?}", packet.pos));
            }
            ClientboundGamePacket::BlockUpdate(update) => {
                self.set_block(update.pos, update.block_state);
            }
            ClientboundGamePacket::SectionBlocksUpdate(update) => {
                let origin = BlockPos::new(
                    update.section_pos.x * 16,
                    update.section_pos.y * 16,
                    update.section_pos.z * 16,
                );
                for change in &update.states {
                    let pos = BlockPos::new(
                        origin.x + i32::from(change.pos.x),
                        origin.y + i32::from(change.pos.y),
                        origin.z + i32::from(change.pos.z),
                    );
                    self.set_block(pos, change.state);
                }
            }
            ClientboundGamePacket::AddEntity(add) => {
                let entity = SeenEntity {
                    uuid: add.uuid,
                    kind: add.entity_type,
                    position: add.position,
                };
                if self.entities.insert(add.id.0, entity).is_some() {
                    self.anomalies
                        .push(format!("entity {} added twice", add.id.0));
                }
            }
            ClientboundGamePacket::RemoveEntities(remove) => {
                for id in &remove.entity_ids {
                    self.equipment.remove(&id.0);
                    if self.entities.remove(&id.0).is_none() {
                        self.anomalies
                            .push(format!("removal of unknown entity {}", id.0));
                    }
                }
            }
            ClientboundGamePacket::MoveEntityPos(packet) => {
                self.move_entity(packet.entity_id.0, |position| {
                    *position = position.with_delta(&packet.delta)
                });
            }
            ClientboundGamePacket::MoveEntityPosRot(packet) => {
                self.move_entity(packet.entity_id.0, |position| {
                    *position = position.with_delta(&packet.delta)
                });
            }
            ClientboundGamePacket::EntityPositionSync(packet) => {
                self.move_entity(packet.id.0, |position| *position = packet.values.pos);
            }
            ClientboundGamePacket::TeleportEntity(packet) => {
                self.move_entity(packet.id.0, |position| {
                    apply_movement(position, &packet.change, &packet.relative)
                });
            }
            ClientboundGamePacket::SetEquipment(packet) => {
                let id = packet.entity_id.0;
                if !self.entities.contains_key(&id) {
                    self.anomalies
                        .push(format!("equipment of unknown entity {id}"));
                }
                let slots = self.equipment.entry(id).or_default();
                for (slot, item) in &packet.slots.slots {
                    slots.insert(*slot, item.clone());
                }
            }
            ClientboundGamePacket::PlayerInfoUpdate(update) if update.actions.add_player => {
                for entry in &update.entries {
                    self.players
                        .insert(entry.profile.uuid, entry.profile.name.clone());
                }
            }
            ClientboundGamePacket::PlayerInfoRemove(remove) => {
                for id in &remove.profile_ids {
                    self.players.remove(id);
                }
            }
            ClientboundGamePacket::ContainerSetContent(content)
                if content.container_id == PLAYER_INVENTORY =>
            {
                self.inventory = content.items.clone();
                self.cursor = content.carried_item.clone();
                self.inventory_state_id = content.state_id;
            }
            ClientboundGamePacket::ContainerSetSlot(slot)
                if slot.container_id == PLAYER_INVENTORY =>
            {
                let index = usize::from(slot.slot);
                if index >= self.inventory.len() {
                    return Err(format!("inventory slot {index} out of range"));
                }
                self.inventory[index] = slot.item_stack.clone();
                self.inventory_state_id = slot.state_id;
            }
            ClientboundGamePacket::SetCursorItem(cursor) => {
                self.cursor = cursor.contents.clone();
            }
            _ => {}
        }
        Ok(())
    }

    fn set_block(&mut self, pos: BlockPos, state: BlockState) {
        let min_y = self.min_y;
        match self.chunks.get_mut(&ChunkPos::from(pos)) {
            Some(chunk) => chunk.set_block_state(&ChunkBlockPos::from(pos), state, min_y),
            None => self
                .anomalies
                .push(format!("block update in unloaded chunk at {pos:?}")),
        }
    }

    fn move_entity(&mut self, id: i32, f: impl FnOnce(&mut Vec3)) {
        match self.entities.get_mut(&id) {
            Some(entity) => f(&mut entity.position),
            None => self
                .anomalies
                .push(format!("movement of unknown entity {id}")),
        }
    }
}

fn apply_movement(
    position: &mut Vec3,
    change: &PositionMoveRotation,
    relative: &RelativeMovements,
) {
    position.x = if relative.x {
        position.x + change.pos.x
    } else {
        change.pos.x
    };
    position.y = if relative.y {
        position.y + change.pos.y
    } else {
        change.pos.y
    };
    position.z = if relative.z {
        position.z + change.pos.z
    } else {
        change.pos.z
    };
}

#[cfg(test)]
mod tests {
    use azalea_protocol::packets::game::c_container_set_slot::ClientboundContainerSetSlot;
    use azalea_protocol::packets::game::c_game_event::ClientboundGameEvent;
    use azalea_registry::builtin::ItemKind;

    use super::*;

    fn view() -> ClientView {
        ClientView::new(&RegistryHolder::default())
    }

    fn shown(view: &ClientView, slot: usize) -> (ItemKind, i32) {
        (view.inventory[slot].kind(), view.inventory[slot].count())
    }

    #[test]
    fn a_predicted_click_shows_at_once_until_the_server_corrects_it() {
        let mut view = view();
        view.inventory[9] = ItemStack::new(ItemKind::Diamond, 10);
        view.inventory_state_id = 7;

        let click = view.predict_click(9, 1, ClickType::Pickup);
        assert_eq!(click.state_id, 7, "sent with the last state id received");
        assert_eq!(click.changed, vec![(9, Some((ItemKind::Diamond, 5)))]);
        assert_eq!(click.carried, Some((ItemKind::Diamond, 5)));
        assert_eq!(shown(&view, 9), (ItemKind::Diamond, 10), "not shown yet");

        view.show_prediction(&click);
        assert_eq!(shown(&view, 9), (ItemKind::Diamond, 5));
        assert_eq!(view.cursor.count(), 5);
        assert_eq!(
            view.inventory_state_id, 7,
            "a prediction keeps the state id"
        );

        let correction = ClientboundGamePacket::ContainerSetSlot(ClientboundContainerSetSlot {
            container_id: 0,
            state_id: 8,
            slot: 9,
            item_stack: ItemStack::new(ItemKind::Diamond, 10),
        });
        view.apply(&correction).unwrap();
        assert_eq!(shown(&view, 9), (ItemKind::Diamond, 10));
        assert_eq!(view.inventory_state_id, 8);
    }

    #[test]
    fn a_drag_lasts_across_clicks_and_a_clone_needs_creative() {
        let mut view = view();
        view.cursor = ItemStack::new(ItemKind::Stone, 9);
        for (slot, button) in [(-999, 0), (9, 1), (10, 1), (11, 1), (-999, 2)] {
            let click = view.predict_click(slot, button, ClickType::QuickCraft);
            view.show_prediction(&click);
        }
        for slot in 9..=11 {
            assert_eq!(shown(&view, slot), (ItemKind::Stone, 3));
        }
        assert_eq!(view.cursor, ItemStack::Empty);

        let clone = view.predict_click(9, 2, ClickType::Clone);
        assert_eq!(clone.carried, None, "a survival player clones nothing");
        view.apply(&ClientboundGamePacket::GameEvent(ClientboundGameEvent {
            event: EventType::ChangeGameMode,
            param: 1.0,
        }))
        .unwrap();
        assert_eq!(view.game_mode, GameMode::Creative);
        let clone = view.predict_click(9, 2, ClickType::Clone);
        assert_eq!(clone.carried, Some((ItemKind::Stone, 64)));
    }
}
