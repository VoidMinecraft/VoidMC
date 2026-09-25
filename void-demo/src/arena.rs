use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};

use bevy_ecs::prelude::*;
use voidmc::{
    BiomeId, ChunkPos, WorldGenerator, WorldPlayers,
    world::{ChunkData, ChunkDimension, ChunkDirty, ChunkIndex, ChunkPosition, DimensionId},
};
use voidmc_protocol::clientbound::Chunk;

use crate::{
    terrain::{self, Alpine},
    track::{HALF_WIDTH, Track},
};

pub const WAIT_Y: f64 = 110.0;

struct Circuit {
    track: Track,
    built: HashSet<ChunkPos>,
}

#[derive(Clone, Resource)]
pub struct Arena {
    pub map: Alpine,
    circuit: Arc<RwLock<Circuit>>,
}

impl Arena {
    pub fn new(map: Alpine) -> Self {
        Self {
            circuit: Arc::new(RwLock::new(Circuit {
                track: Track::new(map.seed),
                built: HashSet::new(),
            })),
            map,
        }
    }

    pub fn track(&self) -> Track {
        self.circuit.read().expect("arena lock").track.clone()
    }

    pub fn prepare(&self, track: Track) {
        let mut circuit = self.circuit.write().expect("arena lock");
        assert!(
            circuit.built.is_empty(),
            "demolish the old circuit before replacing its geometry"
        );
        circuit.track = track;
    }

    pub fn chunks(&self) -> Vec<ChunkPos> {
        self.track().chunks()
    }

    pub fn replace(&self, world: &mut World, pos: ChunkPos, build: bool) {
        {
            let mut circuit = self.circuit.write().expect("arena lock");
            if build {
                circuit.built.insert(pos);
            } else {
                circuit.built.remove(&pos);
            }
        }
        let data = ChunkData::from_protocol_chunk(&self.generate_chunk(&pos));
        let packet = data.to_packet(pos.x, pos.z);
        let key = (DimensionId::Overworld, pos);
        match world.resource::<ChunkIndex>().0.get(&key).copied() {
            Some(entity) => {
                world.entity_mut(entity).insert((data, ChunkDirty));
            }
            None => {
                let entity = world
                    .spawn((ChunkPosition(pos), ChunkDimension(key.0), data, ChunkDirty))
                    .id();
                world.resource_mut::<ChunkIndex>().0.insert(key, entity);
            }
        }
        WorldPlayers::new(world).broadcast_chunk(key.0, pos, packet);
    }
}

impl WorldGenerator for Arena {
    fn generate_chunk(&self, pos: &ChunkPos) -> Chunk {
        let track = {
            let circuit = self.circuit.read().expect("arena lock");
            circuit.built.contains(pos).then(|| circuit.track.clone())
        };
        let mut builder = self.map.builder(pos);
        if let Some(track) = track {
            track.overlay(pos, &self.map, &mut builder);
        }
        builder.build()
    }

    fn surface_height_at(&self, x: i32, z: i32) -> i32 {
        let circuit = self.circuit.read().expect("arena lock");
        if circuit
            .built
            .contains(&ChunkPos::from_block(f64::from(x), f64::from(z)))
        {
            let p = circuit.track.project(f64::from(x), f64::from(z));
            if p.distance <= HALF_WIDTH {
                return p.y as i32;
            }
        }
        self.map.surface_height_at(x, z)
    }

    fn cell_biome(&self, _x: i32, _y: i32, _z: i32) -> BiomeId {
        terrain::biome()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voidmc::{
        components::{ClientId, LoadedChunks, PlayerDimension, PlayerReady},
        network::{NetworkChannels, OutgoingPacket},
    };
    use voidmc_data::v26_1_2::blocks;
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    fn test_world() -> (World, flume::Receiver<OutgoingPacket>) {
        let mut world = World::new();
        let (_, events) = flume::unbounded();
        let (outgoing, receiver) = flume::unbounded();
        world.insert_resource(NetworkChannels { events, outgoing });
        world.init_resource::<ChunkIndex>();
        (world, receiver)
    }

    fn encode(data: &ChunkData) -> Vec<Vec<u8>> {
        data.sections.iter().map(|s| s.encode_to_bytes()).collect()
    }

    #[test]
    fn world_generation_is_deterministic_for_a_seed() {
        let pos = ChunkPos::new(3, -2);
        let a = Arena::new(Alpine { seed: 2026 });
        let b = Arena::new(Alpine { seed: 2026 });
        let same = |pos: &ChunkPos| {
            assert_eq!(
                encode(&ChunkData::from_protocol_chunk(&a.generate_chunk(pos))),
                encode(&ChunkData::from_protocol_chunk(&b.generate_chunk(pos)))
            );
        };
        same(&pos);
        assert_eq!(a.surface_height_at(50, 50), b.surface_height_at(50, 50));
        let (x, _, z) = a.track().point(1.0, 0.0);
        let road = ChunkPos::from_block(x, z);
        let (mut wa, _) = test_world();
        let (mut wb, _) = test_world();
        a.replace(&mut wa, road, true);
        b.replace(&mut wb, road, true);
        same(&road);
        assert_eq!(
            a.surface_height_at(x as i32, z as i32),
            a.track().point(1.0, 0.0).1 as i32
        );
        assert_ne!(
            encode(&ChunkData::from_protocol_chunk(&a.generate_chunk(&road))),
            encode(&ChunkData::from_protocol_chunk(
                &Arena::new(Alpine { seed: 2027 }).generate_chunk(&road)
            ))
        );
    }

    #[test]
    fn generation_replaces_cached_chunks_and_demolition_restores_the_landscape() {
        let (mut world, receiver) = test_world();
        let arena = Arena::new(Alpine { seed: 42 });
        let (x, y, z) = arena.track().point(0.0, 0.0);
        let pos = ChunkPos::from_block(x, z);
        let before = ChunkData::from_protocol_chunk(&arena.generate_chunk(&pos));
        let baseline = encode(&before);
        world.spawn((
            ClientId(1),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            LoadedChunks(HashSet::from([pos])),
        ));
        world.spawn((
            ClientId(2),
            PlayerReady,
            PlayerDimension(DimensionId::Overworld),
            LoadedChunks(HashSet::new()),
        ));
        let block = |data: &ChunkData| {
            data.get_block(
                (x.floor() as i32).rem_euclid(16) as u8,
                y as i32 - 1,
                (z.floor() as i32).rem_euclid(16) as u8,
            )
            .unwrap()
        };
        assert_eq!(block(&before), blocks::AIR);
        arena.replace(&mut world, pos, true);
        let entity = world.resource::<ChunkIndex>().0[&(DimensionId::Overworld, pos)];
        assert_ne!(block(world.get::<ChunkData>(entity).unwrap()), blocks::AIR);
        assert!(world.get::<ChunkDirty>(entity).is_some());
        let mut untouched = 0;
        for x in 0..16u8 {
            for z in 0..16u8 {
                if arena.track().road_distance(
                    f64::from(pos.x * 16) + f64::from(x) + 0.5,
                    f64::from(pos.z * 16) + f64::from(z) + 0.5,
                ) <= HALF_WIDTH + 1.0
                {
                    continue;
                }
                untouched += 1;
                for y in 0..200 {
                    assert_eq!(
                        world.get::<ChunkData>(entity).unwrap().get_block(x, y, z),
                        before.get_block(x, y, z)
                    );
                }
            }
        }
        assert!(untouched > 0);
        arena.replace(&mut world, pos, false);
        assert_eq!(
            world.resource::<ChunkIndex>().0[&(DimensionId::Overworld, pos)],
            entity
        );
        assert_eq!(block(world.get::<ChunkData>(entity).unwrap()), blocks::AIR);
        assert_eq!(encode(world.get::<ChunkData>(entity).unwrap()), baseline);
        assert_eq!(
            encode(&ChunkData::from_protocol_chunk(&arena.generate_chunk(&pos))),
            baseline
        );
        let sent: Vec<_> = receiver
            .try_iter()
            .filter(|p| {
                matches!(
                    p.packet,
                    ClientboundPacket::Play(PlayPacket::ChunkDataAndLight(_))
                )
            })
            .map(|p| p.client_id)
            .collect();
        assert_eq!(sent, vec![1, 1]);
        let center = ChunkPos::new(0, 0);
        for build in [true, false] {
            arena.replace(&mut world, center, build);
            let data = ChunkData::from_protocol_chunk(&arena.generate_chunk(&center));
            assert_eq!(data.get_block(0, WAIT_Y as i32 - 1, 0), Some(blocks::AIR));
        }
    }

    #[test]
    #[should_panic(expected = "demolish the old circuit")]
    fn a_built_circuit_cannot_be_replaced() {
        let (mut world, _) = test_world();
        let arena = Arena::new(Alpine { seed: 42 });
        arena.replace(&mut world, arena.chunks().pop().unwrap(), true);
        arena.prepare(Track::new(43));
    }
}
