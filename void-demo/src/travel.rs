use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{
    Passengers, PlayerAbilities, Teleport, TeleportOutcome,
    components::{PlayerReady, ServerControlledPosition},
    events::{PlayerTeleportEvent, PlayerToggleFlyEvent},
};

use crate::arena::WAIT_Y;
use crate::kart::Kart;
use crate::race::Chat;
use crate::vehicle::{Pilot, SEAT_HEIGHT};

pub const FLYING_SPEED: f32 = 0.07;
pub const LOBBY: (f64, f64, f64) = (0.0, WAIT_Y, 5.0);
pub const LOBBY_YAW: f32 = 180.0;
pub const LOBBY_PITCH: f32 = 15.0;
const TIMEOUT_MESSAGE: &str =
    "Chargement trop long : tu observes cette manche en vol. /join pour la suivante.";

#[derive(Component)]
pub struct Airborne;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    Lobby,
    Boarding,
}

pub fn flying() -> PlayerAbilities {
    PlayerAbilities::new()
        .invulnerable(true)
        .flying(true)
        .flying_speed(FLYING_SPEED)
}

pub fn grounded() -> PlayerAbilities {
    PlayerAbilities::new()
        .invulnerable(true)
        .flying_speed(FLYING_SPEED)
}

#[derive(SystemParam)]
pub struct Travel<'w, 's> {
    commands: Commands<'w, 's>,
    abilities: Query<'w, 's, &'static PlayerAbilities>,
    seats: Query<'w, 's, (&'static Pilot, &'static mut Passengers)>,
    ready: Query<'w, 's, Entity, With<PlayerReady>>,
}

impl Travel<'_, '_> {
    fn seat_of(&mut self, player: Entity) -> Option<Mut<'_, Passengers>> {
        self.seats
            .iter_mut()
            .find(|(pilot, _)| pilot.0 == player)
            .map(|(_, seats)| seats)
    }

    pub fn fly(&mut self, player: Entity) {
        if let Some(mut seats) = self.seat_of(player)
            && seats.0.contains(&player)
        {
            seats.remove(player);
        }
        let Ok(mut entity) = self.commands.get_entity(player) else {
            return;
        };
        entity.insert(Airborne);
        if self.abilities.get(player) != Ok(&flying()) {
            entity.insert(flying());
        }
    }

    pub fn to_lobby(&mut self, player: Entity) {
        self.fly(player);
        if let Ok(mut entity) = self.commands.get_entity(player) {
            entity.insert((
                Transfer::Lobby,
                Teleport::to(LOBBY.0, LOBBY.1, LOBBY.2).facing(LOBBY_YAW, LOBBY_PITCH),
            ));
        }
    }

    pub fn everyone_to_lobby(&mut self) {
        let players: Vec<Entity> = self.ready.iter().collect();
        for player in players {
            self.to_lobby(player);
        }
    }

    pub fn board(&mut self, player: Entity, kart: &Kart) {
        if let Ok(mut entity) = self.commands.get_entity(player) {
            entity.insert((
                Transfer::Boarding,
                ServerControlledPosition,
                Teleport::to(kart.x, kart.y + SEAT_HEIGHT, kart.z)
                    .facing(kart.yaw.to_degrees() as f32, 0.0),
            ));
        }
    }
}

pub fn arrived(
    event: On<PlayerTeleportEvent>,
    transfers: Query<&Transfer>,
    mut karts: Query<(&Pilot, &mut Kart)>,
    mut travel: Travel,
    chat: Chat,
) {
    let player = event.entity;
    let Ok(transfer) = transfers.get(player).copied() else {
        return;
    };
    travel.commands.entity(player).remove::<Transfer>();
    match (transfer, event.outcome) {
        (Transfer::Boarding, TeleportOutcome::Confirmed) => {
            let Some(mut seats) = travel.seat_of(player) else {
                return;
            };
            seats.push(player);
            travel
                .commands
                .entity(player)
                .insert((ServerControlledPosition, grounded()))
                .remove::<Airborne>();
        }
        (Transfer::Boarding, TeleportOutcome::TimedOut) => {
            if let Some(mut kart) = karts
                .iter_mut()
                .find(|(pilot, _)| pilot.0 == player)
                .map(|(_, kart)| kart)
            {
                kart.participant = false;
            }
            chat.tell(player, TIMEOUT_MESSAGE);
            travel.to_lobby(player);
        }
        (Transfer::Lobby, _) | (Transfer::Boarding, TeleportOutcome::Cancelled) => {
            travel
                .commands
                .entity(player)
                .try_remove::<ServerControlledPosition>();
        }
    }
}

pub fn keep_flying(
    event: On<PlayerToggleFlyEvent>,
    mut abilities: Query<&mut PlayerAbilities, With<Airborne>>,
) {
    if event.flying {
        return;
    }
    if let Ok(mut abilities) = abilities.get_mut(event.entity)
        && !abilities.flying
    {
        abilities.flying = true;
    }
}

#[cfg(test)]
mod tests {
    use voidmc::ChunkPos;
    use voidmc::components::{LoadedChunks, MinecraftEntityId, Position, Rotation};
    use voidmc::network::PacketEvent;
    use voidmc_protocol::serverbound;

    use super::*;
    use crate::race::tests::{Harness, Out};
    use crate::race::{Phase, Racer};

    fn abilities(out: &[Out], client: u32) -> Vec<(u8, f32, f32)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Abilities {
                    client: c,
                    flags,
                    flying_speed,
                    walking_speed,
                } if *c == client => Some((*flags, *flying_speed, *walking_speed)),
                _ => None,
            })
            .collect()
    }

    type Sync = (i32, (f64, f64, f64), f32, f32);

    fn syncs(out: &[Out], client: u32) -> Vec<Sync> {
        out.iter()
            .filter_map(|o| match o {
                Out::Sync {
                    client: c,
                    id,
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                } if *c == client => Some((*id, (*x, *y, *z), *yaw, *pitch)),
                _ => None,
            })
            .collect()
    }

    fn pings(out: &[Out], client: u32) -> Vec<i32> {
        out.iter()
            .filter_map(|o| match o {
                Out::Ping(c, id) if *c == client => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn toggle_flight(h: &mut Harness, player: Entity, flying: bool) {
        let client_id = h.client_id(player);
        h.world().trigger(PacketEvent {
            client_id,
            entity: player,
            packet: serverbound::PlayerAbilities {
                flags: if flying { 0x02 } else { 0x00 },
            },
        });
        h.world().flush();
    }

    fn transfer(h: &Harness, player: Entity) -> Option<Transfer> {
        h.app.world().get::<Transfer>(player).copied()
    }

    fn controlled(h: &Harness, player: Entity) -> bool {
        h.app
            .world()
            .get::<ServerControlledPosition>(player)
            .is_some()
    }

    fn airborne(h: &Harness, player: Entity) -> bool {
        h.app.world().get::<Airborne>(player).is_some()
    }

    fn seats(h: &Harness, player: Entity) -> Passengers {
        let kart = h.app.world().get::<Racer>(player).unwrap().kart;
        h.app.world().get::<Passengers>(kart).unwrap().clone()
    }

    fn lobby_chunks() -> Vec<ChunkPos> {
        ChunkPos::from_block(LOBBY.0, LOBBY.2).chunks_in_radius(Teleport::DEFAULT_PRELOAD_RADIUS)
    }

    #[test]
    fn joining_grants_flight_and_launching_sends_everyone_to_the_lobby() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        assert!(airborne(&h, a));
        assert_eq!(h.app.world().get::<PlayerAbilities>(a), Some(&flying()));
        assert!(transfer(&h, a).is_none());
        h.tick();
        let out = h.drain();
        assert_eq!(abilities(&out, 1), vec![(0x07, FLYING_SPEED, 0.1)]);
        h.ticks(5);
        assert!(abilities(&h.drain(), 1).is_empty());

        let b = h.connect(2);
        h.command(b, "leave", &[]);
        h.settle_transfers();
        h.drain();
        h.command(a, "race", &[]);
        for player in [a, b] {
            assert_eq!(transfer(&h, player), Some(Transfer::Lobby));
            assert!(controlled(&h, player));
            let teleport = h.app.world().get::<Teleport>(player).unwrap();
            assert_eq!((teleport.x, teleport.y, teleport.z), LOBBY);
            assert_eq!(
                (teleport.yaw, teleport.pitch),
                (Some(LOBBY_YAW), Some(LOBBY_PITCH))
            );
            assert_eq!(
                h.app.world().get::<Position>(player).unwrap(),
                &Position {
                    x: LOBBY.0,
                    y: LOBBY.1,
                    z: LOBBY.2
                }
            );
        }
        let out = h.settle_transfers();
        assert!(abilities(&out, 1).is_empty() && abilities(&out, 2).is_empty());
        for player in [a, b] {
            assert!(transfer(&h, player).is_none());
            assert!(!controlled(&h, player));
            assert!(airborne(&h, player));
        }
        assert_eq!(seats(&h, a), Passengers::default());
    }

    #[test]
    fn boarding_waits_for_chunks_matching_pong_and_confirmation_then_mounts_after_spawn() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.tick();
        h.command(a, "race", &[]);
        h.settle_transfers();
        h.world().get_mut::<LoadedChunks>(a).unwrap().0 = lobby_chunks().into_iter().collect();
        h.tick();
        h.drain();
        h.race_mut().pending.clear();
        h.tick();
        assert_eq!(h.race().phase, Phase::Loading);
        let kart = h.kart(a).clone();
        let seat = (kart.x, kart.y + SEAT_HEIGHT, kart.z);
        assert_eq!(transfer(&h, a), Some(Transfer::Boarding));
        assert!(controlled(&h, a) && airborne(&h, a));
        let teleport = h.app.world().get::<Teleport>(a).unwrap().clone();
        assert_eq!((teleport.x, teleport.y, teleport.z), seat);
        assert_eq!(
            (teleport.yaw, teleport.pitch),
            (Some(kart.yaw.to_degrees() as f32), Some(0.0))
        );
        assert_eq!(teleport.chunk_budget, Some(2));
        assert_eq!(h.app.world().get::<Rotation>(a).unwrap().pitch, 0.0);
        let kart_id = h
            .app
            .world()
            .get::<MinecraftEntityId>(h.kart_entity(a))
            .unwrap()
            .0;
        let player_id = h.app.world().get::<MinecraftEntityId>(a).unwrap().0;
        let mut stream = h.drain();

        let missing = ChunkPos::from_block(seat.0, seat.2);
        let around: Vec<ChunkPos> = missing
            .chunks_in_radius(2)
            .into_iter()
            .filter(|c| *c != missing)
            .collect();
        h.world()
            .get_mut::<LoadedChunks>(a)
            .unwrap()
            .0
            .extend(around);
        h.ticks(5);
        let out = h.drain();
        assert!(pings(&out, 1).is_empty() && syncs(&out, 1).is_empty());
        assert!(
            !out.iter()
                .any(|o| matches!(o, Out::Spawn { client: 1, id, .. } if *id == kart_id))
        );
        assert_eq!(h.race().phase, Phase::Loading);
        stream.extend(out);

        h.world()
            .get_mut::<LoadedChunks>(a)
            .unwrap()
            .0
            .insert(missing);
        h.tick();
        let out = h.drain();
        let ping = pings(&out, 1);
        assert_eq!(ping.len(), 1);
        assert!(
            out.iter()
                .any(|o| matches!(o, Out::Spawn { client: 1, id, .. } if *id == kart_id))
        );
        stream.extend(out);
        h.pong(a, ping[0] + 1);
        h.tick();
        let out = h.drain();
        assert!(syncs(&out, 1).is_empty());
        stream.extend(out);
        h.pong(a, ping[0]);
        let out = h.drain();
        let sync = syncs(&out, 1);
        assert_eq!(sync.len(), 1);
        assert_eq!(sync[0].1, seat);
        assert_eq!((sync[0].2, sync[0].3), (kart.yaw.to_degrees() as f32, 0.0));
        stream.extend(out);
        h.confirm(a, sync[0].0 + 1);
        h.tick();
        assert_eq!(h.race().phase, Phase::Loading);
        assert_eq!(seats(&h, a), Passengers::default());
        stream.extend(h.drain());

        h.confirm(a, sync[0].0);
        h.tick();
        assert_eq!(h.race().phase, Phase::Countdown);
        assert_eq!(seats(&h, a), Passengers::new([a]));
        assert!(transfer(&h, a).is_none());
        assert!(controlled(&h, a) && !airborne(&h, a));
        assert_eq!(h.app.world().get::<PlayerAbilities>(a), Some(&grounded()));
        assert_eq!(
            h.app.world().get::<Position>(a).unwrap(),
            &Position {
                x: seat.0,
                y: seat.1,
                z: seat.2
            }
        );
        let out = h.drain();
        assert_eq!(abilities(&out, 1), vec![(0x01, FLYING_SPEED, 0.1)]);
        assert!(out.iter().any(|o| matches!(
            o,
            Out::Chat { client: 1, overlay: false, text, .. }
                if text == "[Alpine Rush] Tous les pilotes sont charges ! Depart dans 5 secondes !"
        )));
        stream.extend(out);
        let spawn = stream
            .iter()
            .position(|o| matches!(o, Out::Spawn { client: 1, id, .. } if *id == kart_id))
            .unwrap();
        let mount = stream
            .iter()
            .position(|o| *o == Out::Passengers(1, kart_id, vec![player_id]))
            .unwrap();
        assert!(spawn < mount);
        assert_eq!(
            stream
                .iter()
                .filter(|o| matches!(o, Out::Passengers(1, ..)))
                .count(),
            1
        );
    }

    #[test]
    fn a_slow_pilot_is_released_on_timeout_and_watches_the_round_in_flight() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.command(a, "race", &[]);
        h.settle_transfers();
        h.race_mut().pending.clear();
        h.tick();
        assert_eq!(h.race().phase, Phase::Loading);
        h.settle_transfers_of(&[b]);
        assert_eq!(h.race().phase, Phase::Loading);
        let slow = h
            .app
            .world()
            .get::<Teleport>(a)
            .unwrap()
            .clone()
            .timeout_ticks(3);
        h.world().entity_mut(a).insert(slow);
        h.drain();
        h.ticks(3);
        assert_eq!(transfer(&h, a), Some(Transfer::Boarding));
        assert_eq!(h.race().phase, Phase::Loading);
        h.tick();
        assert_eq!(transfer(&h, a), Some(Transfer::Lobby));
        assert!(!h.kart(a).participant);
        assert!(h.kart(b).participant);
        assert!(airborne(&h, a) && controlled(&h, a));
        assert_eq!(seats(&h, a), Passengers::default());
        assert_eq!(h.race().phase, Phase::Countdown);
        let out = h.drain();
        assert_eq!(syncs(&out, 1).len(), 1);
        assert!(abilities(&out, 1).is_empty());
        let texts: Vec<String> = out
            .iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: 1,
                    overlay: false,
                    text,
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec![
                format!("[Alpine Rush] {TIMEOUT_MESSAGE}"),
                "[Alpine Rush] Tous les pilotes sont charges ! Depart dans 5 secondes !"
                    .to_string(),
            ]
        );
        h.settle_transfers();
        assert!(!controlled(&h, a) && airborne(&h, a));
        assert!(controlled(&h, b) && !airborne(&h, b));
        assert_eq!(seats(&h, b), Passengers::new([b]));
    }

    #[test]
    fn quitting_or_cancelling_mid_transfer_cleans_up_without_blocking_the_start() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.command(a, "race", &[]);
        h.settle_transfers();
        h.race_mut().pending.clear();
        h.tick();
        assert_eq!(h.race().phase, Phase::Loading);
        let kart = h.kart_entity(a);
        h.disconnect(a);
        assert!(h.app.world().get_entity(kart).is_err());
        assert_eq!(h.race().roster, vec![b, c]);
        h.ticks(2);
        assert_eq!(h.race().phase, Phase::Loading);
        h.world().entity_mut(c).remove::<Teleport>();
        h.tick();
        assert!(transfer(&h, c).is_none());
        assert!(!controlled(&h, c) && airborne(&h, c));
        assert_eq!(seats(&h, c), Passengers::default());
        assert_eq!(h.race().phase, Phase::Loading);
        h.settle_transfers();
        assert_eq!(h.race().phase, Phase::Countdown);
        assert_eq!(seats(&h, b), Passengers::new([b]));
    }

    #[test]
    fn flight_is_regranted_only_while_airborne() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.tick();
        h.drain();
        toggle_flight(&mut h, a, false);
        assert!(h.app.world().get::<PlayerAbilities>(a).unwrap().flying);
        h.tick();
        assert_eq!(abilities(&h.drain(), 1), vec![(0x07, FLYING_SPEED, 0.1)]);
        toggle_flight(&mut h, a, true);
        h.tick();
        assert!(abilities(&h.drain(), 1).is_empty());

        h.shortcut_to_countdown(a, &[]);
        h.drain();
        toggle_flight(&mut h, a, true);
        h.tick();
        let abilities_sent = abilities(&h.drain(), 1);
        assert_eq!(abilities_sent, vec![(0x01, FLYING_SPEED, 0.1)]);
        assert!(!h.app.world().get::<PlayerAbilities>(a).unwrap().flying);
    }

    #[test]
    fn leaving_the_grid_flies_back_to_the_lobby_and_rejoining_resends_nothing() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.shortcut_to_countdown(a, &[]);
        h.drain();
        h.command(a, "leave", &[]);
        assert_eq!(transfer(&h, a), Some(Transfer::Lobby));
        assert!(airborne(&h, a));
        h.tick();
        let out = h.drain();
        assert_eq!(abilities(&out, 1), vec![(0x07, FLYING_SPEED, 0.1)]);
        let tick = h.race().tick;
        h.ticks((5 - tick % 5) as usize);
        let overlays: Vec<String> = h
            .drain()
            .into_iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: 1,
                    overlay: true,
                    text,
                    ..
                } => Some(text),
                _ => None,
            })
            .collect();
        assert!(overlays.is_empty());
        h.command(a, "join", &[]);
        assert_eq!(seats(&h, a), Passengers::default());
        h.tick();
        assert!(abilities(&h.drain(), 1).is_empty());
        h.settle_transfers();
        assert!(!controlled(&h, a) && airborne(&h, a));
        assert!(controlled(&h, b) && !airborne(&h, b));
    }

    #[test]
    fn a_lobby_transfer_over_an_unfinished_boarding_still_releases_control() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.command(a, "race", &[]);
        h.settle_transfers();
        h.race_mut().pending.clear();
        h.tick();
        assert_eq!(transfer(&h, a), Some(Transfer::Boarding));
        h.command(a, "leave", &[]);
        assert_eq!(transfer(&h, a), Some(Transfer::Lobby));
        h.settle_transfers();
        assert!(!controlled(&h, a) && airborne(&h, a));
        assert_eq!(h.race().phase, Phase::Countdown);
        assert_eq!(seats(&h, b), Passengers::new([b]));
        assert!(controlled(&h, b));

        let tick = h.race().tick;
        h.race_mut().start = tick;
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        h.kart_mut(b).next_gate = crate::race::LAPS * crate::track::GATES + 1;
        h.tick();
        assert_eq!(h.race().phase, Phase::Destroying);
        assert_eq!(transfer(&h, b), Some(Transfer::Lobby));
        assert_eq!(seats(&h, b), Passengers::default());
        assert!(airborne(&h, b));
        h.tick();
        assert_eq!(abilities(&h.drain(), 2), vec![(0x07, FLYING_SPEED, 0.1)]);
        h.settle_transfers();
        assert!(!controlled(&h, b));
        assert_eq!(
            h.app.world().get::<Position>(b).unwrap(),
            &Position {
                x: LOBBY.0,
                y: LOBBY.1,
                z: LOBBY.2
            }
        );
    }
}
