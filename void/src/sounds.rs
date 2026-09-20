//! Fire-and-forget sound playback: [`Sounds`] from systems, [`WorldSounds`]
//! from `&World`. A [`Sound`] describes what to play and where; the audience
//! defaults to the players who can see the emitting position.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc_data::Version;
use voidmc_protocol::clientbound::{
    ClientboundPacket, EntitySoundEffect, SoundEffect, SoundEvent, StopSound,
};

pub use voidmc_protocol::clientbound::SoundSource;

use crate::components::{EntityDimension, MinecraftEntityId, PlayerDimension, Position};
use crate::players::{Audience, Players, Recipients, WorldPlayers};
use crate::world::{ChunkPos, DimensionId};

const VERSION: Version = Version::V26_1_2;

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Named(String),
    Custom {
        sound_id: String,
        fixed_range: Option<f32>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Emitter {
    Unplaced,
    At {
        x: f64,
        y: f64,
        z: f64,
        dimension: Option<DimensionId>,
    },
    Entity(Entity),
}

#[derive(Debug, Clone)]
pub struct Sound {
    event: Event,
    source: SoundSource,
    volume: f32,
    pitch: f32,
    seed: i64,
    emitter: Emitter,
    audience: Option<Audience>,
}

impl Sound {
    /// A `minecraft:sound_event` registry entry, e.g. `"entity.player.levelup"`
    /// or `"minecraft:entity.player.levelup"`; resolved when played.
    pub fn new(name: impl Into<String>) -> Self {
        Self::build(Event::Named(qualify(name.into())))
    }

    /// A resource-pack sound sent inline; nothing needs registering.
    pub fn custom(sound_id: impl Into<String>) -> Self {
        Self::build(Event::Custom {
            sound_id: qualify(sound_id.into()),
            fixed_range: None,
        })
    }

    fn build(event: Event) -> Self {
        Self {
            event,
            source: SoundSource::Master,
            volume: 1.0,
            pitch: 1.0,
            seed: 0,
            emitter: Emitter::Unplaced,
            audience: None,
        }
    }

    /// Only meaningful for [`Sound::custom`]; registry sounds carry their own range.
    pub fn fixed_range(mut self, blocks: f32) -> Self {
        if let Event::Custom { fixed_range, .. } = &mut self.event {
            *fixed_range = Some(blocks);
        }
        self
    }

    pub fn category(mut self, source: SoundSource) -> Self {
        self.source = source;
        self
    }

    pub fn volume(mut self, volume: f32) -> Self {
        self.volume = volume.max(0.0);
        self
    }

    pub fn pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch.clamp(0.5, 2.0);
        self
    }

    pub fn seed(mut self, seed: i64) -> Self {
        self.seed = seed;
        self
    }

    pub fn at(mut self, position: impl Into<SoundPosition>) -> Self {
        let SoundPosition { x, y, z } = position.into();
        let dimension = match self.emitter {
            Emitter::At { dimension, .. } => dimension,
            _ => None,
        };
        self.emitter = Emitter::At { x, y, z, dimension };
        self
    }

    /// Dimension of an [`Sound::at`] position; narrows the default audience to
    /// players seeing that chunk.
    pub fn in_dimension(mut self, dimension: DimensionId) -> Self {
        self.emitter = match self.emitter {
            Emitter::At { x, y, z, .. } => Emitter::At {
                x,
                y,
                z,
                dimension: Some(dimension),
            },
            other => other,
        };
        self
    }

    pub fn from_entity(mut self, entity: Entity) -> Self {
        self.emitter = Emitter::Entity(entity);
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = Some(audience);
        self
    }

    fn event(&self) -> Option<SoundEvent> {
        match &self.event {
            Event::Named(name) => resolve(name).map(SoundEvent::Registry),
            Event::Custom {
                sound_id,
                fixed_range,
            } => Some(SoundEvent::Inline {
                sound_id: sound_id.clone(),
                fixed_range: *fixed_range,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl From<(f64, f64, f64)> for SoundPosition {
    fn from((x, y, z): (f64, f64, f64)) -> Self {
        Self { x, y, z }
    }
}

impl From<&Position> for SoundPosition {
    fn from(position: &Position) -> Self {
        Self {
            x: position.x,
            y: position.y,
            z: position.z,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SoundStop {
    source: Option<SoundSource>,
    sound: Option<String>,
    audience: Audience,
}

impl SoundStop {
    pub fn all() -> Self {
        Self::default()
    }

    pub fn source(mut self, source: SoundSource) -> Self {
        self.source = Some(source);
        self
    }

    pub fn sound(mut self, name: impl Into<String>) -> Self {
        self.sound = Some(qualify(name.into()));
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    fn packet(&self) -> StopSound {
        StopSound {
            source: self.source,
            sound: self.sound.clone(),
        }
    }
}

/// Registry id of a `minecraft:sound_event`, with or without the `minecraft:` prefix.
pub fn resolve(name: &str) -> Option<i32> {
    let owned;
    let full = if name.contains(':') {
        name
    } else {
        owned = format!("minecraft:{name}");
        &owned
    };
    voidmc_data::sound_event_id(VERSION, full)
}

fn qualify(name: String) -> String {
    if name.contains(':') {
        name
    } else {
        format!("minecraft:{name}")
    }
}

fn unknown_sound(name: &str) {
    debug_assert!(false, "unknown sound event {name:?}");
    static LOGGED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut logged = LOGGED.get_or_init(Default::default).lock().unwrap();
    if logged.insert(name.to_string()) {
        tracing::error!(sound = name, "Unknown sound event; the sound is not played");
    }
}

struct EmitterInfo {
    entity_id: Option<i32>,
    position: Option<SoundPosition>,
    dimension: Option<DimensionId>,
}

fn chunk_audience(position: SoundPosition, dimension: Option<DimensionId>) -> Audience {
    match dimension {
        Some(dimension) => {
            let chunk = ChunkPos::from_block(position.x, position.z);
            Audience::custom(move |r| r.sees_chunk(dimension, chunk))
        }
        None => Audience::All,
    }
}

fn play_with(
    sound: &Sound,
    ready: Recipients<'_>,
    lookup: impl Fn(Entity) -> Option<EmitterInfo>,
) -> bool {
    let event = match sound.event() {
        Some(event) => event,
        None => {
            if let Event::Named(name) = &sound.event {
                unknown_sound(name);
            }
            return false;
        }
    };
    let (packet, default_audience): (ClientboundPacket, Audience) = match sound.emitter {
        Emitter::Unplaced => {
            debug_assert!(false, "Sound played without .at(..) or .from_entity(..)");
            tracing::warn!("Sound played without a position or emitting entity; dropped");
            return false;
        }
        Emitter::At { x, y, z, dimension } => (
            SoundEffect {
                sound: event,
                source: sound.source,
                x: SoundEffect::fixed_point(x),
                y: SoundEffect::fixed_point(y),
                z: SoundEffect::fixed_point(z),
                volume: sound.volume,
                pitch: sound.pitch,
                seed: sound.seed,
            }
            .into(),
            chunk_audience(SoundPosition { x, y, z }, dimension),
        ),
        Emitter::Entity(entity) => {
            let Some(info) = lookup(entity) else {
                tracing::warn!(?entity, "Sound emitter entity no longer exists; dropped");
                return false;
            };
            let Some(entity_id) = info.entity_id else {
                tracing::warn!(?entity, "Sound emitter has no MinecraftEntityId; dropped");
                return false;
            };
            (
                EntitySoundEffect {
                    sound: event,
                    source: sound.source,
                    entity_id,
                    volume: sound.volume,
                    pitch: sound.pitch,
                    seed: sound.seed,
                }
                .into(),
                match info.position {
                    Some(position) => chunk_audience(position, info.dimension),
                    None => Audience::All,
                },
            )
        }
    };
    let audience = sound.audience.as_ref().unwrap_or(&default_audience);
    audience.resolve(ready).send(packet);
    true
}

fn play_to_with(
    player: Entity,
    mut sound: Sound,
    ready: Recipients<'_>,
    lookup: impl Fn(Entity) -> Option<EmitterInfo>,
) -> bool {
    if sound.emitter == Emitter::Unplaced {
        let Some(position) = lookup(player).and_then(|info| info.position) else {
            tracing::warn!(?player, "play_to target has no Position; dropped");
            return false;
        };
        sound.emitter = Emitter::At {
            x: position.x,
            y: position.y,
            z: position.z,
            dimension: None,
        };
    }
    sound.audience = Some(Audience::explicit([player]));
    play_with(&sound, ready, lookup)
}

#[derive(SystemParam)]
pub struct Sounds<'w, 's> {
    players: Players<'w, 's>,
    emitters: Query<
        'w,
        's,
        (
            Option<&'static MinecraftEntityId>,
            Option<&'static Position>,
            Option<&'static EntityDimension>,
            Option<&'static PlayerDimension>,
        ),
    >,
}

impl Sounds<'_, '_> {
    fn lookup(&self, entity: Entity) -> Option<EmitterInfo> {
        let (id, position, entity_dimension, player_dimension) = self.emitters.get(entity).ok()?;
        Some(EmitterInfo {
            entity_id: id.map(|id| id.0),
            position: position.map(SoundPosition::from),
            dimension: entity_dimension
                .map(|d| d.0)
                .or(player_dimension.map(|d| d.0)),
        })
    }

    /// Returns whether a packet was sent.
    pub fn play(&self, sound: Sound) -> bool {
        play_with(&sound, self.players.ready(), |e| self.lookup(e))
    }

    /// Plays for one player only, at their own position unless the sound is placed.
    pub fn play_to(&self, player: Entity, sound: Sound) -> bool {
        play_to_with(player, sound, self.players.ready(), |e| self.lookup(e))
    }

    pub fn stop(&self, stop: SoundStop) {
        stop.audience
            .resolve(self.players.ready())
            .send(stop.packet());
    }
}

pub struct WorldSounds<'w> {
    world: &'w World,
}

impl<'w> WorldSounds<'w> {
    pub fn new(world: &'w World) -> Self {
        Self { world }
    }

    fn lookup(&self, entity: Entity) -> Option<EmitterInfo> {
        let entity = self.world.get_entity(entity).ok()?;
        Some(EmitterInfo {
            entity_id: entity.get::<MinecraftEntityId>().map(|id| id.0),
            position: entity.get::<Position>().map(SoundPosition::from),
            dimension: entity
                .get::<EntityDimension>()
                .map(|d| d.0)
                .or(entity.get::<PlayerDimension>().map(|d| d.0)),
        })
    }

    pub fn play(&self, sound: Sound) -> bool {
        play_with(&sound, WorldPlayers::new(self.world).ready(), |e| {
            self.lookup(e)
        })
    }

    pub fn play_to(&self, player: Entity, sound: Sound) -> bool {
        play_to_with(player, sound, WorldPlayers::new(self.world).ready(), |e| {
            self.lookup(e)
        })
    }

    pub fn stop(&self, stop: SoundStop) {
        stop.audience
            .resolve(WorldPlayers::new(self.world).ready())
            .send(stop.packet());
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, Update};
    use flume::Receiver;
    use voidmc_protocol::clientbound::PlayPacket;

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, kick_rx) = flume::unbounded::<u32>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx));
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32, dimension: DimensionId, chunks: &[(i32, i32)]) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(id),
                PlayerReady,
                PlayerDimension(dimension),
                LoadedChunks(chunks.iter().map(|&(x, z)| ChunkPos::new(x, z)).collect()),
                Position {
                    x: 8.0,
                    y: 64.0,
                    z: 8.0,
                },
            ))
            .id()
    }

    #[derive(Debug, PartialEq)]
    enum Sent {
        At(u32, SoundEvent, i32, i32, i32),
        Entity(u32, SoundEvent, i32),
        Stop(u32, Option<SoundSource>, Option<String>),
    }

    impl Sent {
        fn key(&self) -> (u32, u8, bool) {
            match self {
                Sent::At(c, ..) => (*c, 0, false),
                Sent::Entity(c, ..) => (*c, 1, false),
                Sent::Stop(c, source, _) => (*c, 2, source.is_some()),
            }
        }
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent: Vec<Sent> = rx
            .try_iter()
            .map(|out| match out.packet {
                ClientboundPacket::Play(PlayPacket::SoundEffect(p)) => {
                    Sent::At(out.client_id, p.sound, p.x, p.y, p.z)
                }
                ClientboundPacket::Play(PlayPacket::EntitySoundEffect(p)) => {
                    Sent::Entity(out.client_id, p.sound, p.entity_id)
                }
                ClientboundPacket::Play(PlayPacket::StopSound(p)) => {
                    Sent::Stop(out.client_id, p.source, p.sound)
                }
                other => panic!("unexpected packet {other:?}"),
            })
            .collect();
        sent.sort_by_key(Sent::key);
        sent
    }

    fn levelup() -> SoundEvent {
        SoundEvent::Registry(resolve("minecraft:entity.player.levelup").unwrap())
    }

    #[test]
    fn registry_names_resolve_with_or_without_namespace() {
        let id = resolve("minecraft:entity.player.levelup").unwrap();
        assert!(id >= 0);
        assert_eq!(
            Sound::new("entity.player.levelup").event(),
            Some(SoundEvent::Registry(id))
        );
        assert_eq!(
            Sound::new("minecraft:entity.player.levelup").event(),
            Some(SoundEvent::Registry(id))
        );
        assert_eq!(resolve("entity.player.levelup"), Some(id));
        assert_eq!(resolve("minecraft:does.not.exist"), None);
        assert_eq!(Sound::new("does.not.exist").event(), None);
    }

    #[test]
    fn custom_sounds_are_inline_with_optional_range() {
        assert_eq!(
            Sound::custom("myserver:ui/ding").fixed_range(32.0).event(),
            Some(SoundEvent::Inline {
                sound_id: "myserver:ui/ding".into(),
                fixed_range: Some(32.0),
            })
        );
        assert_eq!(
            Sound::custom("ding").event(),
            Some(SoundEvent::Inline {
                sound_id: "minecraft:ding".into(),
                fixed_range: None,
            })
        );
        assert_eq!(Sound::new("x").fixed_range(1.0).event(), None);
    }

    #[test]
    fn builder_clamps_volume_and_pitch() {
        let sound = Sound::new("x").volume(-1.0).pitch(9.0).seed(7);
        assert_eq!((sound.volume, sound.pitch, sound.seed), (0.0, 2.0, 7));
        assert_eq!(Sound::new("x").pitch(0.1).pitch, 0.5);
    }

    #[test]
    fn positional_sound_defaults_to_players_seeing_the_chunk() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, DimensionId::Overworld, &[(0, 0)]);
        player(&mut app, 2, DimensionId::Overworld, &[(5, 5)]);
        player(&mut app, 3, DimensionId::Nether, &[(0, 0)]);
        app.add_systems(Update, |sounds: Sounds| {
            assert!(
                sounds.play(
                    Sound::new("entity.player.levelup")
                        .at((1.5, 64.0, 0.25))
                        .in_dimension(DimensionId::Overworld)
                )
            );
        });
        app.update();
        assert_eq!(drain(&rx), vec![Sent::At(1, levelup(), 12, 512, 2)]);
    }

    #[test]
    fn positional_sound_without_dimension_reaches_everyone() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, DimensionId::Overworld, &[]);
        player(&mut app, 2, DimensionId::Nether, &[]);
        app.add_systems(Update, |sounds: Sounds| {
            sounds.play(Sound::custom("a:b").at((0.0, 0.0, 0.0)));
        });
        app.update();
        assert_eq!(drain(&rx).len(), 2);
    }

    #[test]
    fn explicit_audience_overrides_the_default() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, DimensionId::Overworld, &[(0, 0)]);
        let second = player(&mut app, 2, DimensionId::Nether, &[]);
        app.add_systems(Update, move |sounds: Sounds| {
            sounds.play(
                Sound::custom("a:b")
                    .at((0.0, 0.0, 0.0))
                    .in_dimension(DimensionId::Overworld)
                    .audience(Audience::explicit([second])),
            );
        });
        app.update();
        let sent = drain(&rx);
        assert_eq!(sent.len(), 1);
        assert!(matches!(sent[0], Sent::At(2, ..)));
    }

    #[test]
    fn entity_sound_uses_the_emitter_position_and_dimension() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, DimensionId::Nether, &[(2, 2)]);
        player(&mut app, 2, DimensionId::Nether, &[(0, 0)]);
        player(&mut app, 3, DimensionId::Overworld, &[(2, 2)]);
        let mob = app
            .world_mut()
            .spawn((
                MinecraftEntityId(77),
                Position {
                    x: 40.0,
                    y: 70.0,
                    z: 33.0,
                },
                EntityDimension(DimensionId::Nether),
            ))
            .id();
        let gone = app.world_mut().spawn(MinecraftEntityId(78)).id();
        app.world_mut().despawn(gone);
        app.add_systems(Update, move |sounds: Sounds| {
            assert!(sounds.play(Sound::new("entity.player.levelup").from_entity(mob)));
            assert!(!sounds.play(Sound::new("entity.player.levelup").from_entity(gone)));
        });
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Entity(1, levelup(), 77)]);
    }

    #[test]
    fn play_to_targets_one_player_at_their_position() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1, DimensionId::Overworld, &[]);
        player(&mut app, 2, DimensionId::Overworld, &[]);
        app.add_systems(Update, move |sounds: Sounds| {
            assert!(sounds.play_to(first, Sound::custom("a:b").category(SoundSource::Ui)));
        });
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::At(
                1,
                SoundEvent::Inline {
                    sound_id: "a:b".into(),
                    fixed_range: None
                },
                64,
                512,
                64
            )]
        );
    }

    #[test]
    fn stop_sends_the_requested_filter_to_the_audience() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, DimensionId::Overworld, &[]);
        player(&mut app, 2, DimensionId::Nether, &[]);
        app.add_systems(Update, |sounds: Sounds| {
            sounds.stop(SoundStop::all().source(SoundSource::Music));
            sounds.stop(
                SoundStop::all()
                    .sound("a:b")
                    .audience(Audience::InDimension(DimensionId::Nether)),
            );
        });
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Stop(1, Some(SoundSource::Music), None),
                Sent::Stop(2, None, Some("a:b".into())),
                Sent::Stop(2, Some(SoundSource::Music), None),
            ]
        );
    }

    #[test]
    fn world_sounds_matches_system_param() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, DimensionId::Overworld, &[(0, 0)]);
        let sounds = WorldSounds::new(app.world());
        assert!(
            sounds.play(
                Sound::custom("a:b")
                    .at((0.0, 0.0, 0.0))
                    .in_dimension(DimensionId::Overworld)
            )
        );
        sounds.stop(SoundStop::all());
        assert_eq!(drain(&rx).len(), 2);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unknown sound event")]
    fn unknown_sound_fails_loudly_in_debug() {
        let (app, _rx) = test_app();
        WorldSounds::new(app.world()).play(Sound::new("does.not.exist").at((0.0, 0.0, 0.0)));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "without .at")]
    fn unplaced_sound_fails_loudly_in_debug() {
        let (app, _rx) = test_app();
        WorldSounds::new(app.world()).play(Sound::new("entity.player.levelup"));
    }
}
