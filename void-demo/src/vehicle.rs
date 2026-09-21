use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{
    EntityBuilder, EntityKind, Hidden, Passengers,
    components::{Position, Rotation},
    events::PlayerInputEvent,
};

use crate::kart::{Input, Kart, collide};
use crate::race::{Phase, Race, Racer};
use crate::track::Track;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pilot(pub Entity);

pub fn spawn(commands: &mut Commands, player: Entity, kart: Kart) -> Entity {
    EntityBuilder::new(EntityKind::Minecart)
        .at(kart.x, kart.y, kart.z)
        .rotation(kart.model_yaw(), 0.0)
        .with(Pilot(player))
        .with(Passengers::default())
        .with(Hidden)
        .with(kart)
        .spawn(commands)
        .id()
}

#[derive(SystemParam)]
pub struct Karts<'w, 's> {
    racers: Query<'w, 's, &'static Racer>,
    karts: Query<'w, 's, &'static mut Kart>,
}

impl Karts<'_, '_> {
    pub fn entity(&self, player: Entity) -> Option<Entity> {
        self.racers.get(player).ok().map(|racer| racer.kart)
    }

    pub fn get(&self, player: Entity) -> Option<&Kart> {
        self.karts.get(self.entity(player)?).ok()
    }

    pub fn get_mut(&mut self, player: Entity) -> Option<Mut<'_, Kart>> {
        let kart = self.entity(player)?;
        self.karts.get_mut(kart).ok()
    }
}

pub fn input(
    event: On<PlayerInputEvent>,
    racers: Query<&Racer>,
    mut karts: Query<(&mut Kart, &mut Passengers)>,
) {
    let Ok(racer) = racers.get(event.entity) else {
        return;
    };
    let Ok((mut kart, mut passengers)) = karts.get_mut(racer.kart) else {
        return;
    };
    if event.sneak && !kart.input.sneak && !passengers.0.is_empty() {
        passengers.set_changed();
    }
    kart.use_item |= event.sprint && !kart.input.sprint;
    kart.input = Input {
        forward: event.forward,
        backward: event.backward,
        left: event.left,
        right: event.right,
        boost: event.jump,
        sprint: event.sprint,
        sneak: event.sneak,
    };
}

pub fn drive(race: Res<Race>, map: Res<Track>, mut karts: Query<&mut Kart>) {
    if race.phase != Phase::Racing {
        return;
    }
    for mut kart in &mut karts {
        if kart.racing() {
            kart.drive(&map);
        }
    }
    let mut pairs = karts.iter_combinations_mut();
    while let Some([mut a, mut b]) = pairs.fetch_next() {
        if a.racing() && b.racing() {
            collide(&mut a, &mut b);
        }
    }
}

pub const SEAT_HEIGHT: f64 = 0.35;

pub fn pose(
    mut karts: Query<(&Kart, &Pilot, &Passengers, &mut Position, &mut Rotation), Changed<Kart>>,
    mut pilots: Query<&mut Position, Without<Kart>>,
) {
    for (kart, pilot, seats, mut position, mut rotation) in &mut karts {
        let next = Position {
            x: kart.x,
            y: kart.y,
            z: kart.z,
        };
        if *position != next {
            *position = next;
        }
        let yaw = kart.model_yaw();
        if rotation.yaw != yaw {
            rotation.yaw = yaw;
        }
        if !seats.0.contains(&pilot.0) {
            continue;
        }
        if let Ok(mut seat) = pilots.get_mut(pilot.0) {
            let next = Position {
                y: kart.y + SEAT_HEIGHT,
                ..next
            };
            if *seat != next {
                *seat = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use bevy_ecs::change_detection::Tick;
    use voidmc::components::{EntityType, MinecraftEntityId};

    use super::*;
    use crate::arena::WAIT_Y;
    use crate::race::tests::{Harness, Out};

    fn press(h: &mut Harness, player: Entity, keys: &[&str]) {
        let key = |k: &str| keys.contains(&k);
        h.world().trigger(PlayerInputEvent {
            entity: player,
            forward: key("forward"),
            backward: key("backward"),
            left: key("left"),
            right: key("right"),
            jump: key("jump"),
            sneak: key("sneak"),
            sprint: key("sprint"),
        });
        h.world().flush();
    }

    fn network_id(h: &Harness, entity: Entity) -> i32 {
        h.app.world().get::<MinecraftEntityId>(entity).unwrap().0
    }

    fn karts(h: &mut Harness) -> Vec<(Entity, Pilot, Passengers)> {
        h.world()
            .query::<(Entity, &Pilot, &Passengers)>()
            .iter(h.app.world())
            .map(|(e, p, passengers)| (e, *p, passengers.clone()))
            .collect()
    }

    fn position(h: &Harness, kart: Entity) -> Position {
        *h.app.world().get::<Position>(kart).unwrap()
    }

    fn position_changed_since(h: &mut Harness, entity: Entity, since: Tick) -> bool {
        let now = h.world().change_tick();
        h.world()
            .query::<Ref<Position>>()
            .get(h.app.world(), entity)
            .unwrap()
            .last_changed()
            .is_newer_than(since, now)
    }

    fn seated(h: &Harness, player: Entity) -> Position {
        let kart = h.kart(player);
        Position {
            x: kart.x,
            y: kart.y + SEAT_HEIGHT,
            z: kart.z,
        }
    }

    fn movement(out: &[Out], client: u32, id: i32) -> Vec<Out> {
        out.iter()
            .filter(|o| match o {
                Out::Move {
                    client: c, id: i, ..
                }
                | Out::Teleport {
                    client: c, id: i, ..
                } => *c == client && *i == id,
                Out::Rotate(c, i, _) => *c == client && *i == id,
                _ => false,
            })
            .cloned()
            .collect()
    }

    fn passengers(out: &[Out], client: u32) -> Vec<(i32, Vec<i32>)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Passengers(c, id, list) if *c == client => Some((*id, list.clone())),
                _ => None,
            })
            .collect()
    }

    struct Spawned {
        id: i32,
        kind: i32,
        at: (f64, f64, f64),
        yaw: u8,
    }

    fn spawns(out: &[Out], client: u32) -> Vec<Spawned> {
        out.iter()
            .filter_map(|o| match o {
                Out::Spawn {
                    client: c,
                    id,
                    kind,
                    x,
                    y,
                    z,
                    yaw,
                } if *c == client => Some(Spawned {
                    id: *id,
                    kind: *kind,
                    at: (*x, *y, *z),
                    yaw: *yaw,
                }),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn join_parks_one_hidden_minecart_without_passenger() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let listed = karts(&mut h);
        assert_eq!(listed.len(), 1);
        let (kart, pilot, seats) = listed[0].clone();
        assert_eq!(pilot, Pilot(a));
        assert_eq!(seats, Passengers::default());
        assert_eq!(h.kart_entity(a), kart);
        assert!(h.app.world().get::<Hidden>(kart).is_some());
        assert_eq!(
            h.app.world().get::<EntityType>(kart).unwrap().0,
            EntityKind::Minecart.id()
        );
        let sim = h.kart(a).clone();
        assert_eq!((sim.x, sim.y, sim.z, sim.yaw), (-4.5, WAIT_Y, -2.0, PI));
        assert_eq!(
            position(&h, kart),
            Position {
                x: sim.x,
                y: sim.y,
                z: sim.z
            }
        );
        assert_eq!(h.app.world().get::<Rotation>(kart).unwrap().yaw, 270.0);
        h.drain();
        h.ticks(21);
        let out = h.drain();
        let kart_id = network_id(&h, kart);
        assert!(spawns(&out, 1).is_empty(), "{out:?}");
        assert!(passengers(&out, 1).is_empty());
        assert!(movement(&out, 1, kart_id).is_empty());

        h.connect(2);
        h.tick();
        let out = h.drain();
        assert_eq!(karts(&mut h).len(), 2);
        assert!(
            karts(&mut h)
                .iter()
                .all(|(kart, ..)| h.app.world().get::<Hidden>(*kart).is_some())
        );
        assert!(
            spawns(&out, 1).is_empty() && spawns(&out, 2).is_empty(),
            "{out:?}"
        );
        assert!(passengers(&out, 1).is_empty() && passengers(&out, 2).is_empty());
    }

    #[test]
    fn input_maps_keys_and_remounts_only_once_per_sneak_press() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let spectator = h.world().spawn(()).id();
        h.tick();
        h.drain();
        press(&mut h, spectator, &["forward", "sneak"]);
        press(&mut h, a, &["forward", "right", "jump", "sprint"]);
        assert_eq!(
            h.kart(a).input,
            Input {
                forward: true,
                right: true,
                boost: true,
                sprint: true,
                ..Default::default()
            }
        );
        assert!(h.kart(a).use_item);
        press(&mut h, a, &["backward", "left", "sprint"]);
        assert_eq!(
            h.kart(a).input,
            Input {
                backward: true,
                left: true,
                sprint: true,
                ..Default::default()
            }
        );
        h.ticks(3);
        let out = h.drain();
        assert!(passengers(&out, 1).is_empty(), "{out:?}");
        let kart_id = network_id(&h, h.kart_entity(a));
        assert!(movement(&out, 1, kart_id).is_empty());

        press(&mut h, a, &["sneak"]);
        h.tick();
        assert!(passengers(&h.drain(), 1).is_empty());
        press(&mut h, a, &[]);
        h.shortcut_to_countdown(a, &[]);
        h.drain();
        press(&mut h, a, &["sneak"]);
        h.tick();
        assert_eq!(
            passengers(&h.drain(), 1),
            vec![(kart_id, vec![network_id(&h, a)])]
        );
        press(&mut h, a, &["sneak", "forward"]);
        press(&mut h, a, &["sneak"]);
        h.ticks(5);
        assert!(passengers(&h.drain(), 1).is_empty());
        press(&mut h, a, &[]);
        press(&mut h, a, &["sneak"]);
        h.tick();
        assert_eq!(passengers(&h.drain(), 1).len(), 1);
        h.ticks(5);
        assert!(passengers(&h.drain(), 1).is_empty());
    }

    #[test]
    fn racing_tick_moves_the_driven_kart_and_a_stationary_kart_writes_nothing() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.tick();
        let out = h.shortcut_to_countdown(a, &[]);
        let (ka, kb) = (h.kart_entity(a), h.kart_entity(b));
        let (ida, idb) = (network_id(&h, ka), network_id(&h, kb));
        let grid = h.kart(a).clone();
        assert!(grid.y < WAIT_Y);
        assert!(movement(&out, 1, ida).is_empty());
        assert!(movement(&out, 2, idb).is_empty());
        let spawned: Vec<Spawned> = spawns(&out, 1)
            .into_iter()
            .filter(|s| s.kind == EntityKind::Minecart.id())
            .collect();
        assert_eq!(spawned.len(), 2);
        let kart = spawned.iter().find(|s| s.id == ida).unwrap();
        assert_eq!(kart.at, (grid.x, grid.y, grid.z));
        assert_eq!(kart.yaw, (grid.model_yaw() * 256.0 / 360.0) as u8);
        assert_eq!(
            position(&h, ka),
            Position {
                x: grid.x,
                y: grid.y,
                z: grid.z
            }
        );
        assert_eq!(position(&h, a), seated(&h, a));
        assert_eq!(position(&h, b), seated(&h, b));

        let tick = h.race().tick;
        h.race_mut().start = tick;
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        h.drain();
        press(&mut h, a, &["forward", "right"]);
        let before = position(&h, ka);
        let since = h.world().change_tick();
        h.tick();
        let sim = h.kart(a).clone();
        assert!(sim.speed > 0.0);
        let after = position(&h, ka);
        assert_ne!(before, after);
        assert_eq!(
            after,
            Position {
                x: sim.x,
                y: sim.y,
                z: sim.z
            }
        );
        assert_eq!(
            h.app.world().get::<Rotation>(ka).unwrap().yaw,
            sim.model_yaw()
        );
        assert!(position_changed_since(&mut h, ka, since));
        assert!(!position_changed_since(&mut h, kb, since));
        assert_eq!(position(&h, a), seated(&h, a));
        assert!(position_changed_since(&mut h, a, since));
        assert!(!position_changed_since(&mut h, b, since));
        let out = h.drain();
        let moved = movement(&out, 1, ida);
        assert_eq!(moved.len(), 1);
        assert!(matches!(
            moved[0],
            Out::Move { yaw: Some(_), delta, .. } if delta != (0, 0, 0)
        ));
        assert!(matches!(
            movement(&out, 2, ida)[..],
            [Out::Move { delta, .. }] if matches!(moved[0], Out::Move { delta: d, .. } if d == delta)
        ));
        assert!(movement(&out, 1, idb).is_empty());
        assert!(movement(&out, 2, idb).is_empty());
        assert!(passengers(&out, 1).is_empty());

        let since = h.world().change_tick();
        let stationary = position(&h, kb);
        h.ticks(10);
        assert_eq!(position(&h, kb), stationary);
        assert!(!position_changed_since(&mut h, kb, since));
        assert_eq!(position(&h, b), seated(&h, b));
        assert!(!position_changed_since(&mut h, b, since));
        assert_eq!(position(&h, a), seated(&h, a));
        let out = h.drain();
        assert!(movement(&out, 1, idb).is_empty());
        assert!(movement(&out, 2, idb).is_empty());
        assert_eq!(movement(&out, 2, ida).len(), 10);
        assert!(
            movement(&out, 2, ida)
                .iter()
                .all(|o| matches!(o, Out::Move { .. }))
        );
    }

    #[test]
    fn pilot_position_follows_the_kart_seat() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.tick();
        assert_eq!(position(&h, a), Position::default());
        h.shortcut_to_countdown(a, &[]);
        assert_eq!(position(&h, a), seated(&h, a));
        assert_eq!(position(&h, b), seated(&h, b));
        assert!(position(&h, a).y < WAIT_Y);
        let tick = h.race().tick;
        h.race_mut().start = tick;
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        let grid = position(&h, a);
        press(&mut h, a, &["forward"]);
        let since = h.world().change_tick();
        h.ticks(5);
        assert_ne!(position(&h, a), grid);
        assert_eq!(position(&h, a), seated(&h, a));
        assert!(position_changed_since(&mut h, a, since));
        assert_eq!(position(&h, b), seated(&h, b));
        assert!(!position_changed_since(&mut h, b, since));
    }

    #[test]
    fn nothing_moves_outside_racing_and_finished_karts_stop() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let _b = h.connect(2);
        h.tick();
        press(&mut h, a, &["forward", "jump"]);
        h.ticks(5);
        assert_eq!(h.kart(a).speed, 0.0);
        assert_eq!(h.kart(a).y, WAIT_Y);
        h.shortcut_to_countdown(a, &[]);
        let grid = h.kart(a).clone();
        h.ticks(5);
        assert_eq!(h.race().phase, Phase::Countdown);
        assert_eq!((h.kart(a).x, h.kart(a).z), (grid.x, grid.z));
        let ka = h.kart_entity(a);
        let ida = network_id(&h, ka);
        h.drain();
        h.kart_mut(a).finished = Some(1);
        let tick = h.race().tick;
        h.race_mut().start = tick;
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        let since = h.world().change_tick();
        h.ticks(5);
        assert!(!position_changed_since(&mut h, ka, since));
        assert!(movement(&h.drain(), 1, ida).is_empty());
    }

    #[test]
    fn leave_and_quit_despawn_the_kart_for_every_viewer() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_countdown(a, &[]);
        h.drain();
        let (ka, kb) = (h.kart_entity(a), h.kart_entity(b));
        let (ida, idb) = (network_id(&h, ka), network_id(&h, kb));
        h.command(a, "leave", &[]);
        assert!(h.app.world().get_entity(ka).is_err());
        assert!(h.app.world().get::<Racer>(a).is_none());
        let out = h.drain();
        assert!(out.contains(&Out::Remove(1, vec![ida])));
        assert!(out.contains(&Out::Remove(2, vec![ida])));
        assert_eq!(karts(&mut h).len(), 1);
        h.tick();
        assert!(h.drain().iter().all(|o| !matches!(o, Out::Remove(..))));

        h.command(a, "join", &[]);
        h.tick();
        let out = h.drain();
        let rejoined = h.kart_entity(a);
        assert_ne!(rejoined, ka);
        assert!(h.app.world().get::<Hidden>(rejoined).is_some());
        assert!(spawns(&out, 1).is_empty() && spawns(&out, 2).is_empty());
        assert!(passengers(&out, 2).is_empty());

        h.disconnect(b);
        assert!(h.app.world().get_entity(kb).is_err());
        assert!(h.drain().contains(&Out::Remove(1, vec![idb])));
        assert_eq!(karts(&mut h).len(), 1);
        h.tick();
        assert_eq!(
            h.app.world().get::<Passengers>(rejoined).unwrap(),
            &Passengers::default()
        );
    }

    #[test]
    fn two_karts_collide_per_the_pure_simulation() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_racing(a, &[]);
        let map = h.app.world().resource::<Track>().clone();
        {
            let mut kart = h.kart_mut(a);
            kart.yaw = -PI / 2.0;
            kart.speed = 0.7;
        }
        {
            let (x, z) = (h.kart(a).x, h.kart(a).z);
            let mut kart = h.kart_mut(b);
            kart.x = x + 1.0;
            kart.z = z;
        }
        let (mut expected_a, mut expected_b) = (h.kart(a).clone(), h.kart(b).clone());
        expected_a.drive(&map);
        expected_b.drive(&map);
        collide(&mut expected_a, &mut expected_b);
        h.tick();
        for (player, expected) in [(a, expected_a), (b, expected_b)] {
            let kart = h.kart(player);
            assert_eq!(
                (kart.x, kart.z, kart.yaw, kart.speed, kart.push, kart.spin),
                (
                    expected.x,
                    expected.z,
                    expected.yaw,
                    expected.speed,
                    expected.push,
                    expected.spin
                )
            );
            assert_eq!((kart.impact, kart.contact_cooldown), (6, 10));
        }
        assert!(h.kart(a).push.0 < 0.0 && h.kart(b).push.0 > 0.0);
        assert!(h.kart(a).spin < 0.0 && h.kart(b).spin > 0.0);
    }
}
