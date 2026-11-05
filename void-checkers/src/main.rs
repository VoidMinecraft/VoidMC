use voidmc::{ServerConfigBuilder, TextColor, VoidServer};
use voidmc_checkers::{art::Art, plugin::CheckersPlugin};
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
    let address = env_or("VOID_CHECKERS_ADDRESS", "127.0.0.1:25565");
    let pack_address = env_or("VOID_CHECKERS_PACK_ADDRESS", "127.0.0.1:25580");
    let pack_url = env_or(
        "VOID_CHECKERS_PACK_URL",
        &format!("http://{pack_address}/checkers.zip"),
    );

    let mut pack = ResourcePack::new("VoidMC checkers board");
    let art = Art::new(&mut pack);
    let server = PackServer::start(pack.build(), &pack_address, pack_url.clone())?;
    tracing::info!(%address, %pack_url, "Dames - Java 26.1.2");

    let config = ServerConfigBuilder::new()
        .address(address)
        .tick_rate(20)
        .motd("Dames | Void")
        .build();
    let prompt = Text::new("Le plateau de dames est dessine par ce pack.").color(TextColor::Yellow);
    VoidServer::new(config)
        .add_plugin(move |app| {
            app.insert_resource(art)
                .add_plugins(UiPlugin::new(server, prompt).required())
                .add_plugins(CheckersPlugin);
        })
        .run();
    Ok(())
}
