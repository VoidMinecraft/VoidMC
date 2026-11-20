//! VMC-ATP-024: every `ServerConfigBuilder` option reaches something a client
//! or the running server can observe, an untouched builder brings back the
//! documented defaults, and values the server cannot run with are replaced.
//!
//! The reference table (names, types, defaults), the startup log line and an
//! address that cannot be bound are checked in `void/tests/server_config.rs`.
//! `packet_ingest_budget_ms` has no assertion here: it only bounds the
//! microseconds spent draining the incoming channel, which no test can
//! exceed deterministically. `execution_mode` and `observability` are not set
//! here: `atp_028_parallel_regions` runs a server built with
//! `execution_mode(ExecutionMode::MultiThreaded)` and checks its systems and
//! regions run concurrently, and `atp_032_observability` queries the
//! endpoints a server built with `observability(..)` serves.
//!
//! The registries `configure_registries` edits are frozen once a client has
//! received them: later changes are refused, so every client gets the same.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy_app::{Last, Update};
use bevy_ecs::prelude::*;
use voidmc::components::{LoadedChunks, PlayerUuid};
use voidmc::events::PlayerJoinEvent;
use voidmc::{
    ChunkIndex, DefaultWorldGenerator, FrameLimits, RegistriesAlreadySent, RegistryDataStore,
    ServerConfigBuilder, SpawnPosition, WorldGenerator, default_registry_data,
};
use voidmc_e2e::core::game_type::GameMode;
use voidmc_e2e::core::position::{ChunkPos, Vec3};
use voidmc_e2e::protocol::packets::game::{ClientboundGamePacket, c_login::ClientboundLogin};
use voidmc_e2e::protocol::simdnbt::owned::NbtCompound;
use voidmc_e2e::{Bot, LoginRefused, TestServer};
use voidmc_protocol::clientbound::{RegistryData, RegistryEntry};

use crate::{feet, ground};

fn square(center: ChunkPos, radius: i32) -> HashSet<ChunkPos> {
    (-radius..=radius)
        .flat_map(|dx| {
            (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
        })
        .collect()
}

fn loaded(bot: &Bot) -> HashSet<ChunkPos> {
    bot.view(|view| view.chunks.keys().copied().collect())
}

async fn login_packet(bot: &Bot) -> ClientboundLogin {
    bot.expect("the Login packet", |packet| match packet {
        ClientboundGamePacket::Login(login) => Some(login.clone()),
        _ => None,
    })
    .await
}

async fn tab_list_game_mode(bot: &Bot) -> GameMode {
    let uuid = bot.uuid();
    bot.expect("its own tab list entry", |packet| match packet {
        ClientboundGamePacket::PlayerInfoUpdate(update) if update.actions.update_game_mode => {
            update
                .entries
                .iter()
                .find(|entry| entry.profile.uuid == uuid)
                .map(|entry| entry.game_mode)
        }
        _ => None,
    })
    .await
}

async fn startup_chunks(server: &TestServer) -> HashSet<ChunkPos> {
    server
        .with_world(|world| {
            let index = world.resource::<ChunkIndex>();
            index
                .0
                .keys()
                .map(|(_, pos)| ChunkPos::new(pos.x, pos.z))
                .collect()
        })
        .await
}

/// A directory under the system temp dir, removed on drop (also when the test
/// fails).
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "voidmc-e2e-atp024-{tag}-{}-{nanos}",
            std::process::id()
        )))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the game thread saw: ticks ended, `ChunkIndex` size at the end of
/// every tick, and the chunks a player held when `PlayerJoinEvent` fired.
#[derive(Clone, Default)]
struct Probe {
    ticks: Arc<AtomicU64>,
    index_sizes: Arc<Mutex<Vec<usize>>>,
    joined_with: Arc<Mutex<Option<HashSet<ChunkPos>>>>,
}

impl Probe {
    fn install(&self, app: &mut bevy_app::App) {
        let probe = self.clone();
        app.add_systems(Last, move |index: Res<ChunkIndex>| {
            probe.ticks.fetch_add(1, Ordering::SeqCst);
            probe.index_sizes.lock().unwrap().push(index.0.len());
        });
        let joined_with = self.joined_with.clone();
        app.add_observer(
            move |event: On<PlayerJoinEvent>, players: Query<&LoadedChunks>| {
                let chunks = players.get(event.entity).expect("a joining player");
                let chunks = chunks.0.iter().map(|pos| ChunkPos::new(pos.x, pos.z));
                *joined_with.lock().unwrap() = Some(chunks.collect());
            },
        );
    }

    fn ticks(&self) -> u64 {
        self.ticks.load(Ordering::SeqCst)
    }

    /// Chunks added to `ChunkIndex` in each tick after the first.
    fn generated_per_tick(&self) -> Vec<usize> {
        let sizes = self.index_sizes.lock().unwrap();
        sizes.windows(2).map(|pair| pair[1] - pair[0]).collect()
    }
}

/// Makes every tick work at least 2 ms, longer than a `slow_tick_ms` of 1,
/// which is compared with the time a tick works (#191).
fn work_2_ms_per_tick(app: &mut bevy_app::App) {
    app.add_systems(Update, || std::thread::sleep(Duration::from_millis(2)));
}

#[tokio::test]
async fn every_option_reaches_the_client_or_the_running_server() {
    let metrics = TempDir::new("metrics");
    let tps_csv = metrics.0.join("tps.csv");
    let tps_output = tps_csv.to_string_lossy().into_owned();
    let flat = || DefaultWorldGenerator {
        base_height: 100,
        amplitude: 0.0,
        ..DefaultWorldGenerator::default()
    };
    let motd = "Ünïcode ✦ «MOTD» 🎮 \"quoted\" \\ end";
    let probe = Probe::default();
    let installed = probe.clone();

    let server = TestServer::builder()
        .config(move |config| {
            config
                .tick_rate(40)
                .max_players(3)
                .motd(motd)
                .view_distance(3)
                .simulation_distance(5)
                .game_mode(2)
                .hardcore(true)
                .spawn_position(SpawnPosition {
                    x: 40.5,
                    z: -23.5,
                    y: Some(130.0),
                })
                .spawn_chunk_radius(2)
                .initial_chunk_radius(1)
                .max_chunk_generations_per_tick(2)
                .max_packets_per_tick(1)
                .packet_ingest_budget_ms(2)
                .metrics_debug(true)
                .metrics_tps_output(tps_output)
                .slow_tick_ms(1)
                .frame_limits(FrameLimits {
                    max_inbound_frame_bytes: 4096,
                    ..FrameLimits::default()
                })
                .compression_threshold(Some(128))
                .world_generator(flat())
                .configure_registries(|registries| {
                    let removed = registries
                        .remove_entry("minecraft:painting_variant", "minecraft:alban")
                        .expect("no client has the registries yet");
                    assert!(removed.is_some(), "the default registries hold alban");
                })
        })
        .plugin(move |app| installed.install(app))
        .plugin(work_2_ms_per_tick)
        .start()
        .await;

    // address: the harness binds 127.0.0.1:0; the server reports the port it got.
    assert!(server.address().ip().is_loopback());
    assert_ne!(server.address().port(), 0);

    // spawn_position (x, z) and spawn_chunk_radius: (2r+1)² chunks around the
    // spawn chunk exist before anyone joins.
    let spawn_chunk = ChunkPos::new(2, -2);
    assert_eq!(startup_chunks(&server).await, square(spawn_chunk, 2));

    // motd and max_players in the server list.
    let status = server.status().await;
    assert_eq!(status.description.to_string(), motd);
    assert_eq!(status.players.max, 3);
    assert_eq!(status.players.online, 0);

    let alice = server.join("Alice").await;

    // compression_threshold: announced, then applied (Azalea decodes every
    // later packet with that threshold).
    assert_eq!(alice.transcript().compression_threshold, Some(128));

    // hardcore, max_players, view_distance, simulation_distance, game_mode.
    let login = login_packet(&alice).await;
    assert!(login.hardcore);
    assert_eq!(login.max_players, 3);
    assert_eq!(login.chunk_radius, 3);
    assert_eq!(login.simulation_distance, 5);
    assert_eq!(login.common.game_type, GameMode::Adventure);
    assert_eq!(tab_list_game_mode(&alice).await, GameMode::Adventure);

    // spawn_position with an explicit y.
    let spawn = Vec3::new(40.5, 130.0, -23.5);
    alice
        .wait_until("the configured spawn", |view| view.position == spawn)
        .await;
    assert_eq!(ChunkPos::from(feet(&alice)), spawn_chunk);

    // initial_chunk_radius: the join sends the square of radius 1.
    let joined_with = probe.joined_with.lock().unwrap().clone();
    assert_eq!(joined_with, Some(square(spawn_chunk, 1)));

    // view_distance caps the 8 chunks Alice asks for at 3.
    alice
        .wait_until("the chunks within the server view distance", |view| {
            view.chunks.keys().copied().collect::<HashSet<_>>() == square(spawn_chunk, 3)
        })
        .await;
    alice.sync().await;
    assert_eq!(loaded(&alice), square(spawn_chunk, 3));

    // max_chunk_generations_per_tick: the 24 chunks outside the spawn area
    // were generated at most two per tick.
    let generated = probe.generated_per_tick();
    assert_eq!(generated.iter().sum::<usize>(), 49 - 25);
    assert_eq!(generated.iter().max(), Some(&2), "{generated:?}");

    // world_generator: the terrain under Alice is the flat generator's, not
    // the default sine terrain.
    let surface = flat().surface_height_at(40, -24);
    assert_ne!(
        surface,
        DefaultWorldGenerator::default().surface_height_at(40, -24)
    );
    assert_eq!(ground(&alice).y, surface - 1);

    // configure_registries: the edited store is the one the server sends.
    let (removed, kept) = server
        .with_world(|world| {
            let registries = world.resource::<RegistryDataStore>();
            (
                registries
                    .get_entry("minecraft:painting_variant", "minecraft:alban")
                    .is_some(),
                registries
                    .get_entry("minecraft:painting_variant", "minecraft:aztec")
                    .is_some(),
            )
        })
        .await;
    assert!(!removed && kept);

    // max_packets_per_tick: one serverbound packet is handled per tick, so 30
    // moves and the sync barrier span at least 30 ticks (about one without
    // the cap).
    let before = probe.ticks();
    let here = alice.position();
    for _ in 0..30 {
        alice.move_to(here).await;
    }
    alice.sync().await;
    let ticks = probe.ticks() - before;
    assert!(ticks >= 30, "31 packets were handled in {ticks} ticks");

    // metrics_debug, metrics_tps_output and slow_tick_ms: TPS samples land in
    // the configured file, and every tick works longer than 1 ms.
    let mut csv = String::new();
    for _ in 0..400 {
        csv = std::fs::read_to_string(&tps_csv).unwrap_or_default();
        if csv.lines().count() >= 2 {
            break;
        }
        server.wait_ticks(1).await;
    }
    let mut lines = csv.lines();
    assert_eq!(
        lines.next(),
        Some("timestamp_ms,tps,window_ms,last_tick_ms,total_ticks")
    );
    let sample = lines.next().expect("a TPS sample within 400 ticks");
    let tps: f64 = sample.split(',').nth(1).unwrap().parse().unwrap();
    assert!(tps > 0.0 && tps <= 41.0, "{sample}");
    assert!(
        server
            .logs()
            .iter()
            .any(|record| record.message.starts_with("Slow tick detected")),
        "a slow tick warning"
    );

    // frame_limits: a 3000-byte packet passes, a 5000-byte one closes the
    // connection.
    alice.command(&"x".repeat(3000)).await;
    alice.sync().await;
    alice.assert_healthy();
    alice.command(&"x".repeat(5000)).await;
    server
        .wait_for("Alice to be disconnected", |world| {
            let online = world.query::<&PlayerUuid>().iter(world).count();
            (online == 0).then_some(())
        })
        .await;

    server.stop().await;
}

const PAINTINGS: &str = "minecraft:painting_variant";

type Names = Vec<(String, Vec<String>)>;

/// Registry ids in sending order, each with its entry ids in order.
fn names_of(registries: &[RegistryData]) -> Names {
    registries
        .iter()
        .map(|registry| {
            let entries = registry.entries.iter().map(|entry| entry.entry_id.clone());
            (registry.registry_id.clone(), entries.collect())
        })
        .collect()
}

fn entries_mut<'a>(names: &'a mut Names, id: &str) -> &'a mut Vec<String> {
    let (_, entries) = names
        .iter_mut()
        .find(|(other, _)| other == id)
        .unwrap_or_else(|| panic!("the registry {id}"));
    entries
}

/// Each registry a bot received, in arrival order, with its entries as Azalea
/// decoded them. Azalea decodes `dimension_type` and `enchantment` into its
/// own types, so only their entry names are kept.
type Received = Vec<(String, Vec<(String, Option<NbtCompound>)>)>;

fn received(bot: &Bot) -> Received {
    let transcript = bot.transcript();
    let data = &transcript.registry_data;
    let mut by_id: HashMap<String, Vec<(String, Option<NbtCompound>)>> = data
        .extra
        .iter()
        .map(|(id, registry)| {
            let entries = registry
                .map
                .iter()
                .map(|(name, nbt)| (name.to_string(), Some(nbt.clone())));
            (id.to_string(), entries.collect())
        })
        .collect();
    let dimension_types = data.dimension_type.map.keys();
    by_id.insert(
        "minecraft:dimension_type".into(),
        dimension_types
            .map(|name| (name.to_string(), None))
            .collect(),
    );
    let enchantments = data.enchantment.map.keys();
    by_id.insert(
        "minecraft:enchantment".into(),
        enchantments.map(|name| (name.to_string(), None)).collect(),
    );
    let mut seen = HashSet::new();
    transcript
        .registries
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .map(|id| (id.clone(), by_id.remove(id).unwrap_or_default()))
        .collect()
}

fn received_names(received: &Received) -> Names {
    received
        .iter()
        .map(|(id, entries)| {
            let names = entries.iter().map(|(name, _)| name.clone());
            (id.clone(), names.collect())
        })
        .collect()
}

/// Ids of the registries that differ between two clients.
fn differing(first: &Received, second: &Received) -> Vec<String> {
    let mut ids: Vec<&String> = Vec::new();
    for (id, _) in first.iter().chain(second) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let find = |received: &Received, id: &String| {
        received
            .iter()
            .find(|(other, _)| other == id)
            .map(|(_, entries)| entries.clone())
    };
    ids.into_iter()
        .filter(|id| find(first, id) != find(second, id))
        .cloned()
        .collect()
}

fn painting(received: &Received, name: &str) -> Option<NbtCompound> {
    let (_, paintings) = received
        .iter()
        .find(|(id, _)| id == PAINTINGS)
        .expect("the painting registry");
    let (_, data) = paintings
        .iter()
        .find(|(entry, _)| entry == name)
        .unwrap_or_else(|| panic!("the painting {name}"));
    data.clone()
}

/// Before any client joins, every kind of registry change succeeds and is
/// sent. Once a client has received the registries, each is refused, and a
/// client joining later is sent exactly what the first one got.
#[tokio::test]
async fn registries_change_until_a_client_receives_them() {
    let server = TestServer::builder()
        .config(|config| {
            config.configure_registries(|registries| {
                const UNSENT: &str = "no client has the registries yet";
                let aztec = registries
                    .get_entry(PAINTINGS, "minecraft:aztec")
                    .expect("the aztec painting")
                    .data
                    .clone();
                let early = RegistryEntry {
                    entry_id: "atp024:early".into(),
                    data: aztec.clone(),
                };
                registries.add_entry(PAINTINGS, early).expect(UNSENT);
                let alban = registries.remove_entry(PAINTINGS, "minecraft:alban");
                assert!(alban.expect(UNSENT).is_some());
                registries
                    .get_entry_mut(PAINTINGS, "minecraft:kebab")
                    .expect(UNSENT)
                    .expect("the kebab painting")
                    .data = aztec;
                registries
                    .get_registry_mut("minecraft:wolf_variant")
                    .expect(UNSENT)
                    .expect("the wolf variants")
                    .entries
                    .reverse();
                let cats = registries
                    .remove_registry("minecraft:cat_variant")
                    .expect(UNSENT)
                    .expect("the cat variants");
                registries.add_registry(cats).expect(UNSENT);
            })
        })
        .start()
        .await;

    // The default registries, with each change above applied.
    let mut expected = names_of(&default_registry_data());
    let paintings = entries_mut(&mut expected, PAINTINGS);
    paintings.retain(|name| name != "minecraft:alban");
    paintings.push("atp024:early".into());
    entries_mut(&mut expected, "minecraft:wolf_variant").reverse();
    let cats = expected
        .iter()
        .position(|(id, _)| id == "minecraft:cat_variant")
        .expect("the cat variants");
    let cats = expected.remove(cats);
    expected.push(cats);
    let configured = server
        .with_world(|world| names_of(world.resource::<RegistryDataStore>().registries()))
        .await;
    assert_eq!(configured, expected, "every change before login applies");

    let alice = server.join("Alice").await;
    let first = received(&alice);
    assert_eq!(received_names(&first), configured, "and is sent");
    let aztec = painting(&first, "minecraft:aztec");
    assert!(aztec.is_some());
    assert_eq!(painting(&first, "minecraft:kebab"), aztec);
    assert_eq!(painting(&first, "atp024:early"), aztec);

    let refused = server
        .with_world(|world| {
            let mut registries = world.resource_mut::<RegistryDataStore>();
            let aztec = registries
                .get_entry(PAINTINGS, "minecraft:aztec")
                .expect("the aztec painting")
                .data
                .clone();
            let late = RegistryEntry {
                entry_id: "atp024:late".into(),
                data: aztec.clone(),
            };
            vec![
                ("add_entry", registries.add_entry(PAINTINGS, late)),
                (
                    "remove_entry",
                    registries
                        .remove_entry(PAINTINGS, "minecraft:aztec")
                        .map(drop),
                ),
                (
                    "get_entry_mut",
                    registries
                        .get_entry_mut(PAINTINGS, "minecraft:bust")
                        .map(|entry| {
                            if let Some(entry) = entry {
                                entry.data = aztec;
                            }
                        }),
                ),
                (
                    "get_registry_mut",
                    registries
                        .get_registry_mut("minecraft:chicken_variant")
                        .map(|registry| {
                            if let Some(registry) = registry {
                                registry.entries.reverse();
                            }
                        }),
                ),
                (
                    "add_registry",
                    registries.add_registry(RegistryData {
                        registry_id: "atp024:late".into(),
                        entries: Vec::new(),
                    }),
                ),
                (
                    "remove_registry",
                    registries
                        .remove_registry("minecraft:frog_variant")
                        .map(drop),
                ),
            ]
        })
        .await;
    for (change, result) in refused {
        assert_eq!(result, Err(RegistriesAlreadySent), "{change} after login");
    }

    let bob = server.join("Bob").await;
    let second = received(&bob);
    assert_eq!(
        differing(&first, &second),
        Vec::<String>::new(),
        "registries Bob was sent differently from Alice"
    );
    assert_eq!(bob.transcript().registries, alice.transcript().registries);
    assert!(second == first, "Bob was sent exactly what Alice got");
    let after = server
        .with_world(|world| names_of(world.resource::<RegistryDataStore>().registries()))
        .await;
    assert_eq!(after, configured, "the store kept what Alice got");

    for bot in [&alice, &bob] {
        bot.sync().await;
        bot.assert_healthy();
    }
    server.stop().await;
}

/// How long `ticks` ticks take on a server running at `rate`.
async fn time_ticks(rate: u64, ticks: u64) -> Duration {
    let server = TestServer::builder()
        .config(move |config| config.tick_rate(rate))
        .start()
        .await;
    server.wait_ticks(1).await;
    let start = Instant::now();
    server.wait_ticks(ticks).await;
    let elapsed = start.elapsed();
    server.stop().await;
    elapsed
}

/// `n` ticks counted from inside a tick span at least `n - 1` full ticks, so
/// the lower bounds hold on any machine. The upper bound only needs the fast
/// server to beat the slow one's lower bound, with a 19x margin.
#[tokio::test]
async fn tick_rate_paces_the_game_loop() {
    let fast = time_ticks(200, 20).await;
    let slow = time_ticks(10, 20).await;

    assert!(fast >= Duration::from_millis(19 * 5), "{fast:?}");
    assert!(slow >= Duration::from_millis(19 * 100), "{slow:?}");
    assert!(fast < Duration::from_millis(19 * 100), "{fast:?}");
}

#[tokio::test]
async fn a_zero_tick_rate_runs_at_one_tick_per_second() {
    let elapsed = time_ticks(0, 2).await;
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
}

#[tokio::test]
async fn an_untouched_builder_restores_the_documented_defaults() {
    let metrics = TempDir::new("inert");
    let tps_output = metrics.0.join("tps.csv").to_string_lossy().into_owned();
    let first = TestServer::builder()
        .config(move |config| {
            config
                .max_players(5)
                .motd("first")
                .view_distance(3)
                .simulation_distance(2)
                .game_mode(0)
                .hardcore(true)
                .spawn_position(SpawnPosition {
                    x: 200.0,
                    z: 200.0,
                    y: Some(120.0),
                })
                .compression_threshold(Some(64))
                .metrics_tps_output(tps_output)
                .slow_tick_ms(1)
        })
        .plugin(work_2_ms_per_tick)
        .start()
        .await;
    let alice = first.join("Alice").await;
    assert_eq!(login_packet(&alice).await.chunk_radius, 3);
    first.wait_ticks(5).await;
    alice.sync().await;
    // metrics_tps_output and slow_tick_ms do nothing while metrics_debug is
    // off: no file, no warning, although every tick works longer than 1 ms.
    assert!(!metrics.0.exists(), "no TPS output without metrics_debug");
    assert!(
        !first
            .logs()
            .iter()
            .any(|record| record.message.starts_with("Slow tick detected"))
    );
    alice.assert_healthy();
    drop(alice);
    first.stop().await;

    let probe = Probe::default();
    let installed = probe.clone();
    let server = TestServer::builder()
        .config(|_| ServerConfigBuilder::new().address("127.0.0.1:0"))
        .plugin(move |app| installed.install(app))
        .start()
        .await;

    // spawn_position (0, 0) and spawn_chunk_radius 10.
    let spawn_chunk = ChunkPos::new(0, 0);
    assert_eq!(startup_chunks(&server).await, square(spawn_chunk, 10));

    let status = server.status().await;
    assert_eq!(status.description.to_string(), "Welcome to Void Server!");
    assert_eq!(status.players.max, 100);

    let bob = server.join("Bob").await;
    assert_eq!(bob.transcript().compression_threshold, Some(256));
    let login = login_packet(&bob).await;
    assert!(!login.hardcore);
    assert_eq!(login.max_players, 100);
    assert_eq!(login.chunk_radius, 10);
    assert_eq!(login.simulation_distance, 10);
    assert_eq!(login.common.game_type, GameMode::Creative);
    assert_eq!(tab_list_game_mode(&bob).await, GameMode::Creative);

    // y: None spawns on the default generator's surface.
    let surface = DefaultWorldGenerator::default().surface_height_at(0, 0);
    let spawn = Vec3::new(0.0, f64::from(surface), 0.0);
    bob.wait_until("the default spawn", |view| view.position == spawn)
        .await;
    assert_eq!(ground(&bob).y, surface - 1);

    // initial_chunk_radius 3, then Bob's own 8 under the cap of 10.
    let joined_with = probe.joined_with.lock().unwrap().clone();
    assert_eq!(joined_with, Some(square(spawn_chunk, 3)));
    bob.wait_until("the chunks Bob asked for", |view| {
        view.chunks.keys().copied().collect::<HashSet<_>>() == square(spawn_chunk, 8)
    })
    .await;

    // tick_rate 20: 20 ticks span at least 19 × 50 ms.
    server.wait_ticks(1).await;
    let start = Instant::now();
    server.wait_ticks(20).await;
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(19 * 50), "{elapsed:?}");

    bob.sync().await;
    bob.assert_healthy();
    server.stop().await;
}

const SERVER_FULL: &str = "multiplayer.disconnect.server_full";

/// The login of `name` is refused with vanilla's "server is full" reason.
async fn assert_refused_as_full(server: &TestServer, name: &str) {
    let refused = match server.try_join(name).await {
        Ok(bot) => panic!("{} joined a full server", bot.name()),
        Err(refused) => refused,
    };
    assert_server_full(&refused);
}

fn assert_server_full(refused: &LoginRefused) {
    assert_eq!(refused.component["translate"], SERVER_FULL, "{refused}");
    assert_eq!(refused.reason, "The server is full!");
}

/// Waits until the server holds no logged-in connection any more.
async fn wait_until_empty(server: &TestServer) {
    server
        .wait_for("every player to be gone", |world| {
            let online = world.query::<&PlayerUuid>().iter(world).count();
            (online == 0).then_some(())
        })
        .await;
}

/// `max_players` is advertised and enforced: once that many players are
/// connected, the next login is refused with vanilla's "server is full"
/// reason, and the players already in are unaffected.
#[tokio::test]
async fn max_players_is_advertised_and_enforced() {
    let server = TestServer::builder()
        .config(|config| config.max_players(1))
        .start()
        .await;
    let alice = server.join("Alice").await;
    assert_refused_as_full(&server, "Bob").await;

    let mut status = server.status().await;
    for _ in 0..100 {
        if status.players.online == 1 {
            break;
        }
        server.wait_ticks(1).await;
        status = server.status().await;
    }
    assert_eq!(status.players.max, 1);
    assert_eq!(status.players.online, 1);
    assert_eq!(login_packet(&alice).await.max_players, 1);

    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}

/// A player leaving frees its slot, and a refused login never took one.
#[tokio::test]
async fn a_slot_frees_up_when_its_player_leaves() {
    let server = TestServer::builder()
        .config(|config| config.max_players(1))
        .start()
        .await;
    let alice = server.join("Alice").await;
    assert_refused_as_full(&server, "Bob").await;

    alice.disconnect().await;
    wait_until_empty(&server).await;
    let carol = server
        .try_join("Carol")
        .await
        .unwrap_or_else(|refused| panic!("Carol was refused a free slot: {refused}"));

    carol.sync().await;
    carol.assert_healthy();
    server.stop().await;
}

/// Every connection past login holds a slot, also one still configuring that
/// has not entered the play state.
#[tokio::test]
async fn a_player_still_configuring_holds_a_slot() {
    let server = TestServer::builder()
        .config(|config| config.max_players(1))
        .start()
        .await;
    let alice = server
        .log_in("Alice")
        .await
        .unwrap_or_else(|refused| panic!("Alice was refused an empty server: {refused}"));
    assert_refused_as_full(&server, "Bob").await;

    alice.disconnect().await;
    wait_until_empty(&server).await;
    let carol = server
        .try_join("Carol")
        .await
        .unwrap_or_else(|refused| panic!("Carol was refused a free slot: {refused}"));

    carol.sync().await;
    carol.assert_healthy();
    server.stop().await;
}

/// Two logins sent together for the last free slot: exactly one gets in, the
/// other is refused as full.
#[tokio::test]
async fn two_logins_for_the_last_slot_let_exactly_one_in() {
    let server = TestServer::builder()
        .config(|config| config.max_players(2))
        .start()
        .await;
    let alice = server.join("Alice").await;

    let (bob, carol) = tokio::join!(server.try_join("Bob"), server.try_join("Carol"));
    let (winner, refused) = match (bob, carol) {
        (Ok(winner), Err(refused)) | (Err(refused), Ok(winner)) => (winner, refused),
        (Ok(_), Ok(_)) => panic!("both Bob and Carol joined a server with one free slot"),
        (Err(bob), Err(carol)) => panic!("both were refused: Bob {bob}, Carol {carol}"),
    };
    assert_server_full(&refused);

    alice.sync().await;
    winner.sync().await;
    alice.assert_healthy();
    winner.assert_healthy();
    server.stop().await;
}

/// The default `max_players` (100) refuses none of a few players logging in
/// at once.
#[tokio::test]
async fn the_default_configuration_refuses_nobody() {
    let server = TestServer::start().await;

    let (alice, bob, carol, dave) = tokio::join!(
        server.try_join("Alice"),
        server.try_join("Bob"),
        server.try_join("Carol"),
        server.try_join("Dave"),
    );
    for bot in [alice, bob, carol, dave] {
        let bot = bot.unwrap_or_else(|refused| panic!("refused by default: {refused}"));
        bot.sync().await;
        bot.assert_healthy();
    }
    server.stop().await;
}

/// Like vanilla, `max_players(0)` refuses everyone.
#[tokio::test]
async fn zero_max_players_refuses_everyone() {
    let server = TestServer::builder()
        .config(|config| config.max_players(0))
        .start()
        .await;
    assert_refused_as_full(&server, "Alice").await;
    server.stop().await;
}

#[tokio::test]
async fn negative_distances_are_read_as_zero() {
    let server = TestServer::builder()
        .config(|config| {
            config
                .view_distance(-3)
                .simulation_distance(-1)
                .initial_chunk_radius(0)
        })
        .start()
        .await;
    let alice = server.join("Alice").await;

    let login = login_packet(&alice).await;
    assert_eq!(login.chunk_radius, 0);
    assert_eq!(login.simulation_distance, 0);

    let own = ChunkPos::from(feet(&alice));
    alice
        .wait_until("its own chunk", |view| view.chunks.contains_key(&own))
        .await;
    alice.sync().await;
    assert_eq!(loaded(&alice), square(own, 0));
    ground(&alice);

    alice.assert_healthy();
    server.stop().await;
}

/// Thresholds above `i32::MAX` cannot be announced as they are; the server
/// announces `i32::MAX`, which compresses no packet on either side.
#[tokio::test]
async fn the_largest_compression_threshold_is_announced_as_i32_max() {
    let server = TestServer::builder()
        .compression_threshold(Some(u32::MAX))
        .start()
        .await;
    let alice = server.join("Alice").await;

    assert_eq!(alice.transcript().compression_threshold, Some(i32::MAX));
    ground(&alice);

    alice.sync().await;
    alice.assert_healthy();
    server.stop().await;
}
