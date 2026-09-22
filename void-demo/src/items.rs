use std::f64::consts::TAU;

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{
    Audience, EndCrystal, EntityBuilder, EntityKind, Particle, Particles,
    components::{EntityViewers, MinecraftEntityId, Position},
};
use voidmc_data::v26_1_2::items as i;
use voidmc_protocol::clientbound::ItemStackTemplate;

use crate::audio::{Audio, Cue};
use crate::chat::{Chat, Tone};
use crate::kart::{Kart, PowerUp, Strike};
use crate::race::{Phase, Race};
use crate::terrain::mix;
use crate::track::{GATES, HALF_WIDTH, Track};
use crate::vehicle::Pilot;

pub const PICKUP_RESPAWN: u64 = 160;
pub const PICKUP_RADIUS: f64 = 1.6;
pub const PICKUP_OFFSETS: [f64; 3] = [-3.5, 0.0, 3.5];
pub const MISSILE_LIFE: u16 = 160;
pub const MISSILE_RADIUS: f64 = 2.5;
pub const BURST_LIFE: u8 = 12;
pub const SHOCKWAVE_RADIUS: f64 = 9.0;
pub const LIGHTNING_STRIKES: u16 = 3;
pub const LIGHTNING_PERIOD: u16 = 6;
pub const LIGHTNING_SPREAD: u16 = 18;
pub const BOLT_LIFE: u8 = 8;
pub const FIREBALL_SPEED: f64 = 1.2;
pub const FIREBALL_LIFE: u16 = 60;
pub const FIREBALL_RADIUS: f64 = 2.0;
pub const FIREBALL_HEIGHT: f64 = 0.3;
const ROLL_STRIDE: u64 = 0x9e3779b97f4a7c15;
const TRAIL_PERIOD: u64 = 3;
const MISSILE_PERIOD: u64 = 2;
const BURST_PERIOD: u64 = 4;
const GROUND_PERIOD: u64 = 10;
const RING_POINTS: u32 = 6;
const BOOST_RING_POINTS: u32 = 12;
const BOOST_RING_RADIUS: f64 = 1.2;

#[derive(Debug, Clone, PartialEq)]
pub struct Pickup {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub ready_at: u64,
    pub crystal: Option<Entity>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trap {
    pub id: u64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub owner: Entity,
    pub placed: u64,
    pub ice: bool,
}

impl Trap {
    pub fn age(&self, tick: u64) -> u64 {
        tick.saturating_sub(self.placed)
    }

    pub fn life(&self) -> u64 {
        if self.ice { 200 } else { 240 }
    }

    pub fn radius(&self) -> f64 {
        if self.ice { 2.5 } else { 1.5 }
    }

    fn hits(&self, kart: Entity, x: f64, z: f64, tick: u64) -> bool {
        (kart != self.owner || self.age(tick) > 20)
            && (x - self.x).hypot(z - self.z) < self.radius()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Missile {
    pub id: u64,
    pub owner: Entity,
    pub target: Option<Entity>,
    pub phase: f64,
    pub offset: f64,
    pub age: u16,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fireball {
    pub id: u64,
    pub owner: Entity,
    pub entity: Entity,
    pub age: u16,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vz: f64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Projectile;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lightning {
    pub delay: u16,
    pub seed: u64,
    pub remaining: u16,
}

impl Lightning {
    pub fn jitter(&self) -> (f64, f64) {
        let spread = |bits: u64| (bits % 1000) as f64 / 1000.0 * 0.8 - 0.4;
        (spread(self.seed >> 8), spread(self.seed >> 40))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bolt {
    pub id: u64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub seed: u64,
    pub age: u8,
}

impl Bolt {
    fn at(id: u64, kart: &Kart, seed: u64, jitter: (f64, f64)) -> Self {
        Self {
            id,
            x: kart.x + jitter.0,
            y: kart.y,
            z: kart.z + jitter.1,
            seed,
            age: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurstKind {
    Shockwave,
    Lightning,
    Recharge,
    Impact,
}

impl BurstKind {
    pub fn particle(self) -> Particle {
        match self {
            Self::Recharge => Particle::HappyVillager,
            Self::Impact => Particle::Firework,
            Self::Shockwave | Self::Lightning => Particle::ElectricSpark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Burst {
    pub id: u64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub age: u8,
    pub radius: f64,
    pub kind: BurstKind,
}

impl Burst {
    fn at(id: u64, kart: &Kart, radius: f64, kind: BurstKind) -> Self {
        Self {
            id,
            x: kart.x,
            y: kart.y + 0.6,
            z: kart.z,
            age: 0,
            radius,
            kind,
        }
    }

    pub fn current_radius(&self) -> f64 {
        self.radius * ease_out(f64::from(self.age) / f64::from(BURST_LIFE - 1))
    }
}

pub fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SparkKind {
    Explosion,
    Lightning,
    Slide,
    Ice,
    Boost,
    Shield,
    Launch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spark {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub kind: SparkKind,
}

impl Spark {
    fn at(kart: &Kart, kind: SparkKind) -> Self {
        Self {
            x: kart.x,
            y: kart.y + 0.5,
            z: kart.z,
            yaw: kart.yaw,
            kind,
        }
    }

    fn along(&self) -> (f32, f32, f32) {
        (
            (self.yaw.sin().abs() * 1.5 + 0.3) as f32,
            0.25,
            (self.yaw.cos().abs() * 1.5 + 0.3) as f32,
        )
    }
}

#[derive(Resource, Default, Debug)]
pub struct Items {
    pub pickups: Vec<Pickup>,
    pub traps: Vec<Trap>,
    pub missiles: Vec<Missile>,
    pub fireballs: Vec<Fireball>,
    pub bursts: Vec<Burst>,
    pub bolts: Vec<Bolt>,
    pub sparks: Vec<Spark>,
    serial: u64,
}

impl Items {
    pub fn serial(&mut self) -> u64 {
        self.serial += 1;
        self.serial
    }

    pub fn place(&mut self, map: &Track) {
        self.pickups = (0..GATES)
            .flat_map(|gate| {
                PICKUP_OFFSETS.into_iter().map(move |offset| {
                    let (x, y, z) = map.point((gate as f64 + 0.45) * TAU / GATES as f64, offset);
                    Pickup {
                        x,
                        y,
                        z,
                        ready_at: 0,
                        crystal: None,
                    }
                })
            })
            .collect();
    }

    pub fn clear(&mut self, commands: &mut Commands) {
        for pickup in self.pickups.drain(..) {
            if let Some(crystal) = pickup.crystal {
                despawn(commands, crystal);
            }
        }
        self.traps.clear();
        self.missiles.clear();
        for fireball in self.fireballs.drain(..) {
            despawn(commands, fireball.entity);
        }
        self.bursts.clear();
        self.bolts.clear();
        self.sparks.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.pickups.is_empty()
            && self.traps.is_empty()
            && self.missiles.is_empty()
            && self.fireballs.is_empty()
            && self.bursts.is_empty()
            && self.bolts.is_empty()
            && self.sparks.is_empty()
    }
}

fn despawn(commands: &mut Commands, entity: Entity) {
    if let Ok(mut entity) = commands.get_entity(entity) {
        entity.despawn();
    }
}

pub fn crystals(race: Res<Race>, mut items: ResMut<Items>, mut commands: Commands) {
    let tick = race.tick;
    for pickup in items
        .pickups
        .iter_mut()
        .filter(|p| p.crystal.is_none() && tick >= p.ready_at)
    {
        pickup.crystal = Some(
            EntityBuilder::new(EntityKind::EndCrystal)
                .at(pickup.x, pickup.y - 0.2, pickup.z)
                .gravity(false)
                .with(EndCrystal::floating())
                .spawn(&mut commands)
                .id(),
        );
    }
}

#[derive(Clone, Copy)]
struct Contender {
    kart: Entity,
    player: Entity,
    id: i32,
    x: f64,
    z: f64,
    progress: f64,
}

type KartQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Pilot,
        &'static MinecraftEntityId,
        &'static mut Kart,
    ),
>;

pub fn describe(item: PowerUp) -> &'static str {
    match item {
        PowerUp::Turbo => "acceleration 3 s",
        PowerUp::Shield => "protege 6 s",
        PowerUp::Banana => "piege depose 12 s",
        PowerUp::Missile => "vise le pilote devant toi",
        PowerUp::Shockwave => "rivaux a moins de 9 blocs repousses",
        PowerUp::Ice => "nappe deposee 10 s",
        PowerUp::Lightning => "orage sur tous les adversaires 2,5 s",
        PowerUp::Recharge => "boost gratuit 5 s : Saut + Avancer",
        PowerUp::Fireball => "tir tout droit apres 2 s",
    }
}

#[derive(SystemParam)]
pub struct Field<'w, 's> {
    map: Res<'w, Track>,
    items: ResMut<'w, Items>,
    karts: KartQuery<'w, 's>,
    storms: Query<'w, 's, (Entity, &'static mut Lightning)>,
    chat: Chat<'w, 's>,
    audio: Audio<'w, 's>,
    commands: Commands<'w, 's>,
}

impl Field<'_, '_> {
    fn racers(&self) -> Vec<Contender> {
        let mut racers: Vec<Contender> = self
            .karts
            .iter()
            .filter(|(_, _, _, kart)| kart.racing())
            .map(|(kart, pilot, id, k)| Contender {
                kart,
                player: pilot.0,
                id: id.0,
                x: k.x,
                z: k.z,
                progress: k.progress(&self.map),
            })
            .collect();
        racers.sort_by_key(|racer| racer.id);
        racers
    }

    fn struck(
        &mut self,
        target: &Contender,
        strike: Strike,
        shielded: bool,
        what: &str,
        hit: &str,
    ) {
        let kind = if shielded {
            SparkKind::Shield
        } else {
            match strike {
                Strike::Missile | Strike::Fireball | Strike::Shockwave { .. } => {
                    SparkKind::Explosion
                }
                Strike::Lightning => SparkKind::Lightning,
                Strike::Banana => SparkKind::Slide,
                Strike::Ice => SparkKind::Ice,
            }
        };
        let spark = Spark::at(self.karts.get(target.kart).unwrap().3, kind);
        self.items.sparks.push(spark);
        if shielded {
            self.audio.at(target.kart, Cue::Shielded);
            self.chat.flash(
                target.player,
                Tone::Good,
                format!("Bouclier ! {what} sans effet"),
            );
        } else {
            self.audio.at(target.kart, Cue::Hit(strike.into()));
            self.chat.flash(target.player, Tone::Warn, hit);
        }
    }

    fn activate(&mut self, item: PowerUp, racer: &Contender, racers: &[Contender], tick: u64) {
        self.chat.flash(
            racer.player,
            Tone::Good,
            format!("{} : {}", item.name(), describe(item)),
        );
        self.audio.at(racer.kart, Cue::Activate(item));
        let mut kart = self.karts.get_mut(racer.kart).unwrap().3;
        kart.activate(item);
        match item {
            PowerUp::Shield | PowerUp::Fireball => {}
            PowerUp::Turbo => {
                let spark = Spark::at(&kart, SparkKind::Boost);
                self.items.sparks.push(spark);
            }
            PowerUp::Recharge => {
                let burst = Burst::at(self.items.serial(), &kart, 3.0, BurstKind::Recharge);
                self.items.bursts.push(burst);
            }
            PowerUp::Banana | PowerUp::Ice => {
                let (x, y, z) = kart.trap_drop();
                let id = self.items.serial();
                self.items.traps.push(Trap {
                    id,
                    x,
                    y,
                    z,
                    owner: racer.kart,
                    placed: tick,
                    ice: item == PowerUp::Ice,
                });
            }
            PowerUp::Missile => {
                let target = racers
                    .iter()
                    .filter(|r| r.kart != racer.kart && r.progress > racer.progress)
                    .min_by(|a, b| a.progress.total_cmp(&b.progress));
                let missile = Missile {
                    id: self.items.serial(),
                    owner: racer.kart,
                    target: target.map(|r| r.kart),
                    phase: self.map.project(kart.x, kart.z).phase,
                    offset: 0.0,
                    age: 0,
                    x: kart.x,
                    y: kart.y + 0.6,
                    z: kart.z,
                };
                self.items.missiles.push(missile);
                if target.is_none() {
                    self.chat.flash(
                        racer.player,
                        Tone::Notice,
                        "MISSILE GUIDE : aucune cible devant, tir en ligne droite",
                    );
                }
            }
            PowerUp::Lightning => {
                let burst = Burst::at(self.items.serial(), &kart, 3.0, BurstKind::Lightning);
                self.items.bursts.push(burst);
                let roll = mix(self.map.seed ^ tick.wrapping_mul(ROLL_STRIDE) ^ racer.id as u64);
                let bolt = Bolt::at(self.items.serial(), &kart, roll, (0.0, 0.0));
                self.items.bolts.push(bolt);
                for (index, other) in racers.iter().filter(|r| r.kart != racer.kart).enumerate() {
                    let delay = (roll as u16).wrapping_add(2 * index as u16) % LIGHTNING_SPREAD;
                    self.commands.entity(other.kart).insert(Lightning {
                        delay,
                        seed: mix(roll ^ other.id as u64),
                        remaining: LIGHTNING_STRIKES,
                    });
                }
            }
            PowerUp::Shockwave => {
                let burst = Burst::at(
                    self.items.serial(),
                    &kart,
                    SHOCKWAVE_RADIUS,
                    BurstKind::Shockwave,
                );
                self.items.bursts.push(burst);
                let spark = Spark::at(&kart, SparkKind::Explosion);
                self.items.sparks.push(spark);
                for other in racers.iter().filter(|r| r.kart != racer.kart) {
                    let (dx, dz) = (other.x - racer.x, other.z - racer.z);
                    let distance = dx.hypot(dz);
                    if distance > SHOCKWAVE_RADIUS {
                        continue;
                    }
                    let (nx, nz) = if distance < 0.001 {
                        (1.0, 0.0)
                    } else {
                        (dx / distance, dz / distance)
                    };
                    let strike = Strike::Shockwave { nx, nz };
                    let mut kart = self.karts.get_mut(other.kart).unwrap().3;
                    let shielded = kart.strike(strike);
                    let burst = Burst::at(self.items.serial(), &kart, 2.5, BurstKind::Impact);
                    self.items.bursts.push(burst);
                    self.struck(
                        other,
                        strike,
                        shielded,
                        item.name(),
                        &format!("Touche par {} !", item.name()),
                    );
                }
            }
        }
    }

    fn storms(&mut self, racers: &[Contender]) {
        let mut strikes: Vec<(Contender, bool)> = Vec::new();
        let mut done = Vec::new();
        for (entity, mut storm) in &mut self.storms {
            let Some(target) = racers.iter().find(|r| r.kart == entity) else {
                done.push(entity);
                continue;
            };
            if storm.delay > 0 {
                storm.delay -= 1;
                continue;
            }
            storm.seed = mix(storm.seed);
            let first = storm.remaining == LIGHTNING_STRIKES;
            storm.remaining -= 1;
            storm.delay = LIGHTNING_PERIOD - 1;
            let kart = self.karts.get(entity).unwrap().3;
            let bolt = Bolt::at(self.items.serial(), kart, storm.seed, storm.jitter());
            self.items.bolts.push(bolt);
            strikes.push((*target, first));
            if storm.remaining == 0 {
                done.push(entity);
            }
        }
        for (target, first) in strikes {
            let mut kart = self.karts.get_mut(target.kart).unwrap().3;
            let burst = Burst::at(self.items.serial(), &kart, 2.5, BurstKind::Lightning);
            self.items.bursts.push(burst);
            self.audio.at(target.kart, Cue::Thunder);
            if first {
                self.audio.at(target.kart, Cue::Impact);
                let shielded = kart.strike(Strike::Lightning);
                self.struck(
                    &target,
                    Strike::Lightning,
                    shielded,
                    PowerUp::Lightning.name(),
                    "Foudroye ! Ralenti 2,5 s",
                );
            }
        }
        for entity in done {
            self.commands.entity(entity).remove::<Lightning>();
        }
    }

    fn calm(&mut self) {
        for (entity, _) in &self.storms {
            self.commands.entity(entity).remove::<Lightning>();
        }
    }

    fn charge(&mut self, racer: &Contender) {
        let (x, y, z, vx, vz, yaw, spark) = {
            let mut kart = self.karts.get_mut(racer.kart).unwrap().3;
            if kart.blaze == 0 {
                return;
            }
            kart.blaze -= 1;
            if kart.blaze > 0 {
                return;
            }
            (
                kart.x,
                kart.y + FIREBALL_HEIGHT,
                kart.z,
                -kart.yaw.sin() * FIREBALL_SPEED,
                kart.yaw.cos() * FIREBALL_SPEED,
                kart.model_yaw(),
                Spark::at(&kart, SparkKind::Launch),
            )
        };
        let entity = EntityBuilder::new(EntityKind::Fireball)
            .at(x, y, z)
            .rotation(yaw, 0.0)
            .gravity(false)
            .with(Projectile)
            .spawn(&mut self.commands)
            .id();
        let id = self.items.serial();
        self.items.fireballs.push(Fireball {
            id,
            owner: racer.kart,
            entity,
            age: 0,
            x,
            y,
            z,
            vx,
            vz,
        });
        self.items.sparks.push(spark);
        self.audio.at(racer.kart, Cue::Launch);
    }

    fn fireballs(&mut self, racers: &[Contender]) {
        let map = &self.map;
        let mut hits: Vec<Contender> = Vec::new();
        let mut spent = Vec::new();
        let mut fireballs = std::mem::take(&mut self.items.fireballs);
        fireballs.retain_mut(|fireball| {
            fireball.age += 1;
            fireball.x += fireball.vx;
            fireball.z += fireball.vz;
            let projected = map.project(fireball.x, fireball.z);
            if fireball.age >= FIREBALL_LIFE || projected.distance > HALF_WIDTH {
                spent.push(fireball.entity);
                return false;
            }
            fireball.y = projected.y + FIREBALL_HEIGHT;
            let hit = racers
                .iter()
                .filter(|r| r.kart != fireball.owner)
                .map(|r| (r, (r.x - fireball.x).hypot(r.z - fireball.z)))
                .filter(|(_, distance)| *distance < FIREBALL_RADIUS)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((racer, _)) = hit {
                hits.push(*racer);
                spent.push(fireball.entity);
            }
            hit.is_none()
        });
        self.items.fireballs = fireballs;
        for entity in spent {
            despawn(&mut self.commands, entity);
        }
        for racer in &hits {
            let mut kart = self.karts.get_mut(racer.kart).unwrap().3;
            let shielded = kart.strike(Strike::Fireball);
            let burst = Burst::at(self.items.serial(), &kart, 3.0, BurstKind::Impact);
            self.items.bursts.push(burst);
            self.struck(
                racer,
                Strike::Fireball,
                shielded,
                "Boule de feu",
                "Boule de feu ! Ralenti 1 s",
            );
        }
    }

    fn collect(&mut self, racer: &Contender, tick: u64) {
        let Some((index, pickup)) = self.items.pickups.iter_mut().enumerate().find(|(_, p)| {
            tick >= p.ready_at && (racer.x - p.x).hypot(racer.z - p.z) < PICKUP_RADIUS
        }) else {
            return;
        };
        let roll = mix(self.map.seed
            ^ tick.wrapping_mul(ROLL_STRIDE)
            ^ ((index as u64) << 32)
            ^ racer.id as u64);
        let item = PowerUp::roll(roll);
        pickup.ready_at = tick + PICKUP_RESPAWN;
        if let Some(crystal) = pickup.crystal.take() {
            despawn(&mut self.commands, crystal);
        }
        self.karts.get_mut(racer.kart).unwrap().3.collect(item);
        self.audio.ui(racer.player, Cue::Pickup);
        self.chat.flash(
            racer.player,
            Tone::Notice,
            format!("Bonus : {} — Sprint pour l'utiliser", item.name()),
        );
    }

    fn traps(&mut self, racer: &Contender, tick: u64) {
        let mut traps = std::mem::take(&mut self.items.traps);
        traps.retain(|trap| {
            if !trap.hits(racer.kart, racer.x, racer.z, tick) {
                return true;
            }
            let mut kart = self.karts.get_mut(racer.kart).unwrap().3;
            if trap.ice {
                let fresh = kart.ice == 0;
                if !kart.strike(Strike::Ice) && fresh {
                    self.struck(
                        racer,
                        Strike::Ice,
                        false,
                        "Glace",
                        "Glace ! Adherence reduite 3 s",
                    );
                }
            } else {
                let shielded = kart.strike(Strike::Banana);
                self.struck(
                    racer,
                    Strike::Banana,
                    shielded,
                    "Banane",
                    "Banane ! Derapage — contre-braque",
                );
            }
            trap.ice
        });
        self.items.traps = traps;
    }

    fn missiles(&mut self, racers: &[Contender]) {
        let map = &self.map;
        let mut hits: Vec<Contender> = Vec::new();
        let mut missiles = std::mem::take(&mut self.items.missiles);
        missiles.retain_mut(|missile| {
            missile.age += 1;
            if missile.age >= MISSILE_LIFE {
                return false;
            }
            if let Some(target) = missile.target {
                let Some(target) = racers.iter().find(|r| r.kart == target) else {
                    return false;
                };
                let projection = map.project(target.x, target.z);
                let center = map.point(projection.phase, 0.0);
                let offset = (target.x - center.0) * projection.normal.0
                    + (target.z - center.2) * projection.normal.1;
                missile.offset += (offset.clamp(-5.5, 5.5) - missile.offset).clamp(-0.3, 0.3);
            }
            missile.phase = (missile.phase + 2.0 / map.length * TAU).rem_euclid(TAU);
            (missile.x, missile.y, missile.z) = map.point(missile.phase, missile.offset);
            missile.y += 0.6;
            let hit = racers.iter().find(|r| {
                r.kart != missile.owner && (r.x - missile.x).hypot(r.z - missile.z) < MISSILE_RADIUS
            });
            if let Some(racer) = hit {
                hits.push(*racer);
            }
            hit.is_none()
        });
        self.items.missiles = missiles;
        for racer in &hits {
            let mut kart = self.karts.get_mut(racer.kart).unwrap().3;
            let shielded = kart.strike(Strike::Missile);
            let burst = Burst::at(self.items.serial(), &kart, 3.0, BurstKind::Impact);
            self.items.bursts.push(burst);
            self.struck(
                racer,
                Strike::Missile,
                shielded,
                "Missile",
                "Missile ! Ralenti 1,5 s",
            );
        }
    }
}

pub fn update(race: Res<Race>, mut field: Field) {
    match race.phase {
        Phase::Racing => {}
        Phase::Loading | Phase::Countdown => return,
        Phase::Lobby | Phase::Generating | Phase::Destroying | Phase::Results => {
            if !field.items.is_empty() {
                let Field {
                    items, commands, ..
                } = &mut field;
                items.clear(commands);
            }
            field.calm();
            return;
        }
    }
    let tick = race.tick;
    field
        .items
        .traps
        .retain(|trap| trap.age(tick) < trap.life());
    for burst in &mut field.items.bursts {
        burst.age += 1;
    }
    field.items.bursts.retain(|burst| burst.age < BURST_LIFE);
    for bolt in &mut field.items.bolts {
        bolt.age += 1;
    }
    field.items.bolts.retain(|bolt| bolt.age < BOLT_LIFE);
    let racers = field.racers();
    for racer in &racers {
        let Ok((_, _, _, kart)) = field.karts.get(racer.kart) else {
            continue;
        };
        if kart.use_item
            && let Some(item) = field.karts.get_mut(racer.kart).unwrap().3.take_item()
        {
            field.activate(item, racer, &racers, tick);
        }
        if field
            .karts
            .get(racer.kart)
            .is_ok_and(|(_, _, _, k)| k.item.is_none())
        {
            field.collect(racer, tick);
        }
        field.traps(racer, tick);
        field.charge(racer);
    }
    field.missiles(&racers);
    field.fireballs(&racers);
    field.storms(&racers);
}

pub fn fly(items: Res<Items>, mut projectiles: Query<&mut Position, With<Projectile>>) {
    for fireball in &items.fireballs {
        let Ok(mut position) = projectiles.get_mut(fireball.entity) else {
            continue;
        };
        let next = Position {
            x: fireball.x,
            y: fireball.y,
            z: fireball.z,
        };
        if *position != next {
            *position = next;
        }
    }
}

pub fn effects(
    race: Res<Race>,
    mut items: ResMut<Items>,
    particles: Particles,
    karts: Query<(&Kart, &EntityViewers)>,
    viewers: Query<&EntityViewers>,
) {
    let tick = race.tick;
    if !items.sparks.is_empty() {
        for spark in items.sparks.drain(..) {
            emit(&particles, &spark);
        }
    }
    let cloud = |particle: Particle, at: [f64; 3], count: i32| {
        particles
            .spawn(particle)
            .at(at)
            .count(count)
            .offset(0.35, 0.35, 0.35)
            .speed(0.025)
            .long_distance(true)
    };
    let point =
        |particle: Particle, at: [f64; 3]| particles.spawn(particle).at(at).long_distance(true);
    let ring = |particle: Particle, center: [f64; 3], radius: f64| {
        let mut dot = point(particle, center).audience(Audience::All);
        let everyone = dot.recipients();
        for i in 0..RING_POINTS {
            let angle = f64::from(i) * TAU / f64::from(RING_POINTS);
            dot = dot.at([
                center[0] + angle.cos() * radius,
                center[1],
                center[2] + angle.sin() * radius,
            ]);
            everyone.send(dot.packet());
        }
    };
    if tick.is_multiple_of(GROUND_PERIOD) {
        for pickup in &items.pickups {
            let Some(seen) = pickup.crystal.and_then(|c| viewers.get(c).ok()) else {
                continue;
            };
            if seen.is_empty() {
                continue;
            }
            let mut dot =
                point(Particle::EndRod, [pickup.x, pickup.y, pickup.z]).viewers(seen.iter());
            let watchers = dot.recipients();
            for i in 0..4 {
                let angle = tick as f64 * 0.06 + f64::from(i) * TAU / 4.0;
                dot = dot.at([
                    pickup.x + angle.cos() * 1.15,
                    pickup.y + 0.5 + (angle * 2.0).sin() * 0.15,
                    pickup.z + angle.sin() * 1.15,
                ]);
                watchers.send(dot.packet());
            }
        }
    }
    if race.phase != Phase::Racing {
        return;
    }
    if tick.is_multiple_of(TRAIL_PERIOD) {
        for (kart, seen) in &karts {
            if !kart.participant || seen.is_empty() {
                continue;
            }
            let Some(effect) = trail(kart) else {
                continue;
            };
            cloud(effect, [kart.x, kart.y + 0.3, kart.z], 3)
                .viewers(seen.iter())
                .send();
        }
    }
    if tick.is_multiple_of(MISSILE_PERIOD) {
        for missile in &items.missiles {
            let at = [missile.x, missile.y, missile.z];
            let flame = cloud(Particle::Flame, at, 2).audience(Audience::All);
            let everyone = flame.recipients();
            everyone.send(flame.packet());
            everyone.send(cloud(Particle::Smoke, at, 1).packet());
        }
        for fireball in &items.fireballs {
            let at = [fireball.x, fireball.y + 0.5, fireball.z];
            cloud(Particle::Flame, at, 3).audience(Audience::All).send();
        }
    }
    if tick.is_multiple_of(BURST_PERIOD) {
        for burst in &items.bursts {
            ring(
                burst.kind.particle(),
                [burst.x, burst.y, burst.z],
                burst.current_radius(),
            );
        }
    }
    if tick.is_multiple_of(GROUND_PERIOD) {
        for trap in &items.traps {
            if trap.ice {
                ring(
                    Particle::EndRod,
                    [trap.x, trap.y + 0.15, trap.z],
                    trap.radius(),
                );
            } else {
                cloud(Particle::ItemSlime, [trap.x, trap.y + 0.2, trap.z], 3)
                    .audience(Audience::All)
                    .send();
            }
        }
    }
}

fn emit(particles: &Particles, spark: &Spark) {
    let at = [spark.x, spark.y, spark.z];
    let seed = particles
        .spawn(Particle::Explosion)
        .at(at)
        .long_distance(true)
        .audience(Audience::All);
    let everyone = seed.recipients();
    let cloud = |particle: Particle, count: i32, offset: (f32, f32, f32), speed: f32| {
        particles
            .spawn(particle)
            .at(at)
            .count(count)
            .offset(offset.0, offset.1, offset.2)
            .speed(speed)
            .long_distance(true)
            .packet()
    };
    match spark.kind {
        SparkKind::Explosion => {
            everyone.send(seed.packet());
            everyone.send(cloud(Particle::LargeSmoke, 10, (0.6, 0.6, 0.6), 0.05));
            everyone.send(cloud(Particle::Flame, 8, (0.3, 0.3, 0.3), 0.1));
            everyone.send(cloud(Particle::Crit, 6, (0.4, 0.4, 0.4), 0.2));
        }
        SparkKind::Lightning => {
            everyone.send(seed.packet());
            everyone.send(cloud(Particle::ElectricSpark, 24, (0.5, 0.8, 0.5), 0.3));
        }
        SparkKind::Slide => {
            let along = spark.along();
            let banana = Particle::Item {
                stack: ItemStackTemplate::simple(i::YELLOW_DYE, 1),
            };
            everyone.send(cloud(banana, 12, along, 0.05));
            everyone.send(cloud(Particle::Cloud, 8, along, 0.02));
        }
        SparkKind::Ice => {
            let along = spark.along();
            everyone.send(cloud(Particle::Snowflake, 20, along, 0.02));
            everyone.send(cloud(Particle::ItemSnowball, 6, along, 0.05));
        }
        SparkKind::Boost => {
            let mut dot = particles.spawn(Particle::Flame).at(at).long_distance(true);
            for i in 0..BOOST_RING_POINTS {
                let angle = f64::from(i) * TAU / f64::from(BOOST_RING_POINTS);
                dot = dot.at([
                    at[0] + angle.cos() * BOOST_RING_RADIUS,
                    at[1],
                    at[2] + angle.sin() * BOOST_RING_RADIUS,
                ]);
                everyone.send(dot.packet());
            }
        }
        SparkKind::Shield => {
            everyone.send(cloud(Particle::ElectricSpark, 30, (0.8, 0.8, 0.8), 0.05));
        }
        SparkKind::Launch => {
            everyone.send(cloud(Particle::Flame, 16, (0.4, 0.4, 0.4), 0.08));
            everyone.send(cloud(Particle::Lava, 4, (0.3, 0.3, 0.3), 0.0));
        }
    }
}

pub fn trail(kart: &Kart) -> Option<Particle> {
    if kart.impact > 0 {
        Some(Particle::Crit)
    } else if kart.shield > 0 {
        Some(Particle::ElectricSpark)
    } else if kart.ice > 0 || kart.slow > 0 {
        Some(Particle::EndRod)
    } else if kart.charge > 0 {
        Some(Particle::HappyVillager)
    } else if kart.blaze > 0
        || kart.turbo > 0
        || (kart.input.boost && kart.input.forward && kart.fuel >= 1.5)
    {
        Some(Particle::Flame)
    } else if kart.speed.abs() > 0.25 {
        Some(Particle::Cloud)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::f64::consts::TAU;

    use voidmc::components::{LoadedChunks, MinecraftEntityId, Position};
    use voidmc::{EntityKind, EntityMetadata};
    use voidmc_codec::{Encode, VarI32};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::*;
    use crate::audio::Hit;
    use crate::chat::HUD_COLOR;
    use crate::displays::{BOLT_FLASH, BOLT_SEGMENTS, KEYFRAME_TICKS, Key, Kind, Scene};
    use crate::kart::FIREBALL_CHARGE;
    use crate::race::tests::{Harness, Out, sounds};
    use voidmc::SoundSource;

    fn scene(h: &Harness) -> &Scene {
        h.app.world().resource::<Scene>()
    }

    fn items(h: &Harness) -> &Items {
        h.app.world().resource::<Items>()
    }

    fn storm(h: &Harness, player: Entity) -> Option<Lightning> {
        h.app
            .world()
            .get::<Lightning>(h.kart_entity(player))
            .copied()
    }

    fn bolt_props(h: &Harness, bolt: &Bolt) -> Vec<Entity> {
        let scene = h.app.world().resource::<Scene>();
        (0..u8::MAX)
            .map_while(|part| scene.entity(Key(bolt.id, Kind::Bolt, part)))
            .collect()
    }

    fn emitted_by(out: &[Out], client: u32, cue: Cue, emitter: i32) -> usize {
        sounds(out, client, cue)
            .iter()
            .filter(|o| matches!(o, Out::Sound { emitter: Some(e), .. } if *e == emitter))
            .count()
    }

    fn spawns_of(out: &[Out], client: u32, kind: EntityKind) -> Vec<(i32, (f64, f64, f64))> {
        out.iter()
            .filter_map(|o| match o {
                Out::Spawn {
                    client: c,
                    id,
                    kind: k,
                    x,
                    y,
                    z,
                    ..
                } if *c == client && *k == kind.id() => Some((*id, (*x, *y, *z))),
                _ => None,
            })
            .collect()
    }

    fn crystals(h: &mut Harness) -> Vec<Entity> {
        h.world()
            .query_filtered::<Entity, With<EndCrystal>>()
            .iter(h.app.world())
            .collect()
    }

    fn place(h: &mut Harness, player: Entity, phase: f64, offset: f64) {
        let map = h.app.world().resource::<Track>().clone();
        let mut kart = h.kart_mut(player);
        (kart.x, kart.y, kart.z) = map.point(phase, offset);
        kart.yaw = map.heading(phase);
        kart.speed = 0.0;
    }

    fn park(h: &mut Harness, player: Entity, x: f64, z: f64) {
        let mut kart = h.kart_mut(player);
        kart.x = x;
        kart.z = z;
        kart.speed = 0.0;
    }

    fn use_item(h: &mut Harness, player: Entity, item: PowerUp) {
        {
            let mut kart = h.kart_mut(player);
            kart.item = Some(item);
            kart.use_item = true;
        }
        h.tick();
        assert_eq!(h.kart(player).item, None);
        assert!(!h.kart(player).use_item);
    }

    fn network_id(h: &Harness, entity: Entity) -> i32 {
        h.app.world().get::<MinecraftEntityId>(entity).unwrap().0
    }

    fn crystal_spawns(out: &[Out], client: u32) -> Vec<(i32, (f64, f64, f64))> {
        out.iter()
            .filter_map(|o| match o {
                Out::Spawn {
                    client: c,
                    id,
                    kind,
                    x,
                    y,
                    z,
                    ..
                } if *c == client && *kind == EntityKind::EndCrystal.id() => {
                    Some((*id, (*x, *y, *z)))
                }
                _ => None,
            })
            .collect()
    }

    fn particles(out: &[Out], client: u32, particle: Particle) -> Vec<Out> {
        out.iter()
            .filter(|o| matches!(o, Out::Particles { client: c, particle: p, .. } if *c == client && *p == particle))
            .cloned()
            .collect()
    }

    fn chats(out: &[Out], client: u32) -> Vec<String> {
        out.iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: c,
                    overlay: false,
                    text,
                    ..
                } if *c == client => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn flashes(out: &[Out], client: u32) -> Vec<(String, String)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: c,
                    overlay: true,
                    text,
                    color,
                } if *c == client && *color != HUD_COLOR.to_string() => {
                    Some((text.clone(), color.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn flash(tone: Tone, text: &str) -> (String, String) {
        (text.to_string(), tone.color().to_string())
    }

    fn as_seen_by(out: &[Out], client: u32) -> Vec<Out> {
        out.iter()
            .cloned()
            .map(|o| match o {
                Out::Particles {
                    particle,
                    at,
                    count,
                    offset,
                    speed,
                    long_distance,
                    ..
                } => Out::Particles {
                    client,
                    particle,
                    at,
                    count,
                    offset,
                    speed,
                    long_distance,
                },
                other => other,
            })
            .collect()
    }

    fn particle_count(out: &[Out], client: u32) -> usize {
        out.iter()
            .filter(|o| matches!(o, Out::Particles { client: c, .. } if *c == client))
            .count()
    }

    #[test]
    fn crystals_spawn_with_the_track_are_collected_once_and_respawn_after_eight_seconds() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.tick();
        h.drain();
        assert!(items(&h).is_empty());
        let mut out = h.shortcut_to_countdown(a, &[]);
        let map = h.app.world().resource::<Track>().clone();
        let pickups = items(&h).pickups.clone();
        assert_eq!(pickups.len(), GATES * 3);
        let mut expected = Vec::new();
        for gate in 0..GATES {
            for offset in PICKUP_OFFSETS {
                expected.push(map.point((gate as f64 + 0.45) * TAU / GATES as f64, offset));
            }
        }
        assert_eq!(
            pickups.iter().map(|p| (p.x, p.y, p.z)).collect::<Vec<_>>(),
            expected
        );
        assert!(
            pickups
                .iter()
                .all(|p| p.ready_at == 0 && p.crystal.is_some())
        );
        let spawned = crystals(&mut h);
        assert_eq!(spawned.len(), GATES * 3);
        for pickup in &pickups {
            let crystal = pickup.crystal.unwrap();
            assert_eq!(
                *h.app.world().get::<Position>(crystal).unwrap(),
                Position {
                    x: pickup.x,
                    y: pickup.y - 0.2,
                    z: pickup.z
                }
            );
            assert_eq!(
                h.app.world().get::<EndCrystal>(crystal),
                Some(&EndCrystal::floating())
            );
            let meta = h.app.world().get::<EntityMetadata>(crystal).unwrap();
            assert!(meta.get(8).is_some() && meta.get(9).is_some());
        }
        out.extend(h.drain());
        for client in [1, 2] {
            let spawns = crystal_spawns(&out, client);
            assert_eq!(spawns.len(), GATES * 3);
            for (id, at) in spawns {
                let pickup = pickups
                    .iter()
                    .find(|p| network_id(&h, p.crystal.unwrap()) == id)
                    .unwrap();
                assert_eq!(at, (pickup.x, pickup.y - 0.2, pickup.z));
                assert!(
                    out.contains(&Out::Metadata(client, id, vec![8, 9])),
                    "{client} {id}"
                );
            }
        }
        h.ticks(3);
        assert!(crystal_spawns(&h.drain(), 1).is_empty());
        assert_eq!(crystals(&mut h).len(), GATES * 3);

        let tick = h.race().tick;
        h.race_mut().start = tick;
        h.tick();
        h.drain();
        let target = pickups[5].clone();
        let crystal = target.crystal.unwrap();
        let id = network_id(&h, crystal);
        park(&mut h, a, target.x + 1.7, target.z);
        h.tick();
        assert_eq!(h.kart(a).item, None);
        park(&mut h, a, target.x + 1.0, target.z);
        h.tick();
        let collected = h.race().tick;
        let item = h.kart(a).item.expect("collected");
        assert_eq!(h.kart(a).impact, 5);
        let pickup = &items(&h).pickups[5];
        assert_eq!(
            (pickup.ready_at, pickup.crystal),
            (collected + PICKUP_RESPAWN, None)
        );
        assert!(h.app.world().get_entity(crystal).is_err());
        assert_eq!(crystals(&mut h).len(), GATES * 3 - 1);
        let out = h.drain();
        assert!(out.contains(&Out::Remove(1, vec![id])));
        assert!(out.contains(&Out::Remove(2, vec![id])));
        assert!(chats(&out, 1).is_empty());
        assert_eq!(
            flashes(&out, 1),
            vec![flash(
                Tone::Notice,
                &format!("Bonus : {} — Sprint pour l'utiliser", item.name())
            )]
        );
        assert_eq!(sounds(&out, 1, Cue::Pickup).len(), 1);
        assert!(sounds(&out, 2, Cue::Pickup).is_empty());
        assert!(flashes(&out, 2).is_empty());

        h.ticks((PICKUP_RESPAWN - 1) as usize);
        assert_eq!(crystals(&mut h).len(), GATES * 3 - 1);
        assert!(crystal_spawns(&h.drain(), 1).is_empty());
        h.tick();
        assert_eq!(h.race().tick, collected + PICKUP_RESPAWN);
        assert_eq!(crystals(&mut h).len(), GATES * 3);
        let respawned = items(&h).pickups[5].crystal.unwrap();
        assert_ne!(respawned, crystal);
        let out = h.drain();
        assert_eq!(crystal_spawns(&out, 1).len(), 1);
        assert_eq!(crystal_spawns(&out, 2).len(), 1);
        assert_eq!(h.kart(a).item, Some(item));
        h.ticks(5);
        assert_eq!(items(&h).pickups[5].crystal, Some(respawned));
        assert_eq!(h.kart(b).item, None);

        h.kart_mut(a).item = None;
        h.tick();
        assert!(h.kart(a).item.is_some());
        assert_eq!(items(&h).pickups[5].crystal, None);
        assert_eq!(crystals(&mut h).len(), GATES * 3 - 1);
        h.drain();

        let done = h.race().laps * GATES + 1;
        h.kart_mut(a).next_gate = done;
        h.kart_mut(b).next_gate = done;
        h.tick();
        assert_eq!(h.race().phase, Phase::Destroying);
        let ids: Vec<i32> = crystals(&mut h)
            .iter()
            .map(|c| network_id(&h, *c))
            .collect();
        assert_eq!(ids.len(), GATES * 3 - 1);
        h.tick();
        assert!(crystals(&mut h).is_empty());
        assert!(items(&h).is_empty());
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
        assert_eq!(removed.len(), GATES * 3 - 1);
        h.ticks(3);
        assert!(crystals(&mut h).is_empty());
    }

    #[test]
    fn two_karts_on_the_same_pickup_in_one_tick_yield_a_single_bonus_and_removal() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        let target = items(&h).pickups[5].clone();
        let crystal = target.crystal.unwrap();
        let id = network_id(&h, crystal);
        park(&mut h, a, target.x + 1.0, target.z);
        park(&mut h, b, target.x - 1.0, target.z);
        h.kart_mut(a).contact_cooldown = 100;
        h.kart_mut(b).contact_cooldown = 100;
        h.tick();
        let tick = h.race().tick;
        let collected = [h.kart(a).item, h.kart(b).item];
        assert_eq!(collected.iter().flatten().count(), 1, "{collected:?}");
        assert!(
            collected[0].is_some(),
            "the lower network id collects first"
        );
        let pickup = &items(&h).pickups[5];
        assert_eq!(
            (pickup.ready_at, pickup.crystal),
            (tick + PICKUP_RESPAWN, None)
        );
        assert!(h.app.world().get_entity(crystal).is_err());
        let out = h.drain();
        for client in [1, 2] {
            assert_eq!(
                out.iter()
                    .filter(
                        |o| matches!(o, Out::Remove(c, ids) if *c == client && ids.contains(&id))
                    )
                    .count(),
                1,
                "{client}"
            );
        }
        assert_eq!(flashes(&out, 1).len(), 1);
        assert!(flashes(&out, 2).is_empty());
        h.tick();
        assert_eq!(h.kart(b).item, None);
    }

    #[test]
    fn a_missile_outlives_its_owner_and_still_hits() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.3, 0.0);
        place(&mut h, c, 0.6, 0.0);
        let kb = h.kart_entity(b);
        use_item(&mut h, a, PowerUp::Missile);
        let launched = items(&h).missiles[0];
        assert_eq!(launched.target, Some(kb));
        h.ticks(3);
        h.disconnect(a);
        assert_eq!(h.race().phase, Phase::Racing);
        assert_eq!(h.race().roster, vec![b, c]);
        let orphan = items(&h).missiles[0];
        assert_eq!(orphan.id, launched.id);
        assert!(h.app.world().get_entity(orphan.owner).is_err());
        h.drain();
        let mut hit_at = None;
        for tick in 0..MISSILE_LIFE {
            h.tick();
            let Some(m) = items(&h).missiles.first().copied() else {
                hit_at = Some(tick);
                break;
            };
            assert_eq!(
                (m.id, m.owner, m.target),
                (launched.id, launched.owner, Some(kb))
            );
            assert_eq!(items(&h).bursts.len(), 0);
        }
        assert!(
            hit_at.is_some(),
            "orphaned missile never reached its target"
        );
        assert_eq!(h.kart(b).slow, 29);
        assert_eq!(h.kart(c).slow, 0);
        assert_eq!(items(&h).bursts[0].kind, BurstKind::Impact);
        let out = h.drain();
        assert!(chats(&out, 2).is_empty());
        assert!(flashes(&out, 2).contains(&flash(Tone::Warn, "Missile ! Ralenti 1,5 s")));
    }

    #[test]
    fn a_kart_parked_on_a_pickup_collects_it_the_tick_it_respawns_without_a_flash() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        let target = items(&h).pickups[5].clone();
        let first = target.crystal.unwrap();
        park(&mut h, a, target.x, target.z);
        h.tick();
        let first_item = h.kart(a).item.expect("collected");
        let ready_at = items(&h).pickups[5].ready_at;
        assert_eq!(ready_at, h.race().tick + PICKUP_RESPAWN);
        assert!(h.app.world().get_entity(first).is_err());
        h.drain();
        h.ticks((PICKUP_RESPAWN - 2) as usize);
        assert_eq!(h.kart(a).item, Some(first_item));
        h.kart_mut(a).item = None;
        h.tick();
        assert_eq!(h.race().tick, ready_at - 1);
        assert_eq!(h.kart(a).item, None);
        assert_eq!(items(&h).pickups[5].crystal, None);
        assert!(crystal_spawns(&h.drain(), 1).is_empty());
        let alive: Vec<i32> = crystals(&mut h)
            .into_iter()
            .map(|c| network_id(&h, c))
            .collect();
        assert_eq!(alive.len(), GATES * 3 - 1);
        h.tick();
        assert_eq!(h.race().tick, ready_at);
        assert!(h.kart(a).item.is_some());
        let pickup = &items(&h).pickups[5];
        assert_eq!(
            (pickup.ready_at, pickup.crystal),
            (ready_at + PICKUP_RESPAWN, None)
        );
        assert_eq!(crystals(&mut h).len(), GATES * 3 - 1);
        let out = h.drain();
        assert!(crystal_spawns(&out, 1).is_empty() && crystal_spawns(&out, 2).is_empty());
        assert!(
            !out.iter().any(
                |o| matches!(o, Out::Remove(_, ids) if ids.iter().any(|id| alive.contains(id)))
            )
        );
        assert_eq!(flashes(&out, 1).len(), 1);
        park(&mut h, b, target.x, target.z);
        h.ticks((PICKUP_RESPAWN - 1) as usize);
        assert_eq!(h.kart(b).item, None);
        h.drain();
        h.tick();
        assert!(h.kart(b).item.is_some());
        assert_eq!(crystals(&mut h).len(), GATES * 3 - 1);
        let out = h.drain();
        assert!(crystal_spawns(&out, 1).is_empty() && crystal_spawns(&out, 2).is_empty());
    }

    #[test]
    fn every_power_up_can_be_rolled_from_a_pickup() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &[]);
        let target = items(&h).pickups[0].clone();
        park(&mut h, a, target.x, target.z);
        let mut found = Vec::new();
        for _ in 0..128 {
            h.kart_mut(a).item = None;
            h.world().resource_mut::<Items>().pickups[0].ready_at = 0;
            h.tick();
            found.push(h.kart(a).item.expect("rolled"));
        }
        for item in PowerUp::ALL {
            assert!(found.contains(&item), "{item:?}");
        }
    }

    #[test]
    fn missile_homes_onto_the_nearest_racer_ahead_and_never_its_owner() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.35, -4.0);
        place(&mut h, c, 0.6, 0.0);
        let (ka, kb) = (h.kart_entity(a), h.kart_entity(b));
        use_item(&mut h, a, PowerUp::Missile);
        let missile = items(&h).missiles[0];
        assert_eq!((missile.owner, missile.target), (ka, Some(kb)));
        assert_eq!(missile.age, 1);
        let map = h.app.world().resource::<Track>().clone();
        let start = map.project(h.kart(a).x, h.kart(a).z).phase;
        assert!((missile.phase - (start + 2.0 / map.length * TAU)).abs() < 1e-9);
        assert_eq!(items(&h).bursts.len(), 0);
        let mut phase = missile.phase;
        let mut offset = missile.offset;
        let mut hit_at = None;
        for tick in 2..80 {
            h.tick();
            let Some(m) = items(&h).missiles.first().copied() else {
                hit_at = Some(tick);
                break;
            };
            assert_eq!(m.age, tick);
            assert!((m.phase - phase).rem_euclid(TAU) > 0.0);
            assert!(m.offset <= offset && m.offset >= -4.0 - 1e-9);
            assert!(offset - m.offset <= 0.3 + 1e-9);
            let point = map.point(m.phase, m.offset);
            assert_eq!((m.x, m.y, m.z), (point.0, point.1 + 0.6, point.2));
            phase = m.phase;
            offset = m.offset;
        }
        assert!(hit_at.is_some(), "missile never reached its target");
        assert!(offset < -3.0);
        assert_eq!(h.kart(b).slow, 29);
        assert_eq!(h.kart(b).impact, 11);
        assert_eq!((h.kart(a).slow, h.kart(c).slow), (0, 0));
        let burst = items(&h).bursts[0];
        assert_eq!((burst.kind, burst.radius), (BurstKind::Impact, 3.0));
        let out = h.drain();
        assert!(chats(&out, 2).is_empty());
        assert!(flashes(&out, 2).contains(&flash(Tone::Warn, "Missile ! Ralenti 1,5 s")));
        let hit = sounds(&out, 2, Cue::Hit(Hit::Missile));
        assert_eq!(hit.len(), 1);
        assert!(
            matches!(hit[0], Out::Sound { emitter: Some(id), .. } if id == network_id(&h, h.kart_entity(b)))
        );
        assert_eq!(sounds(&out, 1, Cue::Hit(Hit::Missile)).len(), 1);

        h.kart_mut(b).slow = 0;
        h.kart_mut(b).shield = 200;
        use_item(&mut h, a, PowerUp::Missile);
        for _ in 0..80 {
            h.tick();
        }
        assert!(items(&h).missiles.is_empty());
        assert_eq!(h.kart(b).slow, 0);
        let out = h.drain();
        assert!(flashes(&out, 2).contains(&flash(Tone::Good, "Bouclier ! Missile sans effet")));
        assert_eq!(sounds(&out, 2, Cue::Shielded).len(), 1);
        assert!(sounds(&out, 2, Cue::Hit(Hit::Missile)).is_empty());

        use_item(&mut h, a, PowerUp::Missile);
        assert_eq!(items(&h).missiles[0].target, Some(kb));
        h.command(b, "leave", &[]);
        h.tick();
        assert!(items(&h).missiles.is_empty());

        place(&mut h, c, 0.05, 0.0);
        use_item(&mut h, a, PowerUp::Missile);
        assert_eq!(items(&h).missiles[0].target, None);
        assert!(flashes(&h.drain(), 1).contains(&flash(
            Tone::Notice,
            "MISSILE GUIDE : aucune cible devant, tir en ligne droite"
        )));
        for _ in 0..(MISSILE_LIFE - 2) {
            h.tick();
            assert_eq!(items(&h).missiles.len(), 1);
            assert_eq!(items(&h).missiles[0].offset, 0.0);
        }
        h.tick();
        assert!(items(&h).missiles.is_empty());
        assert_eq!((h.kart(a).slow, h.kart(c).slow), (0, 0));
    }

    #[test]
    fn a_shielded_kart_sitting_on_ice_stays_silent() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.1, 0.0);
        use_item(&mut h, a, PowerUp::Ice);
        let trap = items(&h).traps[0];
        h.kart_mut(b).shield = 200;
        h.drain();
        for _ in 0..5 {
            park(&mut h, b, trap.x, trap.z);
            h.tick();
            assert_eq!(h.kart(b).ice, 0);
            let out = h.drain();
            assert!(flashes(&out, 2).is_empty());
            assert!(sounds(&out, 2, Cue::Shielded).is_empty());
            assert!(sounds(&out, 2, Cue::Hit(Hit::Ice)).is_empty());
            assert!(particles(&out, 1, Particle::Snowflake).is_empty());
        }
        h.kart_mut(b).shield = 0;
        park(&mut h, b, trap.x, trap.z);
        h.tick();
        assert_eq!(h.kart(b).ice, 59);
        let out = h.drain();
        assert_eq!(flashes(&out, 2).len(), 1);
        assert_eq!(sounds(&out, 2, Cue::Hit(Hit::Ice)).len(), 1);
    }

    #[test]
    fn impact_bursts_are_emitted_once_on_the_event_tick_to_everyone() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let watcher = h.connect(3);
        h.shortcut_to_racing(a, &[]);
        h.kart_mut(watcher).participant = false;
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.13, 1.0);
        h.tick();
        h.drain();

        let (x, y, z) = (h.kart(a).x, h.kart(a).y + 0.5, h.kart(a).z);
        use_item(&mut h, a, PowerUp::Turbo);
        let out = h.drain();
        for client in [1, 2, 3] {
            let ring = particles(&out, client, Particle::Flame);
            assert_eq!(ring.len(), BOOST_RING_POINTS as usize, "{client}");
            assert!(ring.iter().all(|o| matches!(
                o,
                Out::Particles { at, count: 1, long_distance: true, .. }
                    if ((at.0 - x).hypot(at.2 - z) - BOOST_RING_RADIUS).abs() < 1e-9 && at.1 == y
            )));
        }
        assert!(items(&h).sparks.is_empty());
        h.tick();
        assert!(
            particles(&h.drain(), 1, Particle::Flame)
                .iter()
                .all(|o| matches!(o, Out::Particles { count: 3, .. }))
        );
        {
            let mut kart = h.kart_mut(a);
            kart.turbo = 0;
            kart.speed = 0.0;
        }
        place(&mut h, a, 0.1, 0.0);

        use_item(&mut h, a, PowerUp::Shockwave);
        let out = h.drain();
        assert_eq!(particles(&out, 3, Particle::Explosion).len(), 2);
        assert_eq!(particles(&out, 3, Particle::LargeSmoke).len(), 2);
        assert!(
            particles(&out, 3, Particle::Crit)
                .iter()
                .all(|o| matches!(o, Out::Particles { count: 6, .. }))
        );
        h.tick();
        let out = h.drain();
        assert!(particles(&out, 3, Particle::Explosion).is_empty());
        assert!(particles(&out, 3, Particle::LargeSmoke).is_empty());

        h.kart_mut(b).shield = 200;
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.13, 1.0);
        use_item(&mut h, a, PowerUp::Lightning);
        h.drain();
        h.ticks(storm(&h, b).unwrap().delay as usize + 1);
        let out = h.drain();
        let shell: Vec<Out> = particles(&out, 3, Particle::ElectricSpark)
            .into_iter()
            .filter(|o| matches!(o, Out::Particles { count: 30, .. }))
            .collect();
        assert_eq!(shell.len(), 1);
        assert!(matches!(
            shell[0],
            Out::Particles {
                offset: (0.8, 0.8, 0.8),
                speed: 0.05,
                ..
            }
        ));
        assert!(particles(&out, 3, Particle::Explosion).is_empty());
        h.kart_mut(b).shield = 0;
        h.ticks(LIGHTNING_PERIOD as usize * 2);
        h.drain();
        use_item(&mut h, a, PowerUp::Lightning);
        h.drain();
        h.ticks(storm(&h, b).unwrap().delay as usize + 1);
        let out = h.drain();
        assert_eq!(particles(&out, 3, Particle::Explosion).len(), 1);
        assert!(
            particles(&out, 3, Particle::ElectricSpark)
                .iter()
                .any(|o| matches!(o, Out::Particles { count: 24, .. }))
        );
        h.ticks(usize::from(LIGHTNING_PERIOD * LIGHTNING_STRIKES));
        assert!(storm(&h, b).is_none());
        h.drain();

        h.kart_mut(b).ice = 0;
        use_item(&mut h, a, PowerUp::Ice);
        let trap = items(&h).traps[0];
        park(&mut h, b, trap.x, trap.z);
        let yaw = h.kart(b).yaw;
        h.tick();
        let out = h.drain();
        let snow = particles(&out, 3, Particle::Snowflake);
        assert_eq!(snow.len(), 1);
        let along = (
            (yaw.sin().abs() * 1.5 + 0.3) as f32,
            0.25,
            (yaw.cos().abs() * 1.5 + 0.3) as f32,
        );
        assert!(matches!(snow[0], Out::Particles { count: 20, offset, .. } if offset == along));
        assert_eq!(particles(&out, 3, Particle::ItemSnowball).len(), 1);
        h.tick();
        assert!(particles(&h.drain(), 3, Particle::Snowflake).is_empty());

        h.kart_mut(b).spin = 0.0;
        use_item(&mut h, a, PowerUp::Banana);
        let trap = items(&h).traps.last().copied().unwrap();
        park(&mut h, b, trap.x, trap.z);
        h.tick();
        let out = h.drain();
        let banana = Particle::Item {
            stack: ItemStackTemplate::simple(i::YELLOW_DYE, 1),
        };
        let peel = particles(&out, 3, banana);
        assert_eq!(peel.len(), 1);
        assert!(matches!(peel[0], Out::Particles { count: 12, .. }));
        assert_eq!(particles(&out, 3, Particle::Cloud).len(), 1);
        h.tick();
        assert!(particles(&h.drain(), 3, Particle::Cloud).is_empty());
    }

    #[test]
    fn shockwave_and_lightning_burst_strike_in_range_and_age_out() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let shielded = h.connect(3);
        let far = h.connect(4);
        let spectator = h.connect(5);
        let finished = h.connect(6);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.13, 1.0);
        place(&mut h, shielded, 0.1, -2.0);
        place(&mut h, far, 2.0, 0.0);
        place(&mut h, spectator, 0.1, 3.0);
        place(&mut h, finished, 0.1, -3.0);
        h.kart_mut(shielded).shield = 200;
        h.kart_mut(spectator).participant = false;
        h.kart_mut(finished).finished = Some(200);
        h.tick();
        h.drain();
        let (ax, az) = (h.kart(a).x, h.kart(a).z);
        let (bx, bz) = (h.kart(b).x, h.kart(b).z);
        use_item(&mut h, a, PowerUp::Shockwave);
        let b_push = h.kart(b).push;
        assert!((b_push.0.hypot(b_push.1) - 0.9 * 0.85).abs() < 1e-9);
        assert_eq!(h.kart(b).impact, 9);
        assert_eq!(h.kart(shielded).impact, 9);
        for e in [a, shielded, far, spectator, finished] {
            assert_eq!(h.kart(e).push, (0.0, 0.0));
        }
        let bursts = items(&h).bursts.clone();
        assert_eq!(bursts.len(), 3);
        assert_eq!(
            (bursts[0].kind, bursts[0].radius, bursts[0].age),
            (BurstKind::Shockwave, 9.0, 0)
        );
        assert_eq!(
            (bursts[0].x, bursts[0].y, bursts[0].z),
            (ax, h.kart(a).y + 0.6, az)
        );
        assert_eq!((bursts[1].kind, bursts[1].radius), (BurstKind::Impact, 2.5));
        assert_eq!((bursts[1].x, bursts[1].z), (bx, bz));
        assert_eq!(bursts[2].kind, BurstKind::Impact);
        let out = h.drain();
        for client in [1, 2, 3, 4] {
            assert!(chats(&out, client).is_empty(), "{client}");
        }
        assert!(flashes(&out, 2).contains(&flash(Tone::Warn, "Touche par ONDE DE CHOC !")));
        assert!(
            flashes(&out, 3).contains(&flash(Tone::Good, "Bouclier ! ONDE DE CHOC sans effet"))
        );
        assert!(flashes(&out, 4).is_empty());
        assert!(flashes(&out, 1).contains(&flash(
            Tone::Good,
            &format!("ONDE DE CHOC : {}", describe(PowerUp::Shockwave))
        )));
        assert_eq!(sounds(&out, 1, Cue::Activate(PowerUp::Shockwave)).len(), 1);
        assert_eq!(sounds(&out, 1, Cue::Hit(Hit::Shockwave)).len(), 1);
        assert_eq!(sounds(&out, 1, Cue::Shielded).len(), 1);
        for age in 1..BURST_LIFE {
            h.tick();
            assert_eq!(items(&h).bursts.len(), 3);
            assert!(items(&h).bursts.iter().all(|b| b.age == age));
        }
        h.tick();
        assert!(items(&h).bursts.is_empty());

        for e in [a, b, shielded, far] {
            h.kart_mut(e).impact = 0;
        }
        use_item(&mut h, a, PowerUp::Lightning);
        for e in [a, b, shielded, far, spectator, finished] {
            assert_eq!(h.kart(e).slow, 0);
        }
        for e in [b, shielded, far] {
            assert!(storm(&h, e).is_some(), "{e:?}");
        }
        for e in [a, spectator, finished] {
            assert!(storm(&h, e).is_none(), "{e:?}");
        }
        let bursts = items(&h).bursts.clone();
        assert_eq!(bursts.len(), 1);
        assert_eq!(
            (bursts[0].kind, bursts[0].radius),
            (BurstKind::Lightning, 3.0)
        );
        h.ticks(LIGHTNING_SPREAD as usize + 1);
        for e in [b, far] {
            assert!(h.kart(e).slow > 0, "{e:?}");
        }
        for e in [a, shielded, spectator, finished] {
            assert_eq!(h.kart(e).slow, 0);
        }
        assert!(
            items(&h)
                .bursts
                .iter()
                .all(|b| b.kind == BurstKind::Lightning)
        );
        assert_eq!(bursts[0].current_radius(), 0.0);
        assert!((ease_out(1.0) - 1.0).abs() < 1e-12);
        assert!((ease_out(0.5) - 0.875).abs() < 1e-12);
    }

    fn ahead(h: &mut Harness, from: Entity, to: Entity, blocks: f64) {
        let (x, z, yaw) = {
            let kart = h.kart(from);
            (kart.x, kart.z, kart.yaw)
        };
        let mut kart = h.kart_mut(to);
        kart.x = x - yaw.sin() * blocks;
        kart.z = z + yaw.cos() * blocks;
        kart.yaw = yaw;
        kart.speed = 0.0;
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

    #[test]
    fn a_storm_strikes_every_rival_but_the_caster_three_times_six_ticks_apart() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        let d = h.connect(4);
        let watcher = h.connect(5);
        h.shortcut_to_racing(a, &[]);
        h.kart_mut(watcher).participant = false;
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.3, -3.0);
        place(&mut h, c, 0.5, 3.0);
        place(&mut h, d, 0.7, 0.0);
        h.kart_mut(c).shield = 200;
        for e in [b, c, d] {
            h.kart_mut(e).contact_cooldown = 200;
        }
        h.tick();
        h.drain();
        use_item(&mut h, a, PowerUp::Lightning);
        let cast = h.race().tick;
        for e in [a, watcher] {
            assert!(storm(&h, e).is_none());
        }
        let storms: Vec<Lightning> = [b, c, d].map(|e| storm(&h, e).unwrap()).to_vec();
        let delays: HashSet<u16> = storms.iter().map(|s| s.delay).collect();
        assert_eq!(delays.len(), 3, "{storms:?}");
        assert!(storms.iter().all(|s| s.delay < LIGHTNING_SPREAD));
        assert!(storms.iter().all(|s| s.remaining == LIGHTNING_STRIKES));
        let seeds: HashSet<u64> = storms.iter().map(|s| s.seed).collect();
        assert_eq!(seeds.len(), 3);
        assert_eq!(items(&h).bolts.len(), 1);
        let caster_bolt = items(&h).bolts[0];
        assert_eq!((caster_bolt.x, caster_bolt.z), (h.kart(a).x, h.kart(a).z));
        let out = h.drain();
        assert!(spawns_of(&out, 1, EntityKind::LightningBolt).is_empty());
        assert_eq!(flashes(&out, 2).len(), 0);
        assert_eq!(items(&h).bursts.len(), 1);
        for e in [b, c, d] {
            assert_eq!(h.kart(e).slow, 0);
        }
        let mut props: HashMap<u64, Vec<i32>> = HashMap::new();
        if cast.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
            let ids: Vec<i32> = bolt_props(&h, &caster_bolt)
                .iter()
                .map(|e| network_id(&h, *e))
                .collect();
            assert_eq!(ids.len(), BOLT_SEGMENTS);
            let spawned = spawns_of(&out, 1, EntityKind::BlockDisplay);
            assert!(ids.iter().all(|id| spawned.iter().any(|(i, _)| i == id)));
            props.insert(caster_bolt.id, ids);
        }

        let victims: Vec<(Entity, u16, u64)> = [b, c, d]
            .iter()
            .map(|e| {
                let s = storm(&h, *e).unwrap();
                (*e, s.delay, s.seed)
            })
            .collect();
        let mut struck: HashMap<Entity, Vec<u64>> = HashMap::new();
        let mut seen: HashSet<u64> = HashSet::from([caster_bolt.id]);
        let mut removed: HashSet<i32> = HashSet::new();
        for _ in
            0..(LIGHTNING_SPREAD + LIGHTNING_PERIOD * LIGHTNING_STRIKES + u16::from(BOLT_LIFE) + 2)
        {
            h.tick();
            let tick = h.race().tick;
            let fresh: Vec<Bolt> = items(&h)
                .bolts
                .iter()
                .filter(|bolt| seen.insert(bolt.id))
                .copied()
                .collect();
            let out = h.drain();
            assert!(spawns_of(&out, 1, EntityKind::LightningBolt).is_empty());
            let mut thunder = 0;
            for bolt in &fresh {
                assert_eq!(bolt.age, 0);
                assert_ne!(bolt.seed, 0);
                let victim = victims
                    .iter()
                    .map(|(e, _, _)| *e)
                    .find(|e| {
                        let k = h.kart(*e);
                        (k.x - bolt.x).abs() <= 0.4 + 1e-9
                            && (k.z - bolt.z).abs() <= 0.4 + 1e-9
                            && k.y == bolt.y
                    })
                    .expect("a bolt lands on a rival");
                struck.entry(victim).or_default().push(tick);
                let kart = network_id(&h, h.kart_entity(victim));
                thunder += emitted_by(&out, 1, Cue::Thunder, kart);
            }
            assert_eq!(thunder, fresh.len());
            assert_eq!(sounds(&out, 1, Cue::Thunder).len(), fresh.len());
            assert!(sounds(&out, 1, Cue::Hit(Hit::Lightning)).len() <= fresh.len());
            assert!(sounds(&out, 1, Cue::Impact).len() <= fresh.len());
            if tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
                let spawned = spawns_of(&out, 1, EntityKind::BlockDisplay);
                for bolt in &items(&h).bolts {
                    let segments = bolt_props(&h, bolt);
                    assert_eq!(segments.len(), BOLT_SEGMENTS, "{bolt:?}");
                    let ids: Vec<i32> = segments.iter().map(|e| network_id(&h, *e)).collect();
                    if let Some(known) = props.get(&bolt.id) {
                        assert_eq!(*known, ids);
                        continue;
                    }
                    for (segment, id) in segments.iter().zip(&ids) {
                        let at = h.app.world().get::<Position>(*segment).unwrap();
                        assert!(
                            spawned
                                .iter()
                                .any(|(i, p)| i == id && *p == (bolt.x, bolt.y, bolt.z)),
                            "{bolt:?}"
                        );
                        assert_eq!((at.x, at.y, at.z), (bolt.x, bolt.y, bolt.z));
                    }
                    props.insert(bolt.id, ids);
                }
            }
            for id in removals(&out, 1) {
                removed.insert(id);
            }
            for bolt in &items(&h).bolts {
                assert!(bolt.age < BOLT_LIFE);
                if let Some(ids) = props.get(&bolt.id) {
                    assert!(ids.iter().all(|id| !removed.contains(id)));
                }
            }
        }
        for (victim, delay, seed) in &victims {
            let ticks = &struck[victim];
            let first = cast + 1 + u64::from(*delay);
            let expected: Vec<u64> = (0..u64::from(LIGHTNING_STRIKES))
                .map(|n| first + n * u64::from(LIGHTNING_PERIOD))
                .collect();
            assert_eq!(*ticks, expected, "{victim:?}");
            assert!(storm(&h, *victim).is_none());
            assert_ne!(*seed, mix(*seed));
        }
        assert_eq!(seen.len(), 1 + 3 * usize::from(LIGHTNING_STRIKES));
        assert_eq!(props.len(), seen.len());
        for ids in props.values() {
            assert_eq!(ids.len(), BOLT_SEGMENTS);
            assert!(ids.iter().all(|id| removed.contains(id)));
        }
        assert!(items(&h).bolts.is_empty());
        assert!(scene(&h).keys().all(|key| key.1 != Kind::Bolt));
        assert!(h.kart(b).slow > 0 && h.kart(d).slow > 0);
        assert_eq!(h.kart(c).slow, 0);
        assert_eq!(h.kart(a).slow, 0);
        assert_eq!(h.kart(watcher).slow, 0);
        assert_eq!(h.kart(b).slow, 49 - (h.race().tick - struck[&b][0]) as u16);
        h.drain();
        h.ticks(20);
        let out = h.drain();
        assert!(spawns_of(&out, 1, EntityKind::LightningBolt).is_empty());
        assert!(sounds(&out, 1, Cue::Hit(Hit::Lightning)).is_empty());
        assert!(sounds(&out, 1, Cue::Thunder).is_empty());
        assert!(items(&h).bolts.is_empty());
        assert!(items(&h).bursts.is_empty());
    }

    #[test]
    fn the_first_strike_carries_the_hit_and_impact_and_shields_absorb_it_bolts_expire_on_the_wire()
    {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.3, 0.0);
        place(&mut h, c, 0.5, 0.0);
        h.kart_mut(c).shield = 200;
        h.tick();
        h.drain();
        use_item(&mut h, a, PowerUp::Lightning);
        let out = h.drain();
        assert!(flashes(&out, 1).contains(&flash(
            Tone::Good,
            &format!("ECLAIR : {}", describe(PowerUp::Lightning))
        )));
        assert_eq!(sounds(&out, 1, Cue::Activate(PowerUp::Lightning)).len(), 1);
        let (db, dc) = (storm(&h, b).unwrap().delay, storm(&h, c).unwrap().delay);
        let mut all = Vec::new();
        h.ticks(db as usize + 1);
        let out = h.drain();
        all.extend(out.clone());
        assert_eq!(h.kart(b).slow, 49);
        assert_eq!(h.kart(b).impact, 9);
        assert!(flashes(&out, 2).contains(&flash(Tone::Warn, "Foudroye ! Ralenti 2,5 s")));
        let kb = network_id(&h, h.kart_entity(b));
        let hurt = sounds(&out, 2, Cue::Hit(Hit::Lightning));
        assert_eq!(hurt.len(), 1);
        assert!(matches!(hurt[0], Out::Sound { emitter: Some(e), .. } if e == kb));
        assert_eq!(emitted_by(&out, 2, Cue::Thunder, kb), 1);
        assert_eq!(emitted_by(&out, 2, Cue::Impact, kb), 1);
        let thunder = sounds(&out, 2, Cue::Thunder);
        assert!(matches!(
            thunder[0],
            Out::Sound {
                source: SoundSource::Players,
                volume: 1.0,
                pitch: 1.0,
                at: None,
                ..
            }
        ));
        assert!(spawns_of(&out, 2, EntityKind::LightningBolt).is_empty());
        assert!(
            particles(&out, 2, Particle::ElectricSpark)
                .iter()
                .any(|o| matches!(o, Out::Particles { count: 24, .. }))
        );
        let (bx, bz) = (h.kart(b).x, h.kart(b).z);
        let bolt = *items(&h)
            .bolts
            .iter()
            .find(|bolt| (bolt.x - bx).abs() <= 0.4 + 1e-9 && (bolt.z - bz).abs() <= 0.4 + 1e-9)
            .unwrap();
        assert_eq!(bolt.age, 0);
        let raw: Vec<ClientboundPacket> = h
            .packets()
            .into_iter()
            .filter_map(|(client, packet)| (client == 2).then_some(packet))
            .collect();
        assert!(raw.is_empty());
        if !h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
            h.tick();
            all.extend(h.drain());
        }
        let segments: Vec<i32> = bolt_props(&h, &bolt)
            .iter()
            .map(|e| network_id(&h, *e))
            .collect();
        assert_eq!(segments.len(), BOLT_SEGMENTS);
        let spawned: Vec<i32> = spawns_of(&all, 2, EntityKind::BlockDisplay)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(segments.iter().all(|id| spawned.contains(id)));
        let lit: Vec<[f32; 3]> = (0..BOLT_SEGMENTS as u8)
            .map(|part| {
                scene(&h)
                    .frame(Key(bolt.id, Kind::Bolt, part))
                    .unwrap()
                    .transform
                    .scale
            })
            .collect();
        assert!(lit.iter().all(|scale| scale.iter().all(|v| *v > 0.0)));
        while items(&h)
            .bolts
            .iter()
            .any(|b| b.id == bolt.id && b.age < BOLT_FLASH)
        {
            h.tick();
            all.extend(h.drain());
        }
        if !h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
            h.tick();
            all.extend(h.drain());
        }
        assert!(items(&h).bolts.iter().any(|b| b.id == bolt.id));
        for part in 0..BOLT_SEGMENTS as u8 {
            let scale = scene(&h)
                .frame(Key(bolt.id, Kind::Bolt, part))
                .unwrap()
                .transform
                .scale;
            assert_eq!((scale[0], scale[2]), (0.0, 0.0));
            assert!(scale[1] > 0.0);
        }
        assert!(segments.iter().all(|id| !removals(&all, 2).contains(id)));
        while items(&h).bolts.iter().any(|b| b.id == bolt.id) {
            h.tick();
            all.extend(h.drain());
        }
        if !h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
            h.tick();
            all.extend(h.drain());
        }
        assert!(segments.iter().all(|id| removals(&all, 2).contains(id)));
        assert!(bolt_props(&h, &bolt).is_empty());
        h.ticks(usize::from(
            LIGHTNING_SPREAD + LIGHTNING_PERIOD * LIGHTNING_STRIKES,
        ));
        all.extend(h.drain());
        assert_eq!(h.kart(c).slow, 0);
        assert!(dc < LIGHTNING_SPREAD);
        assert!(flashes(&all, 3).contains(&flash(Tone::Good, "Bouclier ! ECLAIR sans effet")));
        assert_eq!(sounds(&all, 3, Cue::Shielded).len(), 1);
        let kc = network_id(&h, h.kart_entity(c));
        assert!(
            sounds(&all, 3, Cue::Hit(Hit::Lightning))
                .iter()
                .all(|o| !matches!(o, Out::Sound { emitter: Some(e), .. } if *e == kc))
        );
        assert_eq!(sounds(&all, 3, Cue::Hit(Hit::Lightning)).len(), 1);
        assert!(spawns_of(&all, 3, EntityKind::LightningBolt).is_empty());
        assert_eq!(
            emitted_by(&all, 3, Cue::Thunder, kb),
            usize::from(LIGHTNING_STRIKES)
        );
        assert_eq!(
            emitted_by(&all, 3, Cue::Thunder, kc),
            usize::from(LIGHTNING_STRIKES)
        );
        assert_eq!(emitted_by(&all, 3, Cue::Impact, kb), 1);
        assert_eq!(emitted_by(&all, 3, Cue::Impact, kc), 1);
        assert!(storm(&h, c).is_none());
        assert!(storm(&h, b).is_none());
    }

    #[test]
    fn bolt_segment_spawn_matches_paper_add_entity_layout() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.3, 0.0);
        h.tick();
        h.drain();
        use_item(&mut h, a, PowerUp::Lightning);
        h.ticks(storm(&h, b).unwrap().delay as usize + 1);
        if !h.race().tick.is_multiple_of(u64::from(KEYFRAME_TICKS)) {
            h.tick();
        }
        let (bx, bz) = (h.kart(b).x, h.kart(b).z);
        let bolt = *items(&h)
            .bolts
            .iter()
            .find(|bolt| (bolt.x - bx).abs() <= 0.4 + 1e-9 && (bolt.z - bz).abs() <= 0.4 + 1e-9)
            .unwrap();
        let segment = bolt_props(&h, &bolt)[0];
        let (spawn, sent) = h
            .packets()
            .into_iter()
            .find_map(|(client, packet)| match packet {
                ClientboundPacket::Play(PlayPacket::SpawnEntity(p))
                    if client == 2 && p.entity_id == network_id(&h, segment) =>
                {
                    Some((p.clone(), PlayPacket::SpawnEntity(p)))
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(spawn.entity_type, EntityKind::BlockDisplay.id());
        assert_eq!((spawn.x, spawn.y, spawn.z), (bolt.x, bolt.y, bolt.z));
        let mut bytes = Vec::new();
        sent.encode(&mut bytes);
        let mut expected = vec![voidmc_data::v26_1_2::packets::play::clientbound::ADD_ENTITY as u8];
        VarI32(spawn.entity_id).encode(&mut expected);
        expected.extend_from_slice(spawn.entity_uuid.as_bytes());
        VarI32(EntityKind::BlockDisplay.id()).encode(&mut expected);
        for value in [bolt.x, bolt.y, bolt.z] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        expected.extend_from_slice(&[0, 0, 0, 0, 0]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_fireball_charges_two_seconds_then_flies_straight_into_the_first_rival_ahead() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        ahead(&mut h, a, b, 6.0);
        ahead(&mut h, a, c, 9.0);
        for e in [a, b, c] {
            h.kart_mut(e).contact_cooldown = 200;
        }
        h.tick();
        h.drain();
        let map = h.app.world().resource::<Track>().clone();
        use_item(&mut h, a, PowerUp::Fireball);
        let out = h.drain();
        assert!(flashes(&out, 1).contains(&flash(
            Tone::Good,
            &format!("BOULE DE FEU : {}", describe(PowerUp::Fireball))
        )));
        assert_eq!(sounds(&out, 1, Cue::Activate(PowerUp::Fireball)).len(), 1);
        assert_eq!(h.kart(a).blaze, FIREBALL_CHARGE - 1);
        assert!(items(&h).fireballs.is_empty());
        assert_eq!(trail(h.kart(a)), Some(Particle::Flame));
        h.ticks(usize::from(FIREBALL_CHARGE) - 2);
        assert_eq!(h.kart(a).blaze, 1);
        assert!(items(&h).fireballs.is_empty());
        assert!(spawns_of(&h.drain(), 2, EntityKind::Fireball).is_empty());
        let (ax, ay, az, yaw) = {
            let k = h.kart(a);
            (k.x, k.y, k.z, k.yaw)
        };
        h.tick();
        let launched = h.race().tick;
        assert_eq!(h.kart(a).blaze, 0);
        let fireball = items(&h).fireballs[0];
        let (vx, vz) = (-yaw.sin() * FIREBALL_SPEED, yaw.cos() * FIREBALL_SPEED);
        assert_eq!(fireball.owner, h.kart_entity(a));
        assert_eq!((fireball.vx, fireball.vz, fireball.age), (vx, vz, 1));
        assert_eq!((fireball.x, fireball.z), (ax + vx, az + vz));
        assert_eq!(
            fireball.y,
            map.project(fireball.x, fireball.z).y + FIREBALL_HEIGHT
        );
        assert!((fireball.y - ay - FIREBALL_HEIGHT).abs() < 1.0);
        let entity = fireball.entity;
        let id = network_id(&h, entity);
        assert!(h.app.world().get::<Projectile>(entity).is_some());
        assert_eq!(
            *h.app.world().get::<Position>(entity).unwrap(),
            Position {
                x: fireball.x,
                y: fireball.y,
                z: fireball.z
            }
        );
        let out = h.drain();
        for client in [1, 2, 3] {
            let spawns = spawns_of(&out, client, EntityKind::Fireball);
            assert_eq!(
                spawns,
                vec![(id, (fireball.x, fireball.y, fireball.z))],
                "{client}"
            );
        }
        assert_eq!(sounds(&out, 1, Cue::Launch).len(), 1);
        assert_eq!(sounds(&out, 2, Cue::Launch).len(), 1);
        assert!(
            particles(&out, 3, Particle::Flame)
                .iter()
                .any(|o| matches!(o, Out::Particles { count: 16, .. }))
        );
        assert_eq!(h.kart(a).slow, 0);
        assert_eq!(h.kart(a).spin, 0.0);

        let (x0, z0) = (ax, az);
        let mut hit_at = None;
        for age in 2..FIREBALL_LIFE {
            h.tick();
            let out = h.drain();
            let Some(f) = items(&h).fireballs.first().copied() else {
                hit_at = Some(age);
                assert!(removals(&out, 1).contains(&id));
                assert!(removals(&out, 3).contains(&id));
                break;
            };
            assert_eq!(f.age, age);
            assert!((f.x - (x0 + vx * f64::from(age))).abs() < 1e-9);
            assert!((f.z - (z0 + vz * f64::from(age))).abs() < 1e-9);
            assert_eq!(f.y, map.project(f.x, f.z).y + FIREBALL_HEIGHT);
            assert_eq!(
                *h.app.world().get::<Position>(entity).unwrap(),
                Position {
                    x: f.x,
                    y: f.y,
                    z: f.z
                }
            );
            assert!(out.iter().any(|o| matches!(
                o,
                Out::Move { client: 2, id: i, .. } | Out::Teleport { client: 2, id: i, .. } if *i == id
            )));
            assert!(particles(&out, 2, Particle::LargeSmoke).is_empty());
            if h.race().tick.is_multiple_of(MISSILE_PERIOD) {
                assert!(particles(&out, 2, Particle::Flame).iter().any(|o| matches!(
                    o,
                    Out::Particles { count: 3, at, .. } if *at == (f.x, f.y + 0.5, f.z)
                )));
            }
            assert_eq!(h.kart(b).slow, 0);
            assert_eq!(h.kart(c).slow, 0);
        }
        assert_eq!(hit_at, Some(4));
        assert_eq!(h.race().tick, launched + 3);
        assert!(h.app.world().get_entity(entity).is_err());
        assert_eq!(h.kart(b).slow, 19);
        assert_eq!(h.kart(b).impact, 11);
        assert!(h.kart(b).spin > 0.0);
        assert_eq!((h.kart(a).slow, h.kart(c).slow), (0, 0));
        let burst = items(&h).bursts[0];
        assert_eq!((burst.kind, burst.radius), (BurstKind::Impact, 3.0));
        h.ticks(2);
        let out = h.drain();
        assert!(spawns_of(&out, 2, EntityKind::Fireball).is_empty());
        assert!(items(&h).fireballs.is_empty());
    }

    #[test]
    fn a_fireball_ignores_its_owner_absorbs_on_shields_and_expires_after_three_seconds() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        ahead(&mut h, a, b, 5.0);
        h.kart_mut(b).shield = 200;
        for e in [a, b] {
            h.kart_mut(e).contact_cooldown = 200;
        }
        h.tick();
        h.drain();
        use_item(&mut h, a, PowerUp::Fireball);
        h.ticks(usize::from(FIREBALL_CHARGE) - 1);
        assert_eq!(items(&h).fireballs.len(), 1);
        h.drain();
        h.ticks(4);
        assert!(items(&h).fireballs.is_empty());
        assert_eq!(h.kart(b).slow, 0);
        let out = h.drain();
        assert!(
            flashes(&out, 2).contains(&flash(Tone::Good, "Bouclier ! Boule de feu sans effet"))
        );
        assert_eq!(sounds(&out, 2, Cue::Shielded).len(), 1);
        assert!(sounds(&out, 2, Cue::Hit(Hit::Fireball)).is_empty());
        assert!(
            flashes(&out, 2)
                .iter()
                .all(|(text, _)| !text.contains("Ralenti")),
            "{:?}",
            flashes(&out, 2)
        );

        h.kart_mut(b).shield = 0;
        ahead(&mut h, a, b, -6.0);
        h.tick();
        h.drain();
        use_item(&mut h, a, PowerUp::Fireball);
        h.ticks(usize::from(FIREBALL_CHARGE) - 1);
        let fireball = items(&h).fireballs[0];
        let id = network_id(&h, fireball.entity);
        h.drain();
        let mut expired = None;
        for age in 1..=FIREBALL_LIFE {
            h.tick();
            if items(&h).fireballs.is_empty() {
                expired = Some(age);
                break;
            }
            assert_eq!(h.kart(a).slow, 0);
            assert_eq!(h.kart(b).slow, 0);
        }
        let expired = expired.expect("fireball expires");
        assert!(expired <= FIREBALL_LIFE);
        let last = items(&h).fireballs.is_empty();
        assert!(last);
        assert!(h.app.world().get_entity(fireball.entity).is_err());
        let out = h.drain();
        assert!(removals(&out, 1).contains(&id));
        assert!(removals(&out, 2).contains(&id));
        assert_eq!((h.kart(a).slow, h.kart(b).slow), (0, 0));
        assert!(flashes(&out, 2).is_empty());
        let world_bolt = h.app.world().get::<Projectile>(fireball.entity);
        assert!(world_bolt.is_none());

        use_item(&mut h, a, PowerUp::Fireball);
        h.ticks(usize::from(FIREBALL_CHARGE) - 1);
        let entity = items(&h).fireballs[0].entity;
        let done = h.race().laps * GATES + 1;
        h.kart_mut(a).next_gate = done;
        h.kart_mut(b).next_gate = done;
        h.tick();
        assert_ne!(h.race().phase, Phase::Racing);
        h.tick();
        assert!(items(&h).is_empty());
        assert!(h.app.world().get_entity(entity).is_err());
    }

    #[test]
    fn fireball_spawn_matches_paper_add_entity_layout_with_zero_data() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.1, 0.0);
        ahead(&mut h, a, b, -6.0);
        h.tick();
        h.drain();
        use_item(&mut h, a, PowerUp::Fireball);
        h.ticks(usize::from(FIREBALL_CHARGE) - 2);
        h.drain();
        h.tick();
        let fireball = items(&h).fireballs[0];
        let (spawn, sent) = h
            .packets()
            .into_iter()
            .find_map(|(client, packet)| match packet {
                ClientboundPacket::Play(PlayPacket::SpawnEntity(p))
                    if client == 2 && p.entity_type == EntityKind::Fireball.id() =>
                {
                    Some((p.clone(), PlayPacket::SpawnEntity(p)))
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(spawn.entity_id, network_id(&h, fireball.entity));
        assert_eq!(spawn.data, 0);
        assert_eq!(spawn.yaw, spawn.head_yaw);
        let mut bytes = Vec::new();
        sent.encode(&mut bytes);
        let mut expected = vec![voidmc_data::v26_1_2::packets::play::clientbound::ADD_ENTITY as u8];
        VarI32(spawn.entity_id).encode(&mut expected);
        expected.extend_from_slice(spawn.entity_uuid.as_bytes());
        VarI32(EntityKind::Fireball.id()).encode(&mut expected);
        for value in [fireball.x, fireball.y, fireball.z] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        expected.extend_from_slice(&[0, 0, spawn.yaw, spawn.yaw, 0]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn traps_expire_ice_persists_bananas_are_consumed_and_shields_absorb() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        place(&mut h, a, 0.2, 0.0);
        place(&mut h, b, 0.1, 0.0);
        let ka = h.kart_entity(a);
        let expected = h.kart(a).trap_drop();
        use_item(&mut h, a, PowerUp::Ice);
        let placed = h.race().tick;
        let trap = items(&h).traps[0];
        assert_eq!(
            (trap.x, trap.y, trap.z, trap.owner, trap.placed, trap.ice),
            (expected.0, expected.1, expected.2, ka, placed, true)
        );
        park(&mut h, a, trap.x, trap.z);
        park(&mut h, b, trap.x + 2.0, trap.z);
        h.tick();
        assert_eq!(h.kart(a).ice, 0);
        assert_eq!(h.kart(b).ice, 59);
        assert_eq!(items(&h).traps.len(), 1);
        let out = h.drain();
        assert!(chats(&out, 2).is_empty());
        assert!(flashes(&out, 2).contains(&flash(Tone::Warn, "Glace ! Adherence reduite 3 s")));
        assert_eq!(sounds(&out, 2, Cue::Hit(Hit::Ice)).len(), 1);
        h.tick();
        assert_eq!(h.kart(b).ice, 59);
        let out = h.drain();
        assert!(flashes(&out, 2).is_empty() && sounds(&out, 2, Cue::Hit(Hit::Ice)).is_empty());
        park(&mut h, b, trap.x + 2.6, trap.z);
        for _ in 0..25 {
            h.tick();
        }
        assert!(h.kart(a).ice > 0);
        assert!(h.kart(b).ice < 60);
        use_item(&mut h, b, PowerUp::Shield);
        assert_eq!(h.kart(b).ice, 0);
        park(&mut h, b, trap.x, trap.z);
        h.tick();
        assert_eq!(h.kart(b).ice, 0);
        park(&mut h, a, trap.x + 5.0, trap.z);
        park(&mut h, b, trap.x + 5.0, trap.z + 1.0);
        while h.race().tick < placed + 199 {
            h.tick();
        }
        assert_eq!(items(&h).traps.len(), 1);
        h.tick();
        assert!(items(&h).traps.is_empty());

        h.kart_mut(b).shield = 0;
        use_item(&mut h, a, PowerUp::Banana);
        let placed = h.race().tick;
        let trap = items(&h).traps[0];
        assert!(!trap.ice);
        park(&mut h, a, trap.x + 1.0, trap.z);
        h.tick();
        assert_eq!(items(&h).traps.len(), 1);
        assert!(h.kart(a).spin.abs() < 1e-6);
        park(&mut h, a, trap.x + 5.0, trap.z);
        park(&mut h, b, trap.x + 1.6, trap.z);
        h.tick();
        assert_eq!(items(&h).traps.len(), 1);
        h.kart_mut(b).speed = 0.5;
        park(&mut h, b, trap.x + 1.4, trap.z);
        h.tick();
        assert!(items(&h).traps.is_empty());
        assert_eq!(h.kart(b).spin, 0.45);
        assert_eq!(h.kart(b).impact, 5);
        assert!(h.kart(b).speed < 0.3);
        let out = h.drain();
        assert!(chats(&out, 2).is_empty());
        assert!(flashes(&out, 2).contains(&flash(Tone::Warn, "Banane ! Derapage — contre-braque")));
        assert_eq!(sounds(&out, 2, Cue::Hit(Hit::Banana)).len(), 1);

        use_item(&mut h, a, PowerUp::Banana);
        let trap = items(&h).traps[0];
        h.kart_mut(b).shield = 200;
        h.kart_mut(b).spin = 0.0;
        park(&mut h, b, trap.x, trap.z);
        h.tick();
        assert!(items(&h).traps.is_empty());
        assert_eq!(h.kart(b).spin, 0.0);
        let out = h.drain();
        assert!(flashes(&out, 2).contains(&flash(Tone::Good, "Bouclier ! Banane sans effet")));
        assert_eq!(sounds(&out, 2, Cue::Shielded).len(), 1);
        assert!(sounds(&out, 2, Cue::Hit(Hit::Banana)).is_empty());

        use_item(&mut h, a, PowerUp::Banana);
        while h.race().tick < placed + 300 {
            h.tick();
        }
        assert!(items(&h).traps.is_empty());
    }

    #[test]
    fn particles_follow_the_reference_cadence_and_audiences() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let far = h.connect(3);
        h.tick();
        h.world().get_mut::<LoadedChunks>(far).unwrap().0.clear();
        h.tick();
        h.drain();
        h.ticks(10);
        assert_eq!(particle_count(&h.drain(), 1), 0);
        h.shortcut_to_countdown(a, &[]);
        h.world().get_mut::<LoadedChunks>(far).unwrap().0.clear();
        h.tick();
        h.drain();
        let pickups = items(&h).pickups.clone();
        let mut seen = 0;
        for _ in 0..10 {
            h.tick();
            let out = h.drain();
            let orbit = particles(&out, 1, Particle::EndRod);
            assert_eq!(particle_count(&out, 1), orbit.len());
            assert_eq!(particle_count(&out, 3), 0);
            if orbit.is_empty() {
                continue;
            }
            seen += 1;
            let tick = h.race().tick;
            assert!(tick.is_multiple_of(10));
            assert_eq!(orbit.len(), pickups.len() * 4);
            assert_eq!(as_seen_by(&particles(&out, 2, Particle::EndRod), 1), orbit);
            let pickup = &pickups[0];
            for (i, dot) in orbit[..4].iter().enumerate() {
                let angle = tick as f64 * 0.06 + i as f64 * TAU / 4.0;
                assert_eq!(
                    *dot,
                    Out::Particles {
                        client: 1,
                        particle: Particle::EndRod,
                        at: (
                            pickup.x + angle.cos() * 1.15,
                            pickup.y + 0.5 + (angle * 2.0).sin() * 0.15,
                            pickup.z + angle.sin() * 1.15
                        ),
                        count: 1,
                        offset: (0.0, 0.0, 0.0),
                        speed: 0.0,
                        long_distance: true,
                    }
                );
            }
        }
        assert_eq!(seen, 1);

        let tick = h.race().tick;
        h.race_mut().start = tick;
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        h.drain();
        h.kart_mut(a).input.forward = true;
        h.kart_mut(a).speed = 0.5;
        let mut trails = 0;
        for _ in 0..6 {
            h.tick();
            let kart = h.kart(a).clone();
            let out = h.drain();
            let cloud = particles(&out, 1, Particle::Cloud);
            assert!(particles(&out, 3, Particle::Cloud).is_empty());
            if !h.race().tick.is_multiple_of(3) || kart.speed <= 0.25 {
                assert!(cloud.is_empty(), "{out:?}");
                continue;
            }
            trails += 1;
            assert_eq!(
                cloud,
                vec![Out::Particles {
                    client: 1,
                    particle: Particle::Cloud,
                    at: (kart.x, kart.y + 0.3, kart.z),
                    count: 3,
                    offset: (0.35, 0.35, 0.35),
                    speed: 0.025,
                    long_distance: true,
                }]
            );
            assert_eq!(particles(&out, 2, Particle::Cloud).len(), 1);
        }
        assert!(trails >= 1);
        assert_eq!(trail(h.kart(b)), None);
        let mut kart = Kart {
            participant: true,
            speed: 0.5,
            ..Default::default()
        };
        assert_eq!(trail(&kart), Some(Particle::Cloud));
        kart.input.boost = true;
        kart.input.forward = true;
        kart.fuel = 50.0;
        assert_eq!(trail(&kart), Some(Particle::Flame));
        kart.charge = 1;
        assert_eq!(trail(&kart), Some(Particle::HappyVillager));
        kart.slow = 1;
        assert_eq!(trail(&kart), Some(Particle::EndRod));
        kart.shield = 1;
        assert_eq!(trail(&kart), Some(Particle::ElectricSpark));
        kart.impact = 1;
        assert_eq!(trail(&kart), Some(Particle::Crit));

        h.kart_mut(a).input.forward = false;
        h.kart_mut(a).speed = 0.0;
        place(&mut h, a, 0.1, 0.0);
        place(&mut h, b, 0.3, 0.0);
        use_item(&mut h, a, PowerUp::Missile);
        use_item(&mut h, a, PowerUp::Banana);
        h.drain();
        let mut checked = (false, false, false);
        for _ in 0..10 {
            h.tick();
            let tick = h.race().tick;
            let snapshot = (
                items(&h).missiles.first().copied(),
                items(&h).traps.first().copied(),
            );
            let out = h.drain();
            let flames: Vec<Out> = particles(&out, 1, Particle::Flame)
                .into_iter()
                .filter(|o| matches!(o, Out::Particles { count: 2, .. }))
                .collect();
            let smoke = particles(&out, 1, Particle::Smoke);
            if let (true, Some(m)) = (tick.is_multiple_of(2), snapshot.0) {
                checked.0 = true;
                assert_eq!(flames.len(), 1);
                assert_eq!(smoke.len(), 1);
                assert_eq!(
                    flames[0],
                    Out::Particles {
                        client: 1,
                        particle: Particle::Flame,
                        at: (m.x, m.y, m.z),
                        count: 2,
                        offset: (0.35, 0.35, 0.35),
                        speed: 0.025,
                        long_distance: true,
                    }
                );
                assert!(matches!(smoke[0], Out::Particles { count: 1, .. }));
                assert_eq!(particles(&out, 3, Particle::Flame).len(), 1);
            } else {
                assert!(flames.is_empty() && smoke.is_empty());
            }
            let slime = particles(&out, 1, Particle::ItemSlime);
            if let (true, Some(t)) = (tick.is_multiple_of(10), snapshot.1) {
                checked.1 = true;
                assert_eq!(
                    slime,
                    vec![Out::Particles {
                        client: 1,
                        particle: Particle::ItemSlime,
                        at: (t.x, t.y + 0.2, t.z),
                        count: 3,
                        offset: (0.35, 0.35, 0.35),
                        speed: 0.025,
                        long_distance: true,
                    }]
                );
                assert_eq!(particles(&out, 3, Particle::ItemSlime).len(), 1);
            } else {
                assert!(slime.is_empty());
            }
            if let Some(b) = items(&h).bursts.first().copied() {
                let sparks = particles(&out, 1, Particle::Firework);
                if tick.is_multiple_of(4) {
                    checked.2 = true;
                    assert_eq!(sparks.len(), RING_POINTS as usize);
                    let radius = b.current_radius();
                    for (i, dot) in sparks.iter().enumerate() {
                        let angle = i as f64 * TAU / f64::from(RING_POINTS);
                        assert_eq!(
                            *dot,
                            Out::Particles {
                                client: 1,
                                particle: Particle::Firework,
                                at: (b.x + angle.cos() * radius, b.y, b.z + angle.sin() * radius),
                                count: 1,
                                offset: (0.0, 0.0, 0.0),
                                speed: 0.0,
                                long_distance: true,
                            }
                        );
                    }
                    for other in [2, 3] {
                        assert_eq!(
                            as_seen_by(&particles(&out, other, Particle::Firework), 1),
                            sparks
                        );
                    }
                } else {
                    assert!(sparks.is_empty());
                }
            }
        }
        assert!(checked.0 && checked.1, "{checked:?}");

        h.world().resource_mut::<Items>().missiles.clear();
        h.world().resource_mut::<Items>().traps.clear();
        h.kart_mut(b).speed = 0.0;
        h.kart_mut(b).input.forward = false;
        let (ax, az) = (h.kart(a).x, h.kart(a).z);
        park(&mut h, b, ax + 1.0, az);
        h.tick();
        h.drain();
        h.kart_mut(a).contact_cooldown = 0;
        h.kart_mut(b).contact_cooldown = 0;
        use_item(&mut h, a, PowerUp::Ice);
        let trap = items(&h).traps[0];
        park(&mut h, a, trap.x + 6.0, trap.z);
        park(&mut h, b, trap.x + 6.0, trap.z + 2.0);
        h.drain();
        let mut rings = 0;
        for _ in 0..10 {
            h.tick();
            let out = h.drain();
            let ring: Vec<Out> = particles(&out, 1, Particle::EndRod)
                .into_iter()
                .filter(|o| matches!(o, Out::Particles { at, .. } if at.1 == trap.y + 0.15))
                .collect();
            if !h.race().tick.is_multiple_of(10) {
                assert!(ring.is_empty());
                continue;
            }
            rings += 1;
            assert_eq!(ring.len(), RING_POINTS as usize);
            assert_eq!(
                ring[0],
                Out::Particles {
                    client: 1,
                    particle: Particle::EndRod,
                    at: (trap.x + 2.5, trap.y + 0.15, trap.z),
                    count: 1,
                    offset: (0.0, 0.0, 0.0),
                    speed: 0.0,
                    long_distance: true,
                }
            );
            for other in [2, 3] {
                let theirs: Vec<Out> = particles(&out, other, Particle::EndRod)
                    .into_iter()
                    .filter(|o| matches!(o, Out::Particles { at, .. } if at.1 == trap.y + 0.15))
                    .collect();
                assert_eq!(as_seen_by(&theirs, 1), ring);
            }
        }
        assert_eq!(rings, 1);
    }
}
