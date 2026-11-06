//! Who stands in the demo, and what they say.

use voidmc_npc::{
    Choice, Conversation, Gesture, Line, Node, Npc, Npcs, Offer, Shop, Theme, Then, Voice,
};

use crate::cards::Cards;
use crate::skins;

/// The ground of the demo, where feet stand.
pub const GROUND: f64 = 65.0;

fn merchant() -> Npc {
    let talk = Conversation::new("first")
        .opening("met_merchant", "again")
        .node(
            Node::new("first")
                .sets("met_merchant")
                .line(Line::new("hmm..?").gesture(Gesture::Wave))
                .say("Tiens, un nouveau visage par ici.")
                .then("offer"),
        )
        .node(
            Node::new("again")
                .line(Line::new("Te revoilà !").gesture(Gesture::Wave))
                .then("offer"),
        )
        .node(
            Node::new("offer")
                .say("Qu'est-ce qui t'amène ?")
                .choice("Qui es-tu ?", "who")
                .open("Je veux acheter", "shop")
                .leave("Rien, à plus"),
        )
        .node(
            Node::new("who")
                .line(
                    Line::new("Une marchande. Je vends ce que les autres oublient.")
                        .gesture(Gesture::Present),
                )
                .line(Line::new("Et je n'oublie jamais un client.").gesture(Gesture::Nod))
                .then("offer"),
        );
    let shop = Shop::new("BOUTIQUE", "emerald", "émeraudes")
        .offer(Offer::new("totem_of_undying", "Totem d'immortalité", 50).limit(1))
        .offer(Offer::new("golden_apple", "Pomme dorée", 8))
        .offer(Offer::new("arrow", "Flèches ×16", 2).bundle(16));
    Npc::new("merchant", "Marchande")
        .at(0.5, GROUND, 3.5)
        .facing(180.0)
        .theme(Theme::EMERALD)
        .skin(skins::merchant(), false)
        .talk(talk)
        .interface("shop", shop)
}

fn fortune_teller() -> Npc {
    let talk = Conversation::new("hello").node(
        Node::new("hello")
            .line(Line::new("Les cartes t'attendaient.").gesture(Gesture::Think))
            .answer(
                Choice::new("Tire-moi les cartes", Then::Open("cards".to_string()))
                    .unless("fortune_told"),
            )
            .answer(
                Choice::new("Encore une fois", Then::Open("cards".to_string()))
                    .needs("fortune_told"),
            )
            .leave("Une autre fois"),
    );
    Npc::new("fortune_teller", "Cartomancienne")
        .at(-6.5, GROUND, 2.5)
        .facing(90.0)
        .theme(Theme::AMETHYST)
        .voice(Voice {
            pitch: 0.7,
            ..Voice::default()
        })
        .skin(skins::fortune_teller(), true)
        .talk(talk)
        .interface("cards", Cards::default())
}

pub fn npcs() -> Npcs {
    Npcs::new().with(merchant()).with(fortune_teller())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_app::App;
    use voidmc::{
        DefaultWorldGenerator, RegistryDataStore, ServerConfig, ServerConfigResource, WorldGen,
        commands::plugin::CommandPlugin,
        network::{ClientConnected, IncomingPacket, NetworkPlugin, OutgoingPacket},
        plugins::DefaultPlugins,
        systems::GameSystemsPlugin,
        world::ChunkIndex,
    };
    use voidmc_ui::{PackServer, ResourcePack, Text, UiPlugin};

    #[test]
    fn the_figures_and_cards_go_in_the_pack() {
        let mut pack = ResourcePack::new("test");
        npcs().build(&mut pack);
        assert!(pack.contains("assets/voidnpc/models/item/figure_merchant_right_arm.json"));
        assert!(pack.contains("assets/voidnpc/textures/item/figure_fortune_teller.png"));
        assert!(pack.contains("assets/voidnpc/items/card_2.json"));
    }

    /// Bevy only reports conflicting system params when the schedule first runs.
    #[test]
    fn the_plugin_ticks_with_the_full_engine() {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, _outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let (_connected_tx, connected_rx) = flume::unbounded::<ClientConnected>();

        let mut pack = ResourcePack::new("test");
        let plugin = npcs().build(&mut pack);
        let server = PackServer::start(pack.build(), "127.0.0.1:0", "http://test").unwrap();

        let mut app = App::new();
        app.add_plugins(NetworkPlugin::new(
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
            kick_tx,
            connected_rx,
        ))
        .add_plugins(DefaultPlugins)
        .add_plugins(CommandPlugin)
        .add_plugins(GameSystemsPlugin)
        .insert_resource(RegistryDataStore::default())
        .insert_resource(ServerConfigResource::from(&ServerConfig::default()))
        .insert_resource(WorldGen(Box::new(DefaultWorldGenerator::default())))
        .init_resource::<ChunkIndex>()
        .add_plugins(UiPlugin::new(server, Text::new("pack")))
        .add_plugins(plugin);

        app.update();
        app.update();
    }
}
