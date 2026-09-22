use std::f64::consts::{PI, TAU};

use bevy_ecs::prelude::Component;

use crate::arena::WAIT_Y;
use crate::track::{GATES, HALF_WIDTH, Track};

pub const MIN_SPEED: f64 = -0.38;
pub const MAX_SPEED: f64 = 0.72;
pub const BOOST_SPEED: f64 = 1.15;
pub const RESET_PENALTY: u64 = 60;
pub const BUMP_COOLDOWN: u8 = 10;
pub const FIREBALL_CHARGE: u16 = 40;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub boost: bool,
    pub sprint: bool,
    pub sneak: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PowerUp {
    Turbo,
    Shield,
    Banana,
    Missile,
    Shockwave,
    Ice,
    Lightning,
    Recharge,
    Fireball,
}

impl PowerUp {
    pub const ALL: [Self; 9] = [
        Self::Turbo,
        Self::Shield,
        Self::Banana,
        Self::Missile,
        Self::Shockwave,
        Self::Ice,
        Self::Lightning,
        Self::Recharge,
        Self::Fireball,
    ];

    pub fn weight(self) -> u64 {
        match self {
            Self::Turbo | Self::Banana => 14,
            Self::Missile | Self::Fireball => 12,
            Self::Shield | Self::Ice => 10,
            Self::Shockwave | Self::Recharge => 8,
            Self::Lightning => 6,
        }
    }

    pub const TOTAL_WEIGHT: u64 = 94;

    pub fn roll(roll: u64) -> Self {
        let mut remaining = roll % Self::TOTAL_WEIGHT;
        for item in Self::ALL {
            if remaining < item.weight() {
                return item;
            }
            remaining -= item.weight();
        }
        unreachable!()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Turbo => "TURBO",
            Self::Shield => "BOUCLIER",
            Self::Banana => "BANANE",
            Self::Missile => "MISSILE GUIDE",
            Self::Shockwave => "ONDE DE CHOC",
            Self::Ice => "NAPPE DE GLACE",
            Self::Lightning => "ECLAIR",
            Self::Recharge => "SUPER-RECHARGE",
            Self::Fireball => "BOULE DE FEU",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Strike {
    Shockwave { nx: f64, nz: f64 },
    Lightning,
    Banana,
    Ice,
    Missile,
    Fireball,
}

#[derive(Component, Default, Clone, Debug)]
pub struct Kart {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub speed: f64,
    pub input: Input,
    pub fuel: f64,
    pub next_gate: usize,
    pub penalty: u64,
    pub participant: bool,
    pub finished: Option<u64>,
    pub push: (f64, f64),
    pub spin: f64,
    pub impact: u8,
    pub contact_cooldown: u8,
    pub bump: u8,
    pub knocked: bool,
    pub item: Option<PowerUp>,
    pub use_item: bool,
    pub turbo: u16,
    pub shield: u16,
    pub slow: u16,
    pub ice: u16,
    pub charge: u16,
    pub blaze: u16,
}

impl Kart {
    pub fn grid(&mut self, map: &Track, slot: usize) {
        let angle = -0.035 - (slot / 4) as f64 * 0.04;
        (self.x, self.y, self.z) = map.point(angle, (slot % 4) as f64 * 2.5 - 3.75);
        self.yaw = map.heading(angle);
        self.speed = 0.0;
        self.fuel = 100.0;
        self.next_gate = 1;
        self.penalty = 0;
        self.finished = None;
        self.clear_effects();
    }

    pub fn clear_effects(&mut self) {
        self.push = (0.0, 0.0);
        self.spin = 0.0;
        self.impact = 0;
        self.contact_cooldown = 0;
        self.bump = 0;
        self.knocked = false;
        self.item = None;
        self.use_item = false;
        self.turbo = 0;
        self.shield = 0;
        self.slow = 0;
        self.ice = 0;
        self.charge = 0;
        self.blaze = 0;
    }

    pub fn wait(&mut self, slot: usize) {
        self.x = (slot % 4) as f64 * 3.0 - 4.5;
        self.y = WAIT_Y;
        self.z = (slot / 4) as f64 * 3.0 - 2.0;
        self.yaw = PI;
        self.speed = 0.0;
        self.clear_effects();
    }

    /// The minecart model's longitudinal axis is 90° off the player's yaw.
    pub fn model_yaw(&self) -> f32 {
        (self.yaw.to_degrees() + 90.0).rem_euclid(360.0) as f32
    }

    pub fn racing(&self) -> bool {
        self.participant && self.finished.is_none()
    }

    pub fn knock(&mut self) {
        if self.bump == 0 {
            self.bump = BUMP_COOLDOWN;
            self.knocked = true;
        }
    }

    pub fn reset(&mut self, map: &Track) {
        let angle = (self.next_gate - 1) as f64 * TAU / GATES as f64;
        (self.x, self.y, self.z) = map.point(angle + 0.015, 0.0);
        self.yaw = map.heading(angle);
        self.speed = 0.0;
        self.penalty += RESET_PENALTY;
        self.clear_effects();
    }

    /// Laps completed plus the fraction of the current one, from gate order
    /// rather than raw position, so the grid section before the line counts as
    /// negative progress.
    pub fn progress(&self, map: &Track) -> f64 {
        let phase = map.project(self.x, self.z).phase.rem_euclid(TAU) / TAU;
        let expected = self.next_gate as f64 / GATES as f64;
        phase + (expected - phase).ceil() - 1.0
    }

    pub fn drive(&mut self, map: &Track) {
        self.impact = self.impact.saturating_sub(1);
        self.contact_cooldown = self.contact_cooldown.saturating_sub(1);
        self.bump = self.bump.saturating_sub(1);
        self.shield = self.shield.saturating_sub(1);
        self.turbo = self.turbo.saturating_sub(1);
        self.slow = self.slow.saturating_sub(1);
        self.ice = self.ice.saturating_sub(1);
        self.charge = self.charge.saturating_sub(1);
        let boosting = self.input.boost && self.input.forward && self.fuel >= 1.5;
        self.fuel = (self.fuel
            + if self.charge > 0 {
                1.0
            } else if boosting {
                -1.5
            } else {
                0.35
            })
        .clamp(0.0, 100.0);
        let acceleration = if self.input.forward { 0.024 } else { 0.0 }
            - if self.input.backward { 0.05 } else { 0.0 };
        self.speed = ((self.speed + acceleration) * 0.985).clamp(
            MIN_SPEED,
            if boosting || self.turbo > 0 {
                BOOST_SPEED
            } else {
                MAX_SPEED
            },
        );
        if boosting || self.turbo > 0 {
            self.speed = (self.speed + 0.035).min(BOOST_SPEED);
        }
        if self.slow > 0 {
            self.speed = self.speed.clamp(-0.2, 0.32);
        }
        let steering = i32::from(self.input.right) - i32::from(self.input.left);
        self.yaw += f64::from(steering)
            * (if self.ice > 0 { 0.018 } else { 0.045 })
            * (self.speed / 0.3).clamp(-1.0, 1.0)
            + self.spin;
        self.spin *= 0.9;
        let vx = -self.yaw.sin() * self.speed + self.push.0;
        let vz = self.yaw.cos() * self.speed + self.push.1;
        let grip = if self.ice > 0 { 0.95 } else { 0.85 };
        self.push.0 *= grip;
        self.push.1 *= grip;
        let x = self.x + vx;
        let z = self.z + vz;
        let projected = map.project(x, z);
        if projected.distance <= HALF_WIDTH - 1.0 {
            let old = map.project(self.x, self.z).phase;
            self.x = x;
            self.z = z;
            self.y = projected.y;
            if crosses_gate(old, projected.phase, self.next_gate) {
                self.next_gate += 1;
            }
        } else {
            let (nx, nz) = projected.normal;
            let dot = vx * nx + vz * nz;
            self.speed *= 0.8;
            self.push = (
                vx - 1.65 * dot * nx + self.yaw.sin() * self.speed,
                vz - 1.65 * dot * nz - self.yaw.cos() * self.speed,
            );
            self.impact = 6;
            self.knock();
        }
    }

    pub fn collect(&mut self, item: PowerUp) -> bool {
        if self.item.is_some() {
            return false;
        }
        self.item = Some(item);
        self.impact = 6;
        true
    }

    pub fn take_item(&mut self) -> Option<PowerUp> {
        if std::mem::take(&mut self.use_item) {
            self.item.take()
        } else {
            None
        }
    }

    /// Effects a power-up has on its own user; the world-facing half
    /// (traps, projectiles, bursts) belongs to the caller.
    pub fn activate(&mut self, item: PowerUp) {
        match item {
            PowerUp::Turbo => {
                self.turbo = 60;
                self.fuel = (self.fuel + 35.0).min(100.0);
            }
            PowerUp::Shield => {
                self.shield = 120;
                self.spin = 0.0;
                self.push = (0.0, 0.0);
                self.slow = 0;
                self.ice = 0;
            }
            PowerUp::Recharge => {
                self.fuel = 100.0;
                self.charge = 100;
            }
            PowerUp::Fireball => self.blaze = FIREBALL_CHARGE,
            PowerUp::Banana
            | PowerUp::Ice
            | PowerUp::Missile
            | PowerUp::Shockwave
            | PowerUp::Lightning => {}
        }
    }

    pub fn trap_drop(&self) -> (f64, f64, f64) {
        (
            self.x + self.yaw.sin() * 2.8,
            self.y,
            self.z - self.yaw.cos() * 2.8,
        )
    }

    /// Returns `true` when the shield absorbed the strike.
    pub fn strike(&mut self, strike: Strike) -> bool {
        let shielded = self.shield > 0;
        match strike {
            Strike::Shockwave { nx, nz } => {
                if !shielded {
                    self.push = (nx * 0.9, nz * 0.9);
                    self.spin = 0.15;
                    self.speed *= 0.7;
                }
                self.impact = 10;
            }
            Strike::Lightning => {
                if !shielded {
                    self.slow = 50;
                    self.speed *= 0.4;
                }
                self.impact = 10;
            }
            Strike::Banana => {
                if !shielded {
                    self.spin = 0.5;
                    self.speed *= 0.45;
                }
                self.impact = 6;
            }
            Strike::Ice => {
                if !shielded {
                    self.ice = 60;
                }
            }
            Strike::Missile | Strike::Fireball => {
                if !shielded {
                    self.spin = 0.35;
                    self.speed *= 0.35;
                    self.slow = if strike == Strike::Missile { 30 } else { 20 };
                }
                self.impact = 12;
            }
        }
        shielded
    }
}

pub fn collide(a: &mut Kart, b: &mut Kart) {
    if !a.racing() || !b.racing() || a.contact_cooldown > 0 || b.contact_cooldown > 0 {
        return;
    }
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let distance = dx.hypot(dz);
    if distance >= 1.65 {
        return;
    }
    let (nx, nz) = if distance < 1e-6 {
        (1.0, 0.0)
    } else {
        (dx / distance, dz / distance)
    };
    let relative = (-a.yaw.sin() * a.speed + b.yaw.sin() * b.speed) * nx
        + (a.yaw.cos() * a.speed - b.yaw.cos() * b.speed) * nz;
    let force = (0.18 + relative.max(0.0) * 0.65).min(0.8);
    for (kart, sign) in [(a, -1.0), (b, 1.0)] {
        if kart.shield == 0 {
            kart.push = (nx * sign * force, nz * sign * force);
            kart.spin = sign * (0.08 + force * 0.18);
            kart.speed *= 0.8;
        }
        kart.impact = 6;
        kart.contact_cooldown = 10;
        kart.knock();
    }
}

pub fn crosses_gate(old: f64, new: f64, gate: usize) -> bool {
    let delta = (new - old + PI).rem_euclid(TAU) - PI;
    let target = (gate % GATES) as f64 * TAU / GATES as f64;
    let distance = (target - old).rem_euclid(TAU);
    delta > 0.0 && distance > 0.0 && distance <= delta
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPS: usize = 3;

    fn kart(map: &Track) -> Kart {
        let mut k = Kart {
            participant: true,
            ..Default::default()
        };
        k.grid(map, 0);
        k
    }

    #[test]
    fn checkpoints_require_forward_order_including_finish_wrap() {
        assert!(crosses_gate(-0.01, 0.01, 8));
        assert!(crosses_gate(-0.01, 0.01, 16));
        assert!(!crosses_gate(-0.01, 0.01, 1));
        assert!(!crosses_gate(0.01, -0.01, 8));
        assert!(crosses_gate(TAU / 8.0 - 0.01, TAU / 8.0 + 0.01, 1));
        assert!(!crosses_gate(TAU / 8.0 - 0.01, TAU / 8.0 + 0.01, 2));
        assert!(!crosses_gate(0.5, 0.5, 1));
        assert!(crosses_gate(3.13, -3.13, 4));
        assert!(crosses_gate(0.7, 0.9, 1));
        assert!(!crosses_gate(0.7, 0.9, 2));
    }

    #[test]
    fn driving_follows_non_circular_courses_through_three_ordered_laps() {
        for seed in [0, 42, 2026, u64::MAX] {
            let map = Track::new(seed);
            let mut k = kart(&map);
            k.input.forward = true;
            for _ in 0..6000 {
                let phase = map.project(k.x, k.z).phase;
                let target = map.point(phase + 3.0 / map.length * TAU, 0.0);
                k.yaw = (k.x - target.0).atan2(target.2 - k.z);
                k.drive(&map);
                assert!(map.road_distance(k.x, k.z) <= HALF_WIDTH - 1.0);
                if k.next_gate > LAPS * GATES {
                    break;
                }
            }
            assert_eq!(k.next_gate, LAPS * GATES + 1, "seed {seed}");
        }
    }

    #[test]
    fn guardrails_stop_shortcuts_and_boost_is_bounded() {
        let map = Track::new(42);
        let mut k = kart(&map);
        k.input = Input {
            forward: true,
            boost: true,
            ..Default::default()
        };
        for _ in 0..2000 {
            k.drive(&map);
            assert!(map.road_distance(k.x, k.z) <= HALF_WIDTH - 1.0);
            assert!((0.0..=100.0).contains(&k.fuel));
            assert!((MIN_SPEED..=BOOST_SPEED).contains(&k.speed));
        }
        assert!(k.next_gate < GATES);
    }

    #[test]
    fn rescue_keeps_progress_and_adds_penalty() {
        let map = Track::new(42);
        let mut k = kart(&map);
        k.next_gate = 13;
        k.reset(&map);
        assert_eq!(k.next_gate, 13);
        assert_eq!(k.penalty, 60);
        assert!(map.road_distance(k.x, k.z) < 1e-8);
        assert_eq!(k.speed, 0.0);
    }

    #[test]
    fn reverse_steers_backwards_and_brakes_before_changing_direction() {
        let map = Track::new(42);
        let mut k = kart(&map);
        k.speed = 0.5;
        k.input.backward = true;
        k.drive(&map);
        assert!(k.speed > 0.0 && k.speed < 0.5);
        for _ in 0..20 {
            k.drive(&map);
        }
        assert!(k.speed < -0.25);
        k.input.right = true;
        let yaw = k.yaw;
        k.drive(&map);
        assert!(k.yaw < yaw);
        assert_eq!(k.next_gate, 1);
    }

    #[test]
    fn wall_reflects_motion_instead_of_sticking() {
        let map = Track::new(42);
        let mut k = kart(&map);
        (k.x, k.y, k.z) = map.point(0.0, HALF_WIDTH - 1.1);
        let normal = map.project(k.x, k.z).normal;
        k.yaw = (-normal.0).atan2(normal.1);
        k.speed = 0.7;
        let distance = map.road_distance(k.x, k.z);
        k.drive(&map);
        assert!(k.speed > 0.5);
        assert!(k.impact > 0);
        k.drive(&map);
        assert!(map.road_distance(k.x, k.z) < distance);
        assert!(map.road_distance(k.x, k.z) < HALF_WIDTH - 1.0);
    }

    #[test]
    fn contact_bumps_spins_and_shields_with_a_cooldown() {
        let map = Track::new(42);
        let mut a = kart(&map);
        let mut b = kart(&map);
        b.x += 1.0;
        a.yaw = -PI / 2.0;
        a.speed = 0.7;
        collide(&mut a, &mut b);
        assert!(a.push.0 < 0.0 && b.push.0 > 0.0);
        assert!(a.spin < 0.0 && b.spin > 0.0);
        let spin = b.spin;
        collide(&mut a, &mut b);
        assert_eq!(b.spin, spin);
        a.contact_cooldown = 0;
        b.contact_cooldown = 0;
        b.shield = 100;
        b.spin = 0.0;
        b.push = (0.0, 0.0);
        collide(&mut a, &mut b);
        assert_eq!(b.spin, 0.0);
        assert_eq!(b.push, (0.0, 0.0));
        a.yaw = 0.0;
        assert_eq!(a.model_yaw(), 90.0);
        assert_eq!(
            kart(&map).model_yaw(),
            (map.heading(-0.035).to_degrees() + 90.0).rem_euclid(360.0) as f32
        );
    }

    #[test]
    fn spectators_and_finished_karts_never_collide() {
        let map = Track::new(42);
        let mut a = kart(&map);
        let mut b = kart(&map);
        b.finished = Some(10);
        collide(&mut a, &mut b);
        assert_eq!((a.push, b.push), ((0.0, 0.0), (0.0, 0.0)));
        b.finished = None;
        b.participant = false;
        collide(&mut a, &mut b);
        assert_eq!(a.impact, 0);
    }

    #[test]
    fn waiting_slots_spread_karts_in_the_air_and_clear_effects() {
        let mut k = Kart {
            turbo: 5,
            item: Some(PowerUp::Turbo),
            ..Default::default()
        };
        k.wait(5);
        assert_eq!((k.x, k.y, k.z), (-1.5, WAIT_Y, 1.0));
        assert_eq!(k.yaw, PI);
        assert_eq!((k.turbo, k.item), (0, None));
    }

    #[test]
    fn pickups_are_single_use_and_self_effects_match_the_reference() {
        let map = Track::new(42);
        let mut k = kart(&map);
        assert!(k.collect(PowerUp::Banana));
        assert!(!k.collect(PowerUp::Turbo));
        assert_eq!((k.item, k.impact), (Some(PowerUp::Banana), 6));
        assert_eq!(k.take_item(), None);
        k.use_item = true;
        assert_eq!(k.take_item(), Some(PowerUp::Banana));
        assert_eq!(k.take_item(), None);
        assert!(!k.use_item);
        let drop = k.trap_drop();
        let behind = (k.x + k.yaw.sin() * 2.8, k.z - k.yaw.cos() * 2.8);
        assert_eq!((drop.0, drop.1, drop.2), (behind.0, k.y, behind.1));
        assert!((drop.0 - k.x).hypot(drop.2 - k.z) - 2.8 < 1e-9);

        k.fuel = 50.0;
        k.activate(PowerUp::Turbo);
        assert_eq!((k.turbo, k.fuel), (60, 85.0));
        k.activate(PowerUp::Turbo);
        assert_eq!(k.fuel, 100.0);

        k.slow = 10;
        k.ice = 10;
        k.spin = 0.3;
        k.push = (0.1, 0.1);
        k.activate(PowerUp::Shield);
        assert_eq!(k.shield, 120);
        assert_eq!((k.slow, k.ice, k.spin, k.push), (0, 0, 0.0, (0.0, 0.0)));

        k.fuel = 2.0;
        k.activate(PowerUp::Recharge);
        k.input.forward = true;
        k.input.boost = true;
        for _ in 0..99 {
            k.drive(&map);
            assert_eq!(k.fuel, 100.0);
        }
        k.drive(&map);
        assert_eq!(k.charge, 0);
        assert!(k.fuel < 100.0);
        k.activate(PowerUp::Fireball);
        assert_eq!(k.blaze, FIREBALL_CHARGE);
        k.reset(&map);
        assert_eq!((k.slow, k.ice, k.charge, k.blaze), (0, 0, 0, 0));
    }

    #[test]
    fn the_roll_table_is_weighted_and_covers_every_power_up() {
        let total = PowerUp::TOTAL_WEIGHT;
        assert_eq!(
            total,
            PowerUp::ALL.iter().map(|item| item.weight()).sum::<u64>()
        );
        let mut counts = std::collections::HashMap::new();
        for roll in 0..total {
            *counts.entry(PowerUp::roll(roll)).or_insert(0) += 1;
        }
        for item in PowerUp::ALL {
            assert_eq!(counts[&item], item.weight(), "{item:?}");
        }
        assert_eq!(PowerUp::roll(total + 3), PowerUp::Turbo);
        assert_eq!(PowerUp::roll(u64::MAX), PowerUp::roll(u64::MAX % total));
        assert!(PowerUp::ALL.contains(&PowerUp::Fireball));
        assert_eq!(PowerUp::Fireball.weight(), PowerUp::Missile.weight());
    }

    #[test]
    fn strikes_slow_spin_and_are_absorbed_by_shields() {
        let map = Track::new(42);
        let mut k = kart(&map);
        k.speed = 0.7;
        assert!(!k.strike(Strike::Lightning));
        assert_eq!((k.slow, k.impact), (50, 10));
        assert!((k.speed - 0.28).abs() < 1e-12);
        k.input.forward = true;
        k.input.boost = true;
        k.drive(&map);
        assert!(k.speed <= 0.32);
        for _ in 0..50 {
            k.drive(&map);
        }
        assert_eq!(k.slow, 0);

        k.speed = 1.0;
        assert!(!k.strike(Strike::Shockwave { nx: 0.0, nz: 1.0 }));
        assert_eq!(
            (k.push, k.spin, k.speed, k.impact),
            ((0.0, 0.9), 0.15, 0.7, 10)
        );
        k.speed = 1.0;
        assert!(!k.strike(Strike::Banana));
        assert_eq!((k.spin, k.speed, k.impact), (0.5, 0.45, 6));
        k.impact = 0;
        assert!(!k.strike(Strike::Ice));
        assert_eq!((k.ice, k.impact), (60, 0));
        k.speed = 1.0;
        assert!(!k.strike(Strike::Missile));
        assert_eq!((k.spin, k.speed, k.slow, k.impact), (0.35, 0.35, 30, 12));
        k.speed = 1.0;
        assert!(!k.strike(Strike::Fireball));
        assert_eq!((k.spin, k.speed, k.slow, k.impact), (0.35, 0.35, 20, 12));

        k.clear_effects();
        k.activate(PowerUp::Shield);
        k.speed = 0.6;
        for strike in [
            Strike::Shockwave { nx: 1.0, nz: 0.0 },
            Strike::Lightning,
            Strike::Banana,
            Strike::Ice,
            Strike::Missile,
            Strike::Fireball,
        ] {
            assert!(k.strike(strike));
        }
        assert_eq!(
            (k.speed, k.spin, k.push, k.slow, k.ice),
            (0.6, 0.0, (0.0, 0.0), 0, 0)
        );
        assert_eq!(k.impact, 12);
    }

    #[test]
    fn ice_reduces_steering_and_grip() {
        let map = Track::new(42);
        let mut dry = kart(&map);
        let mut icy = kart(&map);
        for k in [&mut dry, &mut icy] {
            k.speed = 0.5;
            k.input.right = true;
            k.push = (0.2, 0.0);
        }
        icy.ice = 60;
        let (dry_yaw, icy_yaw) = (dry.yaw, icy.yaw);
        dry.drive(&map);
        icy.drive(&map);
        assert!(icy.yaw - icy_yaw < dry.yaw - dry_yaw);
        assert!(icy.push.0 > dry.push.0);
    }

    #[test]
    fn progress_counts_laps_from_gate_order() {
        let map = Track::new(42);
        let mut k = kart(&map);
        assert!(k.progress(&map) < 0.0 && k.progress(&map) > -0.1);
        (k.x, k.y, k.z) = map.point(TAU * 0.3, 0.0);
        k.next_gate = 3;
        assert!((k.progress(&map) - 0.3).abs() < 0.01);
        k.next_gate = 11;
        assert!((k.progress(&map) - 1.3).abs() < 0.01);
        (k.x, k.y, k.z) = map.point(TAU * 0.99, 0.0);
        k.next_gate = 9;
        assert!((k.progress(&map) - 0.99).abs() < 0.01);
        k.next_gate = 8;
        assert!((k.progress(&map) - 0.99).abs() < 0.01);
    }
}
