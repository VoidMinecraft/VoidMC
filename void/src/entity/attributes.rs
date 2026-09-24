//! Attribute base values and modifiers. [`Attributes`] is the server-side
//! truth for every attribute an entity overrides; one `PostUpdate` system
//! sends only the attributes that changed, and only the ones vanilla syncs
//! to clients (`EntityAttribute::is_client_syncable`), to the entity's own
//! client and to its viewers. Defaults are the entity kind's vanilla
//! `AttributeSupplier` (`EntityKind::default_attribute`), not the attribute
//! registry's: a player walks at 0.1, a zombie at 0.23.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use bevy_app::{App, PostUpdate};
use bevy_ecs::lifecycle::{Insert, Remove};
use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::{AttributeModifier, AttributeSnapshot, UpdateAttributes};

pub use voidmc_data::v26_1_2::EntityAttribute;
pub use voidmc_protocol::clientbound::ModifierOperation;

use super::{EntityKind, EntityShownEvent};
use crate::components::{ClientId, EntityType, EntityViewers, MinecraftEntityId};
use crate::players::Players;
use crate::schedule::VoidSystems;

#[derive(Debug, Clone, PartialEq)]
pub struct Modifier {
    pub id: String,
    pub amount: f64,
    pub operation: ModifierOperation,
}

impl Modifier {
    pub fn new(id: impl Into<String>, amount: f64, operation: ModifierOperation) -> Self {
        Self {
            id: id.into(),
            amount,
            operation,
        }
    }

    pub fn add(id: impl Into<String>, amount: f64) -> Self {
        Self::new(id, amount, ModifierOperation::AddValue)
    }

    pub fn multiply_base(id: impl Into<String>, amount: f64) -> Self {
        Self::new(id, amount, ModifierOperation::AddMultipliedBase)
    }

    pub fn multiply_total(id: impl Into<String>, amount: f64) -> Self {
        Self::new(id, amount, ModifierOperation::AddMultipliedTotal)
    }

    fn wire(&self) -> AttributeModifier {
        AttributeModifier {
            id: self.id.clone(),
            amount: self.amount,
            operation: self.operation,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttributeInstance {
    base: f64,
    base_is_default: bool,
    modifiers: BTreeMap<String, Modifier>,
}

impl AttributeInstance {
    fn seeded(base: f64) -> Self {
        Self {
            base,
            base_is_default: true,
            modifiers: BTreeMap::new(),
        }
    }

    pub fn base(&self) -> f64 {
        self.base
    }

    pub fn modifiers(&self) -> impl Iterator<Item = &Modifier> + '_ {
        self.modifiers.values()
    }

    pub fn modifier(&self, id: &str) -> Option<&Modifier> {
        self.modifiers.get(id)
    }

    /// Vanilla `AttributeInstance.calculateValue`, before clamping.
    fn raw_value(&self) -> f64 {
        let mut value = self.base;
        for modifier in self.modifiers.values() {
            if modifier.operation == ModifierOperation::AddValue {
                value += modifier.amount;
            }
        }
        let mut total = value;
        for modifier in self.modifiers.values() {
            if modifier.operation == ModifierOperation::AddMultipliedBase {
                total += value * modifier.amount;
            }
        }
        for modifier in self.modifiers.values() {
            if modifier.operation == ModifierOperation::AddMultipliedTotal {
                total *= 1.0 + modifier.amount;
            }
        }
        total
    }

    fn snapshot(&self, attribute: EntityAttribute) -> AttributeSnapshot {
        AttributeSnapshot {
            attribute_id: attribute.id(),
            base: self.base,
            modifiers: self.modifiers.values().map(Modifier::wire).collect(),
        }
    }
}

fn clamp(attribute: EntityAttribute, value: f64) -> f64 {
    let (min, max) = attribute.range();
    if value.is_nan() {
        min
    } else {
        value.clamp(min, max)
    }
}

fn default_snapshot(kind: EntityKind, attribute: EntityAttribute) -> AttributeSnapshot {
    AttributeSnapshot {
        attribute_id: attribute.id(),
        base: kind.default_attribute(attribute),
        modifiers: Vec::new(),
    }
}

/// Attribute overrides of a player or living entity. An attribute that is
/// never touched keeps the entity kind's vanilla default on the client;
/// resetting one sends that default back. [`Attributes::new`] assumes a
/// player; an entity spawned with `EntityBuilder` re-seeds the component to
/// its own kind whenever it is inserted or replaced, so `for_kind` only
/// matters for reading `value()` before insertion.
#[derive(Component, Debug, Clone)]
#[require(AttributesState)]
pub struct Attributes {
    kind: EntityKind,
    attributes: BTreeMap<EntityAttribute, AttributeInstance>,
    dirty: BTreeSet<EntityAttribute>,
}

impl Default for Attributes {
    fn default() -> Self {
        Self::for_player()
    }
}

impl Attributes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_player() -> Self {
        Self::for_kind(EntityKind::Player)
    }

    pub fn for_kind(kind: EntityKind) -> Self {
        Self {
            kind,
            attributes: BTreeMap::new(),
            dirty: BTreeSet::new(),
        }
    }

    pub fn kind(&self) -> EntityKind {
        self.kind
    }

    /// Base vanilla gives this entity kind for `attribute`.
    pub fn default_base(&self, attribute: EntityAttribute) -> f64 {
        self.kind.default_attribute(attribute)
    }

    fn rebase(&mut self, kind: EntityKind) {
        if self.kind == kind {
            return;
        }
        self.kind = kind;
        for (attribute, instance) in self.attributes.iter_mut() {
            if instance.base_is_default {
                let base = kind.default_attribute(*attribute);
                if base != instance.base {
                    instance.base = base;
                    self.dirty.insert(*attribute);
                }
            }
        }
    }

    pub fn base(mut self, attribute: EntityAttribute, base: f64) -> Self {
        self.set_base(attribute, base);
        self
    }

    pub fn modifier(mut self, attribute: EntityAttribute, modifier: Modifier) -> Self {
        self.add_modifier(attribute, modifier);
        self
    }

    fn instance(&mut self, attribute: EntityAttribute) -> &mut AttributeInstance {
        self.dirty.insert(attribute);
        let kind = self.kind;
        self.attributes
            .entry(attribute)
            .or_insert_with(|| AttributeInstance::seeded(kind.default_attribute(attribute)))
    }

    pub fn set_base(&mut self, attribute: EntityAttribute, base: f64) {
        let instance = self.instance(attribute);
        instance.base = base;
        instance.base_is_default = false;
    }

    /// Replaces any modifier with the same id.
    pub fn add_modifier(&mut self, attribute: EntityAttribute, modifier: Modifier) {
        self.instance(attribute)
            .modifiers
            .insert(modifier.id.clone(), modifier);
    }

    pub fn remove_modifier(&mut self, attribute: EntityAttribute, id: &str) -> Option<Modifier> {
        let instance = self.attributes.get_mut(&attribute)?;
        let removed = instance.modifiers.remove(id)?;
        self.dirty.insert(attribute);
        Some(removed)
    }

    pub fn clear_modifiers(&mut self, attribute: EntityAttribute) {
        if let Some(instance) = self.attributes.get_mut(&attribute)
            && !instance.modifiers.is_empty()
        {
            instance.modifiers.clear();
            self.dirty.insert(attribute);
        }
    }

    /// Forgets every override of `attribute`; the client goes back to the
    /// entity kind's vanilla default.
    pub fn reset(&mut self, attribute: EntityAttribute) -> bool {
        let removed = self.attributes.remove(&attribute).is_some();
        if removed {
            self.dirty.insert(attribute);
        }
        removed
    }

    pub fn get(&self, attribute: EntityAttribute) -> Option<&AttributeInstance> {
        self.attributes.get(&attribute)
    }

    pub fn has(&self, attribute: EntityAttribute) -> bool {
        self.attributes.contains_key(&attribute)
    }

    /// Final value the client computes: base, then modifiers in operation
    /// order, clamped to the attribute's range.
    pub fn value(&self, attribute: EntityAttribute) -> f64 {
        let raw = self.attributes.get(&attribute).map_or_else(
            || self.default_base(attribute),
            AttributeInstance::raw_value,
        );
        clamp(attribute, raw)
    }

    pub fn iter(&self) -> impl Iterator<Item = (EntityAttribute, &AttributeInstance)> + '_ {
        self.attributes
            .iter()
            .map(|(attribute, instance)| (*attribute, instance))
    }

    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }

    fn take_dirty(&mut self) -> BTreeSet<EntityAttribute> {
        std::mem::take(&mut self.dirty)
    }

    fn snapshot_of(&self, attribute: EntityAttribute) -> AttributeSnapshot {
        self.attributes.get(&attribute).map_or_else(
            || default_snapshot(self.kind, attribute),
            |i| i.snapshot(attribute),
        )
    }

    fn syncable_snapshots(&self) -> Vec<AttributeSnapshot> {
        self.attributes
            .iter()
            .filter(|(attribute, _)| attribute.is_client_syncable())
            .map(|(attribute, instance)| instance.snapshot(*attribute))
            .collect()
    }
}

/// Per-entity sync bookkeeping. `recipients` is only tracked for players,
/// whose audience is every ready player; tracked entities use
/// [`EntityViewers`].
#[derive(Component, Debug, Default)]
pub struct AttributesState {
    sent: BTreeSet<EntityAttribute>,
    recipients: HashSet<Entity>,
}

pub(super) fn register(app: &mut App) {
    app.add_observer(adopt_entity_kind)
        .add_observer(send_attributes_on_shown)
        .add_observer(reset_on_remove)
        .add_systems(
            PostUpdate,
            sync_attributes.in_set(VoidSystems::EntityMetadataSync),
        );
}

fn packet(id: i32, attributes: Vec<AttributeSnapshot>) -> UpdateAttributes {
    UpdateAttributes {
        entity_id: id,
        attributes,
    }
}

fn sync_attributes(
    players: Players,
    mut entities: Query<(
        &MinecraftEntityId,
        &mut Attributes,
        &mut AttributesState,
        Option<&EntityViewers>,
        Has<ClientId>,
    )>,
) {
    let ready = OnceCell::new();
    for (id, mut attributes, mut state, viewers, is_player) in entities.iter_mut() {
        let changed = attributes.is_changed();
        let mut desired: Option<HashSet<Entity>> = None;
        if is_player {
            let ready = ready.get_or_init(|| players.ready());
            let mut count = 0;
            let same_members = ready.entities().all(|e| {
                count += 1;
                state.recipients.contains(&e)
            }) && count == state.recipients.len();
            if !same_members {
                desired = Some(ready.entities().collect());
            }
        }
        if !changed && desired.is_none() {
            continue;
        }

        if changed {
            let dirty: Vec<EntityAttribute> = attributes
                .bypass_change_detection()
                .take_dirty()
                .into_iter()
                .filter(|a| a.is_client_syncable())
                .collect();
            if !dirty.is_empty() {
                let snapshots: Vec<AttributeSnapshot> = dirty
                    .iter()
                    .map(|attribute| attributes.snapshot_of(*attribute))
                    .collect();
                for attribute in &dirty {
                    if attributes.has(*attribute) {
                        state.sent.insert(*attribute);
                    } else {
                        state.sent.remove(attribute);
                    }
                }
                let kept: Vec<Entity> = if is_player {
                    state
                        .recipients
                        .iter()
                        .copied()
                        .filter(|r| desired.as_ref().is_none_or(|d| d.contains(r)))
                        .collect()
                } else {
                    viewers.map(|v| v.iter().collect()).unwrap_or_default()
                };
                if !kept.is_empty() {
                    players.send_to(kept, packet(id.0, snapshots));
                }
            }
        }

        let Some(desired) = desired else {
            continue;
        };
        if !state.sent.is_empty() {
            let reset: Vec<AttributeSnapshot> = state
                .sent
                .iter()
                .map(|attribute| default_snapshot(attributes.kind, *attribute))
                .collect();
            for gone in state.recipients.difference(&desired) {
                players.send(*gone, packet(id.0, reset.clone()));
            }
            let full = attributes.syncable_snapshots();
            for joined in desired.difference(&state.recipients) {
                players.send(*joined, packet(id.0, full.clone()));
            }
        }
        state.recipients = desired;
    }
}

fn adopt_entity_kind(
    event: On<Insert, Attributes>,
    mut entities: Query<(Option<&EntityType>, &mut Attributes, &AttributesState)>,
) {
    let Ok((entity_type, mut attributes, state)) = entities.get_mut(event.entity) else {
        return;
    };
    if let Some(entity_type) = entity_type {
        match EntityKind::from_id(entity_type.0) {
            Some(kind) => attributes.rebase(kind),
            None => tracing::warn!(
                entity = ?event.entity,
                entity_type = entity_type.0,
                "unknown entity type id: attributes keep their player seed"
            ),
        }
    }
    let dropped: Vec<EntityAttribute> = state
        .sent
        .iter()
        .copied()
        .filter(|attribute| !attributes.has(*attribute))
        .collect();
    if !dropped.is_empty() {
        attributes.dirty.extend(dropped);
    }
}

fn send_attributes_on_shown(
    event: On<EntityShownEvent>,
    players: Players,
    entities: Query<(&MinecraftEntityId, &Attributes)>,
) {
    let Ok((id, attributes)) = entities.get(event.entity) else {
        return;
    };
    let full = attributes.syncable_snapshots();
    if full.is_empty() {
        return;
    }
    players.send(event.viewer, packet(id.0, full));
}

fn reset_on_remove(
    event: On<Remove, Attributes>,
    players: Players,
    mut entities: Query<(
        &MinecraftEntityId,
        &Attributes,
        &mut AttributesState,
        Option<&EntityViewers>,
    )>,
) {
    let Ok((id, attributes, mut state, viewers)) = entities.get_mut(event.entity) else {
        return;
    };
    if !state.sent.is_empty() {
        let reset: Vec<AttributeSnapshot> = state
            .sent
            .iter()
            .map(|attribute| default_snapshot(attributes.kind, *attribute))
            .collect();
        let recipients: Vec<Entity> = match viewers {
            Some(viewers) => viewers.iter().collect(),
            None => state.recipients.iter().copied().collect(),
        };
        players.send_to(recipients, packet(id.0, reset));
    }
    state.sent.clear();
    state.recipients.clear();
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{LoadedChunks, PlayerDimension, PlayerReady};
    use crate::entity::{EntityBuilder, EntityKind, EntityPlugin};
    use crate::network::{NetworkChannels, OutgoingPacket};
    use crate::world::{ChunkPos, DimensionId};

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
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
                MinecraftEntityId(100 + id as i32),
                PlayerDimension(DimensionId::Overworld),
                LoadedChunks([ChunkPos::new(0, 0)].into_iter().collect()),
            ))
            .id()
    }

    type Sent = (u32, i32, Vec<(i32, f64, Vec<(String, f64, u8)>)>);

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent: Vec<Sent> = rx
            .try_iter()
            .filter_map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::UpdateAttributes(p)) => Some((
                    out.client_id,
                    p.entity_id,
                    p.attributes
                        .into_iter()
                        .map(|a| {
                            (
                                a.attribute_id,
                                a.base,
                                a.modifiers
                                    .into_iter()
                                    .map(|m| (m.id, m.amount, m.operation as u8))
                                    .collect(),
                            )
                        })
                        .collect(),
                )),
                _ => None,
            })
            .collect();
        sent.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sent
    }

    fn plain(attribute: EntityAttribute, base: f64) -> (i32, f64, Vec<(String, f64, u8)>) {
        (attribute.id(), base, vec![])
    }

    const PLAYER_SPEED: f64 = 0.1f32 as f64;
    const ZOMBIE_SPEED: f64 = 0.23f32 as f64;

    #[test]
    fn builder_value_and_modifier_bookkeeping() {
        let mut attributes = Attributes::new()
            .base(EntityAttribute::MovementSpeed, 0.1)
            .modifier(
                EntityAttribute::MovementSpeed,
                Modifier::multiply_total("voidmc:kart_boost", 0.5),
            )
            .modifier(
                EntityAttribute::MovementSpeed,
                Modifier::add("voidmc:flat", 0.1),
            )
            .modifier(
                EntityAttribute::MovementSpeed,
                Modifier::multiply_base("voidmc:base", 1.0),
            );
        assert!((attributes.value(EntityAttribute::MovementSpeed) - 0.6).abs() < 1e-9);
        assert_eq!(attributes.value(EntityAttribute::Scale), 1.0);
        assert!(!attributes.has(EntityAttribute::Scale));

        attributes.set_base(EntityAttribute::Scale, 40.0);
        assert_eq!(attributes.value(EntityAttribute::Scale), 16.0);
        attributes.set_base(EntityAttribute::Scale, f64::NAN);
        assert_eq!(attributes.value(EntityAttribute::Scale), 0.0625);

        attributes.add_modifier(
            EntityAttribute::MaxHealth,
            Modifier::add("voidmc:bonus", 4.0),
        );
        let health = attributes.get(EntityAttribute::MaxHealth).unwrap();
        assert_eq!(health.base(), 20.0);
        assert_eq!(health.modifiers().count(), 1);
        assert_eq!(attributes.value(EntityAttribute::MaxHealth), 24.0);
        attributes.add_modifier(
            EntityAttribute::MaxHealth,
            Modifier::add("voidmc:bonus", 6.0),
        );
        assert_eq!(attributes.value(EntityAttribute::MaxHealth), 26.0);
        assert_eq!(
            attributes
                .remove_modifier(EntityAttribute::MaxHealth, "voidmc:bonus")
                .map(|m| m.amount),
            Some(6.0)
        );
        assert!(
            attributes
                .remove_modifier(EntityAttribute::MaxHealth, "voidmc:bonus")
                .is_none()
        );
        attributes.clear_modifiers(EntityAttribute::MovementSpeed);
        assert!((attributes.value(EntityAttribute::MovementSpeed) - 0.1).abs() < 1e-9);
        assert!(attributes.reset(EntityAttribute::MovementSpeed));
        assert!(!attributes.reset(EntityAttribute::MovementSpeed));
        assert_eq!(
            attributes.value(EntityAttribute::MovementSpeed),
            PLAYER_SPEED
        );
        assert_eq!(attributes.iter().count(), 2);
    }

    #[test]
    fn defaults_come_from_the_entity_kind_not_the_registry() {
        let mut player = Attributes::new();
        assert_eq!(player.kind(), EntityKind::Player);
        assert_eq!(player.value(EntityAttribute::MovementSpeed), PLAYER_SPEED);
        assert_eq!(
            player.default_base(EntityAttribute::MovementSpeed),
            PLAYER_SPEED
        );
        player.add_modifier(
            EntityAttribute::MovementSpeed,
            Modifier::multiply_total("voidmc:speed", 1.0),
        );
        let snapshot = player.snapshot_of(EntityAttribute::MovementSpeed);
        assert_eq!(snapshot.base, PLAYER_SPEED);
        assert_eq!(snapshot.modifiers.len(), 1);
        assert!((player.value(EntityAttribute::MovementSpeed) - 2.0 * PLAYER_SPEED).abs() < 1e-12);
        assert!(player.reset(EntityAttribute::MovementSpeed));
        assert_eq!(
            player.snapshot_of(EntityAttribute::MovementSpeed).base,
            PLAYER_SPEED
        );
        assert_eq!(player.value(EntityAttribute::MovementSpeed), PLAYER_SPEED);

        let mut zombie = Attributes::for_kind(EntityKind::Zombie);
        assert_eq!(zombie.value(EntityAttribute::MovementSpeed), ZOMBIE_SPEED);
        zombie.add_modifier(
            EntityAttribute::MovementSpeed,
            Modifier::add("voidmc:slow", -0.03),
        );
        assert_eq!(
            zombie.get(EntityAttribute::MovementSpeed).unwrap().base(),
            ZOMBIE_SPEED
        );
        assert_eq!(zombie.value(EntityAttribute::FollowRange), 35.0);
        assert_eq!(zombie.value(EntityAttribute::Scale), 1.0);
    }

    #[test]
    fn inserting_on_a_spawned_entity_rebases_seeded_instances_only() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .insert(
                Attributes::new()
                    .modifier(
                        EntityAttribute::MovementSpeed,
                        Modifier::multiply_total("voidmc:boost", 1.0),
                    )
                    .base(EntityAttribute::MaxHealth, 40.0)
                    .modifier(EntityAttribute::Armor, Modifier::add("voidmc:plate", 1.0)),
            )
            .id();
        let zombie_id = app.world().get::<MinecraftEntityId>(zombie).unwrap().0;
        let attributes = app.world().get::<Attributes>(zombie).unwrap();
        assert_eq!(attributes.kind(), EntityKind::Zombie);
        assert_eq!(
            attributes
                .get(EntityAttribute::MovementSpeed)
                .unwrap()
                .base(),
            ZOMBIE_SPEED
        );
        assert_eq!(
            attributes.get(EntityAttribute::MaxHealth).unwrap().base(),
            40.0
        );
        assert_eq!(attributes.get(EntityAttribute::Armor).unwrap().base(), 2.0);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(
                1,
                zombie_id,
                vec![
                    (
                        EntityAttribute::Armor.id(),
                        2.0,
                        vec![("voidmc:plate".to_string(), 1.0, 0)]
                    ),
                    plain(EntityAttribute::MaxHealth, 40.0),
                    (
                        EntityAttribute::MovementSpeed.id(),
                        ZOMBIE_SPEED,
                        vec![("voidmc:boost".to_string(), 1.0, 2)]
                    ),
                ]
            )]
        );

        app.world_mut()
            .get_mut::<Attributes>(zombie)
            .unwrap()
            .reset(EntityAttribute::MovementSpeed);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(
                1,
                zombie_id,
                vec![plain(EntityAttribute::MovementSpeed, ZOMBIE_SPEED)]
            )]
        );
    }

    #[test]
    fn players_get_diffs_full_state_on_join_and_resets() {
        let (mut app, rx) = test_app();
        let me = player(&mut app, 1);
        let other = player(&mut app, 2);
        app.world_mut().entity_mut(me).insert(
            Attributes::new()
                .base(EntityAttribute::MovementSpeed, 0.2)
                .base(EntityAttribute::AttackDamage, 9.0),
        );
        app.update();
        let speed = plain(EntityAttribute::MovementSpeed, 0.2);
        assert_eq!(
            drain(&rx),
            vec![(1, 101, vec![speed.clone()]), (2, 101, vec![speed.clone()])]
        );
        app.update();
        assert!(drain(&rx).is_empty());

        {
            let mut attributes = app.world_mut().get_mut::<Attributes>(me).unwrap();
            attributes.add_modifier(
                EntityAttribute::Scale,
                Modifier::multiply_total("voidmc:giant", 1.0),
            );
            attributes.set_base(EntityAttribute::AttackDamage, 1.0);
        }
        app.update();
        let scale = (
            EntityAttribute::Scale.id(),
            1.0,
            vec![("voidmc:giant".to_string(), 1.0, 2)],
        );
        assert_eq!(
            drain(&rx),
            vec![(1, 101, vec![scale.clone()]), (2, 101, vec![scale.clone()])]
        );

        let late = player(&mut app, 3);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(3, 101, vec![speed.clone(), scale.clone()])]
        );

        app.world_mut().entity_mut(other).remove::<PlayerReady>();
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(
                2,
                101,
                vec![
                    plain(EntityAttribute::MovementSpeed, PLAYER_SPEED),
                    plain(EntityAttribute::Scale, 1.0)
                ]
            )]
        );

        app.world_mut()
            .get_mut::<Attributes>(me)
            .unwrap()
            .reset(EntityAttribute::Scale);
        app.update();
        let default_scale = plain(EntityAttribute::Scale, 1.0);
        assert_eq!(
            drain(&rx),
            vec![
                (1, 101, vec![default_scale.clone()]),
                (3, 101, vec![default_scale.clone()])
            ]
        );

        app.world_mut().entity_mut(me).remove::<Attributes>();
        let default_speed = plain(EntityAttribute::MovementSpeed, PLAYER_SPEED);
        assert_eq!(
            drain(&rx),
            vec![
                (1, 101, vec![default_speed.clone()]),
                (3, 101, vec![default_speed.clone()])
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());
        let _ = late;
    }

    #[test]
    fn replacing_the_component_rebases_again_and_resets_dropped_attributes() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .insert(
                Attributes::new()
                    .base(EntityAttribute::MaxHealth, 40.0)
                    .base(EntityAttribute::Scale, 2.0),
            )
            .id();
        let zombie_id = app.world().get::<MinecraftEntityId>(zombie).unwrap().0;
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(
                1,
                zombie_id,
                vec![
                    plain(EntityAttribute::MaxHealth, 40.0),
                    plain(EntityAttribute::Scale, 2.0),
                ]
            )]
        );

        app.world_mut().entity_mut(zombie).insert(
            Attributes::new()
                .modifier(
                    EntityAttribute::MovementSpeed,
                    Modifier::multiply_total("voidmc:boost", 1.0),
                )
                .base(EntityAttribute::Scale, 2.0),
        );
        let attributes = app.world().get::<Attributes>(zombie).unwrap();
        assert_eq!(attributes.kind(), EntityKind::Zombie);
        assert_eq!(
            attributes
                .get(EntityAttribute::MovementSpeed)
                .unwrap()
                .base(),
            ZOMBIE_SPEED
        );
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(
                1,
                zombie_id,
                vec![
                    plain(EntityAttribute::MaxHealth, 20.0),
                    (
                        EntityAttribute::MovementSpeed.id(),
                        ZOMBIE_SPEED,
                        vec![("voidmc:boost".to_string(), 1.0, 2)]
                    ),
                    plain(EntityAttribute::Scale, 2.0),
                ]
            )]
        );

        app.world_mut().entity_mut(zombie).remove::<Attributes>();
        assert_eq!(
            drain(&rx),
            vec![(
                1,
                zombie_id,
                vec![
                    plain(EntityAttribute::MovementSpeed, ZOMBIE_SPEED),
                    plain(EntityAttribute::Scale, 1.0),
                ]
            )]
        );
    }

    #[test]
    fn replacing_a_player_component_resets_dropped_attributes() {
        let (mut app, rx) = test_app();
        let me = player(&mut app, 1);
        app.world_mut()
            .entity_mut(me)
            .insert(Attributes::new().base(EntityAttribute::MovementSpeed, 0.2));
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(1, 101, vec![plain(EntityAttribute::MovementSpeed, 0.2)])]
        );

        app.world_mut()
            .entity_mut(me)
            .insert(Attributes::new().base(EntityAttribute::Scale, 3.0));
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(
                1,
                101,
                vec![
                    plain(EntityAttribute::MovementSpeed, PLAYER_SPEED),
                    plain(EntityAttribute::Scale, 3.0),
                ]
            )]
        );
    }

    #[test]
    fn tracked_entities_send_to_viewers_on_show_and_on_change() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .insert(
                Attributes::new()
                    .base(EntityAttribute::Scale, 2.0)
                    .base(EntityAttribute::FollowRange, 1.0),
            )
            .id();
        let zombie_id = app.world().get::<MinecraftEntityId>(zombie).unwrap().0;
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(1, zombie_id, vec![plain(EntityAttribute::Scale, 2.0)])]
        );

        app.world_mut()
            .get_mut::<Attributes>(zombie)
            .unwrap()
            .set_base(EntityAttribute::Gravity, 0.02);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(1, zombie_id, vec![plain(EntityAttribute::Gravity, 0.02)])]
        );

        player(&mut app, 2);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![(
                2,
                zombie_id,
                vec![
                    plain(EntityAttribute::Gravity, 0.02),
                    plain(EntityAttribute::Scale, 2.0)
                ]
            )]
        );

        app.world_mut().entity_mut(zombie).remove::<Attributes>();
        let reset = vec![
            plain(EntityAttribute::Gravity, 0.08),
            plain(EntityAttribute::Scale, 1.0),
        ];
        assert_eq!(
            drain(&rx),
            vec![(1, zombie_id, reset.clone()), (2, zombie_id, reset)]
        );
        app.update();
        assert!(drain(&rx).is_empty());
    }
}
