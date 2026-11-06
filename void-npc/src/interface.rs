//! Screens an NPC opens beside the scene. The conversation goes on after them, so an
//! interface says what to show and what to do with a click, and nothing else.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};

use bevy_ecs::prelude::Component;
use voidmc::{Inventory, Particle, Sound};
use voidmc_ui::{Button, Text};

use crate::art::ArtBuilder;
use crate::canvas::Canvas;
use crate::npc::Theme;
use crate::props::{Order, Placing, Prop};
use crate::rig::Gesture;
use crate::stage::Shot;
use crate::view::interface_click;

/// What a player has done with the NPCs, for as long as they stay connected.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    flags: BTreeSet<String>,
    counts: BTreeMap<String, u32>,
}

impl Memory {
    pub fn set(&mut self, flag: impl Into<String>) {
        self.flags.insert(flag.into());
    }

    pub fn is_set(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }

    pub fn count(&self, name: &str) -> u32 {
        self.counts.get(name).copied().unwrap_or(0)
    }

    pub fn add(&mut self, name: impl Into<String>, count: u32) {
        *self.counts.entry(name.into()).or_default() += count;
    }
}

/// A button of a [`Grid`]. Without an action it shows and does nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    label: Text,
    click: Option<String>,
}

impl Control {
    pub(crate) fn new(label: Text, click: String) -> Self {
        Self {
            label,
            click: Some(click),
        }
    }

    /// A button that reports `action` to [`Interface::click`].
    pub fn action(label: Text, action: &str) -> Self {
        Self::new(label, interface_click(action))
    }

    /// A button that does nothing, to show something greyed out.
    pub fn inert(label: Text) -> Self {
        Self { label, click: None }
    }

    pub(crate) fn button(self, width: i32) -> Button {
        let button = Button::with_label(self.label).width(width);
        match self.click {
            Some(click) => button.on_click(click),
            None => button,
        }
    }
}

/// Buttons in columns. A row is 22 pixels high, and a `None` leaves its cell empty.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Grid {
    /// The left of the first column, from the middle of the screen.
    pub x: i32,
    /// About how far from the top of the screen the first row starts: [`crate::view::grid_top`]
    /// says where it really does.
    pub top: i32,
    pub widths: Vec<i32>,
    pub rows: Vec<Vec<Option<Control>>>,
}

/// One look of an interface. `canvas` draws under the buttons, from the title of the dialog:
/// [`crate::view::from_top`] turns a y on the screen into a y on it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Screen {
    pub canvas: Canvas,
    pub grid: Grid,
}

/// What an interface can reach of the player and the NPC.
pub struct Cx<'a> {
    pub inventory: &'a mut Inventory,
    pub memory: &'a mut Memory,
    pub theme: &'a Theme,
    pub name: &'a str,
    pub(crate) sounds: Vec<Sound>,
    pub(crate) events: Vec<String>,
    pub(crate) props: Vec<Order>,
}

impl Cx<'_> {
    /// Puts `prop` in the world at `slot`, or moves what is there to `placing`, getting there
    /// in `ticks`. A slot keeps the prop it was first shown with, until [`Self::hide`].
    pub fn show(&mut self, slot: u32, prop: &Prop, placing: Placing, ticks: u16) {
        self.props.push(Order::Show {
            slot,
            model: prop.model.clone(),
            placing,
            ticks,
        });
    }

    /// Has the NPC play `gesture`.
    pub fn gesture(&mut self, gesture: Gesture) {
        self.props.push(Order::Gesture(gesture));
    }

    /// Sends `count` of `particle` flying from where `spot` says.
    pub fn burst(&mut self, particle: Particle, spot: Placing, count: i32) {
        self.props.push(Order::Burst {
            particle,
            spot,
            count,
        });
    }

    /// Takes away what is at `slot`. All props go when the interface closes.
    pub fn hide(&mut self, slot: u32) {
        self.props.push(Order::Hide(slot));
    }

    pub fn play(&mut self, sound: Sound) {
        self.sounds.push(sound);
    }

    /// Triggers [`crate::NpcEvent`] with `name`.
    pub fn emit(&mut self, name: impl Into<String>) {
        self.events.push(name.into());
    }
}

/// What happens after a click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Shows the interface again.
    Stay,
    /// Back to the answers it was opened from.
    Back,
    Goto(String),
    Leave,
}

/// A screen of an NPC. `State` lasts from when it opens to when it closes.
pub trait Interface: Send + Sync + 'static {
    type State: Default + Send + Sync + 'static;

    /// Adds what it draws to the pack, once, before the server starts.
    fn install(&mut self, _art: &mut ArtBuilder, _theme: &Theme) {}

    /// How the scene is framed while it is open.
    fn shot(&self) -> Shot {
        Shot::Portrait
    }

    fn view(&self, state: &Self::State, cx: &mut Cx) -> Screen;

    /// `action` is what the [`Control`] said. Answering [`Reply::Stay`] shows it again, which
    /// also clears the white outline a client leaves on what was clicked.
    fn click(&self, state: &mut Self::State, action: &str, cx: &mut Cx) -> Reply;

    /// Runs every tick it stays open, for what moves. Answering true shows it again.
    fn tick(&self, _state: &mut Self::State, _cx: &mut Cx) -> bool {
        false
    }
}

pub(crate) type State = Box<dyn Any + Send + Sync>;

/// [`Interface`] with its state hidden, so that NPCs can hold different ones.
pub(crate) trait AnyInterface: Send + Sync {
    fn install(&mut self, art: &mut ArtBuilder, theme: &Theme);
    fn shot(&self) -> Shot;
    fn state(&self) -> State;
    fn view(&self, state: &State, cx: &mut Cx) -> Screen;
    fn click(&self, state: &mut State, action: &str, cx: &mut Cx) -> Reply;
    fn tick(&self, state: &mut State, cx: &mut Cx) -> bool;
}

impl<T: Interface> AnyInterface for T {
    fn install(&mut self, art: &mut ArtBuilder, theme: &Theme) {
        Interface::install(self, art, theme);
    }

    fn shot(&self) -> Shot {
        Interface::shot(self)
    }

    fn state(&self) -> State {
        Box::<T::State>::default()
    }

    fn view(&self, state: &State, cx: &mut Cx) -> Screen {
        let state = state.downcast_ref().expect("a state of this interface");
        Interface::view(self, state, cx)
    }

    fn click(&self, state: &mut State, action: &str, cx: &mut Cx) -> Reply {
        let state = state.downcast_mut().expect("a state of this interface");
        Interface::click(self, state, action, cx)
    }

    fn tick(&self, state: &mut State, cx: &mut Cx) -> bool {
        let state = state.downcast_mut().expect("a state of this interface");
        Interface::tick(self, state, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_keeps_flags_and_counts() {
        let mut memory = Memory::default();
        assert!(!memory.is_set("met"));
        memory.set("met");
        assert!(memory.is_set("met"));
        memory.add("sold", 2);
        memory.add("sold", 3);
        assert_eq!(memory.count("sold"), 5);
    }
}
