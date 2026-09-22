//! Synched entity data. [`EntityMetadata`] holds the raw indexed values with
//! dirty tracking; typed components ([`CustomName`], [`Glowing`],
//! [`Display`], ...) are projected into it, and one `PostUpdate` system sends
//! only the dirty indices to current viewers.

use std::collections::{BTreeMap, BTreeSet};

use bevy_app::{App, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use ussr_nbt::owned::Nbt;
use voidmc_protocol::clientbound::entity_metadata::{
    display_index, end_crystal_index, entity_flag, entity_index, item_entity_index,
    text_display_flag,
};
use voidmc_protocol::clientbound::{
    EntityMetadataEntry, EntityMetadataValue as Value, SetEntityData,
};

pub use voidmc_protocol::clientbound::{
    Billboard, DisplayTransform, ItemDisplayContext, MAX_TELEPORT_TICKS,
};
use voidmc_protocol::types::BlockPosition;

use super::EntityShownEvent;
use crate::components::{EntityViewers, ItemEntity, MinecraftEntityId, SpawnedEntity};
use crate::item::ItemStack;
use crate::players::Players;
use crate::schedule::VoidSystems;

#[derive(Component, Debug, Default, Clone)]
pub struct EntityMetadata {
    entries: BTreeMap<u8, Value>,
    dirty: BTreeSet<u8>,
}

impl EntityMetadata {
    pub fn get(&self, index: u8) -> Option<&Value> {
        self.entries.get(&index)
    }

    pub fn set(&mut self, index: u8, value: Value) {
        if self.entries.get(&index) != Some(&value) {
            self.entries.insert(index, value);
            self.dirty.insert(index);
        }
    }

    pub fn touch(&mut self, index: u8) {
        if self.entries.contains_key(&index) {
            self.dirty.insert(index);
        }
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    pub fn entries(&self) -> Vec<EntityMetadataEntry> {
        self.entries
            .iter()
            .map(|(index, value)| EntityMetadataEntry {
                index: *index,
                value: value.clone(),
            })
            .collect()
    }

    fn take_dirty(&mut self) -> Vec<EntityMetadataEntry> {
        let dirty = std::mem::take(&mut self.dirty);
        dirty
            .into_iter()
            .filter_map(|index| {
                self.entries.get(&index).map(|value| EntityMetadataEntry {
                    index,
                    value: value.clone(),
                })
            })
            .collect()
    }
}

pub fn text_component(text: &str) -> Nbt {
    crate::messages::plain_text_component(text)
}

pub trait MetadataSource: Component {
    fn write(&self, meta: &mut EntityMetadata);

    fn clear(_meta: &mut EntityMetadata) {}
}

#[derive(Component, Debug, Clone, PartialEq)]
pub struct CustomName {
    pub text: String,
    pub visible: bool,
}

impl CustomName {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            visible: true,
        }
    }

    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }
}

impl MetadataSource for CustomName {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(
            entity_index::CUSTOM_NAME,
            Value::OptionalComponent(Some(text_component(&self.text))),
        );
        meta.set(
            entity_index::CUSTOM_NAME_VISIBLE,
            Value::Boolean(self.visible),
        );
    }

    fn clear(meta: &mut EntityMetadata) {
        meta.set(entity_index::CUSTOM_NAME, Value::OptionalComponent(None));
        meta.set(entity_index::CUSTOM_NAME_VISIBLE, Value::Boolean(false));
    }
}

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Glowing;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Invisible;

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct NoGravity;

impl MetadataSource for NoGravity {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(entity_index::NO_GRAVITY, Value::Boolean(true));
    }

    fn clear(meta: &mut EntityMetadata) {
        meta.set(entity_index::NO_GRAVITY, Value::Boolean(false));
    }
}

#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Silent;

impl MetadataSource for Silent {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(entity_index::SILENT, Value::Boolean(true));
    }

    fn clear(meta: &mut EntityMetadata) {
        meta.set(entity_index::SILENT, Value::Boolean(false));
    }
}

impl MetadataSource for ItemEntity {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(
            item_entity_index::ITEM,
            Value::ItemStack(self.stack.to_slot()),
        );
    }

    fn clear(meta: &mut EntityMetadata) {
        meta.set(
            item_entity_index::ITEM,
            Value::ItemStack(ItemStack::EMPTY.to_slot()),
        );
    }
}

/// Synched data of a `minecraft:end_crystal` entity. The default is the
/// floating variant without the bedrock base and without a beam.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EndCrystal {
    pub show_base: bool,
    pub beam_target: Option<BlockPosition>,
}

impl EndCrystal {
    pub fn floating() -> Self {
        Self::default()
    }

    pub fn show_base(mut self) -> Self {
        self.show_base = true;
        self
    }

    pub fn beam_target(mut self, x: i32, y: i16, z: i32) -> Self {
        self.beam_target = Some(BlockPosition { x, y, z });
        self
    }
}

impl MetadataSource for EndCrystal {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(
            end_crystal_index::BEAM_TARGET,
            Value::OptionalBlockPos(self.beam_target),
        );
        meta.set(
            end_crystal_index::SHOW_BOTTOM,
            Value::Boolean(self.show_base),
        );
    }

    fn clear(meta: &mut EntityMetadata) {
        EndCrystal::default().show_base().write(meta);
    }
}

/// Shared settings of every `*_display` entity; required by [`BlockDisplay`],
/// [`ItemDisplay`] and [`TextDisplay`]. Assigning `transform` starts a new
/// keyframe interpolated over `interpolation_ticks`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Display {
    pub transform: DisplayTransform,
    pub interpolation_ticks: u16,
    pub teleport_ticks: u8,
    pub billboard: Billboard,
    pub brightness: Option<(u8, u8)>,
    pub view_range: f32,
    pub shadow_radius: f32,
    pub shadow_strength: f32,
    pub width: f32,
    pub height: f32,
    pub glow_color: Option<i32>,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            transform: DisplayTransform::default(),
            interpolation_ticks: 0,
            teleport_ticks: 0,
            billboard: Billboard::Fixed,
            brightness: None,
            view_range: 1.0,
            shadow_radius: 0.0,
            shadow_strength: 1.0,
            width: 0.0,
            height: 0.0,
            glow_color: None,
        }
    }
}

impl Display {
    pub fn transform(mut self, transform: DisplayTransform) -> Self {
        self.transform = transform;
        self
    }

    pub fn interpolation_ticks(mut self, ticks: u16) -> Self {
        self.interpolation_ticks = ticks;
        self
    }

    pub fn teleport_ticks(mut self, ticks: u8) -> Self {
        self.teleport_ticks = ticks;
        self
    }

    pub fn billboard(mut self, billboard: Billboard) -> Self {
        self.billboard = billboard;
        self
    }

    pub fn brightness(mut self, block: u8, sky: u8) -> Self {
        self.brightness = Some((block, sky));
        self
    }

    pub fn view_range(mut self, view_range: f32) -> Self {
        self.view_range = view_range;
        self
    }

    pub fn shadow(mut self, radius: f32, strength: f32) -> Self {
        self.shadow_radius = radius;
        self.shadow_strength = strength;
        self
    }

    pub fn glow_color(mut self, argb: i32) -> Self {
        self.glow_color = Some(argb);
        self
    }

    /// `width`/`height` are the client's frustum-culling box around the entity
    /// position, not the render size; a translated or scaled display left at
    /// 0 flickers at screen edges.
    pub fn culling_box(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }
}

impl MetadataSource for Display {
    fn write(&self, meta: &mut EntityMetadata) {
        for entry in self.transform.entries(self.interpolation_ticks) {
            meta.set(entry.index, entry.value);
        }
        meta.touch(display_index::TRANSFORMATION_START_DELTA_TICKS);
        meta.set(
            display_index::POS_ROT_DURATION,
            Value::Int(i32::from(self.teleport_ticks.min(MAX_TELEPORT_TICKS))),
        );
        meta.set(display_index::BILLBOARD, Value::Byte(self.billboard as i8));
        meta.set(
            display_index::BRIGHTNESS,
            Value::Int(voidmc_protocol::clientbound::pack_brightness(
                self.brightness,
            )),
        );
        meta.set(display_index::VIEW_RANGE, Value::Float(self.view_range));
        meta.set(
            display_index::SHADOW_RADIUS,
            Value::Float(self.shadow_radius),
        );
        meta.set(
            display_index::SHADOW_STRENGTH,
            Value::Float(self.shadow_strength),
        );
        meta.set(display_index::WIDTH, Value::Float(self.width));
        meta.set(display_index::HEIGHT, Value::Float(self.height));
        meta.set(
            display_index::GLOW_COLOR,
            Value::Int(self.glow_color.unwrap_or(-1)),
        );
    }

    fn clear(meta: &mut EntityMetadata) {
        Display::default().write(meta);
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[require(Display)]
pub struct BlockDisplay(pub i32);

impl MetadataSource for BlockDisplay {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(display_index::BLOCK_STATE, Value::BlockState(self.0));
    }

    fn clear(meta: &mut EntityMetadata) {
        BlockDisplay(voidmc_data::v26_1_2::blocks::AIR).write(meta);
    }
}

#[derive(Component, Debug, Clone, PartialEq)]
#[require(Display)]
pub struct ItemDisplay {
    pub item: ItemStack,
    pub context: ItemDisplayContext,
}

impl ItemDisplay {
    pub fn new(item: ItemStack) -> Self {
        Self {
            item,
            context: ItemDisplayContext::Fixed,
        }
    }

    pub fn context(mut self, context: ItemDisplayContext) -> Self {
        self.context = context;
        self
    }
}

impl MetadataSource for ItemDisplay {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(display_index::ITEM, Value::ItemStack(self.item.to_slot()));
        meta.set(
            display_index::ITEM_DISPLAY_CONTEXT,
            Value::Byte(self.context as i8),
        );
    }

    fn clear(meta: &mut EntityMetadata) {
        ItemDisplay::new(ItemStack::EMPTY)
            .context(ItemDisplayContext::None)
            .write(meta);
    }
}

#[derive(Component, Debug, Clone, PartialEq)]
#[require(Display)]
pub struct TextDisplay {
    pub text: String,
    pub line_width: i32,
    pub background_color: Option<i32>,
    pub opacity: i8,
    pub shadow: bool,
    pub see_through: bool,
    pub alignment: TextAlignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlignment {
    #[default]
    Center,
    Left,
    Right,
}

impl TextDisplay {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            line_width: 200,
            background_color: None,
            opacity: -1,
            shadow: false,
            see_through: false,
            alignment: TextAlignment::Center,
        }
    }

    pub fn line_width(mut self, width: i32) -> Self {
        self.line_width = width;
        self
    }

    pub fn background_color(mut self, argb: i32) -> Self {
        self.background_color = Some(argb);
        self
    }

    pub fn shadow(mut self) -> Self {
        self.shadow = true;
        self
    }

    pub fn see_through(mut self) -> Self {
        self.see_through = true;
        self
    }

    pub fn alignment(mut self, alignment: TextAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    fn flags(&self) -> u8 {
        let mut flags = match self.alignment {
            TextAlignment::Center => text_display_flag::ALIGN_CENTER,
            TextAlignment::Left => text_display_flag::ALIGN_LEFT,
            TextAlignment::Right => text_display_flag::ALIGN_RIGHT,
        };
        if self.shadow {
            flags |= text_display_flag::SHADOW;
        }
        if self.see_through {
            flags |= text_display_flag::SEE_THROUGH;
        }
        if self.background_color.is_none() {
            flags |= text_display_flag::DEFAULT_BACKGROUND;
        }
        flags
    }
}

const TEXT_DISPLAY_DEFAULT_BACKGROUND: i32 = 0x4000_0000;

impl MetadataSource for TextDisplay {
    fn write(&self, meta: &mut EntityMetadata) {
        meta.set(
            display_index::TEXT,
            Value::Component(text_component(&self.text)),
        );
        meta.set(display_index::TEXT_LINE_WIDTH, Value::Int(self.line_width));
        meta.set(
            display_index::TEXT_BACKGROUND_COLOR,
            Value::Int(
                self.background_color
                    .unwrap_or(TEXT_DISPLAY_DEFAULT_BACKGROUND),
            ),
        );
        meta.set(display_index::TEXT_OPACITY, Value::Byte(self.opacity));
        meta.set(display_index::TEXT_FLAGS, Value::Byte(self.flags() as i8));
    }

    fn clear(meta: &mut EntityMetadata) {
        TextDisplay::new("").write(meta);
    }
}

fn flags_byte(invisible: bool, glowing: bool) -> i8 {
    let mut flags = 0u8;
    if invisible {
        flags |= entity_flag::INVISIBLE;
    }
    if glowing {
        flags |= entity_flag::GLOWING;
    }
    flags as i8
}

fn project_flags(
    mut entities: Query<
        (Option<&Invisible>, Option<&Glowing>, &mut EntityMetadata),
        Or<(Added<Invisible>, Added<Glowing>)>,
    >,
) {
    for (invisible, glowing, mut meta) in entities.iter_mut() {
        meta.set(
            entity_index::FLAGS,
            Value::Byte(flags_byte(invisible.is_some(), glowing.is_some())),
        );
    }
}

fn on_remove_invisible(
    event: On<Remove, Invisible>,
    mut entities: Query<(Option<&Glowing>, &mut EntityMetadata)>,
) {
    if let Ok((glowing, mut meta)) = entities.get_mut(event.entity) {
        meta.set(
            entity_index::FLAGS,
            Value::Byte(flags_byte(false, glowing.is_some())),
        );
    }
}

fn on_remove_glowing(
    event: On<Remove, Glowing>,
    mut entities: Query<(Option<&Invisible>, &mut EntityMetadata)>,
) {
    if let Ok((invisible, mut meta)) = entities.get_mut(event.entity) {
        meta.set(
            entity_index::FLAGS,
            Value::Byte(flags_byte(invisible.is_some(), false)),
        );
    }
}

fn project<T: MetadataSource>(mut entities: Query<(&T, &mut EntityMetadata), Changed<T>>) {
    for (source, mut meta) in entities.iter_mut() {
        source.write(&mut meta);
    }
}

fn on_remove<T: MetadataSource>(event: On<Remove, T>, mut entities: Query<&mut EntityMetadata>) {
    if let Ok(mut meta) = entities.get_mut(event.entity) {
        T::clear(&mut meta);
    }
}

/// Registers a [`MetadataSource`]: its projection runs in
/// `VoidSystems::EntityMetadataSync` before the send, and removing the
/// component restores its defaults through [`MetadataSource::clear`].
pub trait MetadataSourceAppExt {
    fn add_metadata_source<T: MetadataSource>(&mut self) -> &mut Self;
}

impl MetadataSourceAppExt for App {
    fn add_metadata_source<T: MetadataSource>(&mut self) -> &mut Self {
        self.add_observer(on_remove::<T>).add_systems(
            PostUpdate,
            project::<T>
                .before(sync_entity_metadata)
                .in_set(VoidSystems::EntityMetadataSync),
        )
    }
}

pub(super) fn register(app: &mut App) {
    app.add_observer(on_remove_invisible)
        .add_observer(on_remove_glowing)
        .add_observer(send_full_metadata_on_shown)
        .add_systems(
            PostUpdate,
            (
                project_flags.before(sync_entity_metadata),
                sync_entity_metadata,
            )
                .in_set(VoidSystems::EntityMetadataSync),
        )
        .add_metadata_source::<CustomName>()
        .add_metadata_source::<NoGravity>()
        .add_metadata_source::<Silent>()
        .add_metadata_source::<ItemEntity>()
        .add_metadata_source::<EndCrystal>()
        .add_metadata_source::<Display>()
        .add_metadata_source::<BlockDisplay>()
        .add_metadata_source::<ItemDisplay>()
        .add_metadata_source::<TextDisplay>();
}

fn sync_entity_metadata(
    players: Players,
    mut entities: Query<
        (&MinecraftEntityId, &EntityViewers, &mut EntityMetadata),
        (With<SpawnedEntity>, Changed<EntityMetadata>),
    >,
) {
    let ready = players.ready();
    for (id, viewers, mut meta) in entities.iter_mut() {
        if !meta.is_dirty() {
            continue;
        }
        let entries = meta.bypass_change_detection().take_dirty();
        if viewers.is_empty() || entries.is_empty() {
            continue;
        }
        let packet = SetEntityData {
            entity_id: id.0,
            entries,
        };
        ready.send_where(|r| viewers.contains(r.entity()), packet);
    }
}

fn send_full_metadata_on_shown(
    event: On<EntityShownEvent>,
    players: Players,
    entities: Query<(&MinecraftEntityId, &EntityMetadata)>,
) {
    let Ok((id, meta)) = entities.get(event.entity) else {
        return;
    };
    let entries = meta.entries();
    if entries.is_empty() {
        return;
    }
    players.send(
        event.viewer,
        SetEntityData {
            entity_id: id.0,
            entries,
        },
    );
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use voidmc_codec::Encode;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
    use crate::entity::{EntityBuilder, EntityKind, EntityPlugin};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::world::{ChunkPos, DimensionId};

    #[test]
    fn oversized_custom_name_round_trips_below_the_nbt_limit() {
        let text = "😀".repeat(11000);
        crate::messages::assert_guarded(&text_component(&text), &text);
    }

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
        .configure_sets(
            PostUpdate,
            (
                VoidSystems::EntityMetadataSync,
                VoidSystems::EntityVisibility,
            )
                .chain(),
        )
        .add_plugins(EntityPlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(id),
                PlayerReady,
                PlayerDimension(DimensionId::Overworld),
                LoadedChunks([ChunkPos::new(0, 0)].into_iter().collect()),
            ))
            .id()
    }

    fn metadata(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, Vec<(u8, i32)>)> {
        rx.try_iter()
            .filter_map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::SetEntityData(p)) => Some((
                    out.client_id,
                    p.entries
                        .iter()
                        .map(|e| (e.index, e.value.serializer_id()))
                        .collect(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn set_marks_dirty_only_on_change_and_touch_always() {
        let mut meta = EntityMetadata::default();
        meta.set(5, Value::Boolean(true));
        assert!(meta.is_dirty());
        assert_eq!(meta.take_dirty().len(), 1);
        meta.set(5, Value::Boolean(true));
        assert!(!meta.is_dirty());
        meta.touch(5);
        assert!(meta.is_dirty());
        meta.touch(9);
        assert_eq!(meta.take_dirty().len(), 1);
        assert_eq!(meta.entries().len(), 1);
    }

    #[test]
    fn typed_components_project_and_only_changes_are_sent() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .insert((CustomName::new("Bob"), Glowing))
            .id();
        app.update();
        let sent = metadata(&rx);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].1, vec![(0, 0), (2, 6), (3, 8)]);
        assert_eq!(
            app.world().get::<EntityMetadata>(zombie).unwrap().get(0),
            Some(&Value::Byte(0x40))
        );

        app.world_mut().get_mut::<CustomName>(zombie).unwrap().text = "Alice".into();
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(2, 6)])]);

        app.world_mut().entity_mut(zombie).insert(Invisible);
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(0, 0)])]);
        assert_eq!(
            app.world().get::<EntityMetadata>(zombie).unwrap().get(0),
            Some(&Value::Byte(0x60))
        );

        app.world_mut().entity_mut(zombie).remove::<Glowing>();
        app.world_mut().entity_mut(zombie).remove::<CustomName>();
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(0, 0), (2, 6), (3, 8)])]);
        let meta = app.world().get::<EntityMetadata>(zombie).unwrap();
        assert_eq!(meta.get(0), Some(&Value::Byte(0x20)));
        assert_eq!(meta.get(2), Some(&Value::OptionalComponent(None)));

        app.update();
        assert!(metadata(&rx).is_empty());
    }

    #[test]
    fn late_viewer_gets_the_full_state_once() {
        let (mut app, rx) = test_app();
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .insert((NoGravity, Silent))
            .id();
        app.update();
        assert!(metadata(&rx).is_empty());

        player(&mut app, 7);
        app.update();
        assert_eq!(metadata(&rx), vec![(7, vec![(4, 8), (5, 8)])]);

        app.world_mut().entity_mut(zombie).remove::<NoGravity>();
        app.update();
        assert_eq!(metadata(&rx), vec![(7, vec![(5, 8)])]);
    }

    #[test]
    fn display_keyframes_always_resend_the_clock_reset() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1);
        let shield = EntityBuilder::new(EntityKind::BlockDisplay)
            .spawn_in(app.world_mut())
            .insert(BlockDisplay(300))
            .id();
        app.update();
        let sent = metadata(&rx);
        let indices: Vec<u8> = sent[0].1.iter().map(|(i, _)| *i).collect();
        assert_eq!(indices, (8..=23).collect::<Vec<u8>>());
        assert_eq!(sent[0].1[15], (23, 14));

        app.world_mut()
            .get_mut::<Display>(shield)
            .unwrap()
            .transform = DisplayTransform::default().translation(1.0, 0.0, 0.0);
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(8, 1), (11, 39)])]);

        app.world_mut()
            .get_mut::<Display>(shield)
            .unwrap()
            .transform = DisplayTransform::default().translation(1.0, 0.0, 0.0);
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(8, 1)])]);
    }

    #[test]
    fn removing_display_components_restores_defaults_before_a_later_show() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1);
        let shield = EntityBuilder::new(EntityKind::BlockDisplay)
            .spawn_in(app.world_mut())
            .insert((
                BlockDisplay(300),
                Display::default().brightness(15, 15).view_range(3.0),
            ))
            .id();
        app.update();
        metadata(&rx);

        app.world_mut()
            .entity_mut(shield)
            .remove::<(BlockDisplay, Display)>();
        app.update();
        assert_eq!(metadata(&rx).len(), 1);
        let mut expected = EntityMetadata::default();
        Display::default().write(&mut expected);
        BlockDisplay(0).write(&mut expected);
        let meta = app.world().get::<EntityMetadata>(shield).unwrap();
        assert_eq!(meta.entries(), expected.entries());

        let late = player(&mut app, 2);
        app.update();
        let sent = metadata(&rx);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, 2);
        let _ = late;
    }

    #[test]
    fn downstream_metadata_source_registers_through_the_app_extension() {
        #[derive(Component)]
        struct Frozen(i32);

        impl MetadataSource for Frozen {
            fn write(&self, meta: &mut EntityMetadata) {
                meta.set(entity_index::TICKS_FROZEN, Value::Int(self.0));
            }

            fn clear(meta: &mut EntityMetadata) {
                meta.set(entity_index::TICKS_FROZEN, Value::Int(0));
            }
        }

        let (mut app, rx) = test_app();
        app.add_metadata_source::<Frozen>();
        let _viewer = player(&mut app, 1);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .insert(Frozen(140))
            .id();
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(7, 1)])]);
        app.world_mut().entity_mut(zombie).remove::<Frozen>();
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(7, 1)])]);
        assert_eq!(
            app.world().get::<EntityMetadata>(zombie).unwrap().get(7),
            Some(&Value::Int(0))
        );
    }

    #[test]
    fn item_and_text_displays_write_their_indices() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1);
        EntityBuilder::new(EntityKind::ItemDisplay)
            .spawn_in(app.world_mut())
            .insert(
                ItemDisplay::new(ItemStack::new(crate::item::ItemId(1), 1))
                    .context(ItemDisplayContext::Ground),
            );
        EntityBuilder::new(EntityKind::TextDisplay)
            .spawn_in(app.world_mut())
            .insert(
                TextDisplay::new("hi")
                    .shadow()
                    .alignment(TextAlignment::Right)
                    .background_color(0x1000_0000),
            );
        app.update();
        let mut sent = metadata(&rx);
        sent.sort_by_key(|(_, e)| e.len());
        assert!(sent[0].1.ends_with(&[(23, 7), (24, 0)]));
        assert!(
            sent[1]
                .1
                .ends_with(&[(23, 5), (24, 1), (25, 1), (26, 0), (27, 0)])
        );
        let text = TextDisplay::new("x")
            .shadow()
            .alignment(TextAlignment::Right);
        assert_eq!(text.flags(), 0b0001_0101);
    }

    #[test]
    fn end_crystal_writes_its_indices_and_clears_to_vanilla_defaults() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1);
        let crystal = EntityBuilder::new(EntityKind::EndCrystal)
            .spawn_in(app.world_mut())
            .insert(EndCrystal::floating())
            .id();
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(8, 11), (9, 8)])]);
        let meta = app.world().get::<EntityMetadata>(crystal).unwrap();
        assert_eq!(meta.get(8), Some(&Value::OptionalBlockPos(None)));
        assert_eq!(meta.get(9), Some(&Value::Boolean(false)));
        let mut bytes = Vec::new();
        SetEntityData {
            entity_id: 0,
            entries: meta.entries(),
        }
        .encode(&mut bytes);
        assert_eq!(bytes, [0, 8, 11, 0, 9, 8, 0, 0xFF]);

        app.update();
        assert!(metadata(&rx).is_empty());

        app.world_mut()
            .get_mut::<EndCrystal>(crystal)
            .unwrap()
            .beam_target = Some(BlockPosition { x: 1, y: 2, z: 3 });
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(8, 11)])]);
        let mut bytes = Vec::new();
        SetEntityData {
            entity_id: 0,
            entries: app
                .world()
                .get::<EntityMetadata>(crystal)
                .unwrap()
                .entries(),
        }
        .encode(&mut bytes);
        assert_eq!(
            bytes,
            [0, 8, 11, 1, 0, 0, 0, 0x40, 0, 0, 0x30, 0x02, 9, 8, 0, 0xFF]
        );

        app.world_mut().entity_mut(crystal).remove::<EndCrystal>();
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(8, 11), (9, 8)])]);
        let meta = app.world().get::<EntityMetadata>(crystal).unwrap();
        assert_eq!(meta.get(8), Some(&Value::OptionalBlockPos(None)));
        assert_eq!(meta.get(9), Some(&Value::Boolean(true)));
    }

    #[test]
    fn dropped_item_metadata_flows_through_entity_metadata() {
        let (mut app, rx) = test_app();
        let _viewer = player(&mut app, 1);
        EntityBuilder::new(EntityKind::Item)
            .spawn_in(app.world_mut())
            .insert(ItemEntity {
                stack: ItemStack::new(crate::item::ItemId(5), 3),
            });
        app.update();
        assert_eq!(metadata(&rx), vec![(1, vec![(8, 7)])]);
    }
}
