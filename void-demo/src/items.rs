use std::f64::consts::TAU;

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{
    Audience, EndCrystal, EntityBuilder, EntityKind, Particle, Particles,
    components::{EntityViewers, MinecraftEntityId},
};

use crate::kart::{Kart, PowerUp, Strike};
use crate::race::{Chat, Phase, Race};
use crate::terrain::mix;
use crate::track::{GATES, Track};
use crate::vehicle::Pilot;

pub const PICKUP_RESPAWN: u64 = 160;
pub const PICKUP_RADIUS: f64 = 1.6;
pub const PICKUP_OFFSETS: [f64; 3] = [-3.5, 0.0, 3.5];
pub const MISSILE_LIFE: u16 = 160;
pub const MISSILE_RADIUS: f64 = 2.5;
pub const BURST_LIFE: u8 = 12;
pub const SHOCKWAVE_RADIUS: f64 = 9.0;
const ROLL_STRIDE: u64 = 0x9e3779b97f4a7c15;
const TRAIL_PERIOD: u64 = 3;
const MISSILE_PERIOD: u64 = 2;
const BURST_PERIOD: u64 = 4;
const GROUND_PERIOD: u64 = 10;
const RING_POINTS: u32 = 6;

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
    pub owner: Entity,
    pub target: Option<Entity>,
    pub phase: f64,
    pub offset: f64,
    pub age: u16,
    pub x: f64,
    pub y: f64,
    pub z: f64,
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
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub age: u8,
    pub radius: f64,
    pub kind: BurstKind,
}

impl Burst {
    fn at(kart: &Kart, radius: f64, kind: BurstKind) -> Self {
        Self {
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

#[derive(Resource, Default, Debug)]
pub struct Items {
    pub pickups: Vec<Pickup>,
    pub traps: Vec<Trap>,
    pub missiles: Vec<Missile>,
    pub bursts: Vec<Burst>,
}

impl Items {
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
        self.bursts.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.pickups.is_empty()
            && self.traps.is_empty()
            && self.missiles.is_empty()
            && self.bursts.is_empty()
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
struct Racer {
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

fn describe(item: PowerUp) -> &'static str {
    match item {
        PowerUp::Turbo => "Acceleration 3 s, +35 % de boost.",
        PowerUp::Shield => "Protection pendant 6 s.",
        PowerUp::Banana => "Piege depose pour 12 s.",
        PowerUp::Missile => "Projectile guide vers le prochain pilote devant toi (8 s maximum).",
        PowerUp::Shockwave => "Les rivaux a moins de 9 blocs sont repousses !",
        PowerUp::Ice => "Nappe de glace deposee pour 10 s : perte d'adherence pendant 3 s.",
        PowerUp::Lightning => "Les adversaires sont ralentis pendant 2,5 s !",
        PowerUp::Recharge => "Boost rempli et gratuit pendant 5 s : maintiens Saut + Avancer !",
    }
}

#[derive(SystemParam)]
pub struct Field<'w, 's> {
    map: Res<'w, Track>,
    items: ResMut<'w, Items>,
    karts: KartQuery<'w, 's>,
    chat: Chat<'w, 's>,
    commands: Commands<'w, 's>,
}

impl Field<'_, '_> {
    fn racers(&self) -> Vec<Racer> {
        let mut racers: Vec<Racer> = self
            .karts
            .iter()
            .filter(|(_, _, _, kart)| kart.racing())
            .map(|(kart, pilot, id, k)| Racer {
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

    fn activate(&mut self, item: PowerUp, racer: &Racer, racers: &[Racer], tick: u64) {
        self.chat.tell(
            racer.player,
            format!("{} active ! {}", item.name(), describe(item)),
        );
        let mut kart = self.karts.get_mut(racer.kart).unwrap().3;
        kart.activate(item);
        match item {
            PowerUp::Turbo | PowerUp::Shield => {}
            PowerUp::Recharge => {
                let burst = Burst::at(&kart, 3.0, BurstKind::Recharge);
                self.items.bursts.push(burst);
            }
            PowerUp::Banana | PowerUp::Ice => {
                let (x, y, z) = kart.trap_drop();
                self.items.traps.push(Trap {
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
                    self.chat.tell(
                        racer.player,
                        "Aucune cible devant : missile tire le long de la piste.",
                    );
                }
            }
            PowerUp::Shockwave | PowerUp::Lightning => {
                let shockwave = item == PowerUp::Shockwave;
                let burst = Burst::at(
                    &kart,
                    if shockwave { SHOCKWAVE_RADIUS } else { 3.0 },
                    if shockwave {
                        BurstKind::Shockwave
                    } else {
                        BurstKind::Lightning
                    },
                );
                self.items.bursts.push(burst);
                for other in racers.iter().filter(|r| r.kart != racer.kart) {
                    let (dx, dz) = (other.x - racer.x, other.z - racer.z);
                    let distance = dx.hypot(dz);
                    if shockwave && distance > SHOCKWAVE_RADIUS {
                        continue;
                    }
                    let strike = if shockwave {
                        let (nx, nz) = if distance < 0.001 {
                            (1.0, 0.0)
                        } else {
                            (dx / distance, dz / distance)
                        };
                        Strike::Shockwave { nx, nz }
                    } else {
                        Strike::Lightning
                    };
                    let mut kart = self.karts.get_mut(other.kart).unwrap().3;
                    let shielded = kart.strike(strike);
                    let burst = Burst::at(
                        &kart,
                        2.5,
                        if shockwave {
                            BurstKind::Impact
                        } else {
                            BurstKind::Lightning
                        },
                    );
                    self.items.bursts.push(burst);
                    self.chat.tell(
                        other.player,
                        if shielded {
                            "Bouclier : attaque absorbee !".to_string()
                        } else {
                            format!("Touche par {} !", item.name())
                        },
                    );
                }
            }
        }
    }

    fn collect(&mut self, racer: &Racer, tick: u64) {
        let Some((index, pickup)) = self.items.pickups.iter_mut().enumerate().find(|(_, p)| {
            tick >= p.ready_at && (racer.x - p.x).hypot(racer.z - p.z) < PICKUP_RADIUS
        }) else {
            return;
        };
        let roll = mix(self.map.seed
            ^ tick.wrapping_mul(ROLL_STRIDE)
            ^ ((index as u64) << 32)
            ^ racer.id as u64);
        let item = PowerUp::ALL[roll as usize % PowerUp::ALL.len()];
        pickup.ready_at = tick + PICKUP_RESPAWN;
        if let Some(crystal) = pickup.crystal.take() {
            despawn(&mut self.commands, crystal);
        }
        self.karts.get_mut(racer.kart).unwrap().3.collect(item);
        self.chat.tell(
            racer.player,
            format!(
                "Bonus obtenu : {} ! Appuie sur Sprint pour l'utiliser.",
                item.name()
            ),
        );
    }

    fn traps(&mut self, racer: &Racer, tick: u64) {
        let mut traps = std::mem::take(&mut self.items.traps);
        traps.retain(|trap| {
            if !trap.hits(racer.kart, racer.x, racer.z, tick) {
                return true;
            }
            let mut kart = self.karts.get_mut(racer.kart).unwrap().3;
            if trap.ice {
                let fresh = kart.ice == 0;
                if !kart.strike(Strike::Ice) && fresh {
                    self.chat
                        .tell(racer.player, "Glace ! Adherence reduite pendant 3 s.");
                }
            } else {
                self.chat.tell(
                    racer.player,
                    if kart.strike(Strike::Banana) {
                        "Le bouclier a absorbe une banane !"
                    } else {
                        "Banane ! Derapage — contre-braque pour repartir."
                    },
                );
            }
            trap.ice
        });
        self.items.traps = traps;
    }

    fn missiles(&mut self, racers: &[Racer]) {
        let map = &self.map;
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
            for racer in racers.iter().filter(|r| r.kart != missile.owner) {
                if (racer.x - missile.x).hypot(racer.z - missile.z) >= MISSILE_RADIUS {
                    continue;
                }
                let mut kart = self.karts.get_mut(racer.kart).unwrap().3;
                let shielded = kart.strike(Strike::Missile);
                self.items
                    .bursts
                    .push(Burst::at(&kart, 3.0, BurstKind::Impact));
                self.chat.tell(
                    racer.player,
                    if shielded {
                        "Bouclier : missile intercepte !"
                    } else {
                        "Missile ! Ralentissement pendant 1,5 s."
                    },
                );
                return false;
            }
            true
        });
        self.items.missiles = missiles;
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
    }
    field.missiles(&racers);
}

pub fn effects(
    race: Res<Race>,
    items: Res<Items>,
    particles: Particles,
    karts: Query<(&Kart, &EntityViewers)>,
    viewers: Query<&EntityViewers>,
) {
    let tick = race.tick;
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
        for i in 0..RING_POINTS {
            let angle = f64::from(i) * TAU / f64::from(RING_POINTS);
            point(
                particle.clone(),
                [
                    center[0] + angle.cos() * radius,
                    center[1],
                    center[2] + angle.sin() * radius,
                ],
            )
            .audience(Audience::All)
            .send();
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
            for i in 0..4 {
                let angle = tick as f64 * 0.06 + f64::from(i) * TAU / 4.0;
                point(
                    Particle::EndRod,
                    [
                        pickup.x + angle.cos() * 1.15,
                        pickup.y + 0.5 + (angle * 2.0).sin() * 0.15,
                        pickup.z + angle.sin() * 1.15,
                    ],
                )
                .viewers(seen.iter())
                .send();
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
            cloud(Particle::Flame, at, 2).audience(Audience::All).send();
            cloud(Particle::Smoke, at, 1).audience(Audience::All).send();
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

pub fn trail(kart: &Kart) -> Option<Particle> {
    if kart.impact > 0 {
        Some(Particle::Crit)
    } else if kart.shield > 0 {
        Some(Particle::ElectricSpark)
    } else if kart.ice > 0 || kart.slow > 0 {
        Some(Particle::EndRod)
    } else if kart.charge > 0 {
        Some(Particle::HappyVillager)
    } else if kart.turbo > 0 || (kart.input.boost && kart.input.forward && kart.fuel >= 1.5) {
        Some(Particle::Flame)
    } else if kart.speed.abs() > 0.25 {
        Some(Particle::Cloud)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use voidmc::components::{LoadedChunks, MinecraftEntityId, Position};
    use voidmc::{EntityKind, EntityMetadata};

    use super::*;
    use crate::race::tests::{Harness, Out};

    fn items(h: &Harness) -> &Items {
        h.app.world().resource::<Items>()
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
        h.shortcut_to_countdown(a, &[]);
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
        let out = h.drain();
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
        assert!(h.chats(1).is_empty());
        assert!(out.iter().any(|o| matches!(o, Out::Chat { client: 1, text, .. } if *text == format!("[Alpine Rush] Bonus obtenu : {} ! Appuie sur Sprint pour l'utiliser.", item.name()))));

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
        assert_eq!(crystals(&mut h).len(), GATES * 3 - 1);
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
            .collect();
        assert_eq!(removed.len(), GATES * 3 - 1);
        h.ticks(3);
        assert!(crystals(&mut h).is_empty());
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
        assert!(
            h.chats(2)
                .contains(&"[Alpine Rush] Missile ! Ralentissement pendant 1,5 s.".to_string())
        );

        h.kart_mut(b).slow = 0;
        h.kart_mut(b).shield = 200;
        use_item(&mut h, a, PowerUp::Missile);
        for _ in 0..80 {
            h.tick();
        }
        assert!(items(&h).missiles.is_empty());
        assert_eq!(h.kart(b).slow, 0);
        assert!(
            h.chats(2)
                .contains(&"[Alpine Rush] Bouclier : missile intercepte !".to_string())
        );

        use_item(&mut h, a, PowerUp::Missile);
        assert_eq!(items(&h).missiles[0].target, Some(kb));
        h.command(b, "leave", &[]);
        h.tick();
        assert!(items(&h).missiles.is_empty());

        place(&mut h, c, 0.05, 0.0);
        use_item(&mut h, a, PowerUp::Missile);
        assert_eq!(items(&h).missiles[0].target, None);
        assert!(h.chats(1).contains(
            &"[Alpine Rush] Aucune cible devant : missile tire le long de la piste.".to_string()
        ));
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
        assert!(chats(&out, 2).contains(&"[Alpine Rush] Touche par ONDE DE CHOC !".to_string()));
        assert!(
            chats(&out, 3).contains(&"[Alpine Rush] Bouclier : attaque absorbee !".to_string())
        );
        assert!(chats(&out, 4).is_empty());
        assert!(
            chats(&out, 1)
                .iter()
                .any(|t| t.starts_with("[Alpine Rush] ONDE DE CHOC active !"))
        );
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
        for e in [b, far] {
            assert_eq!(h.kart(e).slow, 49);
        }
        for e in [a, shielded, spectator, finished] {
            assert_eq!(h.kart(e).slow, 0);
        }
        let bursts = items(&h).bursts.clone();
        assert_eq!(bursts.len(), 4);
        assert_eq!(
            (bursts[0].kind, bursts[0].radius),
            (BurstKind::Lightning, 3.0)
        );
        assert!(
            bursts[1..]
                .iter()
                .all(|b| b.kind == BurstKind::Lightning && b.radius == 2.5)
        );
        assert_eq!(bursts[0].current_radius(), 0.0);
        assert!((ease_out(1.0) - 1.0).abs() < 1e-12);
        assert!((ease_out(0.5) - 0.875).abs() < 1e-12);
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
        assert!(
            h.chats(2)
                .contains(&"[Alpine Rush] Glace ! Adherence reduite pendant 3 s.".to_string())
        );
        h.tick();
        assert_eq!(h.kart(b).ice, 59);
        assert!(h.chats(2).is_empty());
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
        assert!(h.chats(2).contains(
            &"[Alpine Rush] Banane ! Derapage — contre-braque pour repartir.".to_string()
        ));

        use_item(&mut h, a, PowerUp::Banana);
        let trap = items(&h).traps[0];
        h.kart_mut(b).shield = 200;
        h.kart_mut(b).spin = 0.0;
        park(&mut h, b, trap.x, trap.z);
        h.tick();
        assert!(items(&h).traps.is_empty());
        assert_eq!(h.kart(b).spin, 0.0);
        assert!(
            h.chats(2)
                .contains(&"[Alpine Rush] Le bouclier a absorbe une banane !".to_string())
        );

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
            assert_eq!(particles(&out, 2, Particle::EndRod).len(), orbit.len());
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
            let flames = particles(&out, 1, Particle::Flame);
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
                    assert_eq!(particles(&out, 3, Particle::Firework).len(), sparks.len());
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
            assert_eq!(
                particles(&out, 3, Particle::EndRod).len(),
                RING_POINTS as usize
            );
        }
        assert_eq!(rings, 1);
    }
}
