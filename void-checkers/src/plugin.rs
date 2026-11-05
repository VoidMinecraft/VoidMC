use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use voidmc::{
    Command, CommandBuilder, CommandContext, CommandRegistry, Messages, Players, WorldPlayers,
    events::PlayerReadyEvent,
};
use voidmc_ui::{Dialog, PackSettled, UiClick};

use crate::{
    art::Art,
    board::{self, Action, Match},
    game::{Outcome, Square},
};

const PACK_NEEDED: &str = "Le plateau a besoin du resource pack.";

/// How often the open board is sent again, so its clocks move.
const REFRESH_TICKS: u64 = 20;

/// The game a player is in.
#[derive(Component, Default)]
pub struct Session {
    state: Match,
    open: bool,
    ticks: u64,
}

impl Session {
    fn apply(&mut self, square: Square) {
        if let Outcome::Moved(step) = self.state.game.click(square) {
            self.state.last = Some(step);
            if !self.state.game.chaining() {
                self.state.moves += 1;
            }
        }
    }
}

pub struct CheckersPlugin;

impl Plugin for CheckersPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(start_session)
            .add_observer(open_when_ready)
            .add_observer(click)
            .add_systems(Update, tick);
        app.world_mut()
            .resource_mut::<CommandRegistry>()
            .register(checkers_command());
    }
}

fn start_session(event: On<PlayerReadyEvent>, mut commands: Commands) {
    commands.entity(event.entity).insert(Session::default());
}

fn open_when_ready(
    event: On<PackSettled>,
    mut sessions: Query<&mut Session>,
    art: Res<Art>,
    players: Players,
    messages: Messages,
) {
    if !event.loaded {
        messages.message(event.player, PACK_NEEDED).send();
        return;
    }
    if let Ok(mut session) = sessions.get_mut(event.player) {
        session.open = true;
        players.send(event.player, board::dialog(&session.state, &art).packet());
    }
}

fn click(event: On<UiClick>, mut sessions: Query<&mut Session>, art: Res<Art>, players: Players) {
    let Some(action) = Action::parse(&event.id) else {
        return;
    };
    let Ok(mut session) = sessions.get_mut(event.player) else {
        return;
    };
    match action {
        Action::Click(square) => session.apply(square),
        Action::NewGame => session.state = Match::default(),
        Action::Close => {
            session.open = false;
            players.send(event.player, Dialog::close());
            return;
        }
    }
    // Sent even after a refused click: clicking focuses the text, which the client
    // outlines in white until the screen is rebuilt.
    players.send(event.player, board::dialog(&session.state, &art).packet());
}

fn tick(mut sessions: Query<(Entity, &mut Session)>, art: Res<Art>, players: Players) {
    for (player, mut session) in &mut sessions {
        session.ticks += 1;
        if !session.open {
            continue;
        }
        session.state.tick();
        if session.ticks.is_multiple_of(REFRESH_TICKS) {
            players.send(player, board::dialog(&session.state, &art).packet());
        }
    }
}

fn checkers_command() -> Command {
    CommandBuilder::new("dames")
        .description("Rouvre le plateau de dames")
        .handler(open_board)
        .build()
}

fn open_board(ctx: &mut CommandContext) {
    let player = ctx.entity;
    ctx.with_world_mut(|world| {
        let Some(mut session) = world.get_mut::<Session>(player) else {
            return;
        };
        session.open = true;
        let state = session.state.clone();
        let Some(art) = world.get_resource::<Art>() else {
            return;
        };
        let packet = board::dialog(&state, art).packet();
        WorldPlayers::new(world).send(player, packet);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use voidmc::{
        DefaultWorldGenerator, RegistryDataStore, ServerConfig, ServerConfigResource, WorldGen,
        commands::plugin::CommandPlugin,
        network::{ClientConnected, IncomingPacket, NetworkPlugin, OutgoingPacket},
        plugins::DefaultPlugins,
        systems::GameSystemsPlugin,
        world::ChunkIndex,
    };
    use voidmc_ui::{PackServer, ResourcePack, Text, UiPlugin};

    /// Bevy only reports conflicting system params when the schedule first runs.
    #[test]
    fn the_plugins_tick_with_the_full_engine() {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, _outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let (_connected_tx, connected_rx) = flume::unbounded::<ClientConnected>();

        let mut pack = ResourcePack::new("test");
        let art = Art::new(&mut pack);
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
        .add_plugins(CheckersPlugin);

        app.update();
        app.update();
    }
}
