//! NPCs a player talks to on the vanilla client, with no mod. A right click turns the world
//! into a scene: the view goes over the player's shoulder, the NPC speaks letter by letter,
//! the player answers in a native dialog, and an NPC can open an interface of its own.
//!
//! An NPC and what it says are plain data:
//!
//! ```no_run
//! use voidmc_npc::{Conversation, Node, Npc, Theme};
//!
//! let talk = Conversation::new("hello")
//!     .node(
//!         Node::new("hello")
//!             .say("Bonjour, voyageur.")
//!             .choice("Qui es-tu ?", "who")
//!             .leave("Au revoir"),
//!     )
//!     .node(Node::new("who").say("Personne d'important.").then("hello"));
//! let guard = Npc::new("guard", "Garde").at(4.5, 65.0, 2.5).theme(Theme::EMBER).talk(talk);
//! ```
//!
//! [`Npcs`] gathers them, adds what they need to the pack and goes in the app as
//! [`NpcPlugin`]. An interface is anything that implements [`Interface`], as [`Shop`] does.
//! In a scene, an NPC with a picture for a skin moves: it speaks with its hands and plays a
//! [`Gesture`] when a line asks for one. An interface can also put a [`Prop`] in the world and
//! animate it from [`Interface::tick`].

pub mod art;
pub mod canvas;
pub mod conversation;
pub mod font;
pub mod interface;
pub mod npc;
pub mod plugin;
pub mod props;
pub mod rig;
pub mod shop;
pub mod stage;
pub mod view;

pub use art::{Art, Icon, Picture};
pub use canvas::Canvas;
pub use conversation::{Choice, Conversation, Line, Node, Then};
pub use interface::{Control, Cx, Interface, Memory, Reply, Screen};
pub use npc::{Npc, Npcs, Theme, Voice};
pub use plugin::{NpcEvent, NpcPlugin, Talking};
pub use props::{Placing, Prop};
pub use rig::Gesture;
pub use shop::{Offer, Shop};
pub use stage::Shot;
