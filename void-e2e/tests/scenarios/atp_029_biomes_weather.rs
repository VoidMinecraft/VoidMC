//! VMC-ATP-029: configured biome and weather states reach the client and stay
//! in step with the server.
//!
//! The scenario fixture: a flat world whose test area, the 4×4×4 cells with
//! `x` in `100..124`, `y` in `32..128` and `z` in `-12..12`, is
//! `minecraft:snowy_plains` (set through `WorldGenerator::cell_biome`) while
//! every other cell is `minecraft:plains`; the area straddles chunk borders,
//! so its chunks mix both biomes. The world weather starts clear and its
//! configured transition is a 100-tick ramp of the rain and thunder levels
//! (`Weather::transition`), the vanilla fade. Clients decode chunks and
//! registries with Azalea, which shares no code with VoidMC.

use std::collections::{BTreeMap, HashSet};

use bevy_app::{App, PostUpdate, Update};
use bevy_ecs::prelude::*;
use voidmc::components::{PlayerReady, Position};
use voidmc::{
    Audience, BiomeId, ChunkPos as ServerChunkPos, Players, SpawnPosition, VoidSystems, Weather,
    WeatherState, WorldGenerator,
};
use voidmc_e2e::core::position::{ChunkPos, ChunkSectionBiomePos, Vec3};
use voidmc_e2e::protocol::packets::game::ClientboundGamePacket;
use voidmc_e2e::protocol::packets::game::c_game_event::EventType;
use voidmc_e2e::registry::DataRegistry;
use voidmc_e2e::registry::builtin::EntityKind;
use voidmc_e2e::registry::identifier::Identifier;
use voidmc_e2e::{Bot, BotOptions, TestServer};
use voidmc_protocol::clientbound::Ping;
use voidmc_protocol::clientbound::chunk::{Chunk, ChunkBuilder, blocks};

use crate::feet;

const AREA_BIOME: &str = "minecraft:snowy_plains";
const DEFAULT_BIOME: &str = "minecraft:plains";
const AREA_MIN: [i32; 3] = [100, 32, -12];
const AREA_MAX: [i32; 3] = [124, 128, 12];
/// A spot in the middle of the test area, in chunk (7, 0).
const AREA_CENTER: (f64, f64) = (112.5, 0.5);
const AREA_CHUNKS: [(i32, i32); 4] = [(6, -1), (6, 0), (7, -1), (7, 0)];
const TRANSITION: u32 = 100;
const SURFACE_Y: i32 = 64;
const VIEW_DISTANCE: u8 = 2;
const MIN_Y: i32 = -64;
const SECTIONS: usize = 24;
/// Tags the per-tick marker pings; the harness's own barriers use bit 31.
const MARKER: u32 = 0x4000_0000;

/// The configured biome of the cell whose lowest corner is at these block
/// coordinates.
fn configured_biome(x: i32, y: i32, z: i32) -> &'static str {
    let inside = (AREA_MIN[0]..AREA_MAX[0]).contains(&x)
        && (AREA_MIN[1]..AREA_MAX[1]).contains(&y)
        && (AREA_MIN[2]..AREA_MAX[2]).contains(&z);
    if inside { AREA_BIOME } else { DEFAULT_BIOME }
}

fn in_area_column(x: f64, z: f64) -> bool {
    (f64::from(AREA_MIN[0])..f64::from(AREA_MAX[0])).contains(&x)
        && (f64::from(AREA_MIN[2])..f64::from(AREA_MAX[2])).contains(&z)
}

struct ScenarioWorld;

impl WorldGenerator for ScenarioWorld {
    fn generate_chunk(&self, pos: &ServerChunkPos) -> Chunk {
        ChunkBuilder::new(pos.x, pos.z)
            .biomes_from(|x, y, z| self.cell_biome(x, y, z).0)
            .fill_below(SURFACE_Y - 1, blocks::STONE)
            .fill_range(SURFACE_Y - 1, SURFACE_Y, blocks::GRASS_BLOCK)
            .build()
    }

    fn surface_height_at(&self, _block_x: i32, _block_z: i32) -> i32 {
        SURFACE_Y
    }

    fn cell_biome(&self, x: i32, y: i32, z: i32) -> BiomeId {
        BiomeId::named(configured_biome(x, y, z)).expect("a vanilla biome")
    }
}

/// The world weather of the scenario, under `Audience::All`.
#[derive(Component)]
struct WorldWeather;

/// Weather seen only by the players standing in the test area.
#[derive(Component)]
struct AreaStorm;

async fn start(spawn: (f64, f64), setup: impl FnOnce(&mut App) + Send + 'static) -> TestServer {
    TestServer::builder()
        .config(move |config| {
            config
                .world_generator(ScenarioWorld)
                .spawn_position(SpawnPosition {
                    x: spawn.0,
                    z: spawn.1,
                    y: None,
                })
                .view_distance(i32::from(VIEW_DISTANCE))
        })
        .plugin(move |app| {
            app.world_mut()
                .spawn((Weather::clear().transition(TRANSITION), WorldWeather));
            setup(app);
        })
        .start()
        .await
}

async fn join(server: &TestServer, name: &str) -> Bot {
    server
        .join_with(
            name,
            BotOptions {
                view_distance: VIEW_DISTANCE,
            },
        )
        .await
}

fn square(center: ChunkPos, radius: i32) -> HashSet<ChunkPos> {
    (-radius..=radius)
        .flat_map(|dx| {
            (-radius..=radius).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz))
        })
        .collect()
}

async fn wait_for_chunks_around(bot: &Bot, center: ChunkPos) {
    let expected = square(center, i32::from(VIEW_DISTANCE));
    bot.wait_until(&format!("the chunks around {center:?}"), |view| {
        view.chunks.keys().copied().collect::<HashSet<_>>() == expected
    })
    .await;
}

/// Walks in steps of at most 8 blocks, like the other movement scenarios.
async fn walk_to(bot: &Bot, x: f64, z: f64) {
    let mut position = bot.position();
    while position.x != x || position.z != z {
        position = Vec3::new(
            position.x + (x - position.x).clamp(-8.0, 8.0),
            position.y,
            position.z + (z - position.z).clamp(-8.0, 8.0),
        );
        bot.move_to(position).await;
    }
}

// ---- biomes, as the client decoded them

fn biome_registry_names(bot: &Bot) -> Vec<String> {
    bot.transcript()
        .registry_data
        .extra
        .get(&Identifier::new("minecraft:worldgen/biome"))
        .unwrap_or_else(|| panic!("{} received no biome registry", bot.name()))
        .map
        .keys()
        .map(ToString::to_string)
        .collect()
}

#[derive(Debug, PartialEq)]
enum Precipitation {
    None,
    Rain,
    Snow,
}

/// The client-visible climate and colours of a biome, read from the
/// registry entry the client received.
#[derive(Debug, PartialEq)]
struct BiomeLook {
    has_precipitation: bool,
    temperature: f32,
    downfall: f32,
    water_color: String,
    sky_color: String,
}

impl BiomeLook {
    /// `Biome::getPrecipitationAt` below the height where temperature drops.
    fn precipitation(&self) -> Precipitation {
        if !self.has_precipitation {
            Precipitation::None
        } else if self.temperature < 0.15 {
            Precipitation::Snow
        } else {
            Precipitation::Rain
        }
    }
}

fn biome_look(bot: &Bot, name: &str) -> BiomeLook {
    let registry = bot
        .transcript()
        .registry_data
        .extra
        .get(&Identifier::new("minecraft:worldgen/biome"))
        .expect("a biome registry");
    let entry = registry
        .map
        .get(&Identifier::new(name))
        .unwrap_or_else(|| panic!("{} has no biome {name}", bot.name()));
    macro_rules! color {
        ($tag:expr) => {{
            let tag = $tag.unwrap_or_else(|| panic!("{name} lacks a colour"));
            tag.string()
                .map(|text| text.to_str().into_owned())
                .or_else(|| tag.int().map(|rgb| format!("#{rgb:06x}")))
                .unwrap_or_else(|| panic!("{name} has a colour of an unexpected type: {tag:?}"))
        }};
    }
    BiomeLook {
        has_precipitation: entry.byte("has_precipitation") == Some(1),
        temperature: entry.float("temperature").expect("temperature"),
        downfall: entry.float("downfall").expect("downfall"),
        water_color: color!(entry.compound("effects").and_then(|e| e.get("water_color"))),
        sky_color: color!(
            entry
                .compound("attributes")
                .and_then(|a| a.get("minecraft:visual/sky_color"))
        ),
    }
}

/// Values of the vanilla 26.1.2 data pack, the reference a vanilla client
/// would render.
fn assert_registry_carries_the_configured_biomes(bot: &Bot) {
    let snowy = biome_look(bot, AREA_BIOME);
    assert_eq!(
        snowy,
        BiomeLook {
            has_precipitation: true,
            temperature: 0.0,
            downfall: 0.5,
            water_color: "#3f76e4".into(),
            sky_color: "#7fa1ff".into(),
        },
        "{AREA_BIOME} as {} received it",
        bot.name()
    );
    assert_eq!(snowy.precipitation(), Precipitation::Snow);
    let plains = biome_look(bot, DEFAULT_BIOME);
    assert_eq!(
        plains,
        BiomeLook {
            has_precipitation: true,
            temperature: 0.8,
            downfall: 0.4,
            water_color: "#3f76e4".into(),
            sky_color: "#78a7ff".into(),
        },
        "{DEFAULT_BIOME} as {} received it",
        bot.name()
    );
    assert_eq!(plains.precipitation(), Precipitation::Rain);
    assert_eq!(
        biome_look(bot, "minecraft:desert").precipitation(),
        Precipitation::None
    );
}

/// Every biome cell of every chunk the bot holds, resolved to a name through
/// the registry it received: chunk -> cells, bottom section first.
fn held_biomes(bot: &Bot) -> BTreeMap<(i32, i32), Vec<String>> {
    let names = biome_registry_names(bot);
    bot.view(|view| {
        view.chunks
            .iter()
            .map(|(pos, chunk)| {
                assert_eq!(chunk.sections.len(), SECTIONS);
                let cells = chunk
                    .sections
                    .iter()
                    .flat_map(|section| {
                        cell_coordinates().map(move |(x, y, z)| {
                            section
                                .biomes
                                .get(ChunkSectionBiomePos { x, y, z })
                                .protocol_id()
                        })
                    })
                    .map(|id| {
                        names
                            .get(id as usize)
                            .cloned()
                            .unwrap_or_else(|| format!("<biome id {id} outside the registry>"))
                    })
                    .collect();
                ((pos.x, pos.z), cells)
            })
            .collect()
    })
}

fn cell_coordinates() -> impl Iterator<Item = (u8, u8, u8)> {
    (0..4u8).flat_map(|y| (0..4u8).flat_map(move |z| (0..4u8).map(move |x| (x, y, z))))
}

/// Compares every held cell with the fixture; returns the area chunks held.
fn assert_biomes_match_the_fixture(bot: &Bot) -> usize {
    let held = held_biomes(bot);
    assert!(!held.is_empty(), "{} holds no chunk", bot.name());
    let mut wrong = Vec::new();
    for (&(chunk_x, chunk_z), cells) in &held {
        let expected = (0..SECTIONS as i32).flat_map(|section| {
            cell_coordinates().map(move |(x, y, z)| {
                configured_biome(
                    chunk_x * 16 + i32::from(x) * 4,
                    MIN_Y + section * 16 + i32::from(y) * 4,
                    chunk_z * 16 + i32::from(z) * 4,
                )
            })
        });
        for (index, (got, want)) in cells.iter().zip(expected).enumerate() {
            if got != want {
                wrong.push(format!(
                    "chunk ({chunk_x}, {chunk_z}) cell {index}: {got}, expected {want}"
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} sees {} misplaced biome cell(s), first ones: {:#?}",
        bot.name(),
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );
    held.keys().filter(|pos| AREA_CHUNKS.contains(pos)).count()
}

/// The biome the bot's client renders at its feet.
fn biome_at_feet(bot: &Bot) -> String {
    let feet = feet(bot);
    let chunk = ChunkPos::from(feet);
    let names = biome_registry_names(bot);
    bot.view(|view| {
        let chunk = view.chunks.get(&chunk).expect("the chunk under the bot");
        let section = ((feet.y - MIN_Y) / 16) as usize;
        let id = chunk.sections[section]
            .biomes
            .get(ChunkSectionBiomePos {
                x: (feet.x.rem_euclid(16) / 4) as u8,
                y: (feet.y.rem_euclid(16) / 4) as u8,
                z: (feet.z.rem_euclid(16) / 4) as u8,
            })
            .protocol_id();
        names[id as usize].clone()
    })
}

// ---- weather, as the client and the server see it

#[derive(Clone, Copy, Debug, PartialEq)]
enum Seen {
    Begin,
    End,
    Rain(f32),
    Thunder(f32),
    Tick(u32),
}

fn weather_event(packet: &ClientboundGamePacket) -> Option<Seen> {
    match packet {
        ClientboundGamePacket::GameEvent(event) => match event.event {
            EventType::StartRaining => Some(Seen::Begin),
            EventType::StopRaining => Some(Seen::End),
            EventType::RainLevelChange => Some(Seen::Rain(event.param)),
            EventType::ThunderLevelChange => Some(Seen::Thunder(event.param)),
            _ => None,
        },
        _ => None,
    }
}

fn weather_or_marker(packet: &ClientboundGamePacket) -> Option<Seen> {
    match packet {
        ClientboundGamePacket::Ping(ping) if ping.id & 0xC000_0000 == MARKER => {
            Some(Seen::Tick(ping.id & !MARKER))
        }
        packet => weather_event(packet),
    }
}

/// The weather a client shows: a vanilla client starts clear, and
/// `ClientPacketListener::handleGameEvent` sets the rain level to 0 on Begin
/// Raining and to 1 on End Raining before the level packets that follow.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sky {
    raining: bool,
    rain: f32,
    thunder: f32,
}

const CLEAR: Sky = Sky {
    raining: false,
    rain: 0.0,
    thunder: 0.0,
};

/// Equal to nothing, so a fresh `Watch` waits until the full state arrives.
const UNKNOWN: Sky = Sky {
    raining: true,
    rain: f32::NAN,
    thunder: f32::NAN,
};

impl Sky {
    fn apply(&mut self, seen: Seen) {
        match seen {
            Seen::Begin => {
                self.raining = true;
                self.rain = 0.0;
            }
            Seen::End => {
                self.raining = false;
                self.rain = 1.0;
            }
            Seen::Rain(level) => self.rain = level,
            Seen::Thunder(level) => self.thunder = level,
            Seen::Tick(_) => {}
        }
    }
}

/// One bot's weather, folded from the Game Event packets it took off its
/// queue.
struct Watch<'a> {
    bot: &'a Bot,
    sky: Sky,
}

impl<'a> Watch<'a> {
    fn new(bot: &'a Bot) -> Self {
        Self { bot, sky: UNKNOWN }
    }

    /// Applies weather events until the client shows `target`, then checks
    /// that the rest of that tick's events leave it there without flipping
    /// the raining flag the other way.
    async fn settles_on(&mut self, what: &str, target: Sky) {
        while self.sky != target {
            let seen = self.bot.expect(what, weather_event).await;
            self.sky.apply(seen);
        }
        self.bot.sync().await;
        for seen in self.bot.queued(weather_event) {
            let wrong_flag = match seen {
                Seen::Begin => !target.raining,
                Seen::End => target.raining,
                _ => false,
            };
            assert!(
                !wrong_flag,
                "{} got {seen:?} after reaching {what}",
                self.bot.name()
            );
            self.bot.expect(what, weather_event).await;
            self.sky.apply(seen);
        }
        assert_eq!(self.sky, target, "{} once {what} settled", self.bot.name());
    }

    async fn is_quiet(&self, why: &str) {
        self.bot.sync().await;
        let queued = self.bot.queued(weather_event);
        assert!(
            queued.is_empty(),
            "{} got weather events while {why}: {queued:?}",
            self.bot.name()
        );
    }
}

/// The world weather after the sync of one tick: what the server holds in
/// `WeatherState` (and the target it ramps to).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Logged {
    sky: Sky,
    target: Sky,
}

#[derive(Resource, Default)]
struct WeatherLog {
    tick: u32,
    ticks: BTreeMap<u32, Logged>,
}

/// After the weather sync of every tick: logs the server state and sends
/// every ready player a marker ping, so each client stream can be cut at
/// tick boundaries.
fn log_weather(
    mut log: ResMut<WeatherLog>,
    weather: Query<(&Weather, &WeatherState), With<WorldWeather>>,
    players: Players,
) {
    let Ok((weather, state)) = weather.single() else {
        return;
    };
    log.tick += 1;
    let tick = log.tick;
    let (rain, thunder) = state.levels();
    let (target_rain, target_thunder) = weather.kind.levels();
    log.ticks.insert(
        tick,
        Logged {
            sky: Sky {
                raining: weather.is_raining(),
                rain,
                thunder,
            },
            target: Sky {
                raining: weather.is_raining(),
                rain: target_rain,
                thunder: target_thunder,
            },
        },
    );
    players.ready().send(Ping {
        id: (MARKER | tick) as i32,
    });
}

fn world_weather(world: &mut World) -> Mut<'_, Weather> {
    let entity = world
        .query_filtered::<Entity, With<WorldWeather>>()
        .single(world)
        .expect("one world weather");
    world.get_mut::<Weather>(entity).expect("weather")
}

fn world_levels(world: &mut World) -> (f32, f32) {
    world
        .query_filtered::<&WeatherState, With<WorldWeather>>()
        .single(world)
        .expect("one world weather")
        .levels()
}

fn sees_world_weather(world: &mut World, player: Entity) -> bool {
    world
        .query_filtered::<&WeatherState, With<WorldWeather>>()
        .single(world)
        .expect("one world weather")
        .viewers()
        .any(|viewer| viewer == player)
}

/// Checks a bot's whole weather stream against the server log, tick by tick.
/// Returns the server state at the bot's first marker.
fn assert_lockstep(bot: &Bot, log: &BTreeMap<u32, Logged>) -> Logged {
    let stream = bot.queued(weather_or_marker);
    let name = bot.name();
    let mut sky = CLEAR;
    let mut flag_known = false;
    let (mut flags, mut rains, mut thunders) = (0, 0, 0);
    let mut previous: Option<(u32, Logged)> = None;
    let mut first = None;
    for seen in &stream {
        sky.apply(*seen);
        match *seen {
            Seen::Begin | Seen::End => {
                flags += 1;
                flag_known = true;
            }
            Seen::Rain(_) => rains += 1,
            Seen::Thunder(_) => thunders += 1,
            Seen::Tick(tick) => {
                let server = *log
                    .get(&tick)
                    .unwrap_or_else(|| panic!("{name} got a marker for unlogged tick {tick}"));
                assert!(
                    flag_known,
                    "{name} had no raining flag when it became ready (tick {tick})"
                );
                assert_eq!(
                    sky, server.sky,
                    "{name} shows other weather than the server at tick {tick}"
                );
                let flipped = match previous {
                    Some((last, before)) => {
                        assert_eq!(tick, last + 1, "{name} missed the ticks after {last}");
                        before.sky.raining != server.sky.raining
                    }
                    None => true,
                };
                assert_eq!(
                    flags,
                    usize::from(flipped),
                    "{name} got {flags} start/stop event(s) in tick {tick}"
                );
                assert!(
                    rains <= 1 && thunders <= 1,
                    "{name} got {rains} rain and {thunders} thunder level(s) in tick {tick}"
                );
                (flags, rains, thunders) = (0, 0, 0);
                previous = Some((tick, server));
                first.get_or_insert(server);
            }
        }
    }
    assert_eq!(
        (flags, rains, thunders),
        (0, 0, 0),
        "{name} got weather events after the last marker"
    );
    first.unwrap_or_else(|| panic!("{name} never got a marker"))
}

/// The configured ramp: each level moves linearly from where it stood to the
/// target in exactly `TRANSITION` ticks, lands on the target exactly and
/// stays there. Returns `(completed ramps, interrupted ramps)`.
fn assert_ramps_follow_the_configured_transition(log: &BTreeMap<u32, Logged>) -> (usize, usize) {
    let ticks: Vec<(u32, Logged)> = log.iter().map(|(tick, logged)| (*tick, *logged)).collect();
    let mut starts: Vec<usize> = (1..ticks.len())
        .filter(|&i| ticks[i].1.target != ticks[i - 1].1.target)
        .collect();
    starts.push(ticks.len());
    let (mut completed, mut interrupted) = (0, 0);
    for pair in starts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let from = ticks[start - 1].1.sky;
        let target = ticks[start].1.target;
        let n = TRANSITION as usize;
        for (k, (tick, logged)) in ticks[start..end].iter().enumerate() {
            let step = (k + 1).min(n) as f32 / TRANSITION as f32;
            let expect = |from: f32, to: f32| from + (to - from) * step;
            if k + 1 >= n {
                assert_eq!(logged.sky, target, "tick {tick}: the ramp ended off target");
            } else {
                for (got, want) in [
                    (logged.sky.rain, expect(from.rain, target.rain)),
                    (logged.sky.thunder, expect(from.thunder, target.thunder)),
                ] {
                    assert!(
                        (got - want).abs() < 1e-6,
                        "tick {tick}, step {}: level {got}, the linear ramp gives {want}",
                        k + 1
                    );
                }
            }
        }
        if end - start >= n {
            completed += 1;
        } else {
            interrupted += 1;
        }
    }
    (completed, interrupted)
}

// ---- scenarios

/// Steps 1–3 with weather scoped to the test area: a player walking in sees
/// the area's biome and its storm, a player at spawn sees neither, and both
/// come back identical after leaving and re-entering.
#[tokio::test]
async fn travelling_into_the_test_area_shows_its_biome_and_its_weather() {
    let server = start((0.5, 0.5), |app| {
        app.world_mut()
            .spawn((Weather::rain().viewers([]), AreaStorm));
        app.add_systems(Update, storm_over_the_area);
    })
    .await;
    let alice = join(&server, "Alice").await;
    let bob = join(&server, "Bob").await;
    let mut alice_sky = Watch::new(&alice);
    let mut bob_sky = Watch::new(&bob);

    for bot in [&alice, &bob] {
        assert_registry_carries_the_configured_biomes(bot);
        wait_for_chunks_around(bot, ChunkPos::new(0, 0)).await;
        assert_eq!(assert_biomes_match_the_fixture(bot), 0);
        assert_eq!(biome_at_feet(bot), DEFAULT_BIOME);
    }
    alice_sky.settles_on("the clear world weather", CLEAR).await;
    bob_sky.settles_on("the clear world weather", CLEAR).await;

    let storm = Sky {
        raining: true,
        rain: 1.0,
        thunder: 0.0,
    };
    walk_to(&alice, AREA_CENTER.0, AREA_CENTER.1).await;
    wait_for_chunks_around(&alice, ChunkPos::new(7, 0)).await;
    assert_eq!(assert_biomes_match_the_fixture(&alice), AREA_CHUNKS.len());
    assert_eq!(biome_at_feet(&alice), AREA_BIOME);
    alice_sky.settles_on("the storm over the area", storm).await;
    let first_visit = held_biomes(&alice);
    bob_sky.is_quiet("only Alice is in the area").await;

    walk_to(&alice, 0.5, 0.5).await;
    wait_for_chunks_around(&alice, ChunkPos::new(0, 0)).await;
    alice_sky
        .settles_on("the clear world weather again", CLEAR)
        .await;
    assert_eq!(biome_at_feet(&alice), DEFAULT_BIOME);

    walk_to(&alice, AREA_CENTER.0, AREA_CENTER.1).await;
    wait_for_chunks_around(&alice, ChunkPos::new(7, 0)).await;
    alice_sky.settles_on("the storm again", storm).await;
    assert_eq!(assert_biomes_match_the_fixture(&alice), AREA_CHUNKS.len());
    assert!(
        held_biomes(&alice) == first_visit,
        "the area looks different on the second visit"
    );
    bob_sky.is_quiet("only Alice is in the area").await;
    assert_eq!(assert_biomes_match_the_fixture(&bob), 0);

    let carol = join(&server, "Carol").await;
    assert_registry_carries_the_configured_biomes(&carol);
    assert_eq!(biome_registry_names(&carol), biome_registry_names(&alice));
    wait_for_chunks_around(&carol, ChunkPos::new(0, 0)).await;
    assert_eq!(assert_biomes_match_the_fixture(&carol), 0);
    Watch::new(&carol)
        .settles_on("the clear world weather", CLEAR)
        .await;

    for bot in [&alice, &bob, &carol] {
        bot.assert_healthy();
    }
    server.stop().await;
}

fn storm_over_the_area(
    players: Query<(Entity, &Position), With<PlayerReady>>,
    mut storms: Query<&mut Weather, With<AreaStorm>>,
) {
    let inside: HashSet<Entity> = players
        .iter()
        .filter(|(_, position)| in_area_column(position.x, position.z))
        .map(|(entity, _)| entity)
        .collect();
    for mut storm in &mut storms {
        if !matches!(&storm.audience, Audience::Explicit(current) if *current == inside) {
            storm.audience = Audience::Explicit(inside.clone());
        }
    }
}

/// Step 4: the world weather goes from clear to thunder over the configured
/// transition, back towards clear, is reversed mid-ramp, and finally clears.
/// Every tick, each client shows exactly the server's `WeatherState`; a
/// player joining mid-transition is in step from its first tick.
#[tokio::test]
async fn weather_transitions_stay_in_step_with_the_server_every_tick() {
    let server = start(AREA_CENTER, |app| {
        app.init_resource::<WeatherLog>()
            .add_systems(PostUpdate, log_weather.after(VoidSystems::WeatherSync));
    })
    .await;
    let alice = join(&server, "Alice").await;
    let alice_entity = server.player_entity(alice.uuid()).await;
    assert_registry_carries_the_configured_biomes(&alice);
    wait_for_chunks_around(&alice, ChunkPos::new(7, 0)).await;
    assert_eq!(assert_biomes_match_the_fixture(&alice), AREA_CHUNKS.len());
    assert_eq!(biome_at_feet(&alice), AREA_BIOME);
    server
        .wait_for("Alice to see the world weather", move |world| {
            sees_world_weather(world, alice_entity).then_some(())
        })
        .await;

    server
        .with_world(|world| world_weather(world).set_thunder())
        .await;
    server
        .wait_for("the thunder ramp to end", |world| {
            (world_levels(world) == (1.0, 1.0)).then_some(())
        })
        .await;

    server
        .with_world(|world| world_weather(world).set_clear())
        .await;
    server
        .wait_for("the clearing ramp to be under way", |world| {
            (world_levels(world).0 <= 0.9).then_some(())
        })
        .await;
    let bob = join(&server, "Bob").await;
    let bob_entity = server.player_entity(bob.uuid()).await;
    server
        .wait_for("Bob to see the world weather", move |world| {
            sees_world_weather(world, bob_entity).then_some(())
        })
        .await;
    let reversed_at = server
        .with_world(|world| {
            let levels = world_levels(world);
            world_weather(world).set_thunder();
            levels
        })
        .await;
    assert!(
        reversed_at.0 > 0.0 && reversed_at.0 < 1.0,
        "the clearing ramp ended before Bob joined: {reversed_at:?}"
    );
    server
        .wait_for("the reversed ramp to end", |world| {
            (world_levels(world) == (1.0, 1.0)).then_some(())
        })
        .await;

    server
        .with_world(|world| world_weather(world).set_clear())
        .await;
    server
        .wait_for("the weather to clear", |world| {
            (world_levels(world) == (0.0, 0.0)).then_some(())
        })
        .await;
    server.wait_ticks(u64::from(TRANSITION)).await;
    alice.sync().await;
    bob.sync().await;

    let log = server
        .with_world(|world| world.resource::<WeatherLog>().ticks.clone())
        .await;
    assert_eq!(assert_ramps_follow_the_configured_transition(&log), (3, 1));
    let last = log.values().next_back().expect("a logged tick");
    assert_eq!(last.sky, CLEAR);
    let (&cleared, _) = log
        .iter()
        .rev()
        .find(|(_, logged)| logged.sky != CLEAR)
        .expect("a stormy tick");
    let (&final_tick, _) = log.iter().next_back().expect("a logged tick");
    assert!(
        final_tick - cleared >= TRANSITION,
        "the sky must stay clear on its own after the last ramp"
    );

    let alice_first = assert_lockstep(&alice, &log);
    assert_eq!(alice_first.sky, CLEAR);
    let bob_first = assert_lockstep(&bob, &log);
    assert!(
        !bob_first.sky.raining && bob_first.sky.rain > 0.0 && bob_first.sky.rain < 1.0,
        "Bob should have joined mid-transition: {bob_first:?}"
    );
    assert_eq!(biome_at_feet(&bob), AREA_BIOME);
    assert_eq!(assert_biomes_match_the_fixture(&bob), AREA_CHUNKS.len());

    // Nothing beyond the configured state is simulated: no snow or ice
    // placed in the snowy area, no lightning or mobs, no biome change.
    for bot in [&alice, &bob] {
        let simulated = bot.queued(|packet| match packet {
            ClientboundGamePacket::BlockUpdate(_)
            | ClientboundGamePacket::SectionBlocksUpdate(_)
            | ClientboundGamePacket::ChunksBiomes(_) => Some(format!("{packet:?}")),
            ClientboundGamePacket::AddEntity(add) if add.entity_type != EntityKind::Player => {
                Some(format!("{packet:?}"))
            }
            _ => None,
        });
        assert!(
            simulated.is_empty(),
            "{} saw simulated world changes: {simulated:#?}",
            bot.name()
        );
        bot.assert_healthy();
    }
    server.stop().await;
}
