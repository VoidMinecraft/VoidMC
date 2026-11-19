use bevy_ecs::prelude::*;

use crate::components::{Position, SpawnedEntity};
use crate::events::EntityDespawnEvent;
use crate::world::CHUNK_MIN_Y;

/// How far below the bottom of the world an entity may go before it is
/// removed, as in vanilla.
pub const OUT_OF_WORLD_DEPTH: f64 = 64.0;

/// The height under which a non-player entity is removed:
/// [`OUT_OF_WORLD_DEPTH`] blocks below the bottom of the world.
pub const OUT_OF_WORLD_Y: f64 = CHUNK_MIN_Y as f64 - OUT_OF_WORLD_DEPTH;

/// Removes every server-owned entity that is below [`OUT_OF_WORLD_Y`],
/// firing [`EntityDespawnEvent`] for it, so that an entity falling without
/// block collision does not fall forever. Players are not `SpawnedEntity`
/// and are never removed here.
///
/// Like vanilla's below-world check, it runs before the entity moves in the
/// tick: the position below the limit reached in one tick is broadcast, and
/// the entity is removed at the start of the next.
pub fn despawn_out_of_world_entities(
    entities: Query<(Entity, &Position), With<SpawnedEntity>>,
    mut commands: Commands,
) {
    for (entity, position) in &entities {
        if position.y < OUT_OF_WORLD_Y {
            commands.trigger(EntityDespawnEvent { entity });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_app::{App, Update};

    use super::*;
    use crate::entity::{EntityBuilder, EntityKind};

    #[derive(Resource, Default)]
    struct Fired(Vec<Entity>);

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Fired>()
            .add_observer(
                |event: On<EntityDespawnEvent>,
                 mut commands: Commands,
                 mut fired: ResMut<Fired>| {
                    fired.0.push(event.entity);
                    commands.entity(event.entity).despawn();
                },
            )
            .add_systems(Update, despawn_out_of_world_entities);
        app
    }

    #[test]
    fn removes_only_entities_more_than_64_blocks_below_the_world() {
        let mut app = app();
        let spawn = |app: &mut App, y: f64| {
            EntityBuilder::new(EntityKind::Pig)
                .at(0.5, y, 0.5)
                .spawn_in(app.world_mut())
                .id()
        };
        let at_limit = spawn(&mut app, -128.0);
        let below = spawn(&mut app, -128.001);
        let far_below = spawn(&mut app, -10_000.0);
        let in_world = spawn(&mut app, 64.0);
        let player = app
            .world_mut()
            .spawn(Position {
                x: 0.0,
                y: -500.0,
                z: 0.0,
            })
            .id();

        app.update();

        let fired: HashSet<_> = app.world().resource::<Fired>().0.iter().copied().collect();
        assert_eq!(fired, HashSet::from([below, far_below]));
        for entity in [at_limit, in_world, player] {
            assert!(app.world().get_entity(entity).is_ok());
        }
        for entity in [below, far_below] {
            assert!(app.world().get_entity(entity).is_err());
        }
    }
}
