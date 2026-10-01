use bevy_app::App;
use voidmc::commands::plugin::CommandPlugin;
use voidmc::components::{ClientId, PlayerDimension, Position, Rotation};
use voidmc::config::{ServerConfig, ServerConfigResource};
use voidmc::network::{ClientConnected, IncomingPacket, NetworkPlugin, OutgoingPacket};
use voidmc::plugins::DefaultPlugins;
use voidmc::registry::RegistryDataStore;
use voidmc::systems::GameSystemsPlugin;
use voidmc::world::generation::{DefaultWorldGenerator, WorldGen};
use voidmc::world::{ChunkData, ChunkDimension, ChunkIndex, ChunkPos, ChunkPosition, DimensionId};
use voidmc::{EntityBuilder, EntityKind};
use voidmc_data::v26_1_2::blocks;
use voidmc_navigation::remote::select;
use voidmc_navigation::{
    Goal, NavRemotePlugin, NavigationPlugin, Navigator, RemoteAccess, walking_profile,
};
use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
use voidmc_protocol::clientbound::{
    ClientboundPacket, ManualPlayPacket, PlayPacket, WaypointOperation,
};

const OPERATOR_CLIENT: u32 = 77;

struct Server {
    app: App,
    outgoing: flume::Receiver<OutgoingPacket>,
    _channels: (
        flume::Sender<IncomingPacket>,
        flume::Sender<u32>,
        flume::Receiver<u32>,
        flume::Sender<ClientConnected>,
    ),
}

fn server() -> Server {
    let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
    let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
    let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
    let (kick_tx, kick_rx) = flume::unbounded::<u32>();
    let (connected_tx, connected_rx) = flume::unbounded::<ClientConnected>();
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
    .add_plugins((
        NavigationPlugin::new(),
        NavRemotePlugin::default().access(RemoteAccess::Everyone),
    ));
    for x in -2..2 {
        for z in -2..2 {
            let mut sections: Vec<ChunkSection> = (0..4)
                .map(|_| ChunkSection::filled(blocks::STONE, 0))
                .collect();
            sections.push(ChunkSection::with_floor(blocks::STONE, 0));
            sections.extend((0..19).map(|_| ChunkSection::empty()));
            let data = ChunkData::new(sections, ChunkHeightmaps::empty(), LightData::empty());
            let pos = ChunkPos::new(x, z);
            let entity = app
                .world_mut()
                .spawn((
                    ChunkPosition(pos),
                    data,
                    ChunkDimension(DimensionId::Overworld),
                ))
                .id();
            app.world_mut()
                .resource_mut::<ChunkIndex>()
                .0
                .insert((DimensionId::Overworld, pos), entity);
        }
    }
    Server {
        app,
        outgoing: outgoing_rx,
        _channels: (incoming_tx, disconnect_tx, kick_rx, connected_tx),
    }
}

fn received(server: &Server) -> Vec<ClientboundPacket> {
    server
        .outgoing
        .try_iter()
        .filter(|packet| packet.client_id == OPERATOR_CLIENT)
        .map(|packet| packet.packet)
        .collect()
}

#[test]
fn the_watcher_sees_the_path_marker_and_waypoints_and_nothing_lingers() {
    let mut server = server();
    let world = server.app.world_mut();
    let operator = world
        .spawn((
            ClientId(OPERATOR_CLIENT),
            Position {
                x: 0.5,
                y: 1.0,
                z: -4.5,
            },
            Rotation::default(),
            PlayerDimension(DimensionId::Overworld),
        ))
        .id();
    let mob = EntityBuilder::new(EntityKind::Zombie)
        .at(0.5, 1.0, 0.5)
        .gravity(true)
        .block_collision(true)
        .settle_ticks(0)
        .with(
            Navigator::new(walking_profile(EntityKind::Zombie))
                .with_speed(0.2)
                .with_goal(Goal::move_to([12.5, 1.0, 8.5])),
        )
        .spawn_in(world)
        .id();
    assert!(select(world, operator, Some(mob)));

    let mut spawned = Vec::new();
    let mut removed = Vec::new();
    let mut tracked = 0;
    let mut untracked = 0;
    for _ in 0..200 {
        server.app.update();
        for packet in received(&server) {
            match packet {
                ClientboundPacket::Play(PlayPacket::SpawnEntity(spawn)) => {
                    assert_eq!(spawn.entity_type, EntityKind::BlockDisplay.id());
                    spawned.push(spawn.entity_id);
                }
                ClientboundPacket::ManualPlay(ManualPlayPacket::RemoveEntities(remove)) => {
                    removed.extend(remove.entity_ids);
                }
                ClientboundPacket::Play(PlayPacket::TrackedWaypoint(waypoint)) => {
                    match waypoint.operation {
                        WaypointOperation::Track => tracked += 1,
                        WaypointOperation::Untrack => untracked += 1,
                        WaypointOperation::Update => {}
                    }
                }
                _ => {}
            }
        }
    }
    assert!(spawned.len() >= 2, "beams and a marker: {spawned:?}");
    assert!(tracked >= 2, "mob and destination waypoints");
    let navigator = server.app.world().get::<Navigator>(mob).expect("mob");
    assert!(navigator.is_idle(), "the mob reached its goal");
    assert!(
        untracked >= 1,
        "the reached destination left the locator bar"
    );
    let mut leftover: Vec<i32> = spawned
        .iter()
        .copied()
        .filter(|id| !removed.contains(id))
        .collect();
    leftover.sort_unstable();
    assert!(
        leftover.is_empty(),
        "displays still on the client: {leftover:?}"
    );

    assert!(select(server.app.world_mut(), operator, None));
    server.app.update();
    let packets = received(&server);
    assert!(packets.iter().any(|packet| matches!(
        packet,
        ClientboundPacket::Play(PlayPacket::TrackedWaypoint(w)) if w.operation == WaypointOperation::Untrack
    )));
    server.app.update();
    assert!(received(&server).is_empty(), "an idle remote sends nothing");
}
