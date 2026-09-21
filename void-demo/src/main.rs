use voidmc::{ServerConfigBuilder, SpawnPosition, VoidServer};
use voidmc_demo::{arena, race::RacePlugin, terrain};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let seed = std::env::var("VOID_DEMO_SEED")
        .unwrap_or_else(|_| "42".into())
        .parse::<u64>()?;
    let address = std::env::var("VOID_DEMO_ADDRESS").unwrap_or_else(|_| "0.0.0.0:25565".into());
    let arena = arena::Arena::new(terrain::Alpine { seed });
    tracing::info!(seed, %address, "Alpine Rush — Java 26.1.2");
    let config = ServerConfigBuilder::new()
        .address(address)
        .tick_rate(20)
        .max_players(8)
        .game_mode(2)
        .view_distance(12)
        .spawn_chunk_radius(4)
        .initial_chunk_radius(4)
        .max_chunk_generations_per_tick(4)
        .spawn_position(SpawnPosition {
            x: 0.0,
            z: 0.0,
            y: Some(arena::WAIT_Y),
        })
        .world_generator(arena.clone())
        .motd("Alpine Rush | Void | Minecart racing")
        .build();
    VoidServer::new(config)
        .add_plugin(move |app| {
            app.add_plugins(RacePlugin(arena));
        })
        .run();
    Ok(())
}
