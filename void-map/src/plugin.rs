use bevy_app::{App, Plugin, Startup};
use bevy_ecs::prelude::*;
use voidmc::{
    Command, CommandBuilder, CommandContext, CommandRegistry, Messages, Players, WorldGen,
    WorldPlayers, events::PlayerReadyEvent,
};
use voidmc_ui::{Dialog, PackSettled, UiClick};

use crate::markers::{Marker, MarkerField};
use crate::view::{self, Action, MapArt, SEA_LEVEL, View};

const PACK_NEEDED: &str = "La carte a besoin du resource pack.";

/// The fixed scatter of markers around the origin.
pub const MARKERS: MarkerField = MarkerField {
    seed: 0x766f_6964_6d61_7000,
    count: 24,
    radius: 600,
    min_spacing: 48,
    sea_level: SEA_LEVEL,
};

/// The server's markers, placed once on its terrain.
#[derive(Resource, Default)]
pub struct Markers(pub Vec<Marker>);

/// Needs [`MapArt`] and `voidmc_ui::UiPlugin`. The map shows the server's
/// [`WorldGen`], so it follows whatever generator the server is configured with.
pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Markers>()
            .add_systems(Startup, place_markers)
            .add_observer(start_view)
            .add_observer(open_when_ready)
            .add_observer(click);
        app.world_mut()
            .resource_mut::<CommandRegistry>()
            .register(map_command());
    }
}

fn place_markers(mut markers: ResMut<Markers>, generator: Res<WorldGen>) {
    markers.0 = MARKERS.place(generator.0.as_ref());
}

fn start_view(event: On<PlayerReadyEvent>, mut commands: Commands) {
    commands.entity(event.entity).insert(View::default());
}

fn open_when_ready(
    event: On<PackSettled>,
    views: Query<&View>,
    art: Res<MapArt>,
    markers: Res<Markers>,
    generator: Res<WorldGen>,
    players: Players,
    messages: Messages,
) {
    if !event.loaded {
        messages.message(event.player, PACK_NEEDED).send();
        return;
    }
    if let Ok(view) = views.get(event.player) {
        let dialog = view::dialog(&art, view, &markers.0, generator.0.as_ref());
        players.send(event.player, dialog.packet());
    }
}

fn click(
    event: On<UiClick>,
    mut views: Query<&mut View>,
    art: Res<MapArt>,
    markers: Res<Markers>,
    generator: Res<WorldGen>,
    players: Players,
) {
    let Some(action) = Action::parse(&event.id) else {
        return;
    };
    if action == Action::Close {
        players.send(event.player, Dialog::close());
        return;
    }
    let Ok(mut view) = views.get_mut(event.player) else {
        return;
    };
    view.apply(action, &markers.0);
    // Sent even when nothing changed: clicking focuses the text, which the client
    // outlines in white until the screen is rebuilt.
    let dialog = view::dialog(&art, &view, &markers.0, generator.0.as_ref());
    players.send(event.player, dialog.packet());
}

fn map_command() -> Command {
    CommandBuilder::new("map")
        .description("Rouvre la carte du monde")
        .handler(open_map)
        .build()
}

fn open_map(ctx: &mut CommandContext) {
    let player = ctx.entity;
    ctx.with_world_mut(|world| {
        let (Some(view), Some(art), Some(markers), Some(generator)) = (
            world.get::<View>(player),
            world.get_resource::<MapArt>(),
            world.get_resource::<Markers>(),
            world.get_resource::<WorldGen>(),
        ) else {
            return;
        };
        let packet = view::dialog(art, view, &markers.0, generator.0.as_ref()).packet();
        WorldPlayers::new(world).send(player, packet);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use voidmc::{
        DefaultWorldGenerator, RegistryDataStore, ServerConfig, ServerConfigResource,
        commands::plugin::CommandPlugin,
        network::{ClientConnected, IncomingPacket, NetworkPlugin, OutgoingPacket},
        plugins::DefaultPlugins,
        systems::GameSystemsPlugin,
        world::ChunkIndex,
    };
    use voidmc_ui::{PackServer, ResourcePack, Text, UiPlugin};

    use crate::window::Direction;

    /// Bevy only reports conflicting system params when the schedule first runs.
    #[test]
    fn the_plugins_tick_and_answer_clicks_with_the_full_engine() {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, _outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let (_connected_tx, connected_rx) = flume::unbounded::<ClientConnected>();

        let mut pack = ResourcePack::new("test");
        let art = MapArt::new(&mut pack);
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
        .insert_resource(art)
        .add_plugins(UiPlugin::new(server, Text::new("pack")))
        .add_plugins(MapPlugin);

        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<Markers>().0.len(),
            MARKERS.count,
            "markers are placed at startup"
        );

        let player = app.world_mut().spawn(View::default()).id();
        app.world_mut().trigger(UiClick {
            player,
            id: Action::Pan(Direction::East).id(),
        });
        app.update();
        let view = app.world().get::<View>(player).unwrap();
        assert_eq!(view.window().center(), (96, 0));
    }
}
