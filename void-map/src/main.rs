use voidmc::{ServerConfigBuilder, TextColor, VoidServer};
use voidmc_map::{art, plugin::MapPlugin};
use voidmc_ui::{PackServer, ResourcePack, Text, UiPlugin};

/// More than a client needs around the camera, so that the server never limits it.
const VIEW_DISTANCE: i32 = 12;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let address = std::env::var("VOID_MAP_ADDRESS").unwrap_or_else(|_| "127.0.0.1:25565".into());
    let pack_address =
        std::env::var("VOID_MAP_PACK_ADDRESS").unwrap_or_else(|_| "127.0.0.1:25566".into());
    let pack_url = format!("http://{pack_address}/map.zip");
    let mut pack = ResourcePack::new("VoidMC map cards");
    art::install(&mut pack);
    let server = PackServer::start(pack.build(), &pack_address, pack_url.clone())?;
    tracing::info!(%address, %pack_url, "Carte - Java 26.1.2");

    // No generator set: the map shows `DefaultWorldGenerator`, through `WorldGen`.
    let config = ServerConfigBuilder::new()
        .address(address)
        .tick_rate(20)
        .view_distance(VIEW_DISTANCE)
        .motd("Carte | Void")
        .build();
    let prompt =
        Text::new("Les fiches de la carte sont dessinées par ce pack.").color(TextColor::Yellow);
    VoidServer::new(config)
        .add_plugin(move |app| {
            app.add_plugins(UiPlugin::new(server, prompt))
                .add_plugins(MapPlugin);
        })
        .run();
    Ok(())
}
