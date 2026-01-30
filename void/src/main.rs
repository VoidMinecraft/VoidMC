use std::time::Duration;

use bevy_app::{App, ScheduleRunnerPlugin, TaskPoolPlugin};
use bevy_ecs::prelude::*;
use tracing_subscriber::prelude::*;
use void::{
    Server,
    network::{IncomingPacket, NetworkPlugin, OutgoingPacket},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging()?;

    let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
    let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();

    // Start the server in a separate thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            let mut server = Server::new("127.0.0.1:25565")
                .await
                .expect("Failed to start server");
            server.run(incoming_tx, outgoing_rx).await;
        })
    });

    App::new()
        .add_plugins((
            TaskPoolPlugin::default(),
            ScheduleRunnerPlugin::run_loop(Duration::from_millis(1000 / 20)),
        ))
        .add_plugins(NetworkPlugin::new(incoming_rx, outgoing_tx))
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
