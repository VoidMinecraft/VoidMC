//! An NPC as data, and the set of them a server runs with.

use std::collections::BTreeMap;
use std::sync::Arc;

use voidmc::components::Position;
use voidmc_protocol::clientbound::ResolvableProfile;
use voidmc_ui::{Image, ResourcePack};

use crate::art::{Art, ArtBuilder};
use crate::conversation::Conversation;
use crate::font::NAMESPACE;
use crate::interface::{AnyInterface, Interface};
use crate::plugin::NpcPlugin;
use crate::rig::{self, Figure};
use crate::stage::Shot;

/// The colours of an NPC's scenes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub accent: u32,
}

impl Theme {
    pub const EMERALD: Self = Self { accent: 0x4fe3a0 };
    pub const EMBER: Self = Self { accent: 0xff9a4a };
    pub const AMETHYST: Self = Self { accent: 0xc48cff };

    /// The accent gone dark, for what text sits on.
    pub fn shade(&self) -> u32 {
        let dim = |shift: u32| (((self.accent >> shift) & 0xff) / 6) << shift;
        dim(16) | dim(8) | dim(0)
    }
}

/// What an NPC sounds like as its words come.
#[derive(Debug, Clone, PartialEq)]
pub struct Voice {
    pub sound: String,
    pub pitch: f32,
    pub volume: f32,
    /// A sound every this many letters.
    pub every: usize,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            sound: "block.note_block.hat".to_string(),
            pitch: 1.4,
            volume: 0.25,
            every: 2,
        }
    }
}

#[derive(Clone)]
enum Skin {
    Picture(Image, bool),
    Profile(ResolvableProfile),
}

/// Someone a player can talk to.
pub struct Npc {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) at: Position,
    pub(crate) yaw: f32,
    pub(crate) theme: Theme,
    pub(crate) voice: Voice,
    pub(crate) shot: Shot,
    pub(crate) talk: Conversation,
    pub(crate) interfaces: BTreeMap<String, Box<dyn AnyInterface>>,
    skin: Skin,
}

impl Npc {
    /// `id` names it in the pack: letters, digits and underscores.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            at: Position::default(),
            yaw: 0.0,
            theme: Theme::EMERALD,
            voice: Voice::default(),
            shot: Shot::default(),
            talk: Conversation::default(),
            interfaces: BTreeMap::new(),
            skin: Skin::Profile(ResolvableProfile::default()),
        }
    }

    pub fn at(mut self, x: f64, y: f64, z: f64) -> Self {
        self.at = Position { x, y, z };
        self
    }

    pub fn facing(mut self, yaw: f32) -> Self {
        self.yaw = yaw;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn voice(mut self, voice: Voice) -> Self {
        self.voice = voice;
        self
    }

    /// How its conversations are framed.
    pub fn shot(mut self, shot: Shot) -> Self {
        self.shot = shot;
        self
    }

    pub fn talk(mut self, talk: Conversation) -> Self {
        self.talk = talk;
        self
    }

    /// A 64 by 64 skin, which goes in the pack.
    pub fn skin(mut self, skin: Image, slim: bool) -> Self {
        self.skin = Skin::Picture(skin, slim);
        self
    }

    /// A skin the client finds by itself.
    pub fn profile(mut self, profile: ResolvableProfile) -> Self {
        self.skin = Skin::Profile(profile);
        self
    }

    /// Adds what a choice can open under `name`.
    pub fn interface(mut self, name: impl Into<String>, interface: impl Interface) -> Self {
        self.interfaces.insert(name.into(), Box::new(interface));
        self
    }

    pub(crate) fn profile_shown(&self) -> ResolvableProfile {
        match &self.skin {
            Skin::Picture(_, slim) => {
                ResolvableProfile::textured(format!("{NAMESPACE}:entity/{}", self.id), *slim)
            }
            Skin::Profile(profile) => profile.clone(),
        }
    }

    /// What it is made of in a scene. A skin the client finds by itself cannot be cut up, and
    /// the NPC stays a mannequin that does not move.
    pub(crate) fn figure(&self) -> Option<Figure> {
        match &self.skin {
            Skin::Picture(_, slim) => Some(Figure::new(&self.id, *slim)),
            Skin::Profile(_) => None,
        }
    }

    fn problems(&self) -> Vec<String> {
        self.talk
            .problems(|name| self.interfaces.contains_key(name))
            .into_iter()
            .map(|problem| format!("{}: {problem}", self.id))
            .collect()
    }
}

/// All the NPCs of a server.
#[derive(Default)]
pub struct Npcs {
    all: Vec<Npc>,
}

impl Npcs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, npc: Npc) -> Self {
        self.all.push(npc);
        self
    }

    /// Adds all they need to `pack`, and gives what goes in the app.
    ///
    /// # Panics
    ///
    /// If a conversation leads nowhere or says more than fits: better at startup than in
    /// front of a player.
    pub fn build(mut self, pack: &mut ResourcePack) -> NpcPlugin {
        let problems: Vec<String> = self.all.iter().flat_map(Npc::problems).collect();
        assert!(
            problems.is_empty(),
            "NPCs not ready:\n{}",
            problems.join("\n")
        );
        let mut art = ArtBuilder::default();
        for npc in &mut self.all {
            if let Skin::Picture(image, slim) = &npc.skin {
                let file = format!("assets/{NAMESPACE}/textures/entity/{}.png", npc.id);
                pack.add(file, image.to_png());
                pack.add(rig::texture_file(&npc.id), image.to_png());
                for (file, bytes) in rig::files(&npc.id, *slim) {
                    pack.add(file, bytes);
                }
            }
            let theme = npc.theme;
            for interface in npc.interfaces.values_mut() {
                interface.install(&mut art, &theme);
            }
        }
        let art = Art::new(art, pack);
        NpcPlugin::new(Arc::new(Cast {
            npcs: self.all,
            art,
        }))
    }
}

/// What the plugin runs on, once it is all built.
pub(crate) struct Cast {
    pub npcs: Vec<Npc>,
    pub art: Art,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, Shop};

    fn sound_npc() -> Npc {
        Npc::new("guard", "Garde")
            .talk(Conversation::new("a").node(Node::new("a").say("x").open("buy", "shop")))
            .interface("shop", Shop::new("Boutique", "emerald", "émeraudes"))
    }

    #[test]
    fn a_sound_cast_builds() {
        let mut pack = ResourcePack::new("test");
        Npcs::new().with(sound_npc()).build(&mut pack);
    }

    #[test]
    #[should_panic(expected = "guard")]
    fn a_broken_conversation_stops_the_server_from_starting() {
        let npc = Npc::new("guard", "Garde").talk(Conversation::new("nowhere"));
        Npcs::new().with(npc).build(&mut ResourcePack::new("test"));
    }

    #[test]
    fn a_picture_skin_goes_in_the_pack() {
        let mut pack = ResourcePack::new("test");
        let npc = sound_npc().skin(Image::new(64, 64), false);
        Npcs::new().with(npc).build(&mut pack);
        assert!(pack.contains("assets/voidnpc/textures/entity/guard.png"));
    }
}
