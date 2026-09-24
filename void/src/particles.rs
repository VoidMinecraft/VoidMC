//! Fire-and-forget particle requests: `particles.spawn(Particle::Flame).at(pos).send()`,
//! delivered to the players who have the position's chunk loaded unless an
//! `Audience` is set.

use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc_protocol::clientbound::LevelParticles;
use voidmc_protocol::types::BlockPosition;

pub use voidmc_protocol::clientbound::{
    ItemStackTemplate, Particle, ParticleColor, PositionSource,
};

use crate::components::Position;
use crate::item::ItemStack;
use crate::players::{Audience, Players, Recipients, WorldPlayers};
use crate::world::{ChunkPos, DimensionId};

impl From<&Position> for [f64; 3] {
    fn from(position: &Position) -> Self {
        [position.x, position.y, position.z]
    }
}

impl From<Position> for [f64; 3] {
    fn from(position: Position) -> Self {
        [position.x, position.y, position.z]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyItemStack;

impl TryFrom<&ItemStack> for ItemStackTemplate {
    type Error = EmptyItemStack;

    fn try_from(stack: &ItemStack) -> Result<Self, EmptyItemStack> {
        let slot = stack.to_slot();
        if slot.is_empty() {
            return Err(EmptyItemStack);
        }
        Ok(ItemStackTemplate {
            item_id: slot.item_id,
            count: slot.count,
            components: voidmc_protocol::slot::DataComponentPatch {
                components_to_add: slot.components_to_add,
                components_to_remove: slot.components_to_remove,
            },
        })
    }
}

#[derive(SystemParam)]
pub struct Particles<'w, 's> {
    players: Players<'w, 's>,
}

impl Particles<'_, '_> {
    pub fn spawn(&self, particle: Particle) -> ParticleRequest<'_> {
        ParticleRequest::new(Sink::Players(&self.players), particle)
    }
}

pub struct WorldParticles<'w> {
    players: WorldPlayers<'w>,
}

impl<'w> WorldParticles<'w> {
    pub fn new(world: &'w World) -> Self {
        Self {
            players: WorldPlayers::new(world),
        }
    }

    pub fn spawn(&self, particle: Particle) -> ParticleRequest<'_> {
        ParticleRequest::new(Sink::World(&self.players), particle)
    }
}

enum Sink<'a> {
    Players(&'a Players<'a, 'a>),
    World(&'a WorldPlayers<'a>),
}

impl<'a> Sink<'a> {
    fn ready(&self) -> Recipients<'a> {
        match self {
            Sink::Players(players) => players.ready(),
            Sink::World(players) => players.ready(),
        }
    }
}

#[must_use = "a particle request does nothing until `.send()`"]
pub struct ParticleRequest<'a> {
    sink: Sink<'a>,
    particle: Particle,
    position: [f64; 3],
    dimension: DimensionId,
    count: i32,
    offset: [f32; 3],
    speed: f32,
    long_distance: bool,
    always_visible: bool,
    audience: Option<Audience>,
}

impl<'a> ParticleRequest<'a> {
    fn new(sink: Sink<'a>, particle: Particle) -> Self {
        Self {
            sink,
            particle,
            position: [0.0; 3],
            dimension: DimensionId::Overworld,
            count: 1,
            offset: [0.0; 3],
            speed: 0.0,
            long_distance: false,
            always_visible: false,
            audience: None,
        }
    }

    pub fn at(mut self, position: impl Into<[f64; 3]>) -> Self {
        self.position = position.into();
        self
    }

    /// Centre of the block.
    pub fn at_block(self, position: BlockPosition) -> Self {
        self.at([
            position.x as f64 + 0.5,
            position.y as f64 + 0.5,
            position.z as f64 + 0.5,
        ])
    }

    pub fn dimension(mut self, dimension: DimensionId) -> Self {
        self.dimension = dimension;
        self
    }

    pub fn count(mut self, count: i32) -> Self {
        self.count = count;
        self
    }

    /// Random spread on each axis around the position.
    pub fn offset(mut self, x: f32, y: f32, z: f32) -> Self {
        self.offset = [x, y, z];
        self
    }

    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// Directed mode (`count = 0`): every particle moves along `direction`
    /// scaled by `speed` instead of spreading randomly.
    pub fn directed(mut self, direction: [f32; 3], speed: f32) -> Self {
        self.count = 0;
        self.offset = direction;
        self.speed = speed;
        self
    }

    pub fn long_distance(mut self, long_distance: bool) -> Self {
        self.long_distance = long_distance;
        self
    }

    pub fn always_visible(mut self, always_visible: bool) -> Self {
        self.always_visible = always_visible;
        self
    }

    /// Replaces the default audience (players who have the position's chunk
    /// loaded in the request's dimension).
    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = Some(audience);
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    pub fn packet(&self) -> LevelParticles {
        LevelParticles {
            long_distance: self.long_distance,
            always_visible: self.always_visible,
            x: self.position[0],
            y: self.position[1],
            z: self.position[2],
            offset_x: self.offset[0],
            offset_y: self.offset[1],
            offset_z: self.offset[2],
            max_speed: self.speed,
            count: self.count,
            particle: self.particle.clone(),
        }
    }

    pub fn recipients(&self) -> Recipients<'a> {
        let ready = self.sink.ready();
        match &self.audience {
            Some(audience) => audience.resolve(ready),
            None => ready.seeing_chunk(
                self.dimension,
                ChunkPos::from_block(self.position[0], self.position[2]),
            ),
        }
    }

    pub fn send(self) {
        let recipients = self.recipients();
        let particle = self.particle;
        recipients.send(LevelParticles {
            long_distance: self.long_distance,
            always_visible: self.always_visible,
            x: self.position[0],
            y: self.position[1],
            z: self.position[2],
            offset_x: self.offset[0],
            offset_y: self.offset[1],
            offset_z: self.offset[2],
            max_speed: self.speed,
            count: self.count,
            particle,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_app::{App, Update};
    use flume::Receiver;
    use voidmc_protocol::clientbound;

    use super::*;
    use crate::components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady};
    use crate::network::{NetworkChannels, OutgoingPacket};

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        });
        app.insert_non_send_resource(incoming_tx);
        (app, outgoing_rx)
    }

    fn viewer(
        app: &mut App,
        client_id: u32,
        dimension: DimensionId,
        chunks: &[(i32, i32)],
    ) -> Entity {
        app.world_mut()
            .spawn((
                ClientId(client_id),
                PlayerReady,
                PlayerDimension(dimension),
                LoadedChunks(chunks.iter().map(|&(x, z)| ChunkPos::new(x, z)).collect()),
            ))
            .id()
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<(u32, LevelParticles)> {
        rx.try_iter()
            .map(|out| {
                let clientbound::ClientboundPacket::Play(clientbound::PlayPacket::LevelParticles(
                    packet,
                )) = out.packet
                else {
                    panic!("expected level particles");
                };
                (out.client_id, packet)
            })
            .collect()
    }

    fn client_ids(sent: &[(u32, LevelParticles)]) -> HashSet<u32> {
        sent.iter().map(|(id, _)| *id).collect()
    }

    #[test]
    fn default_audience_is_players_seeing_the_chunk() {
        let (mut app, rx) = test_app();
        viewer(&mut app, 1, DimensionId::Overworld, &[(2, -1)]);
        viewer(&mut app, 2, DimensionId::Overworld, &[(0, 0)]);
        viewer(&mut app, 3, DimensionId::Nether, &[(2, -1)]);
        app.world_mut().spawn((ClientId(4), PlayerReady));

        app.add_systems(Update, |particles: Particles| {
            particles
                .spawn(Particle::Flame)
                .at([40.0, 64.0, -3.0])
                .count(5)
                .send();
            particles
                .spawn(Particle::Smoke)
                .at([40.0, 64.0, -3.0])
                .dimension(DimensionId::Nether)
                .send();
        });
        app.update();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, 1);
        assert_eq!(sent[0].1.particle, Particle::Flame);
        assert_eq!(sent[0].1.count, 5);
        assert_eq!(sent[1].0, 3);
        assert_eq!(sent[1].1.particle, Particle::Smoke);
    }

    #[test]
    fn explicit_audience_overrides_chunk_default() {
        let (mut app, rx) = test_app();
        let far = viewer(&mut app, 1, DimensionId::Overworld, &[(50, 50)]);
        viewer(&mut app, 2, DimensionId::Overworld, &[(0, 0)]);
        viewer(&mut app, 3, DimensionId::End, &[(0, 0)]);

        app.add_systems(Update, move |particles: Particles| {
            particles
                .spawn(Particle::Heart)
                .at([1.0, 2.0, 3.0])
                .viewers([far])
                .send();
            particles
                .spawn(Particle::Heart)
                .at([1.0, 2.0, 3.0])
                .audience(Audience::InDimension(DimensionId::End))
                .send();
            particles
                .spawn(Particle::Heart)
                .at([1.0, 2.0, 3.0])
                .audience(Audience::All)
                .send();
        });
        app.update();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 5);
        assert_eq!(sent[0].0, 1);
        assert_eq!(sent[1].0, 3);
        assert_eq!(client_ids(&sent[2..]), HashSet::from([1, 2, 3]));
    }

    #[test]
    fn builder_fields_reach_the_packet_and_unsent_requests_send_nothing() {
        let (mut app, rx) = test_app();
        viewer(&mut app, 1, DimensionId::Overworld, &[(0, 0)]);

        app.add_systems(Update, |particles: Particles| {
            let request = particles
                .spawn(Particle::Dust {
                    color: ParticleColor::rgb(1, 2, 3),
                    scale: 2.0,
                })
                .at([1.5, 70.0, 2.5])
                .count(20)
                .offset(0.5, 1.0, 0.5)
                .speed(0.1)
                .long_distance(true)
                .always_visible(true);
            assert_eq!(request.packet().count, 20);
            assert_eq!(request.recipients().len(), 1);
            request.send();
            particles
                .spawn(Particle::Crit)
                .at([1.0, 1.0, 1.0])
                .directed([0.0, 1.0, 0.0], 0.3)
                .send();
            let _abandoned = particles.spawn(Particle::Lava).at([1.0, 1.0, 1.0]);
        });
        app.update();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 2);
        let packet = &sent[0].1;
        assert_eq!(packet.count, 20);
        assert_eq!((packet.x, packet.y, packet.z), (1.5, 70.0, 2.5));
        assert_eq!(
            (packet.offset_x, packet.offset_y, packet.offset_z),
            (0.5, 1.0, 0.5)
        );
        assert_eq!(packet.max_speed, 0.1);
        assert!(packet.long_distance && packet.always_visible);
        let directed = &sent[1].1;
        assert_eq!(directed.count, 0);
        assert_eq!((directed.offset_y, directed.max_speed), (1.0, 0.3));
    }

    #[test]
    fn world_particles_twin_matches() {
        let (mut app, rx) = test_app();
        viewer(&mut app, 1, DimensionId::Overworld, &[(0, 0)]);
        viewer(&mut app, 2, DimensionId::Overworld, &[(9, 9)]);

        WorldParticles::new(app.world())
            .spawn(Particle::Note)
            .at_block(BlockPosition { x: 3, y: 64, z: 3 })
            .send();

        let sent = drain(&rx);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, 1);
        assert_eq!((sent[0].1.x, sent[0].1.y, sent[0].1.z), (3.5, 64.5, 3.5));
    }

    #[test]
    fn item_stack_converts_to_template_unless_empty() {
        let stack = ItemStack::of("minecraft:stone", 3).unwrap();
        let template = ItemStackTemplate::try_from(&stack).unwrap();
        assert_eq!(template.count, 3);
        assert_eq!(template.item_id, stack.to_slot().item_id);
        assert_eq!(
            ItemStackTemplate::try_from(&ItemStack::EMPTY),
            Err(EmptyItemStack)
        );
    }
}
