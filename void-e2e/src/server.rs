use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};
use std::thread::JoinHandle;
use std::time::Duration;

use bevy_app::{App, AppExit, First, Last};
use bevy_ecs::prelude::*;
use tokio::sync::{Mutex, OwnedMutexGuard, oneshot, watch};
use voidmc::components::PlayerUuid;
use voidmc::network::PacketEvent;
use voidmc::{
    Command, CommandRegistry, ListenAddress, ServerConfig, ServerConfigBuilder, VoidServer,
    WorldPlayers, register_default_commands,
};
use voidmc_protocol::{clientbound, serverbound};

use azalea_protocol::packets::status::c_status_response::ClientboundStatusResponse;

use crate::bot::{Bot, BotOptions};
use crate::logs::{ActiveCapture, LogCapture, LogRecord};
use crate::timeout;

pub(crate) const BARRIER_BIT: u32 = 0x8000_0000;

static ONE_SERVER_AT_A_TIME: LazyLock<Arc<Mutex<()>>> = LazyLock::new(Default::default);

type Job = Box<dyn FnOnce(&mut World) + Send>;
type Plugin = Box<dyn FnOnce(&mut App) + Send>;
type CommandFactory = Box<dyn FnOnce() -> Command + Send>;

#[derive(Resource)]
struct Control {
    jobs: flume::Receiver<Job>,
    ticks: watch::Sender<u64>,
    ready: Option<oneshot::Sender<SocketAddr>>,
}

#[derive(Resource, Default)]
struct PendingBarriers(Vec<(Entity, u32)>);

#[derive(Clone)]
pub(crate) struct ServerHandle {
    jobs: flume::Sender<Job>,
    ticks: watch::Receiver<u64>,
}

impl ServerHandle {
    pub(crate) async fn with_world<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut World) -> R + Send + 'static,
    ) -> R {
        let (tx, rx) = oneshot::channel();
        self.jobs
            .send(Box::new(move |world| {
                let _ = tx.send(f(world));
            }))
            .expect("the test server has stopped");
        timeout("the server to run a world closure", rx)
            .await
            .expect("the test server stopped before running the closure")
    }

    pub(crate) async fn wait_ticks(&self, n: u64) {
        let mut ticks = self.ticks.clone();
        let target = *ticks.borrow_and_update() + n;
        timeout("server ticks", ticks.wait_for(|tick| *tick >= target))
            .await
            .expect("the test server stopped while waiting for ticks");
    }
}

/// Configures a [`TestServer`] before it boots.
pub struct TestServerBuilder {
    config: ServerConfigBuilder,
    plugins: Vec<Plugin>,
    commands: Vec<CommandFactory>,
    default_commands: bool,
}

impl TestServerBuilder {
    fn new() -> Self {
        Self {
            config: ServerConfigBuilder::new()
                .address("127.0.0.1:0")
                .tick_rate(50)
                .view_distance(4)
                .simulation_distance(4)
                .spawn_chunk_radius(4)
                .initial_chunk_radius(2),
            plugins: Vec::new(),
            commands: Vec::new(),
            default_commands: true,
        }
    }

    pub fn config(mut self, f: impl FnOnce(ServerConfigBuilder) -> ServerConfigBuilder) -> Self {
        self.config = f(self.config);
        self
    }

    pub fn compression_threshold(self, threshold: Option<u32>) -> Self {
        self.config(|config| config.compression_threshold(threshold))
    }

    pub fn plugin(mut self, f: impl FnOnce(&mut App) + Send + 'static) -> Self {
        self.plugins.push(Box::new(f));
        self
    }

    pub fn command(mut self, command: impl FnOnce() -> Command + Send + 'static) -> Self {
        self.commands.push(Box::new(command));
        self
    }

    pub fn without_default_commands(mut self) -> Self {
        self.default_commands = false;
        self
    }

    pub async fn start(self) -> TestServer {
        let turn = tokio::time::timeout(
            Duration::from_secs(120),
            ONE_SERVER_AT_A_TIME.clone().lock_owned(),
        )
        .await
        .expect("another TestServer of this process did not stop within 120s; a test runs at most one at a time");
        let (jobs_tx, jobs_rx) = flume::unbounded::<Job>();
        let (ticks_tx, ticks_rx) = watch::channel(0u64);
        let (ready_tx, ready_rx) = oneshot::channel();
        let logs = LogCapture::default();
        let active = logs.activate();
        let Self {
            config,
            plugins,
            commands,
            default_commands,
        } = self;

        let thread = std::thread::Builder::new()
            .name("void-e2e-server".into())
            .spawn(move || {
                let config: ServerConfig = config.build();
                let mut server = VoidServer::new(config).add_plugin(move |app: &mut App| {
                    app.insert_resource(Control {
                        jobs: jobs_rx,
                        ticks: ticks_tx,
                        ready: Some(ready_tx),
                    })
                    .init_resource::<PendingBarriers>()
                    .add_systems(First, run_control)
                    .add_systems(Last, answer_barriers)
                    .add_observer(queue_barrier);
                    if default_commands {
                        register_default_commands(
                            &mut app.world_mut().resource_mut::<CommandRegistry>(),
                            &[],
                        );
                    }
                    for plugin in plugins {
                        plugin(app);
                    }
                });
                for command in commands {
                    server = server.add_command(command());
                }
                server.run();
            })
            .expect("failed to spawn the server thread");

        let address = timeout("the server to boot", ready_rx)
            .await
            .unwrap_or_else(|_| panic!("the test server exited during startup"));

        TestServer {
            handle: ServerHandle {
                jobs: jobs_tx,
                ticks: ticks_rx,
            },
            address,
            thread: Some(thread),
            logs,
            _active: active,
            _turn: turn,
        }
    }
}

fn run_control(world: &mut World) {
    let address = world.resource::<ListenAddress>().0;
    let jobs: Vec<Job> = {
        let mut control = world.resource_mut::<Control>();
        if let Some(ready) = control.ready.take() {
            let _ = ready.send(address);
        }
        control.ticks.send_modify(|tick| *tick += 1);
        control.jobs.try_iter().collect()
    };
    for job in jobs {
        job(world);
    }
}

fn queue_barrier(event: On<PacketEvent<serverbound::Pong>>, mut pending: ResMut<PendingBarriers>) {
    let id = event.packet.id as u32;
    if id & BARRIER_BIT != 0 {
        pending.0.push((event.entity, id));
    }
}

fn answer_barriers(world: &mut World) {
    let pending = std::mem::take(&mut world.resource_mut::<PendingBarriers>().0);
    let players = WorldPlayers::new(world);
    for (entity, id) in pending {
        players.send(entity, clientbound::Ping { id: id as i32 });
    }
}

/// A VoidMC server running in-process on an ephemeral port.
///
/// Only one runs at a time per process, so that every warning and error
/// logged while it runs, on any thread, is attributed to it. Dropping it
/// stops the server without checking its logs; call [`TestServer::stop`] at
/// the end of a test to also fail on server errors.
pub struct TestServer {
    handle: ServerHandle,
    address: SocketAddr,
    thread: Option<JoinHandle<()>>,
    logs: LogCapture,
    _active: ActiveCapture,
    _turn: OwnedMutexGuard<()>,
}

impl TestServer {
    pub fn builder() -> TestServerBuilder {
        TestServerBuilder::new()
    }

    pub async fn start() -> Self {
        Self::builder().start().await
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Runs `f` on the game thread at the start of the next tick and returns
    /// its result.
    pub async fn with_world<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut World) -> R + Send + 'static,
    ) -> R {
        self.handle.with_world(f).await
    }

    /// Waits until `n` more ticks have started.
    pub async fn wait_ticks(&self, n: u64) {
        self.handle.wait_ticks(n).await;
    }

    /// Polls `f` once per tick until it returns `Some`.
    pub async fn wait_for<R: Send + 'static>(
        &self,
        what: &str,
        f: impl Fn(&mut World) -> Option<R> + Send + Sync + 'static,
    ) -> R {
        let f = Arc::new(f);
        timeout(what, async {
            loop {
                let f = f.clone();
                if let Some(result) = self.with_world(move |world| f(world)).await {
                    return result;
                }
                self.wait_ticks(1).await;
            }
        })
        .await
    }

    /// Connects a bot with default options and plays it into the world.
    pub async fn join(&self, name: &str) -> Bot {
        self.join_with(name, BotOptions::default()).await
    }

    pub async fn join_with(&self, name: &str, options: BotOptions) -> Bot {
        Bot::join(self.address, name, options).await
    }

    /// Asks for the server-list status like the multiplayer screen does.
    pub async fn status(&self) -> ClientboundStatusResponse {
        timeout("the status response", crate::bot::status(self.address)).await
    }

    /// The server-side entity of the player with this UUID, once it exists.
    pub async fn player_entity(&self, uuid: uuid::Uuid) -> Entity {
        self.wait_for("the player entity to exist", move |world| {
            world
                .query::<(Entity, &PlayerUuid)>()
                .iter(world)
                .find(|(_, id)| id.0 == uuid)
                .map(|(entity, _)| entity)
        })
        .await
    }

    pub fn logs(&self) -> Vec<LogRecord> {
        self.logs.records()
    }

    /// Stops the server and panics if it logged an error or dropped a packet
    /// it could not recognise.
    pub async fn stop(mut self) {
        self.shutdown().await;
        let failures: Vec<String> = self
            .logs
            .records()
            .into_iter()
            .filter(LogRecord::is_failure)
            .map(|record| record.to_string())
            .collect();
        assert!(
            failures.is_empty(),
            "the server logged {} failure(s):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    async fn shutdown(&mut self) {
        let _ = self.handle.jobs.send(Box::new(|world: &mut World| {
            world.write_message(AppExit::Success);
        }));
        if let Some(thread) = self.thread.take() {
            let joined = tokio::task::spawn_blocking(move || thread.join());
            match tokio::time::timeout(Duration::from_secs(10), joined).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(panic))) => std::panic::resume_unwind(panic),
                Ok(Err(error)) => panic!("failed to join the server thread: {error}"),
                Err(_) => panic!("the server did not stop within 10s"),
            }
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        let _ = self.handle.jobs.send(Box::new(|world: &mut World| {
            world.write_message(AppExit::Success);
        }));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !thread.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if !thread.is_finished() {
            eprintln!(
                "void-e2e: the server did not stop within 10s; its later logs may be attributed to the next test"
            );
        }
    }
}
