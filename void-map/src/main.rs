use voidmc::{ServerConfigBuilder, TextColor, VoidServer};
use voidmc_map::{plugin::MapPlugin, view::MapArt};
use voidmc_ui::{PackServer, ResourcePack, Text, UiPlugin};

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let address = env_or("VOID_MAP_ADDRESS", "127.0.0.1:25565");
    let pack_address = env_or("VOID_MAP_PACK_ADDRESS", "127.0.0.1:25580");
    let pack_url = env_or(
        "VOID_MAP_PACK_URL",
        &format!("http://{pack_address}/map.zip"),
    );

    let mut pack = ResourcePack::new("VoidMC map");
    let art = MapArt::new(&mut pack);
    let server = PackServer::start(pack.build(), &pack_address, pack_url.clone())?;
    tracing::info!(%address, %pack_url, "Carte - Java 26.1.2");

    // No generator set: the map shows `DefaultWorldGenerator`, as `WorldGen`.
    let config = ServerConfigBuilder::new()
        .address(address)
        .tick_rate(20)
        .motd("Carte | Void")
        .build();
    let prompt = Text::new("La carte est dessinee par ce pack.").color(TextColor::Yellow);
    VoidServer::new(config)
        .add_plugin(move |app| {
            app.insert_resource(art)
                .add_plugins(UiPlugin::new(server, prompt).required())
                .add_plugins(MapPlugin);
        })
        .run();
    Ok(())
}
