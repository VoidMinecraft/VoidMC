use voidmc::{ServerConfigBuilder, SpawnPosition, VoidServer};
use voidmc_checkers::{geometry, pack, plugin::CheckersPlugin};

const ADVENTURE: u8 = 2;

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let address = env_or("VOID_CHECKERS_ADDRESS", "127.0.0.1:25565");
    let pack_address = env_or("VOID_CHECKERS_PACK_ADDRESS", "127.0.0.1:25580");
    let pack_url = env_or(
        "VOID_CHECKERS_PACK_URL",
        &format!("http://{pack_address}/checkers.zip"),
    );
    pack::serve(&pack_address)?;
    tracing::info!(%address, %pack_url, "Dames - Java 26.1.2");
    let [x, y, z] = geometry::seat();
    let config = ServerConfigBuilder::new()
        .address(address)
        .game_mode(ADVENTURE)
        .spawn_position(SpawnPosition { x, z, y: Some(y) })
        .motd("Dames | Void")
        .build();
    VoidServer::new(config)
        .add_plugin(move |app| {
            app.add_plugins(CheckersPlugin { pack_url });
        })
        .run();
    Ok(())
}
