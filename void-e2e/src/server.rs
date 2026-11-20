use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};
use std::thread::JoinHandle;
use std::time::Duration;

use bevy_app::{App, AppExit, First, Last};
use bevy_ecs::lifecycle::Add;
use bevy_ecs::prelude::*;
use tokio::sync::{Mutex, OwnedMutexGuard, oneshot, watch};
use voidmc::components::{Operator, PlayerUuid};
use voidmc::network::PacketEvent;
use voidmc::{
    Command, CommandRegistry, ListenAddress, ServerConfig, ServerConfigBuilder, VoidServer,
    WorldPlayers, register_default_commands,
};
use voidmc_protocol::{clientbound, serverbound};

use azalea_protocol::packets::status::c_status_response::ClientboundStatusResponse;

use crate::bot::{Bot, BotOptions, ConfiguringClient, LoginRefused};
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

    /// Makes every player an [`Operator`] from the moment it logs in, so the
    /// command tree and permission level it gets at join are an operator's.
    pub fn operators(self) -> Self {
        self.plugin(|app| {
            app.add_observer(make_operator);
        })
    }

    /// Boots the server and returns once its first tick has started.
    pub async fn start(self) -> TestServer {
        assert_eq!(
            azalea_protocol::packets::PROTOCOL_VERSION,
            voidmc_protocol::PROTOCOL_VERSION,
            "Azalea and VoidMC speak different protocol versions: bump azalea-protocol in void-e2e/Cargo.toml"
        );
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
                server.run()
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

fn make_operator(event: On<Add, PlayerUuid>, mut commands: Commands) {
    commands.entity(event.entity).insert(Operator);
}

fn run_control(world: &mut World) {
    let address = world.resource::<ListenAddress>().0;
    let jobs: Vec<Job> = {
        let mut control = world.resource_mut::<Control>();
        control.ticks.send_modify(|tick| *tick += 1);
        let jobs = control.jobs.try_iter().collect();
        // Signalled only once this tick's jobs are collected, so a job sent
        // after `start` returns never runs in the first tick: it waits for the
        // second, after the first has run to its end (its `PostUpdate`
        // schedule, for one, has run and been initialized).
        if let Some(ready) = control.ready.take() {
            let _ = ready.send(address);
        }
        jobs
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
    thread: Option<JoinHandle<AppExit>>,
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
    /// its result. That is never the first tick: by the time `f` runs, the
    /// first tick has run to its end.
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

    /// Like [`TestServer::join`], but a login the server refuses returns its
    /// reason instead of failing the test.
    pub async fn try_join(&self, name: &str) -> Result<Bot, LoginRefused> {
        Bot::try_join(self.address, name, BotOptions::default()).await
    }

    /// Logs a client in and leaves it in the configuration state, holding
    /// its connection open without ever playing.
    pub async fn log_in(&self, name: &str) -> Result<ConfiguringClient, LoginRefused> {
        ConfiguringClient::log_in(self.address, name).await
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

    /// Grants [`Operator`] to the bot's player while it is connected.
    pub async fn op(&self, bot: &Bot) {
        let player = self.player_entity(bot.uuid()).await;
        self.with_world(move |world| {
            world.entity_mut(player).insert(Operator);
        })
        .await;
    }

    /// Removes [`Operator`] from the bot's player while it is connected.
    pub async fn deop(&self, bot: &Bot) {
        let player = self.player_entity(bot.uuid()).await;
        self.with_world(move |world| {
            world.entity_mut(player).remove::<Operator>();
        })
        .await;
    }

    pub fn logs(&self) -> Vec<LogRecord> {
        self.logs.records()
    }

    /// Stops the server and panics if it logged an error or dropped a packet
    /// it could not recognise.
    pub async fn stop(mut self) {
        self.shutdown().await;
        self.assert_no_failures();
    }

    /// Stops the server like [`TestServer::stop`], except that the failures
    /// `expected` accepts are allowed; returns them, so that the test can
    /// check that the server logged what it provoked.
    pub async fn stop_expecting(mut self, expected: impl Fn(&LogRecord) -> bool) -> Vec<LogRecord> {
        self.shutdown().await;
        let (expected, unexpected): (Vec<LogRecord>, Vec<LogRecord>) = self
            .logs
            .records()
            .into_iter()
            .filter(LogRecord::is_failure)
            .partition(|record| expected(record));
        assert!(
            unexpected.is_empty(),
            "the server logged {} unexpected failure(s):\n{}",
            unexpected.len(),
            unexpected
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
        expected
    }

    /// Waits for the server to stop on its own, for example after a player
    /// ran `/stop`, without asking it to; then fails like [`TestServer::stop`].
    /// Returns what `VoidServer::run` returned.
    pub async fn wait_for_exit(mut self) -> AppExit {
        let exit = self
            .join_thread()
            .await
            .expect("the server thread is joined only once");
        self.assert_no_failures();
        exit
    }

    fn assert_no_failures(&self) {
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

    fn request_exit(&self) {
        let _ = self.handle.jobs.send(Box::new(|world: &mut World| {
            world.write_message(AppExit::Success);
        }));
    }

    async fn shutdown(&mut self) {
        self.request_exit();
        self.join_thread().await;
    }

    /// Waits for the server thread and returns what `VoidServer::run`
    /// returned, or `None` if the thread was already joined.
    async fn join_thread(&mut self) -> Option<AppExit> {
        let thread = self.thread.take()?;
        let joined = tokio::task::spawn_blocking(move || thread.join());
        match tokio::time::timeout(Duration::from_secs(10), joined).await {
            Ok(Ok(Ok(exit))) => Some(exit),
            Ok(Ok(Err(panic))) => std::panic::resume_unwind(panic),
            Ok(Err(error)) => panic!("failed to join the server thread: {error}"),
            Err(_) => {
                // The join is still blocking the test runtime: stop the
                // server so the runtime can shut down after the panic.
                self.request_exit();
                panic!("the server did not stop within 10s");
            }
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        self.request_exit();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct CompletedTicks(u64);

    fn count_completed_tick(mut completed: ResMut<CompletedTicks>) {
        completed.0 += 1;
    }

    #[tokio::test]
    async fn a_world_closure_sent_right_after_start_runs_after_the_first_tick() {
        let server = TestServer::builder()
            .plugin(|app| {
                app.init_resource::<CompletedTicks>()
                    .add_systems(Last, count_completed_tick);
            })
            .start()
            .await;

        let completed = server
            .with_world(|world| world.resource::<CompletedTicks>().0)
            .await;
        assert!(completed >= 1, "ran during tick {}", completed + 1);
        server.stop().await;
    }
}
