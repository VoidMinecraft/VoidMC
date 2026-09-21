use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{Sound, SoundSource, Sounds, components::PlayerReady, world::DimensionId};

use crate::kart::{PowerUp, Strike};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Missile,
    Lightning,
    Shockwave,
    Banana,
    Ice,
    Fireball,
}

impl From<Strike> for Hit {
    fn from(strike: Strike) -> Self {
        match strike {
            Strike::Missile => Hit::Missile,
            Strike::Lightning => Hit::Lightning,
            Strike::Shockwave { .. } => Hit::Shockwave,
            Strike::Banana => Hit::Banana,
            Strike::Ice => Hit::Ice,
            Strike::Fireball => Hit::Fireball,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Beep,
    Go,
    Start,
    Lap,
    FinalLap,
    Finish,
    Record,
    Pickup,
    Activate(PowerUp),
    Hit(Hit),
    Shielded,
    Bump,
    Portal,
    Launch,
}

impl Cue {
    pub const ALL: [Cue; 27] = [
        Cue::Beep,
        Cue::Go,
        Cue::Start,
        Cue::Lap,
        Cue::FinalLap,
        Cue::Finish,
        Cue::Record,
        Cue::Pickup,
        Cue::Activate(PowerUp::Turbo),
        Cue::Activate(PowerUp::Shield),
        Cue::Activate(PowerUp::Banana),
        Cue::Activate(PowerUp::Missile),
        Cue::Activate(PowerUp::Shockwave),
        Cue::Activate(PowerUp::Ice),
        Cue::Activate(PowerUp::Lightning),
        Cue::Activate(PowerUp::Recharge),
        Cue::Activate(PowerUp::Fireball),
        Cue::Hit(Hit::Missile),
        Cue::Hit(Hit::Lightning),
        Cue::Hit(Hit::Shockwave),
        Cue::Hit(Hit::Banana),
        Cue::Hit(Hit::Ice),
        Cue::Hit(Hit::Fireball),
        Cue::Shielded,
        Cue::Bump,
        Cue::Portal,
        Cue::Launch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Cue::Beep | Cue::Go => "block.note_block.pling",
            Cue::Start => "entity.firework_rocket.large_blast",
            Cue::Lap => "block.note_block.bell",
            Cue::FinalLap => "block.bell.use",
            Cue::Finish => "entity.player.levelup",
            Cue::Record => "ui.toast.challenge_complete",
            Cue::Pickup => "entity.item.pickup",
            Cue::Activate(PowerUp::Turbo) => "entity.firework_rocket.launch",
            Cue::Activate(PowerUp::Shield) => "block.beacon.activate",
            Cue::Activate(PowerUp::Banana) => "block.slime_block.place",
            Cue::Activate(PowerUp::Ice) => "block.glass.place",
            Cue::Activate(PowerUp::Missile) => "entity.wither.shoot",
            Cue::Activate(PowerUp::Shockwave) => "entity.generic.explode",
            Cue::Activate(PowerUp::Lightning) => "entity.lightning_bolt.thunder",
            Cue::Activate(PowerUp::Recharge) => "block.respawn_anchor.charge",
            Cue::Activate(PowerUp::Fireball) => "entity.blaze.ambient",
            Cue::Hit(Hit::Missile) | Cue::Hit(Hit::Fireball) => "entity.generic.explode",
            Cue::Hit(Hit::Lightning) | Cue::Hit(Hit::Shockwave) => "entity.player.hurt",
            Cue::Hit(Hit::Banana) => "entity.slime.squish",
            Cue::Hit(Hit::Ice) => "block.glass.break",
            Cue::Shielded => "item.shield.block",
            Cue::Bump => "block.anvil.land",
            Cue::Portal => "block.portal.trigger",
            Cue::Launch => "entity.ghast.shoot",
        }
    }

    fn volume(self) -> f32 {
        match self {
            Cue::Go | Cue::FinalLap => 1.2,
            Cue::Bump => 0.5,
            Cue::Portal => 0.6,
            Cue::Activate(PowerUp::Lightning)
            | Cue::Hit(Hit::Missile)
            | Cue::Hit(Hit::Fireball) => 0.8,
            _ => 1.0,
        }
    }

    fn pitch(self) -> f32 {
        match self {
            Cue::Beep => 1.0,
            Cue::Go => 2.0,
            Cue::Lap => 1.5,
            Cue::Pickup => 1.2,
            Cue::Bump => 1.6,
            Cue::Activate(PowerUp::Recharge) | Cue::Record => 1.3,
            Cue::Hit(Hit::Shockwave) => 0.8,
            _ => 1.0,
        }
    }

    fn ui(self) -> bool {
        matches!(
            self,
            Cue::Beep
                | Cue::Go
                | Cue::Start
                | Cue::Lap
                | Cue::FinalLap
                | Cue::Finish
                | Cue::Record
                | Cue::Pickup
                | Cue::Portal
        )
    }

    pub fn sound(self) -> Sound {
        Sound::new(self.name())
            .category(if self.ui() {
                SoundSource::Ui
            } else {
                SoundSource::Players
            })
            .volume(self.volume())
            .pitch(self.pitch())
    }
}

#[derive(SystemParam)]
pub struct Audio<'w, 's> {
    sounds: Sounds<'w, 's>,
    ready: Query<'w, 's, Entity, With<PlayerReady>>,
}

impl Audio<'_, '_> {
    pub fn ui(&self, player: Entity, cue: Cue) {
        self.sounds.play_to(player, cue.sound());
    }

    pub fn ui_at(&self, player: Entity, cue: Cue, position: (f64, f64, f64)) {
        self.sounds
            .play_to(player, cue.sound().at(DimensionId::Overworld, position));
    }

    pub fn everyone(&self, cue: Cue) {
        for player in &self.ready {
            self.ui(player, cue);
        }
    }

    pub fn at(&self, emitter: Entity, cue: Cue) {
        self.sounds.play(cue.sound().from_entity(emitter));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cue_names_a_registered_sound_event() {
        for cue in Cue::ALL {
            assert!(
                voidmc::sounds::resolve(cue.name()).is_some(),
                "{cue:?} → {}",
                cue.name()
            );
        }
    }

    #[test]
    fn countdown_beeps_are_low_and_go_is_high() {
        assert_eq!(Cue::Beep.name(), Cue::Go.name());
        assert!(Cue::Beep.pitch() < Cue::Go.pitch());
        assert_eq!(Cue::Go.pitch(), 2.0);
    }
}
