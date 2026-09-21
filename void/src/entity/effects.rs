//! Server-authoritative status effects. [`StatusEffects`] holds the active
//! effects with their remaining duration; one `PostUpdate` system counts them
//! down and sends only the changes. Vanilla routing: effect packets go to the
//! entity's own client and to its player passengers; other viewers learn about
//! visible effects through entity metadata (particles, glowing, invisibility).

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::{Deref, DerefMut};

use bevy_app::{App, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::entity_metadata::living_entity_index;
use voidmc_protocol::clientbound::{
    EntityMetadataValue as Value, INFINITE_DURATION, MobEffectFlags, Particle, ParticleColor,
    RemoveMobEffect, UpdateMobEffect,
};

pub use voidmc_data::v26_1_2::{Effect, EffectCategory};

use super::metadata::{EntityMetadata, sync_entity_metadata};
use super::passengers::Passengers;
use crate::components::{ClientId, MinecraftEntityId, PlayerReady};
use crate::players::Players;
use crate::schedule::VoidSystems;

const AMBIENT_PARTICLE_ALPHA: u8 = 38;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectDuration {
    Ticks(u32),
    Infinite,
}

impl EffectDuration {
    pub const fn ticks(ticks: u32) -> Self {
        Self::Ticks(ticks)
    }

    pub const fn seconds(seconds: u32) -> Self {
        Self::Ticks(seconds * 20)
    }

    fn wire(self) -> i32 {
        match self {
            Self::Ticks(ticks) => i32::try_from(ticks).unwrap_or(i32::MAX),
            Self::Infinite => INFINITE_DURATION,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectInstance {
    pub amplifier: u8,
    pub duration: EffectDuration,
    pub ambient: bool,
    pub show_particles: bool,
    pub show_icon: bool,
}

impl EffectInstance {
    pub const fn new(amplifier: u8, duration: EffectDuration) -> Self {
        Self {
            amplifier,
            duration,
            ambient: false,
            show_particles: true,
            show_icon: true,
        }
    }

    pub const fn ambient(mut self) -> Self {
        self.ambient = true;
        self
    }

    pub const fn hide_particles(mut self) -> Self {
        self.show_particles = false;
        self
    }

    pub const fn hide_icon(mut self) -> Self {
        self.show_icon = false;
        self
    }

    /// Icon off, particles off: the effect applies without any HUD or world
    /// trace.
    pub const fn hidden(self) -> Self {
        self.hide_particles().hide_icon()
    }

    fn flags(&self, blend: bool) -> MobEffectFlags {
        let mut flags = MobEffectFlags::empty();
        flags.set(MobEffectFlags::AMBIENT, self.ambient);
        flags.set(MobEffectFlags::SHOW_PARTICLES, self.show_particles);
        flags.set(MobEffectFlags::SHOW_ICON, self.show_icon);
        flags.set(MobEffectFlags::BLEND, blend);
        flags
    }

    fn particle(&self, effect: Effect) -> Particle {
        match effect.particle().and_then(Particle::simple) {
            Some(particle) => particle,
            None => {
                let alpha = if self.ambient {
                    AMBIENT_PARTICLE_ALPHA
                } else {
                    0xFF
                };
                Particle::EntityEffect {
                    color: ParticleColor((i32::from(alpha) << 24) | (effect.color() & 0x00FF_FFFF)),
                }
            }
        }
    }
}

/// Active effects of a player or living entity. Durations tick down on the
/// server; an effect at zero is removed and the client told.
#[derive(Component, Debug, Clone, Default)]
#[require(StatusEffectsState)]
pub struct StatusEffects {
    effects: BTreeMap<Effect, EffectInstance>,
    dirty: BTreeSet<Effect>,
    removed: BTreeSet<Effect>,
}

/// `&mut` handle returned by [`StatusEffects::add`]; the builder-style
/// setters write through to the stored instance.
pub struct EffectSlot<'a>(&'a mut EffectInstance);

impl EffectSlot<'_> {
    pub fn ambient(self) -> Self {
        self.0.ambient = true;
        self
    }

    pub fn hide_particles(self) -> Self {
        self.0.show_particles = false;
        self
    }

    pub fn hide_icon(self) -> Self {
        self.0.show_icon = false;
        self
    }

    pub fn hidden(self) -> Self {
        self.hide_particles().hide_icon()
    }
}

impl Deref for EffectSlot<'_> {
    type Target = EffectInstance;

    fn deref(&self) -> &EffectInstance {
        self.0
    }
}

impl DerefMut for EffectSlot<'_> {
    fn deref_mut(&mut self) -> &mut EffectInstance {
        self.0
    }
}

impl StatusEffects {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(self, effect: Effect, amplifier: u8, duration: EffectDuration) -> Self {
        self.with_instance(effect, EffectInstance::new(amplifier, duration))
    }

    pub fn with_instance(mut self, effect: Effect, instance: EffectInstance) -> Self {
        self.insert(effect, instance);
        self
    }

    /// Adds or replaces `effect`; the returned slot tweaks its flags.
    pub fn add(
        &mut self,
        effect: Effect,
        amplifier: u8,
        duration: EffectDuration,
    ) -> EffectSlot<'_> {
        self.insert(effect, EffectInstance::new(amplifier, duration))
    }

    pub fn insert(&mut self, effect: Effect, instance: EffectInstance) -> EffectSlot<'_> {
        self.dirty.insert(effect);
        self.removed.remove(&effect);
        let slot = match self.effects.entry(effect) {
            Entry::Occupied(mut slot) => {
                slot.insert(instance);
                slot.into_mut()
            }
            Entry::Vacant(slot) => slot.insert(instance),
        };
        EffectSlot(slot)
    }

    pub fn remove(&mut self, effect: Effect) -> Option<EffectInstance> {
        let removed = self.effects.remove(&effect)?;
        self.dirty.remove(&effect);
        self.removed.insert(effect);
        Some(removed)
    }

    pub fn clear(&mut self) {
        let effects: Vec<Effect> = self.effects.keys().copied().collect();
        for effect in effects {
            self.remove(effect);
        }
    }

    pub fn get(&self, effect: Effect) -> Option<&EffectInstance> {
        self.effects.get(&effect)
    }

    /// Marks `effect` dirty so the change is sent even if only a field is
    /// touched.
    pub fn get_mut(&mut self, effect: Effect) -> Option<EffectSlot<'_>> {
        let instance = self.effects.get_mut(&effect)?;
        self.dirty.insert(effect);
        Some(EffectSlot(instance))
    }

    pub fn has(&self, effect: Effect) -> bool {
        self.effects.contains_key(&effect)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Effect, &EffectInstance)> + '_ {
        self.effects
            .iter()
            .map(|(effect, instance)| (*effect, instance))
    }

    pub fn len(&self) -> usize {
        self.effects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub(crate) fn glowing(&self) -> bool {
        self.has(Effect::Glowing)
    }

    pub(crate) fn invisible(&self) -> bool {
        self.has(Effect::Invisibility)
    }

    /// Effects changed this tick keep their full duration for the packet
    /// about to go out; the countdown starts the tick after.
    fn tick_down(&mut self) -> Vec<Effect> {
        let mut expired = Vec::new();
        for (effect, instance) in self.effects.iter_mut() {
            if self.dirty.contains(effect) {
                continue;
            }
            if let EffectDuration::Ticks(ticks) = &mut instance.duration {
                let next = ticks.saturating_sub(1);
                if next == 0 {
                    expired.push(*effect);
                } else {
                    *ticks = next;
                }
            }
        }
        expired
    }

    fn take_changes(&mut self) -> (BTreeSet<Effect>, BTreeSet<Effect>) {
        (
            std::mem::take(&mut self.dirty),
            std::mem::take(&mut self.removed),
        )
    }

    fn write_metadata(&self, meta: &mut EntityMetadata) {
        let particles: Vec<Particle> = self
            .effects
            .iter()
            .filter(|(_, instance)| instance.show_particles)
            .map(|(effect, instance)| instance.particle(*effect))
            .collect();
        let ambient = self.effects.values().all(|instance| instance.ambient);
        meta.set(
            living_entity_index::EFFECT_PARTICLES,
            Value::Particles(particles),
        );
        meta.set(
            living_entity_index::EFFECT_AMBIENCE,
            Value::Boolean(ambient),
        );
    }

    fn clear_metadata(meta: &mut EntityMetadata) {
        Self::default().write_metadata(meta);
    }
}

#[derive(Component, Debug, Default)]
pub struct StatusEffectsState {
    recipients: HashSet<Entity>,
    sent: BTreeSet<Effect>,
}

impl StatusEffectsState {
    pub fn recipients(&self) -> impl Iterator<Item = Entity> + '_ {
        self.recipients.iter().copied()
    }
}

pub(super) fn register(app: &mut App) {
    app.add_observer(remove_from_recipients).add_systems(
        PostUpdate,
        (sync_status_effects, project_effects_metadata)
            .chain()
            .before(sync_entity_metadata)
            .in_set(VoidSystems::EntityMetadataSync),
    );
}

fn update_packet(
    id: i32,
    effect: Effect,
    instance: &EffectInstance,
    blend: bool,
) -> UpdateMobEffect {
    UpdateMobEffect {
        entity_id: id,
        effect_id: effect.id(),
        amplifier: i32::from(instance.amplifier),
        duration: instance.duration.wire(),
        flags: instance.flags(blend),
    }
}

fn remove_packet(id: i32, effect: Effect) -> RemoveMobEffect {
    RemoveMobEffect {
        entity_id: id,
        effect_id: effect.id(),
    }
}

fn sync_status_effects(
    players: Players,
    mut entities: Query<(
        Entity,
        &MinecraftEntityId,
        &mut StatusEffects,
        &mut StatusEffectsState,
        Option<&Passengers>,
    )>,
    ready_players: Query<(), (With<ClientId>, With<PlayerReady>)>,
) {
    for (entity, id, mut effects, mut state, passengers) in entities.iter_mut() {
        let expired = effects.bypass_change_detection().tick_down();
        for effect in expired {
            effects.remove(effect);
        }

        let mut desired: HashSet<Entity> = HashSet::new();
        if ready_players.contains(entity) {
            desired.insert(entity);
        }
        if let Some(passengers) = passengers {
            desired.extend(
                passengers
                    .0
                    .iter()
                    .copied()
                    .filter(|p| ready_players.contains(*p)),
            );
        }

        let (dirty, removed) = if effects.dirty.is_empty() && effects.removed.is_empty() {
            (BTreeSet::new(), BTreeSet::new())
        } else {
            effects.bypass_change_detection().take_changes()
        };
        let kept: Vec<Entity> = state
            .recipients
            .iter()
            .copied()
            .filter(|r| desired.contains(r))
            .collect();
        for effect in removed {
            if state.sent.remove(&effect) {
                players.send_to(kept.iter().copied(), remove_packet(id.0, effect));
            }
        }
        for effect in dirty {
            let Some(instance) = effects.get(effect) else {
                continue;
            };
            let blend = state.sent.insert(effect);
            players.send_to(
                kept.iter().copied(),
                update_packet(id.0, effect, instance, blend),
            );
        }

        if desired == state.recipients {
            continue;
        }
        for gone in state.recipients.difference(&desired) {
            for effect in &state.sent {
                players.send(*gone, remove_packet(id.0, *effect));
            }
        }
        for joined in desired.difference(&state.recipients) {
            for (effect, instance) in effects.iter() {
                players.send(*joined, update_packet(id.0, effect, instance, false));
            }
        }
        state.recipients = desired;
    }
}

fn project_effects_metadata(
    mut entities: Query<(&StatusEffects, &mut EntityMetadata), Changed<StatusEffects>>,
) {
    for (effects, mut meta) in entities.iter_mut() {
        effects.write_metadata(&mut meta);
    }
}

fn remove_from_recipients(
    event: On<Remove, StatusEffects>,
    players: Players,
    mut entities: Query<(
        &MinecraftEntityId,
        &mut StatusEffectsState,
        Option<&mut EntityMetadata>,
    )>,
) {
    let Ok((id, mut state, meta)) = entities.get_mut(event.entity) else {
        return;
    };
    for effect in &state.sent {
        players.send_to(
            state.recipients.iter().copied(),
            remove_packet(id.0, *effect),
        );
    }
    state.sent.clear();
    state.recipients.clear();
    if let Some(mut meta) = meta {
        StatusEffects::clear_metadata(&mut meta);
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::entity_metadata::{entity_flag, entity_index};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::components::{LoadedChunks, PlayerDimension, PlayerReady};
    use crate::entity::{EntityBuilder, EntityKind, EntityPlugin, Glowing};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::world::{ChunkPos, DimensionId};

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
                MinecraftEntityId(100 + id as i32),
                PlayerDimension(DimensionId::Overworld),
                LoadedChunks([ChunkPos::new(0, 0)].into_iter().collect()),
            ))
            .id()
    }

    #[derive(Debug, PartialEq)]
    enum Sent {
        Update {
            to: u32,
            entity: i32,
            effect: i32,
            amplifier: i32,
            duration: i32,
            flags: u8,
        },
        Remove {
            to: u32,
            entity: i32,
            effect: i32,
        },
        Metadata {
            to: u32,
            entity: i32,
            entries: Vec<(u8, Value)>,
        },
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent: Vec<Sent> = rx
            .try_iter()
            .filter_map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::UpdateMobEffect(p)) => Some(Sent::Update {
                    to: out.client_id,
                    entity: p.entity_id,
                    effect: p.effect_id,
                    amplifier: p.amplifier,
                    duration: p.duration,
                    flags: p.flags.bits(),
                }),
                ClientboundPacket::Play(PlayPacket::RemoveMobEffect(p)) => Some(Sent::Remove {
                    to: out.client_id,
                    entity: p.entity_id,
                    effect: p.effect_id,
                }),
                ClientboundPacket::Play(PlayPacket::SetEntityData(p)) => Some(Sent::Metadata {
                    to: out.client_id,
                    entity: p.entity_id,
                    entries: p.entries.into_iter().map(|e| (e.index, e.value)).collect(),
                }),
                _ => None,
            })
            .collect();
        sent.sort_by_key(|s| format!("{s:?}"));
        sent
    }

    fn update(
        to: u32,
        entity: i32,
        effect: Effect,
        amplifier: i32,
        duration: i32,
        flags: u8,
    ) -> Sent {
        Sent::Update {
            to,
            entity,
            effect: effect.id(),
            amplifier,
            duration,
            flags,
        }
    }

    fn remove(to: u32, entity: i32, effect: Effect) -> Sent {
        Sent::Remove {
            to,
            entity,
            effect: effect.id(),
        }
    }

    const VISIBLE: u8 = 0x06;
    const VISIBLE_BLEND: u8 = 0x0E;

    #[test]
    fn builder_slot_and_queries() {
        let mut effects = StatusEffects::new()
            .with(Effect::Speed, 1, EffectDuration::ticks(600))
            .with_instance(
                Effect::Glowing,
                EffectInstance::new(0, EffectDuration::Infinite).hidden(),
            );
        assert_eq!(effects.len(), 2);
        assert_eq!(
            effects.get(Effect::Speed),
            Some(&EffectInstance::new(1, EffectDuration::Ticks(600)))
        );
        let glowing = effects.get(Effect::Glowing).unwrap();
        assert!(!glowing.show_icon && !glowing.show_particles && !glowing.ambient);
        assert!(effects.glowing() && !effects.invisible());

        effects
            .add(Effect::Invisibility, 0, EffectDuration::seconds(5))
            .hide_particles()
            .ambient();
        let invisibility = effects.get(Effect::Invisibility).unwrap();
        assert_eq!(invisibility.duration, EffectDuration::Ticks(100));
        assert!(invisibility.ambient && !invisibility.show_particles && invisibility.show_icon);
        assert!(effects.invisible());

        effects.get_mut(Effect::Speed).unwrap().amplifier = 3;
        assert_eq!(effects.get(Effect::Speed).unwrap().amplifier, 3);
        assert!(effects.get_mut(Effect::Slowness).is_none());

        assert!(effects.remove(Effect::Glowing).is_some());
        assert!(effects.remove(Effect::Glowing).is_none());
        assert!(!effects.has(Effect::Glowing));
        effects.clear();
        assert!(effects.is_empty());
        assert_eq!(
            EffectInstance::new(2, EffectDuration::Infinite).flags(true),
            MobEffectFlags::SHOW_PARTICLES | MobEffectFlags::SHOW_ICON | MobEffectFlags::BLEND
        );
    }

    #[test]
    fn effect_particles_follow_paper_colour_and_ambient_alpha() {
        let speed = EffectInstance::new(0, EffectDuration::Infinite);
        assert_eq!(
            speed.particle(Effect::Speed),
            Particle::EntityEffect {
                color: ParticleColor(0xFF33_EBFF_u32 as i32)
            }
        );
        assert_eq!(
            speed.ambient().particle(Effect::Speed),
            Particle::EntityEffect {
                color: ParticleColor(0x2633_EBFF)
            }
        );
        assert_eq!(speed.particle(Effect::TrialOmen), Particle::TrialOmen);
    }

    #[test]
    fn own_client_gets_diffs_and_expiry() {
        let (mut app, rx) = test_app();
        let me = player(&mut app, 1);
        player(&mut app, 2);
        app.world_mut().entity_mut(me).insert(
            StatusEffects::new()
                .with(Effect::Speed, 1, EffectDuration::ticks(3))
                .with(Effect::Glowing, 0, EffectDuration::Infinite),
        );
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                update(1, 101, Effect::Speed, 1, 3, VISIBLE),
                update(1, 101, Effect::Glowing, 0, -1, VISIBLE),
            ]
        );

        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut()
            .get_mut::<StatusEffects>(me)
            .unwrap()
            .get_mut(Effect::Glowing)
            .unwrap()
            .amplifier = 2;
        app.update();
        assert_eq!(
            drain(&rx),
            vec![update(1, 101, Effect::Glowing, 2, -1, VISIBLE)]
        );
        assert_eq!(
            app.world()
                .get::<StatusEffects>(me)
                .unwrap()
                .get(Effect::Speed)
                .unwrap()
                .duration,
            EffectDuration::Ticks(1)
        );

        app.update();
        assert_eq!(drain(&rx), vec![remove(1, 101, Effect::Speed)]);
        assert!(
            !app.world()
                .get::<StatusEffects>(me)
                .unwrap()
                .has(Effect::Speed)
        );

        app.world_mut()
            .get_mut::<StatusEffects>(me)
            .unwrap()
            .add(Effect::Haste, 0, EffectDuration::ticks(20))
            .hidden();
        app.update();
        assert_eq!(drain(&rx), vec![update(1, 101, Effect::Haste, 0, 20, 0x08)]);

        app.world_mut()
            .get_mut::<StatusEffects>(me)
            .unwrap()
            .remove(Effect::Haste);
        app.update();
        assert_eq!(drain(&rx), vec![remove(1, 101, Effect::Haste)]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn effects_wait_for_the_player_to_be_ready_and_removal_clears_the_client() {
        let (mut app, rx) = test_app();
        let me = app
            .world_mut()
            .spawn((
                ClientId(1),
                MinecraftEntityId(101),
                StatusEffects::new().with(Effect::Speed, 0, EffectDuration::Infinite),
            ))
            .id();
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().entity_mut(me).insert(PlayerReady);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![update(1, 101, Effect::Speed, 0, -1, VISIBLE)]
        );

        app.world_mut().entity_mut(me).remove::<StatusEffects>();
        assert_eq!(drain(&rx), vec![remove(1, 101, Effect::Speed)]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn player_passengers_receive_the_vehicle_effects() {
        let (mut app, rx) = test_app();
        let rider = player(&mut app, 1);
        let horse = EntityBuilder::new(EntityKind::Horse)
            .spawn_in(app.world_mut())
            .insert(StatusEffects::new().with(Effect::Speed, 2, EffectDuration::Infinite))
            .id();
        let horse_id = app.world().get::<MinecraftEntityId>(horse).unwrap().0;
        app.update();
        let sent = drain(&rx);
        assert!(!sent.iter().any(|s| matches!(s, Sent::Update { .. })));

        app.world_mut()
            .entity_mut(horse)
            .insert(Passengers::new([rider]));
        app.update();
        let sent = drain(&rx);
        assert!(sent.contains(&update(1, horse_id, Effect::Speed, 2, -1, VISIBLE)));

        app.world_mut()
            .get_mut::<StatusEffects>(horse)
            .unwrap()
            .add(Effect::Strength, 0, EffectDuration::Infinite);
        app.update();
        let sent = drain(&rx);
        assert!(sent.contains(&update(1, horse_id, Effect::Strength, 0, -1, VISIBLE_BLEND)));

        app.world_mut()
            .get_mut::<Passengers>(horse)
            .unwrap()
            .remove(rider);
        app.update();
        let sent = drain(&rx);
        assert!(sent.contains(&remove(1, horse_id, Effect::Speed)));
        assert!(sent.contains(&remove(1, horse_id, Effect::Strength)));
    }

    fn metadata_of(sent: &[Sent], entity_id: i32) -> Vec<(u8, Value)> {
        sent.iter()
            .filter_map(|s| match s {
                Sent::Metadata {
                    entity, entries, ..
                } if *entity == entity_id => Some(entries.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn viewers_see_effects_through_metadata_only() {
        let (mut app, rx) = test_app();
        player(&mut app, 1);
        let zombie = EntityBuilder::new(EntityKind::Zombie)
            .spawn_in(app.world_mut())
            .insert(
                StatusEffects::new()
                    .with(Effect::Speed, 0, EffectDuration::Infinite)
                    .with_instance(
                        Effect::Glowing,
                        EffectInstance::new(0, EffectDuration::Infinite).hide_particles(),
                    ),
            )
            .id();
        let zombie_id = app.world().get::<MinecraftEntityId>(zombie).unwrap().0;
        app.update();
        let sent = drain(&rx);
        assert!(!sent.iter().any(|s| matches!(s, Sent::Update { .. })));
        let meta = metadata_of(&sent, zombie_id);
        assert!(meta.contains(&(entity_index::FLAGS, Value::Byte(entity_flag::GLOWING as i8))));
        assert!(meta.contains(&(
            living_entity_index::EFFECT_PARTICLES,
            Value::Particles(vec![Particle::EntityEffect {
                color: ParticleColor(0xFF33_EBFF_u32 as i32)
            }])
        )));
        assert!(meta.contains(&(living_entity_index::EFFECT_AMBIENCE, Value::Boolean(false))));

        app.world_mut()
            .get_mut::<StatusEffects>(zombie)
            .unwrap()
            .remove(Effect::Glowing);
        app.update();
        let meta = metadata_of(&drain(&rx), zombie_id);
        assert_eq!(meta, vec![(entity_index::FLAGS, Value::Byte(0))]);

        app.world_mut().entity_mut(zombie).insert(Glowing);
        app.update();
        let meta = metadata_of(&drain(&rx), zombie_id);
        assert_eq!(
            meta,
            vec![(entity_index::FLAGS, Value::Byte(entity_flag::GLOWING as i8))]
        );

        app.world_mut().entity_mut(zombie).remove::<StatusEffects>();
        app.update();
        let meta = metadata_of(&drain(&rx), zombie_id);
        assert_eq!(
            meta,
            vec![
                (
                    living_entity_index::EFFECT_PARTICLES,
                    Value::Particles(vec![])
                ),
                (living_entity_index::EFFECT_AMBIENCE, Value::Boolean(true)),
            ]
        );

        let late = player(&mut app, 2);
        app.update();
        let sent = drain(&rx);
        assert!(sent.iter().any(|s| matches!(
            s,
            Sent::Metadata { to: 2, entity, .. } if *entity == zombie_id
        )));
        let _ = late;
    }
}
