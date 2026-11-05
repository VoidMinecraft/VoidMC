use voidmc::{ServerConfigBuilder, VoidServer};
use voidmc_map::plugin::MapPlugin;

/// More than a client needs around the camera, so that the server never limits it.
const VIEW_DISTANCE: i32 = 12;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let address = std::env::var("VOID_MAP_ADDRESS").unwrap_or_else(|_| "127.0.0.1:25565".into());
    tracing::info!(%address, "Carte - Java 26.1.2");

    // No generator set: the map shows `DefaultWorldGenerator`, through `WorldGen`.
    let config = ServerConfigBuilder::new()
        .address(address)
        .tick_rate(20)
        .view_distance(VIEW_DISTANCE)
        .motd("Carte | Void")
        .build();
    VoidServer::new(config)
        .add_plugin(|app| {
            app.add_plugins(MapPlugin);
        })
        .run();
}
