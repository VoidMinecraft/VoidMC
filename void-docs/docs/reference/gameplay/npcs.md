# NPCs & Conversations

`voidmc-npc` gives a server NPCs a player talks to on the vanilla client, with no
mod. A right click turns the world into a scene: the view moves over the player's
shoulder, black bars close in, the NPC speaks letter by letter and moves its hands,
and the player answers in a native dialog. An NPC can also open an interface of its
own, such as a shop. `voidmc-npc-demo` is a complete example: run it with
`cargo run -p voidmc-npc-demo`.

## Describing an NPC

An NPC and what it says are plain data. A conversation is a set of nodes: lines to
say, then either answers, another node, or the end.

```rust
use voidmc_npc::{Conversation, Gesture, Line, Node, Npc, Npcs, Offer, Shop, Theme};

let talk = Conversation::new("first")
    .opening("met", "again")
    .node(
        Node::new("first")
            .sets("met")
            .line(Line::new("Bonjour !").gesture(Gesture::Wave))
            .then("offer"),
    )
    .node(Node::new("again").say("Te revoilà !").then("offer"))
    .node(
        Node::new("offer")
            .say("Qu'est-ce qui t'amène ?")
            .choice("Qui es-tu ?", "who")
            .open("Je veux acheter", "shop")
            .leave("Rien, à plus"),
    )
    .node(Node::new("who").say("Une marchande.").then("offer"));

let shop = Shop::new("BOUTIQUE", "emerald", "émeraudes")
    .offer(Offer::new("golden_apple", "Pomme dorée", 8))
    .offer(Offer::new("totem_of_undying", "Totem", 50).limit(1));

let merchant = Npc::new("merchant", "Marchande")
    .at(0.5, 65.0, 3.5)
    .theme(Theme::EMERALD)
    .talk(talk)
    .interface("shop", shop);

let plugin = Npcs::new().with(merchant).build(&mut pack);
```

`Npcs::build` adds everything to the resource pack and returns the plugin, which goes in the app after `voidmc_ui::UiPlugin`. A
conversation that points at a node or an interface that does not exist stops the
server from starting, with the list of what is wrong.

| On a | Call | Does |
| --- | --- | --- |
| `Conversation` | `opening(flag, node)` | Starts at `node` for who has `flag` |
| `Node` | `say`, `line` | Adds a line. `Line` also takes a `shot` and a `gesture` |
| `Node` | `choice`, `open`, `leave`, `answer` | Adds an answer |
| `Node` | `then`, `sets` | Goes on after the last line; sets a flag on arrival |
| `Choice` | `needs`, `unless` | Shows the answer depending on flags |
| `Choice` | `sets`, `emits` | Sets a flag; triggers `NpcEvent` for your own systems |

Flags and counts live in `Memory`, a component of the player that lasts as long as
the connection. To keep them, save and restore that component yourself.

## Shots and gestures

`Shot` frames the scene: `Shoulder`, `Portrait` (the NPC on the left, which leaves
the right to an interface), `Face` (its head at the top of the screen) or `Custom`.
The view glides from one to the next.

An NPC with a picture for a skin (`Npc::skin`) moves in a scene. For the player
talking to it, its mannequin is replaced by a figure of four item displays cut from
the skin; other players keep seeing the mannequin. It breathes, moves its hands while
words appear, and plays a `Gesture` (`Wave`, `Present`, `Shrug`, `Nod`, `Think`) when
a line asks for one. An NPC whose skin comes from `Npc::profile` cannot be cut up, as
the server does not have the picture: it stays still.

## Interfaces

An interface says what to show, what a click does, and what moves.

```rust
impl Interface for Cards {
    type State = Table;

    fn install(&mut self, art: &mut ArtBuilder, theme: &Theme) { /* add to the pack */ }
    fn shot(&self) -> Shot { Shot::Face }
    fn view(&self, table: &Table, cx: &mut Cx) -> Screen { /* canvas and buttons */ }
    fn click(&self, table: &mut Table, action: &str, cx: &mut Cx) -> Reply { Reply::Stay }
    fn tick(&self, table: &mut Table, cx: &mut Cx) -> bool { false }
}
```

`Screen` is a `Canvas` of text and pictures, and a `Grid` of buttons placed from the
middle of the screen. `Cx` reaches the player's inventory and memory, plays sounds,
and emits events.

### Props

A `Prop` is a model of the pack shown in the world for this player only, from
`ArtBuilder::card`. `Cx::show` puts it at a `Placing`, measured from the feet of the
NPC, and moves it there in so many ticks: the client glides between two keyframes at
the speed of the screen. A turn takes the short way between two keyframes, so keep it
under half a lap per keyframe. `Cx::burst` sends particles from a place, and
`Cx::gesture` has the NPC play a gesture. Props go when the interface closes.

`void-npc-demo/src/cards.rs` deals three cards from the hands of a fortune teller and
spins the chosen one around.

## What the pack changes

The pack is global to the client while it is loaded: it takes the blur away from
every screen, restyles buttons so that they let the world through, and hides the
warning button of dialogs.

## Limits

- Scenes need a window of about 1280×720 at GUI scale 3, or an equivalent space.
- The mouse wheel scrolls the dialog and moves the buttons a little.
- A canvas with icons sits a few pixels off the middle, as the client trims them.
