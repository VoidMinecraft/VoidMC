use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use bevy_ecs::prelude::*;
use voidmc::{
    BlockDisplay, Display, DisplayTransform, EntityBuilder, EntityKind, ItemDisplay, ItemId,
    ItemStack, components::Position,
};
use voidmc_data::v26_1_2::{blocks as b, items as i};

use crate::items::{BURST_LIFE, Bolt, BurstKind, Items, MISSILE_LIFE};
use crate::kart::{FIREBALL_CHARGE, Kart, PowerUp};
use crate::race::{Phase, Race};
use crate::track::Track;

pub const KEYFRAME_TICKS: u16 = 2;
pub const JUMP: f64 = 6.0;
pub const BOLT_SEGMENTS: usize = 5;
pub const BOLT_HEIGHT: f64 = 6.75;
pub const BOLT_FLASH: u8 = 4;
const BOLT_WIDTH: f64 = 0.2;
const BOLT_SWAY: f64 = 0.25;
const BRIGHTNESS: (u8, u8) = (15, 15);
const VIEW_RANGE: f32 = 2.0;
pub const RAINBOW: [i32; 11] = [
    b::RED_STAINED_GLASS,
    b::ORANGE_STAINED_GLASS,
    b::YELLOW_STAINED_GLASS,
    b::LIME_STAINED_GLASS,
    b::GREEN_STAINED_GLASS,
    b::CYAN_STAINED_GLASS,
    b::LIGHT_BLUE_STAINED_GLASS,
    b::BLUE_STAINED_GLASS,
    b::PURPLE_STAINED_GLASS,
    b::MAGENTA_STAINED_GLASS,
    b::PINK_STAINED_GLASS,
];

pub fn rainbow(tick: u64) -> i32 {
    RAINBOW[(tick / u64::from(KEYFRAME_TICKS)) as usize % RAINBOW.len()]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Held,
    Jet,
    Shield,
    Charge,
    Debuff,
    Trap,
    Missile,
    Burst,
    Rainbow,
    Blaze,
    Bolt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key(pub u64, pub Kind, pub u8);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Model {
    Block(i32),
    Item(i32),
}

impl Model {
    fn kind(self) -> EntityKind {
        match self {
            Model::Block(_) => EntityKind::BlockDisplay,
            Model::Item(_) => EntityKind::ItemDisplay,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub at: Position,
    pub model: Model,
    pub transform: DisplayTransform,
}

#[derive(Component)]
pub struct Prop;

struct Slot {
    entity: Entity,
    frame: Frame,
    stamp: u64,
}

#[derive(Resource, Default)]
pub struct Scene(HashMap<Key, Slot>);

impl Scene {
    pub fn entity(&self, key: Key) -> Option<Entity> {
        self.0.get(&key).map(|slot| slot.entity)
    }

    pub fn frame(&self, key: Key) -> Option<Frame> {
        self.0.get(&key).map(|slot| slot.frame)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.0.keys().copied()
    }
}

type Props<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Position,
        &'static mut Display,
        Option<&'static mut BlockDisplay>,
        Option<&'static mut ItemDisplay>,
    ),
    With<Prop>,
>;

fn settings(transform: DisplayTransform) -> Display {
    Display::default()
        .transform(transform)
        .interpolation_ticks(KEYFRAME_TICKS)
        .teleport_ticks(KEYFRAME_TICKS as u8)
        .brightness(BRIGHTNESS.0, BRIGHTNESS.1)
        .view_range(VIEW_RANGE)
}

struct Stage<'a, 'w, 's, 'cw, 'cs> {
    scene: &'a mut Scene,
    props: &'a mut Props<'w, 's>,
    commands: &'a mut Commands<'cw, 'cs>,
    tick: u64,
}

impl Stage<'_, '_, '_, '_, '_> {
    fn apply(&mut self, key: Key, frame: Frame) {
        if let Some(slot) = self.scene.0.get_mut(&key) {
            slot.stamp = self.tick;
            if slot.frame == frame {
                return;
            }
            if slot.frame.model.kind() == frame.model.kind()
                && !jumped(slot.frame.at, frame.at)
                && let Ok((mut position, mut display, block, item)) =
                    self.props.get_mut(slot.entity)
            {
                if slot.frame.at != frame.at {
                    *position = frame.at;
                }
                if slot.frame.transform != frame.transform {
                    display.transform = frame.transform;
                }
                if slot.frame.model != frame.model {
                    match (frame.model, block, item) {
                        (Model::Block(state), Some(mut block), _) => block.0 = state,
                        (Model::Item(id), _, Some(mut item)) => {
                            item.item = ItemStack::new(ItemId(id), 1);
                        }
                        _ => {}
                    }
                }
                slot.frame = frame;
                return;
            }
            despawn(self.commands, slot.entity);
        }
        let mut builder = EntityBuilder::new(frame.model.kind())
            .position(frame.at)
            .with_bundle((Prop, settings(frame.transform)));
        builder = match frame.model {
            Model::Block(state) => builder.with(BlockDisplay(state)),
            Model::Item(id) => builder.with(ItemDisplay::new(ItemStack::new(ItemId(id), 1))),
        };
        let entity = builder.spawn(self.commands).id();
        self.scene.0.insert(
            key,
            Slot {
                entity,
                frame,
                stamp: self.tick,
            },
        );
    }

    fn finish(self) {
        let tick = self.tick;
        let commands = self.commands;
        self.scene.0.retain(|_, slot| {
            if slot.stamp == tick {
                return true;
            }
            despawn(commands, slot.entity);
            false
        });
    }

    fn clear(self) {
        let commands = self.commands;
        for slot in self.scene.0.drain() {
            despawn(commands, slot.1.entity);
        }
    }
}

fn jumped(from: Position, to: Position) -> bool {
    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
    dx * dx + dy * dy + dz * dz > JUMP * JUMP
}

fn despawn(commands: &mut Commands, entity: Entity) {
    if let Ok(mut entity) = commands.get_entity(entity) {
        entity.despawn();
    }
}

pub fn sync(
    race: Res<Race>,
    items: Res<Items>,
    map: Res<Track>,
    karts: Query<(Entity, &Kart)>,
    mut scene: ResMut<Scene>,
    mut props: Props,
    mut commands: Commands,
) {
    if !race.tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
        return;
    }
    let mut stage = Stage {
        scene: &mut scene,
        props: &mut props,
        commands: &mut commands,
        tick: race.tick,
    };
    if race.phase != Phase::Racing {
        stage.clear();
        return;
    }
    let time = race.tick as f64;
    for (entity, kart) in karts.iter().filter(|(_, kart)| kart.racing()) {
        kart_frames(&mut stage, entity.to_bits(), kart, time);
    }
    for trap in &items.traps {
        let age = trap.age(race.tick) as f64;
        let fade = envelope(age, trap.life() as f64);
        let mut g = Group::new(&mut stage, trap.id, Kind::Trap, (trap.x, trap.y, trap.z));
        if trap.ice {
            for yaw in [0.0, PI / 4.0] {
                g.block(
                    b::BLUE_ICE,
                    [0.0, 0.045, 0.0],
                    [3.45 * fade, 0.07 * fade, 3.45 * fade],
                    yaw,
                    0.0,
                );
            }
            for i in 0..4 {
                let a = f64::from(i) * TAU / 4.0 + 0.3;
                g.block(
                    b::LIGHT_BLUE_STAINED_GLASS,
                    [a.cos() * 1.7, 0.22, a.sin() * 1.7],
                    [0.2 * fade, 0.45 * fade, 0.2 * fade],
                    time * 0.035 + a,
                    PI / 4.0,
                );
            }
        } else {
            for i in 0..3 {
                let a = f64::from(i) * TAU / 3.0;
                g.block(
                    b::YELLOW_CONCRETE,
                    [a.cos() * 0.3, 0.24, a.sin() * 0.3],
                    [0.7 * fade, 0.16 * fade, 0.25 * fade],
                    -a,
                    -PI / 6.0,
                );
            }
            g.block(
                b::GOLD_BLOCK,
                [0.0, 0.43, 0.0],
                [0.18 * fade, 0.55 * fade, 0.18 * fade],
                0.0,
                0.12,
            );
        }
    }
    for m in &items.missiles {
        let mut g = Group::new(&mut stage, m.id, Kind::Missile, (m.x, m.y, m.z));
        let yaw = -map.heading(m.phase);
        let age = f64::from(m.age);
        let fade =
            smoothstep((age + 1.0) / 3.0) * smoothstep((f64::from(MISSILE_LIFE) - 2.0 - age) / 4.0);
        g.block(
            b::IRON_BLOCK,
            [0.0, 0.0, 0.0],
            [0.3 * fade, 0.3 * fade, 1.1 * fade],
            yaw,
            0.0,
        );
        g.block(
            b::RED_CONCRETE,
            [yaw.sin() * 0.68, 0.0, yaw.cos() * 0.68],
            [0.32 * fade; 3],
            yaw,
            PI / 4.0,
        );
        g.block(
            b::SEA_LANTERN,
            [-yaw.sin() * 0.65, 0.0, -yaw.cos() * 0.65],
            [0.22 * fade, 0.22 * fade, 0.5 * fade],
            yaw,
            time * 0.2,
        );
    }
    for burst in &items.bursts {
        let mut g = Group::new(
            &mut stage,
            burst.id,
            Kind::Burst,
            (burst.x, burst.y, burst.z),
        );
        let age = f64::from(burst.age);
        let fade = smoothstep((f64::from(BURST_LIFE) - 2.0 - age) / 6.0);
        let r = burst.current_radius();
        for i in 0..12 {
            let a = f64::from(i) * TAU / 12.0;
            let state = match burst.kind {
                BurstKind::Recharge => b::EMERALD_BLOCK,
                BurstKind::Impact => b::ORANGE_STAINED_GLASS,
                BurstKind::Shockwave | BurstKind::Lightning => {
                    if i % 2 == 0 {
                        b::SEA_LANTERN
                    } else {
                        b::CYAN_STAINED_GLASS
                    }
                }
            };
            g.block(
                state,
                [a.cos() * r, 0.0, a.sin() * r],
                [
                    0.16 * fade,
                    0.3 * fade,
                    (2.0 * r * (PI / 12.0).sin()).max(0.01),
                ],
                -a,
                0.0,
            );
        }
    }
    for bolt in &items.bolts {
        let mut g = Group::new(&mut stage, bolt.id, Kind::Bolt, (bolt.x, bolt.y, bolt.z));
        let fade = if bolt.age < BOLT_FLASH { 1.0 } else { 0.0 };
        let path = bolt_path(bolt);
        for (i, pair) in path.windows(2).enumerate() {
            let (from, to) = (pair[0], pair[1]);
            let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
            let reach = d[0].hypot(d[2]);
            let len = reach.hypot(d[1]);
            g.block(
                if i % 2 == 0 {
                    b::SEA_LANTERN
                } else {
                    b::YELLOW_CONCRETE
                },
                std::array::from_fn(|k| (from[k] + to[k]) * 0.5),
                [BOLT_WIDTH * fade, len * fade, BOLT_WIDTH * fade],
                d[2].atan2(-d[0]),
                reach.atan2(d[1]),
            );
        }
    }
    stage.finish();
}

pub fn bolt_path(bolt: &Bolt) -> [[f64; 3]; BOLT_SEGMENTS + 1] {
    let sway = |bits: u64| (bits % 1000) as f64 / 1000.0 * 2.0 * BOLT_SWAY - BOLT_SWAY;
    std::array::from_fn(|i| {
        if i == 0 {
            return [0.0; 3];
        }
        let bits = bolt.seed.rotate_right(9 * i as u32);
        [
            sway(bits),
            i as f64 * BOLT_HEIGHT / BOLT_SEGMENTS as f64,
            sway(bits >> 10),
        ]
    })
}

fn kart_frames(stage: &mut Stage, source: u64, k: &Kart, time: f64) {
    let at = (k.x, k.y, k.z);
    if let Some(item) = k.item {
        let icon = match item {
            PowerUp::Turbo => i::FIRE_CHARGE,
            PowerUp::Shield => i::SHIELD,
            PowerUp::Banana => i::YELLOW_DYE,
            PowerUp::Missile => i::FIREWORK_ROCKET,
            PowerUp::Shockwave => i::ENDER_PEARL,
            PowerUp::Ice => i::BLUE_ICE,
            PowerUp::Lightning => i::LIGHTNING_ROD,
            PowerUp::Recharge => i::NETHER_STAR,
            PowerUp::Fireball => i::BLAZE_ROD,
        };
        Group::new(stage, source, Kind::Held, at).model(
            Model::Item(icon),
            [0.0, 2.5 + (time * 0.06).sin() * 0.12, 0.0],
            [0.65; 3],
            rotation(time * 0.045, 0.0),
        );
    }
    if k.turbo > 0 || (k.input.boost && k.input.forward && k.fuel >= 1.5) {
        let mut g = Group::new(stage, source, Kind::Jet, at);
        let fade = if k.turbo > 0 {
            envelope(60.0 - f64::from(k.turbo), 60.0)
        } else {
            1.0
        };
        for side in [-1.0, 1.0] {
            let rear = 1.2 + 0.1 * (time * 0.3).sin();
            let center = [
                k.yaw.sin() * rear + k.yaw.cos() * side * 0.45,
                0.32,
                -k.yaw.cos() * rear + k.yaw.sin() * side * 0.45,
            ];
            g.block(
                b::ORANGE_STAINED_GLASS,
                center,
                [
                    0.28 * fade,
                    0.28 * fade,
                    (1.1 + 0.2 * (time * 0.3).sin()) * fade,
                ],
                -k.yaw,
                PI / 4.0,
            );
            g.block(
                b::SEA_LANTERN,
                center,
                [0.12 * fade, 0.12 * fade, 0.65 * fade],
                -k.yaw,
                PI / 4.0,
            );
        }
    }
    if k.turbo > 0 || k.charge > 0 {
        Group::new(stage, source, Kind::Rainbow, at).block(
            rainbow(time as u64),
            [0.0, 0.45, 0.0],
            [1.5, 0.9, 2.0],
            -k.yaw,
            0.0,
        );
    }
    if k.blaze > 0 {
        let mut g = Group::new(stage, source, Kind::Blaze, at);
        let fade = envelope(
            f64::from(FIREBALL_CHARGE - k.blaze),
            f64::from(FIREBALL_CHARGE),
        );
        for i in 0..3 {
            let a = time * 0.16 + f64::from(i) * TAU / 3.0;
            g.model(
                Model::Item(i::BLAZE_ROD),
                [a.cos() * 0.9, 1.1 + 0.2 * (a * 2.0).sin(), a.sin() * 0.9],
                [0.7 * fade; 3],
                rotation(-a, PI / 4.0),
            );
        }
    }
    if k.shield > 0 {
        let mut g = Group::new(stage, source, Kind::Shield, at);
        let fade = envelope(120.0 - f64::from(k.shield), 120.0);
        for i in 0..8 {
            let a = time * 0.055 + f64::from(i) * TAU / 8.0;
            g.block(
                b::CYAN_STAINED_GLASS,
                [a.cos() * 1.25, 0.85 + 0.3 * (a * 2.0).sin(), a.sin() * 1.25],
                [0.14 * fade, 1.05 * fade, 0.7 * fade],
                -a,
                0.15 * (a * 2.0).sin(),
            );
        }
    }
    if k.charge > 0 {
        let mut g = Group::new(stage, source, Kind::Charge, at);
        let fade = envelope(100.0 - f64::from(k.charge), 100.0);
        for i in 0..3 {
            let a = -time * 0.085 + f64::from(i) * TAU / 3.0;
            g.block(
                b::EMERALD_BLOCK,
                [a.cos() * 0.95, 1.35 + 0.35 * a.sin(), a.sin() * 0.95],
                [0.3 * fade; 3],
                a,
                PI / 4.0,
            );
        }
    }
    if k.ice > 0 || k.slow > 0 {
        let mut g = Group::new(stage, source, Kind::Debuff, at);
        let fade = smoothstep(f64::from(k.ice.max(k.slow).saturating_sub(2)) / 6.0);
        for i in 0..3 {
            let a = time * 0.04 + f64::from(i) * TAU / 3.0;
            g.block(
                b::BLUE_ICE,
                [a.cos() * 0.8, 0.4 + 0.15 * a.sin(), a.sin() * 0.8],
                [0.2 * fade, 0.5 * fade, 0.2 * fade],
                a,
                PI / 4.0,
            );
        }
    }
}

struct Group<'a, 'b, 'w, 's, 'cw, 'cs> {
    stage: &'a mut Stage<'b, 'w, 's, 'cw, 'cs>,
    source: u64,
    kind: Kind,
    at: Position,
    part: u8,
}

impl<'a, 'b, 'w, 's, 'cw, 'cs> Group<'a, 'b, 'w, 's, 'cw, 'cs> {
    fn new(
        stage: &'a mut Stage<'b, 'w, 's, 'cw, 'cs>,
        source: u64,
        kind: Kind,
        at: (f64, f64, f64),
    ) -> Self {
        Self {
            stage,
            source,
            kind,
            at: Position {
                x: at.0,
                y: at.1,
                z: at.2,
            },
            part: 0,
        }
    }

    fn model(&mut self, model: Model, center: [f64; 3], scale: [f64; 3], rotation: [f32; 4]) {
        self.stage.apply(
            Key(self.source, self.kind, self.part),
            Frame {
                at: self.at,
                model,
                transform: transform(model, center, scale, rotation),
            },
        );
        self.part += 1;
    }

    fn block(&mut self, state: i32, center: [f64; 3], scale: [f64; 3], yaw: f64, tilt: f64) {
        self.model(Model::Block(state), center, scale, rotation(yaw, tilt));
    }
}

// A block model pivots around its corner: the rotated half-size is moved back
// to the requested center so scaling and orbits never wobble.
fn transform(
    model: Model,
    center: [f64; 3],
    scale: [f64; 3],
    rotation: [f32; 4],
) -> DisplayTransform {
    let scale = scale.map(|v| v.max(0.0) as f32);
    let mut translation = center.map(|v| v as f32);
    if matches!(model, Model::Block(_)) {
        let half = rotate(rotation, scale.map(|v| v * 0.5));
        translation = std::array::from_fn(|i| translation[i] - half[i]);
    }
    DisplayTransform {
        translation,
        scale,
        left_rotation: rotation,
        ..Default::default()
    }
}

// Quaternion composition keeps the client's shortest-arc slerp continuous
// across yaw wraparound; Euler angles would not.
pub fn rotation(yaw: f64, tilt: f64) -> [f32; 4] {
    let (y, w) = (yaw * 0.5).sin_cos();
    let (z, v) = (tilt * 0.5).sin_cos();
    [
        (y * z) as f32,
        (y * v) as f32,
        (w * z) as f32,
        (w * v) as f32,
    ]
}

pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let xyz = [q[0], q[1], q[2]];
    let t = cross(xyz, v).map(|x| x * 2.0);
    let u = cross(xyz, t);
    std::array::from_fn(|i| v[i] + q[3] * t[i] + u[i])
}

pub fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn envelope(age: f64, life: f64) -> f64 {
    smoothstep(age / 4.0) * smoothstep((life - age - 2.0) / 6.0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use voidmc::EntityMetadata;
    use voidmc::components::MinecraftEntityId;
    use voidmc_codec::{Encode, VarI32};
    use voidmc_protocol::clientbound::SetEntityData;

    use super::*;
    use crate::items::Lightning;
    use crate::race::tests::{Harness, Out};

    fn keyframe(h: &mut Harness) {
        h.tick();
        if !h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
            h.tick();
        }
    }

    fn props(h: &mut Harness) -> Vec<Entity> {
        let mut props: Vec<Entity> = h
            .world()
            .query_filtered::<Entity, With<Prop>>()
            .iter(h.app.world())
            .collect();
        props.sort();
        props
    }

    fn network_id(h: &Harness, entity: Entity) -> i32 {
        h.app.world().get::<MinecraftEntityId>(entity).unwrap().0
    }

    fn scene(h: &Harness) -> &Scene {
        h.app.world().resource::<Scene>()
    }

    fn removals(out: &[Out], client: u32) -> Vec<i32> {
        out.iter()
            .filter_map(|o| match o {
                Out::Remove(c, ids) if *c == client => Some(ids.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn touching(out: &[Out], id: i32) -> Vec<Out> {
        out.iter()
            .filter(|o| match o {
                Out::Spawn { id: i, .. }
                | Out::Move { id: i, .. }
                | Out::Teleport { id: i, .. }
                | Out::Metadata(_, i, _)
                | Out::Rotate(_, i, _)
                | Out::HeadRotation(_, i) => *i == id,
                Out::Remove(_, ids) => ids.contains(&id),
                _ => false,
            })
            .cloned()
            .collect()
    }

    fn use_item(h: &mut Harness, player: Entity, item: PowerUp) {
        let mut kart = h.kart_mut(player);
        kart.item = Some(item);
        kart.use_item = true;
        h.tick();
        assert_eq!(h.kart(player).item, None);
    }

    #[test]
    fn a_held_item_is_one_persistent_display_keyframed_at_ten_hertz_and_despawned_when_used() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let _b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        h.kart_mut(a).item = Some(PowerUp::Turbo);
        keyframe(&mut h);
        let icon = props(&mut h);
        assert_eq!(icon.len(), 1);
        let icon = icon[0];
        let id = network_id(&h, icon);
        let kart = h.kart(a).clone();
        let world = h.app.world();
        assert_eq!(
            world.get::<ItemDisplay>(icon).unwrap().item,
            ItemStack::new(ItemId(i::FIRE_CHARGE), 1)
        );
        assert!(world.get::<BlockDisplay>(icon).is_none());
        let display = world.get::<Display>(icon).unwrap();
        assert_eq!(display.interpolation_ticks, KEYFRAME_TICKS);
        assert_eq!(display.teleport_ticks, KEYFRAME_TICKS as u8);
        assert_eq!(display.brightness, Some(BRIGHTNESS));
        assert_eq!(display.view_range, VIEW_RANGE);
        assert_eq!(display.transform.scale, [0.65; 3]);
        assert_eq!(
            *world.get::<Position>(icon).unwrap(),
            Position {
                x: kart.x,
                y: kart.y,
                z: kart.z
            }
        );
        assert_eq!(scene(&h).len(), 1);
        assert_eq!(
            scene(&h).keys().collect::<Vec<_>>(),
            vec![Key(h.kart_entity(a).to_bits(), Kind::Held, 0)]
        );
        let out = h.drain();
        for client in [1, 2] {
            let mine: Vec<Out> = touching(&out, id)
                .into_iter()
                .filter(|o| matches!(o, Out::Spawn { client: c, .. } | Out::Metadata(c, _, _) if *c == client))
                .collect();
            assert_eq!(mine.len(), 2, "{mine:?}");
            assert!(matches!(
                mine[0],
                Out::Spawn { kind, x, y, z, .. }
                    if kind == EntityKind::ItemDisplay.id() && (x, y, z) == (kart.x, kart.y, kart.z)
            ));
            assert_eq!(
                mine[1],
                Out::Metadata(client, id, (8..=24).collect()),
                "a newly shown display receives its whole metadata once"
            );
        }

        for _ in 0..10 {
            h.tick();
            let out = h.drain();
            let mine = touching(&out, id);
            if h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
                assert_eq!(
                    mine,
                    vec![
                        Out::Metadata(1, id, vec![8, 11, 13]),
                        Out::Metadata(2, id, vec![8, 11, 13])
                    ],
                    "a keyframe pushes only the interpolation restart and the changed entries"
                );
            } else {
                assert!(mine.is_empty(), "odd ticks are skipped entirely: {mine:?}");
            }
            assert_eq!(props(&mut h), vec![icon]);
        }

        h.kart_mut(a).item = Some(PowerUp::Shield);
        keyframe(&mut h);
        assert_eq!(
            props(&mut h),
            vec![icon],
            "a new bonus reuses the same display"
        );
        assert_eq!(
            h.app.world().get::<ItemDisplay>(icon).unwrap().item,
            ItemStack::new(ItemId(i::SHIELD), 1)
        );
        let out = h.drain();
        assert_eq!(
            touching(&out, id)
                .iter()
                .filter(|o| matches!(o, Out::Metadata(1, _, e) if e.contains(&23)))
                .count(),
            1
        );

        h.kart_mut(a).item = None;
        keyframe(&mut h);
        assert!(props(&mut h).is_empty());
        assert!(scene(&h).is_empty());
        let out = h.drain();
        for client in [1, 2] {
            assert_eq!(
                touching(&out, id)
                    .into_iter()
                    .filter(|o| matches!(o, Out::Remove(c, _) if *c == client))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn a_kart_that_jumps_gets_fresh_displays_instead_of_a_slide() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &[]);
        h.kart_mut(a).item = Some(PowerUp::Turbo);
        keyframe(&mut h);
        let icon = props(&mut h)[0];
        let id = network_id(&h, icon);
        h.drain();
        h.kart_mut(a).next_gate = 3;
        h.command(a, "reset", &[]);
        h.kart_mut(a).item = Some(PowerUp::Turbo);
        let kart = h.kart(a).clone();
        keyframe(&mut h);
        let replaced = props(&mut h);
        assert_eq!(replaced.len(), 1);
        assert_ne!(replaced[0], icon);
        let fresh = network_id(&h, replaced[0]);
        assert_eq!(
            *h.app.world().get::<Position>(replaced[0]).unwrap(),
            Position {
                x: kart.x,
                y: kart.y,
                z: kart.z
            }
        );
        let out = h.drain();
        assert!(
            touching(&out, id)
                .iter()
                .all(|o| matches!(o, Out::Remove(1, _))),
            "{out:?}"
        );
        assert!(
            touching(&out, fresh)
                .iter()
                .any(|o| matches!(o, Out::Spawn { client: 1, x, y, z, .. } if (*x, *y, *z) == (kart.x, kart.y, kart.z)))
        );
        assert!(
            !out.iter()
                .any(|o| matches!(o, Out::Teleport { id: i, .. } if *i == id || *i == fresh))
        );
        assert!(jumped(
            Position::default(),
            Position {
                x: JUMP + 0.01,
                y: 0.0,
                z: 0.0
            }
        ));
        assert!(!jumped(
            Position::default(),
            Position {
                x: 2.0,
                y: 2.0,
                z: 2.0
            }
        ));
    }

    #[test]
    fn a_driven_kart_moves_its_held_item_on_even_ticks_only() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &[]);
        h.kart_mut(a).item = Some(PowerUp::Turbo);
        keyframe(&mut h);
        let icon = props(&mut h)[0];
        let id = network_id(&h, icon);
        h.kart_mut(a).input.forward = true;
        h.drain();
        for _ in 0..8 {
            h.tick();
            let out = h.drain();
            let moved = touching(&out, id)
                .into_iter()
                .filter(|o| matches!(o, Out::Move { client: 1, .. }))
                .count();
            if h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
                assert_eq!(moved, 1, "{out:?}");
            } else {
                assert_eq!(moved, 0, "{out:?}");
            }
        }
    }

    #[test]
    fn a_disconnect_mid_race_clears_the_scene_for_the_remaining_viewer() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.connect(2);
        h.shortcut_to_racing(a, &[]);
        h.kart_mut(a).item = Some(PowerUp::Turbo);
        keyframe(&mut h);
        let icon = props(&mut h)[0];
        let id = network_id(&h, icon);
        h.drain();
        h.disconnect(a);
        keyframe(&mut h);
        assert!(scene(&h).is_empty());
        assert!(props(&mut h).is_empty());
        let out = h.drain();
        assert!(out.contains(&Out::Remove(2, vec![id])), "{out:?}");
    }

    #[test]
    fn a_boosting_kart_wears_a_rainbow_shell_that_changes_colour_once_per_keyframe() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let _b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        {
            let mut kart = h.kart_mut(a);
            kart.charge = 100;
            kart.fuel = 100.0;
            kart.speed = 0.0;
        }
        keyframe(&mut h);
        let key = Key(h.kart_entity(a).to_bits(), Kind::Rainbow, 0);
        let shell = scene(&h).entity(key).expect("rainbow shell");
        assert!(scene(&h).entity(Key(key.0, Kind::Rainbow, 1)).is_none());
        let id = network_id(&h, shell);
        let frame = scene(&h).frame(key).unwrap();
        assert_eq!(frame.model, Model::Block(rainbow(h.race().tick)));
        assert_eq!(frame.transform.scale, [1.5, 0.9, 2.0]);
        let out = h.drain();
        assert!(
            out.iter()
                .any(|o| matches!(o, Out::Spawn { client: 2, id: i, kind, .. }
                    if *i == id && *kind == EntityKind::BlockDisplay.id()))
        );
        let mut previous = frame.model;
        let mut colours = HashSet::new();
        for _ in 0..(RAINBOW.len() * 2 * KEYFRAME_TICKS as usize) {
            h.tick();
            let tick = h.race().tick;
            let out = h.drain();
            let touched: Vec<&Out> = out
                .iter()
                .filter(|o| match o {
                    Out::Metadata(2, i, _)
                    | Out::Move {
                        client: 2, id: i, ..
                    }
                    | Out::Teleport {
                        client: 2, id: i, ..
                    }
                    | Out::Spawn {
                        client: 2, id: i, ..
                    } => *i == id,
                    Out::Remove(2, ids) => ids.contains(&id),
                    _ => false,
                })
                .collect();
            let model = scene(&h).frame(key).unwrap().model;
            if tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
                assert_eq!(model, Model::Block(rainbow(tick)));
                assert_ne!(model, previous, "{tick}");
                assert_eq!(touched.len(), 1, "{tick}: {touched:?}");
                assert!(matches!(touched[0], Out::Metadata(2, _, indices) if indices == &vec![23]));
                previous = model;
            } else {
                assert_eq!(model, previous);
                assert!(touched.is_empty(), "{tick}: {touched:?}");
            }
            if let Model::Block(state) = model {
                colours.insert(state);
            }
        }
        assert_eq!(colours.len(), RAINBOW.len());
        assert_eq!(scene(&h).entity(key), Some(shell));
        h.kart_mut(a).charge = 0;
        keyframe(&mut h);
        assert!(scene(&h).entity(key).is_none());
        assert!(h.app.world().get_entity(shell).is_err());
        assert!(removals(&h.drain(), 2).contains(&id));

        h.kart_mut(a).turbo = 60;
        keyframe(&mut h);
        assert!(scene(&h).entity(key).is_some());
        h.kart_mut(a).turbo = 0;
        keyframe(&mut h);
        assert!(scene(&h).entity(key).is_none());
    }

    #[test]
    fn a_charging_fireball_orbits_three_blaze_rods_until_the_shot() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &[]);
        use_item(&mut h, a, PowerUp::Fireball);
        keyframe(&mut h);
        let source = h.kart_entity(a).to_bits();
        let rods: Vec<Entity> = (0..3)
            .map(|part| scene(&h).entity(Key(source, Kind::Blaze, part)).unwrap())
            .collect();
        assert!(scene(&h).entity(Key(source, Kind::Blaze, 3)).is_none());
        for (part, rod) in rods.iter().enumerate() {
            let frame = scene(&h)
                .frame(Key(source, Kind::Blaze, part as u8))
                .unwrap();
            assert_eq!(frame.model, Model::Item(i::BLAZE_ROD));
            assert_eq!(
                h.app.world().get::<ItemDisplay>(*rod).unwrap().item,
                ItemStack::new(ItemId(i::BLAZE_ROD), 1)
            );
            let t = frame.transform.translation;
            assert!((f64::from(t[0]).hypot(f64::from(t[2])) - 0.9).abs() < 1e-5);
        }
        while h.kart(a).blaze > 0 {
            h.tick();
        }
        keyframe(&mut h);
        for part in 0..3 {
            assert!(scene(&h).entity(Key(source, Kind::Blaze, part)).is_none());
        }
        for rod in rods {
            assert!(h.app.world().get_entity(rod).is_err());
        }
    }

    #[test]
    fn a_settled_banana_sends_nothing_and_late_joiners_are_served_by_the_engine() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &[]);
        use_item(&mut h, a, PowerUp::Banana);
        h.ticks(12);
        let parts = props(&mut h);
        assert_eq!(parts.len(), 4);
        let mut states: Vec<i32> = parts
            .iter()
            .map(|p| h.app.world().get::<BlockDisplay>(*p).unwrap().0)
            .collect();
        states.sort();
        assert_eq!(states, {
            let mut expected = vec![b::YELLOW_CONCRETE; 3];
            expected.push(b::GOLD_BLOCK);
            expected.sort();
            expected
        });
        let trap = h.app.world().resource::<Items>().traps[0];
        assert_eq!(
            scene(&h).keys().map(|k| (k.0, k.1)).collect::<HashSet<_>>(),
            HashSet::from([(trap.id, Kind::Trap)])
        );
        let ids: Vec<i32> = parts.iter().map(|p| network_id(&h, *p)).collect();
        h.drain();
        h.ticks(10);
        let out = h.drain();
        for id in &ids {
            assert!(touching(&out, *id).is_empty(), "static frames push nothing");
        }
        assert_eq!(props(&mut h), parts);

        let c = h.connect(3);
        h.tick();
        let out = h.drain();
        for id in &ids {
            let mine = touching(&out, *id);
            assert_eq!(mine.len(), 2, "{mine:?}");
            assert!(
                matches!(mine[0], Out::Spawn { client: 3, kind, .. } if kind == EntityKind::BlockDisplay.id())
            );
            assert_eq!(mine[1], Out::Metadata(3, *id, (8..=23).collect()));
        }
        h.disconnect(c);

        h.race_mut().phase = Phase::Destroying;
        keyframe(&mut h);
        assert!(props(&mut h).is_empty());
        assert!(scene(&h).is_empty());
        let out = h.drain();
        let removed: Vec<i32> = out
            .iter()
            .filter_map(|o| match o {
                Out::Remove(1, ids) => Some(ids.clone()),
                _ => None,
            })
            .flatten()
            .filter(|id| ids.contains(id))
            .collect();
        assert_eq!(removed.len(), 4);
    }

    #[test]
    fn every_kind_has_geometry_with_bounded_lifetime_and_no_respawn_per_keyframe() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &[]);
        for item in PowerUp::ALL {
            use_item(&mut h, a, item);
        }
        {
            let mut kart = h.kart_mut(a);
            kart.slow = 50;
            kart.item = Some(PowerUp::Recharge);
        }
        keyframe(&mut h);
        let kinds: HashSet<Kind> = scene(&h).keys().map(|k| k.1).collect();
        for kind in [
            Kind::Held,
            Kind::Jet,
            Kind::Shield,
            Kind::Charge,
            Kind::Debuff,
            Kind::Trap,
            Kind::Missile,
            Kind::Burst,
            Kind::Rainbow,
            Kind::Blaze,
            Kind::Bolt,
        ] {
            assert!(kinds.contains(&kind), "{kind:?}");
        }
        let alive = props(&mut h);
        assert_eq!(alive.len(), scene(&h).len());
        for key in scene(&h).keys().collect::<Vec<_>>() {
            assert!(alive.contains(&scene(&h).entity(key).unwrap()));
        }
        h.drain();
        h.kart_mut(a).item = None;
        for _ in 0..250 {
            h.tick();
            for key in scene(&h).keys().collect::<Vec<_>>() {
                let t = scene(&h).frame(key).unwrap().transform;
                assert!(
                    t.translation
                        .iter()
                        .all(|x| x.is_finite() && x.abs() < 15.0)
                );
                assert!(
                    t.scale
                        .iter()
                        .all(|x| x.is_finite() && (0.0..6.0).contains(x))
                );
                assert!((t.left_rotation.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.00001);
            }
            let out = h.drain();
            assert!(
                !out.iter().any(|o| matches!(o, Out::Spawn { kind, .. }
                    if *kind == EntityKind::BlockDisplay.id() || *kind == EntityKind::ItemDisplay.id())),
                "keyframes must reuse display entities"
            );
        }
        assert!(scene(&h).is_empty());
        assert!(props(&mut h).is_empty());
    }

    #[test]
    fn a_bolt_is_a_seeded_zigzag_of_upright_segments_from_the_sky_to_the_kart() {
        let seeded = |seed| Bolt {
            id: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            seed,
            age: 0,
        };
        let path = bolt_path(&seeded(0x1234_5678_9abc_def0));
        assert_eq!(path[0], [0.0; 3]);
        assert_eq!(path[BOLT_SEGMENTS][1], BOLT_HEIGHT);
        for (i, point) in path.iter().enumerate() {
            let height = i as f64 * BOLT_HEIGHT / BOLT_SEGMENTS as f64;
            assert!((point[1] - height).abs() < 1e-9);
            assert!(point[0].abs() <= BOLT_SWAY && point[2].abs() <= BOLT_SWAY);
        }
        assert!(path.iter().any(|p| p[0].abs() > BOLT_SWAY / 4.0));
        assert!(path.iter().any(|p| p[2].abs() > BOLT_SWAY / 4.0));
        assert_ne!(path, bolt_path(&seeded(0x0fed_cba9_8765_4321)));
        for pair in path.windows(2) {
            let d = [
                pair[1][0] - pair[0][0],
                pair[1][1] - pair[0][1],
                pair[1][2] - pair[0][2],
            ];
            let reach = d[0].hypot(d[2]);
            let len = reach.hypot(d[1]);
            let up = rotate(
                rotation(d[2].atan2(-d[0]), reach.atan2(d[1])),
                [0.0, len as f32, 0.0],
            );
            for k in 0..3 {
                assert!((f64::from(up[k]) - d[k]).abs() < 0.0001, "{up:?} vs {d:?}");
            }
        }
    }

    #[test]
    fn a_strike_spawns_its_segments_once_fades_them_in_one_keyframe_and_removes_them() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        h.kart_mut(b).contact_cooldown = 200;
        h.tick();
        h.drain();
        use_item(&mut h, a, PowerUp::Lightning);
        let delay = h
            .app
            .world()
            .get::<Lightning>(h.kart_entity(b))
            .unwrap()
            .delay;
        h.ticks(usize::from(delay));
        h.drain();
        h.tick();
        let (bx, bz) = (h.kart(b).x, h.kart(b).z);
        let bolt = *h
            .app
            .world()
            .resource::<Items>()
            .bolts
            .iter()
            .find(|bolt| (bolt.x - bx).abs() <= 0.4 + 1e-9 && (bolt.z - bz).abs() <= 0.4 + 1e-9)
            .unwrap();
        assert_eq!(bolt.age, 0);
        assert_ne!(bolt.seed, 0);
        let mut out = h.drain();
        if !h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
            h.tick();
            out.extend(h.drain());
        }
        let segments: Vec<i32> = (0..u8::MAX)
            .map_while(|part| scene(&h).entity(Key(bolt.id, Kind::Bolt, part)))
            .map(|entity| network_id(&h, entity))
            .collect();
        assert_eq!(segments.len(), BOLT_SEGMENTS);
        let path = bolt_path(&bolt);
        for (part, id) in segments.iter().enumerate() {
            let spawns: Vec<Out> = touching(&out, *id)
                .into_iter()
                .filter(|o| matches!(o, Out::Spawn { client: 1, .. }))
                .collect();
            assert_eq!(spawns.len(), 1);
            assert!(matches!(
                spawns[0],
                Out::Spawn { kind, x, y, z, .. }
                    if kind == EntityKind::BlockDisplay.id() && (x, y, z) == (bolt.x, bolt.y, bolt.z)
            ));
            let frame = scene(&h)
                .frame(Key(bolt.id, Kind::Bolt, part as u8))
                .unwrap();
            let state = if part % 2 == 0 {
                b::SEA_LANTERN
            } else {
                b::YELLOW_CONCRETE
            };
            assert_eq!(frame.model, Model::Block(state));
            let len = (0..3)
                .map(|k| (path[part + 1][k] - path[part][k]).powi(2))
                .sum::<f64>()
                .sqrt();
            assert_eq!(frame.transform.scale[0], BOLT_WIDTH as f32);
            assert!((f64::from(frame.transform.scale[1]) - len).abs() < 0.0001);
            let mid: [f64; 3] = std::array::from_fn(|k| (path[part][k] + path[part + 1][k]) * 0.5);
            let corner = rotate(
                frame.transform.left_rotation,
                frame.transform.scale.map(|v| v * 0.5),
            );
            for k in 0..3 {
                assert!(
                    (f64::from(frame.transform.translation[k] + corner[k]) - mid[k]).abs() < 0.001
                );
            }
        }
        let mut fades = 0;
        let mut dead = Vec::new();
        let mut alive = true;
        while alive {
            h.tick();
            let out = h.drain();
            dead.extend(removals(&out, 1));
            alive = h
                .app
                .world()
                .resource::<Items>()
                .bolts
                .iter()
                .any(|b| b.id == bolt.id);
            for id in &segments {
                let events = touching(&out, *id)
                    .into_iter()
                    .filter(|o| !matches!(o, Out::Remove(2, _) | Out::Metadata(2, _, _)))
                    .collect::<Vec<_>>();
                for event in &events {
                    match event {
                        Out::Metadata(1, _, _) => fades += 1,
                        Out::Remove(1, _) => assert!(!alive),
                        other => panic!("{other:?}"),
                    }
                }
            }
        }
        assert_eq!(fades, BOLT_SEGMENTS);
        if !h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
            h.tick();
        }
        for part in 0..BOLT_SEGMENTS as u8 {
            assert!(scene(&h).frame(Key(bolt.id, Kind::Bolt, part)).is_none());
        }
        dead.extend(removals(&h.drain(), 1));
        assert!(segments.iter().all(|id| dead.contains(id)));
    }

    #[test]
    fn easing_has_smooth_endpoints_and_transforms_keep_the_model_center_fixed() {
        for age in [0.0, 118.0, 119.0, 120.0] {
            assert_eq!(envelope(age, 120.0), 0.0);
        }
        assert_eq!(envelope(6.0, 120.0), 1.0);
        assert!(smoothstep(0.001) < 0.00001 && 1.0 - smoothstep(0.999) < 0.00001);
        let center = [1.0, 2.0, -3.0];
        for i in 0..100 {
            let t = transform(
                Model::Block(b::GOLD_BLOCK),
                center,
                [0.2, 0.5, 1.2],
                rotation(f64::from(i) * 0.3, PI / 4.0),
            );
            let rotated = rotate(t.left_rotation, t.scale.map(|v| v * 0.5));
            for i in 0..3 {
                assert!((f64::from(rotated[i] + t.translation[i]) - center[i]).abs() < 0.00001);
            }
        }
        let t = transform(
            Model::Item(i::SHIELD),
            center,
            [0.65; 3],
            rotation(0.0, 0.0),
        );
        assert_eq!(t.translation, [1.0, 2.0, -3.0]);
        assert_eq!(t.left_rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            rotate([0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 3.0]),
            [1.0, 2.0, 3.0]
        );
        let quarter = rotation(PI / 2.0, 0.0);
        let turned = rotate(quarter, [1.0, 0.0, 0.0]);
        assert!(turned[0].abs() < 0.00001 && (turned[2] + 1.0).abs() < 0.00001);
    }

    #[test]
    fn keyframe_metadata_matches_paper_display_wire_layout() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &[]);
        h.kart_mut(a).item = Some(PowerUp::Turbo);
        keyframe(&mut h);
        let icon = props(&mut h)[0];
        h.tick();
        h.tick();
        let world = h.app.world();
        let id = network_id(&h, icon);
        let transform = world.get::<Display>(icon).unwrap().transform;
        let meta = world.get::<EntityMetadata>(icon).unwrap();
        let packet = SetEntityData {
            entity_id: id,
            entries: [8u8, 11, 13]
                .into_iter()
                .map(|index| voidmc_protocol::clientbound::EntityMetadataEntry {
                    index,
                    value: meta.get(index).unwrap().clone(),
                })
                .collect(),
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        let mut expected = Vec::new();
        VarI32(id).encode(&mut expected);
        expected.extend([8, 1, 0, 11, 39]);
        for v in transform.translation {
            expected.extend(v.to_be_bytes());
        }
        expected.extend([13, 40]);
        for v in transform.left_rotation {
            expected.extend(v.to_be_bytes());
        }
        expected.push(0xFF);
        assert_eq!(buf, expected);
        assert_eq!(
            meta.get(9),
            Some(&voidmc_protocol::clientbound::EntityMetadataValue::Int(2))
        );
        assert_eq!(
            meta.get(10),
            Some(&voidmc_protocol::clientbound::EntityMetadataValue::Int(2))
        );
        assert_eq!(
            meta.get(16),
            Some(&voidmc_protocol::clientbound::EntityMetadataValue::Int(
                0x00f000f0
            ))
        );
    }
}
