use std::any::Any;

use bevy_ecs::prelude::{Entity, With};
use bevy_ecs::world::World;
use rand::seq::SliceRandom;
use voidmc_protocol::clientbound::commands::Parser;

use super::parser::{ArgParser, ParseContext};
use crate::components::{PlayerName, PlayerReady, Position};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerSelector {
    Name(String),
    Executor,
    Nearest,
    All,
    Random,
}

impl PlayerSelector {
    pub fn parse(input: &str) -> Result<Self, String> {
        Ok(match input {
            "@s" => PlayerSelector::Executor,
            "@p" => PlayerSelector::Nearest,
            "@a" => PlayerSelector::All,
            "@r" => PlayerSelector::Random,
            _ if input.starts_with('@') => {
                return Err(format!(
                    "'{input}' is not a supported selector (expected @s, @p, @a, @r or a player name)"
                ));
            }
            _ if input.is_empty() => return Err("expected a player".to_string()),
            _ => PlayerSelector::Name(input.to_string()),
        })
    }

    pub fn is_single(&self) -> bool {
        !matches!(self, PlayerSelector::All)
    }

    pub fn resolve(&self, ctx: &ParseContext<'_>) -> Result<Vec<Entity>, String> {
        let world = ctx.world;
        let mut ready = ready_players(world);
        match self {
            PlayerSelector::Name(name) => ready
                .into_iter()
                .find(|(_, player_name)| player_name.eq_ignore_ascii_case(name))
                .map(|(entity, _)| vec![entity])
                .ok_or_else(|| format!("player '{name}' not found")),
            PlayerSelector::Executor => Ok(vec![ctx.executor]),
            PlayerSelector::All => Ok(ready.into_iter().map(|(entity, _)| entity).collect()),
            PlayerSelector::Random => ready
                .choose(&mut rand::thread_rng())
                .map(|(entity, _)| vec![*entity])
                .ok_or_else(|| "no player online".to_string()),
            PlayerSelector::Nearest => {
                let origin = ctx
                    .position()
                    .ok_or_else(|| "@p needs an executor position".to_string())?;
                ready.retain(|(entity, _)| world.get::<Position>(*entity).is_some());
                ready
                    .into_iter()
                    .map(|(entity, _)| {
                        let position = world.get::<Position>(entity).unwrap();
                        let dx = position.x - origin.x;
                        let dy = position.y - origin.y;
                        let dz = position.z - origin.z;
                        (entity, dx * dx + dy * dy + dz * dz)
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(entity, _)| vec![entity])
                    .ok_or_else(|| "no player online".to_string())
            }
        }
    }
}

fn ready_players(world: &World) -> Vec<(Entity, String)> {
    let mut query = world.try_query_filtered::<(Entity, &PlayerName), With<PlayerReady>>();
    let Some(query) = query.as_mut() else {
        return Vec::new();
    };
    query
        .iter(world)
        .map(|(entity, name)| (entity, name.0.clone()))
        .collect()
}

pub(crate) fn ready_player_names(world: &World) -> impl Iterator<Item = String> {
    ready_players(world).into_iter().map(|(_, name)| name)
}

fn parse_selector(input: &str, single: bool) -> Result<PlayerSelector, String> {
    let selector = PlayerSelector::parse(input)?;
    if single && !selector.is_single() {
        return Err(format!("'{input}' selects several players, expected one"));
    }
    Ok(selector)
}

/// A single ready player (`@s`, `@p`, `@r` or a name) resolved to its `Entity`.
pub struct PlayerArg;

impl ArgParser for PlayerArg {
    fn type_name(&self) -> &str {
        "player"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        parse_selector(input, true)?;
        Err("player selectors need an executor".to_string())
    }

    fn parse_in(
        &self,
        input: &str,
        ctx: &ParseContext<'_>,
    ) -> Result<Box<dyn Any + Send + Sync>, String> {
        let targets = parse_selector(input, true)?.resolve(ctx)?;
        Ok(Box::new(targets[0]))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Entity {
            single: true,
            players_only: true,
        })
    }
}

/// One or more ready players (`@a`, `@s`, `@p`, `@r` or a name) as `Vec<Entity>`.
pub struct PlayersArg;

impl ArgParser for PlayersArg {
    fn type_name(&self) -> &str {
        "players"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        parse_selector(input, false)?;
        Err("player selectors need an executor".to_string())
    }

    fn parse_in(
        &self,
        input: &str,
        ctx: &ParseContext<'_>,
    ) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(parse_selector(input, false)?.resolve(ctx)?))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Entity {
            single: false,
            players_only: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(world: &mut World, name: &str, x: f64) -> Entity {
        world
            .spawn((
                PlayerName(name.into()),
                PlayerReady,
                Position { x, y: 0.0, z: 0.0 },
            ))
            .id()
    }

    fn single(world: &World, executor: Entity, input: &str) -> Result<Entity, String> {
        PlayerArg
            .parse_in(input, &ParseContext { world, executor })
            .map(|v| *v.downcast::<Entity>().unwrap())
    }

    fn many(world: &World, executor: Entity, input: &str) -> Result<Vec<Entity>, String> {
        PlayersArg
            .parse_in(input, &ParseContext { world, executor })
            .map(|v| *v.downcast::<Vec<Entity>>().unwrap())
    }

    #[test]
    fn selector_syntax() {
        assert_eq!(PlayerSelector::parse("@a").unwrap(), PlayerSelector::All);
        assert_eq!(
            PlayerSelector::parse("Steve").unwrap(),
            PlayerSelector::Name("Steve".into())
        );
        assert!(PlayerSelector::parse("@e").is_err());
        assert!(PlayerSelector::parse("@a[distance=..3]").is_err());
        assert!(PlayerSelector::parse("").is_err());
    }

    #[test]
    fn names_resolve_case_insensitively_among_ready_players() {
        let mut world = World::new();
        let alice = player(&mut world, "Alice", 0.0);
        let bob = player(&mut world, "Bob", 5.0);
        world.spawn(PlayerName("Carol".into()));

        assert_eq!(single(&world, alice, "bob").unwrap(), bob);
        assert_eq!(single(&world, alice, "@s").unwrap(), alice);
        assert_eq!(
            single(&world, alice, "Carol").unwrap_err(),
            "player 'Carol' not found"
        );
    }

    #[test]
    fn nearest_picks_the_closest_ready_player_including_self() {
        let mut world = World::new();
        let alice = player(&mut world, "Alice", 0.0);
        let bob = player(&mut world, "Bob", 5.0);
        let carol = player(&mut world, "Carol", 6.0);

        assert_eq!(single(&world, alice, "@p").unwrap(), alice);
        let observer = world
            .spawn(Position {
                x: 5.4,
                y: 0.0,
                z: 0.0,
            })
            .id();
        assert_eq!(single(&world, observer, "@p").unwrap(), bob);
        let far = world
            .spawn(Position {
                x: 100.0,
                y: 0.0,
                z: 0.0,
            })
            .id();
        assert_eq!(single(&world, far, "@p").unwrap(), carol);
        let nowhere = world.spawn_empty().id();
        assert!(single(&world, nowhere, "@p").is_err());
    }

    #[test]
    fn all_and_random_select_ready_players() {
        let mut world = World::new();
        let alice = player(&mut world, "Alice", 0.0);
        let bob = player(&mut world, "Bob", 5.0);

        let mut all = many(&world, alice, "@a").unwrap();
        all.sort();
        let mut expected = vec![alice, bob];
        expected.sort();
        assert_eq!(all, expected);

        let random = single(&world, alice, "@r").unwrap();
        assert!(random == alice || random == bob);

        assert_eq!(
            single(&world, alice, "@a").unwrap_err(),
            "'@a' selects several players, expected one"
        );
        assert_eq!(many(&world, alice, "@s").unwrap(), vec![alice]);
    }

    #[test]
    fn empty_server_errors_for_selectors() {
        let mut world = World::new();
        let executor = world.spawn_empty().id();
        assert!(single(&world, executor, "@r").is_err());
        assert_eq!(many(&world, executor, "@a").unwrap(), Vec::<Entity>::new());
    }

    #[test]
    fn protocol_flags_match_selector_arity() {
        assert_eq!(
            PlayerArg.protocol_parser(),
            Some(Parser::Entity {
                single: true,
                players_only: true
            })
        );
        assert_eq!(
            PlayersArg.protocol_parser(),
            Some(Parser::Entity {
                single: false,
                players_only: true
            })
        );
        assert!(PlayerArg.parse("Steve").is_err());
    }
}
