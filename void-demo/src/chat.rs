use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{Audience, Messages, TextColor, components::PlayerName};

pub const FLASH_PERIODS: u8 = 8;
pub const HUD_COLOR: TextColor = TextColor::Gold;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Event,
    Notice,
    Good,
    Warn,
    Alert,
    Record,
}

impl Tone {
    pub const fn color(self) -> TextColor {
        match self {
            Tone::Info => TextColor::Gray,
            Tone::Event => TextColor::Gold,
            Tone::Notice => TextColor::Aqua,
            Tone::Good => TextColor::Green,
            Tone::Warn => TextColor::Red,
            Tone::Alert => TextColor::Yellow,
            Tone::Record => TextColor::LightPurple,
        }
    }
}

pub const fn podium(rank: usize) -> TextColor {
    match rank {
        1 => TextColor::Gold,
        2 => TextColor::Gray,
        3 => TextColor::rgb(0xCD7F32),
        _ => TextColor::White,
    }
}

#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Flash(pub u8);

#[derive(SystemParam)]
pub struct Chat<'w, 's> {
    messages: Messages<'w, 's>,
    names: Query<'w, 's, &'static PlayerName>,
    flashes: Query<'w, 's, &'static mut Flash>,
}

impl Chat<'_, '_> {
    pub fn name(&self, player: Entity) -> String {
        self.names
            .get(player)
            .map_or_else(|_| "Pilote".into(), |n| n.0.clone())
    }

    pub fn say(&self, player: Entity, tone: Tone, text: impl Into<String>) {
        self.messages
            .message(player, text)
            .color(tone.color())
            .send();
    }

    pub fn say_all(&self, tone: Tone, text: impl Into<String>) {
        self.messages.broadcast(text).color(tone.color()).send();
    }

    pub fn say_others(&self, player: Entity, tone: Tone, text: impl Into<String>) {
        self.messages
            .broadcast(text)
            .color(tone.color())
            .audience(Audience::custom(move |r| r.entity() != player))
            .send();
    }

    pub fn podium(&self, rank: usize, text: impl Into<String>) {
        self.messages.broadcast(text).color(podium(rank)).send();
    }

    pub fn hud(&self, player: Entity, text: impl Into<String>) {
        self.messages
            .action_bar(player, text)
            .color(HUD_COLOR)
            .send();
    }

    pub fn flash(&mut self, player: Entity, tone: Tone, text: impl Into<String>) {
        if let Ok(mut flash) = self.flashes.get_mut(player) {
            flash.0 = FLASH_PERIODS;
        }
        self.messages
            .action_bar(player, text)
            .color(tone.color())
            .send();
    }

    pub fn hud_free(&mut self, player: Entity) -> bool {
        match self.flashes.get_mut(player) {
            Ok(mut flash) if flash.0 > 0 => {
                flash.0 -= 1;
                false
            }
            _ => true,
        }
    }
}
