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
    PlayerAbilities::new().invulnerable(true)
}

#[derive(SystemParam)]
pub struct Travel<'w, 's> {
    commands: Commands<'w, 's>,
    abilities: Query<'w, 's, &'static PlayerAbilities>,
    seats: Query<'w, 's, (&'static Pilot, &'static mut Passengers)>,
    ready: Query<'w, 's, Entity, With<PlayerReady>>,
}

impl Travel<'_, '_> {
    fn seat_of(&mut self, player: Entity) -> Option<(Entity, Mut<'_, Passengers>)> {
        self.seats
            .iter_mut()
            .find(|(pilot, _)| pilot.0 == player)
            .map(|(pilot, seats)| (pilot.0, seats))
    }

    pub fn fly(&mut self, player: Entity) {
        if let Some((_, mut seats)) = self.seat_of(player)
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
        (_, TeleportOutcome::Cancelled) => {}
        (Transfer::Boarding, TeleportOutcome::Confirmed) => {
            let Some((_, mut seats)) = travel.seat_of(player) else {
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
        (Transfer::Lobby, _) => {
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
