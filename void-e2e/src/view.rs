use std::collections::HashMap;
use std::io::Cursor;

use azalea_block::BlockState;
use azalea_core::position::{BlockPos, ChunkBlockPos, ChunkPos, Vec3};
use azalea_core::registry_holder::RegistryHolder;
use azalea_inventory::ItemStack;
use azalea_protocol::common::movements::{PositionMoveRotation, RelativeMovements};
use azalea_protocol::packets::game::ClientboundGamePacket;
use azalea_registry::builtin::EntityKind;
use azalea_world::Chunk;
use uuid::Uuid;

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
#[derive(Debug, Default)]
pub struct ClientView {
    pub entity_id: Option<i32>,
    pub position: Vec3,
    pub chunk_center: Option<ChunkPos>,
    pub chunks: HashMap<ChunkPos, Chunk>,
    pub entities: HashMap<i32, SeenEntity>,
    pub players: HashMap<Uuid, String>,
    pub inventory: Vec<ItemStack>,
    pub cursor: ItemStack,
    pub inventory_state_id: u32,
    pub anomalies: Vec<String>,
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
