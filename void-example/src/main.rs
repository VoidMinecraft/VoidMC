use tracing_subscriber::prelude::*;
use void::components::PlayerName;
use void::events::PlayerStartDiggingEvent;
use void::{
    CommandBuilder, CommandRegistry, On, Query, ServerConfigBuilder, VoidServer,
    register_default_commands,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging()?;

    VoidServer::new(
        ServerConfigBuilder::new()
            .spawn_chunk_radius(4)
            .initial_chunk_radius(4)
            .build(),
    )
    .add_plugin(|app| {
        // Register all default commands
        let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
        register_default_commands(&mut registry, &[]);

        // Observe block-breaking events
        app.add_observer(on_player_dig);
    })
    .add_command(
        CommandBuilder::new("hello")
            .description("Greet the player")
            .handler(|ctx| {
                ctx.reply("Hello from void-example!");
            })
            .build(),
    )
    .run();

    Ok(())
}

fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    // Create logs directory if it doesn't exist
    std::fs::create_dir_all("logs")?;

    let file_appender = tracing_appender::rolling::daily("logs", "void.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_level(true)
        .with_file(true)
        .pretty()
        .with_writer(std::io::stderr);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_level(true)
        .with_file(true)
        .with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(())
}

fn on_player_dig(event: On<PlayerStartDiggingEvent>, query: Query<&PlayerName>) {
    let name = query
        .get(event.entity)
        .map(|n| n.0.as_str())
        .unwrap_or("Unknown");
    tracing::info!(
        "{} broke a block at ({}, {}, {})",
        name,
        event.position.x,
        event.position.y,
        event.position.z,
    );
}
