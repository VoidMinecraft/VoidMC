use bevy_ecs::prelude::*;
use voidmc::{
    DefaultWorldGenerator, Inventory, ItemStack, ServerConfigBuilder, SpawnPosition, TextColor,
    VoidServer, events::PlayerReadyEvent,
};
use voidmc_npc_demo::cast::{GROUND, npcs};
use voidmc_ui::{PackServer, ResourcePack, Text, UiPlugin};

const PURSE: u8 = 64;

fn fill_purse(event: On<PlayerReadyEvent>, mut inventories: Query<&mut Inventory>) {
    if let Ok(mut inventory) = inventories.get_mut(event.entity)
        && let Some(emeralds) = ItemStack::of("minecraft:emerald", PURSE)
    {
        inventory.give(emeralds);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let address = std::env::var("VOID_NPC_ADDRESS").unwrap_or_else(|_| "127.0.0.1:25660".into());
    let pack_address =
        std::env::var("VOID_NPC_PACK_ADDRESS").unwrap_or_else(|_| "127.0.0.1:25661".into());
    let pack_url = format!("http://{pack_address}/npc.zip");

    let mut pack = ResourcePack::new("VoidMC NPC");
    let plugin = npcs().build(&mut pack);
    let server = PackServer::start(pack.build(), &pack_address, pack_url.clone())?;
    tracing::info!(%address, %pack_url, "NPC - Java 26.1.2");

    let flat = DefaultWorldGenerator {
        base_height: GROUND as i32,
        amplitude: 0.0,
        ..DefaultWorldGenerator::default()
    };
    let config = ServerConfigBuilder::new()
        .address(address)
        .tick_rate(20)
        .motd("NPC | Void")
        .world_generator(flat)
        .spawn_position(SpawnPosition {
            x: 0.5,
            z: 0.5,
            y: Some(GROUND),
        })
        .build();
    let prompt =
        Text::new("Les scènes des NPC sont dessinées par ce pack.").color(TextColor::Yellow);
    VoidServer::new(config)
        .add_plugin(move |app| {
            app.add_plugins(UiPlugin::new(server, prompt))
                .add_plugins(plugin)
                .add_observer(fill_purse);
        })
        .run();
    Ok(())
}
