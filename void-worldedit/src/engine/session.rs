use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc::{DimensionId, ItemId};

use super::WorldEditConfig;
use crate::brush::Brush;
use crate::clipboard::Clipboard;
use crate::history::History;
use crate::math::BlockPos;
use crate::selection::Selection;

/// One player's editing state: selection, clipboard, undo history and the
/// brushes bound to their items. Inserted the first time the player edits.
#[derive(Component, Debug)]
pub struct EditSession {
    pub selection: Selection,
    pub history: History<DimensionId>,
    clipboard: Option<(Arc<Clipboard>, BlockPos)>,
    brushes: HashMap<ItemId, Brush>,
    edits: u64,
}

impl EditSession {
    pub fn new(config: &WorldEditConfig) -> Self {
        Self {
            selection: Selection::default(),
            history: History::new(config.history_size, config.history_bytes),
            clipboard: None,
            brushes: HashMap::new(),
            edits: 0,
        }
    }

    pub fn clipboard(&self) -> Option<&Arc<Clipboard>> {
        self.clipboard.as_ref().map(|(clipboard, _)| clipboard)
    }

    /// Where the clipboard was copied from, used by `//paste -o`.
    pub fn clipboard_origin(&self) -> Option<BlockPos> {
        self.clipboard.as_ref().map(|(_, origin)| *origin)
    }

    pub fn set_clipboard(&mut self, clipboard: Clipboard, origin: BlockPos) {
        self.clipboard = Some((Arc::new(clipboard), origin));
    }

    pub fn clear_clipboard(&mut self) {
        self.clipboard = None;
    }

    pub fn brush(&self, item: ItemId) -> Option<&Brush> {
        self.brushes.get(&item)
    }

    pub fn brush_mut(&mut self, item: ItemId) -> Option<&mut Brush> {
        self.brushes.get_mut(&item)
    }

    pub fn bind_brush(&mut self, item: ItemId, brush: Brush) {
        self.brushes.insert(item, brush);
    }

    pub fn unbind_brush(&mut self, item: ItemId) -> Option<Brush> {
        self.brushes.remove(&item)
    }

    pub fn has_brushes(&self) -> bool {
        !self.brushes.is_empty()
    }

    pub(crate) fn next_seed(&mut self) -> u64 {
        self.edits += 1;
        self.edits.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }
}

pub fn session_mut(world: &mut World, player: Entity) -> Option<Mut<'_, EditSession>> {
    if world.get_entity(player).is_err() {
        return None;
    }
    if world.get::<EditSession>(player).is_none() {
        let session = EditSession::new(world.resource::<WorldEditConfig>());
        world.entity_mut(player).insert(session);
    }
    world.get_mut::<EditSession>(player)
}
