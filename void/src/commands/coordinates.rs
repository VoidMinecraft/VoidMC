use std::any::Any;

use voidmc_protocol::clientbound::commands::Parser;

use super::parser::{ArgParser, ParseContext};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Coordinate {
    Absolute(f64),
    Relative(f64),
    Local(f64),
}

impl Coordinate {
    fn parse(token: &str, integer: bool) -> Result<Self, String> {
        let number = |text: &str| -> Result<f64, String> {
            if text.is_empty() {
                return Ok(0.0);
            }
            text.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("'{token}' is not a valid coordinate"))
        };
        if let Some(rest) = token.strip_prefix('~') {
            return number(rest).map(Coordinate::Relative);
        }
        if let Some(rest) = token.strip_prefix('^') {
            return number(rest).map(Coordinate::Local);
        }
        if token.is_empty() {
            return Err("expected a coordinate".to_string());
        }
        if integer {
            return token
                .parse::<i32>()
                .map(|value| Coordinate::Absolute(value as f64))
                .map_err(|_| format!("'{token}' is not a valid block coordinate"));
        }
        number(token).map(Coordinate::Absolute)
    }

    fn is_local(self) -> bool {
        matches!(self, Coordinate::Local(_))
    }
}

/// Three coordinates as typed on the command line, before resolution
/// against the executor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinates(pub [Coordinate; 3]);

impl Coordinates {
    pub fn parse(input: &str, integer: bool) -> Result<Self, String> {
        let tokens: Vec<&str> = input.split_whitespace().collect();
        if tokens.len() != 3 {
            return Err(format!(
                "'{input}' is not a valid position (expected x y z)"
            ));
        }
        let mut coords = [Coordinate::Absolute(0.0); 3];
        for (slot, token) in coords.iter_mut().zip(tokens) {
            *slot = Coordinate::parse(token, integer)?;
        }
        let locals = coords.iter().filter(|c| c.is_local()).count();
        if locals != 0 && locals != 3 {
            return Err("cannot mix world and local (^) coordinates".to_string());
        }
        Ok(Self(coords))
    }

    pub fn is_absolute(&self) -> bool {
        self.0.iter().all(|c| matches!(c, Coordinate::Absolute(_)))
    }

    pub fn absolute(&self) -> Option<[f64; 3]> {
        self.is_absolute().then(|| {
            self.0.map(|c| match c {
                Coordinate::Absolute(v) => v,
                _ => unreachable!(),
            })
        })
    }

    pub fn resolve(&self, ctx: &ParseContext<'_>) -> Result<[f64; 3], String> {
        if let Some(absolute) = self.absolute() {
            return Ok(absolute);
        }
        let position = ctx
            .position()
            .ok_or_else(|| "relative coordinates need an executor position".to_string())?;
        let base = [position.x, position.y, position.z];
        if self.0[0].is_local() {
            let rotation = ctx.rotation().unwrap_or_default();
            let local = self.0.map(|c| match c {
                Coordinate::Local(v) => v,
                _ => unreachable!(),
            });
            let offset = local_to_world(rotation.yaw, rotation.pitch, local);
            return Ok([
                base[0] + offset[0],
                base[1] + offset[1],
                base[2] + offset[2],
            ]);
        }
        let mut out = [0.0; 3];
        for (i, coordinate) in self.0.iter().enumerate() {
            out[i] = match *coordinate {
                Coordinate::Absolute(v) => v,
                Coordinate::Relative(v) => base[i] + v,
                Coordinate::Local(_) => unreachable!(),
            };
        }
        Ok(out)
    }
}

/// Vanilla `Vec3.applyLocalCoordinatesToRotation`: `local` is
/// `[left, up, forward]`.
fn local_to_world(yaw: f32, pitch: f32, local: [f64; 3]) -> [f64; 3] {
    let (y_sin, y_cos) = (yaw + 90.0).to_radians().sin_cos();
    let (x_sin, x_cos) = (-pitch).to_radians().sin_cos();
    let (x_sin_up, x_cos_up) = (-pitch + 90.0).to_radians().sin_cos();
    let forward = [(y_cos * x_cos) as f64, x_sin as f64, (y_sin * x_cos) as f64];
    let up = [
        (y_cos * x_cos_up) as f64,
        x_sin_up as f64,
        (y_sin * x_cos_up) as f64,
    ];
    let left = [
        -(forward[1] * up[2] - forward[2] * up[1]),
        -(forward[2] * up[0] - forward[0] * up[2]),
        -(forward[0] * up[1] - forward[1] * up[0]),
    ];
    let [l, u, f] = local;
    [
        forward[0] * f + up[0] * u + left[0] * l,
        forward[1] * f + up[1] * u + left[1] * l,
        forward[2] * f + up[2] * u + left[2] * l,
    ]
}

/// `x y z` doubles with `~` relative and `^` local forms, resolved against the
/// executor to `[f64; 3]`.
pub struct Vec3Arg;

impl ArgParser for Vec3Arg {
    fn type_name(&self) -> &str {
        "vec3"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let coords = Coordinates::parse(input, false)?;
        coords
            .absolute()
            .map(|position| Box::new(position) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| "relative coordinates need an executor".to_string())
    }

    fn parse_in(
        &self,
        input: &str,
        ctx: &ParseContext<'_>,
    ) -> Result<Box<dyn Any + Send + Sync>, String> {
        Ok(Box::new(Coordinates::parse(input, false)?.resolve(ctx)?))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::Vec3)
    }

    fn token_count(&self) -> usize {
        3
    }
}

/// `x y z` block coordinates (integers, or `~`/`^` forms floored after
/// resolution) as `[i32; 3]`.
pub struct BlockPosArg;

impl ArgParser for BlockPosArg {
    fn type_name(&self) -> &str {
        "block_pos"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        let coords = Coordinates::parse(input, true)?;
        coords
            .absolute()
            .map(|position| Box::new(position.map(|v| v as i32)) as Box<dyn Any + Send + Sync>)
            .ok_or_else(|| "relative coordinates need an executor".to_string())
    }

    fn parse_in(
        &self,
        input: &str,
        ctx: &ParseContext<'_>,
    ) -> Result<Box<dyn Any + Send + Sync>, String> {
        let resolved = Coordinates::parse(input, true)?.resolve(ctx)?;
        Ok(Box::new(resolved.map(|v| v.floor() as i32)))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::BlockPos)
    }

    fn token_count(&self) -> usize {
        3
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::world::World;

    use super::*;
    use crate::components::{Position, Rotation};

    fn executor(world: &mut World, yaw: f32, pitch: f32) -> bevy_ecs::entity::Entity {
        world
            .spawn((
                Position {
                    x: 10.0,
                    y: 64.0,
                    z: -5.0,
                },
                Rotation { yaw, pitch },
            ))
            .id()
    }

    fn vec3(
        world: &World,
        executor: bevy_ecs::entity::Entity,
        input: &str,
    ) -> Result<[f64; 3], String> {
        Vec3Arg
            .parse_in(input, &ParseContext { world, executor })
            .map(|v| *v.downcast::<[f64; 3]>().unwrap())
    }

    fn block_pos(
        world: &World,
        executor: bevy_ecs::entity::Entity,
        input: &str,
    ) -> Result<[i32; 3], String> {
        BlockPosArg
            .parse_in(input, &ParseContext { world, executor })
            .map(|v| *v.downcast::<[i32; 3]>().unwrap())
    }

    fn approx(actual: [f64; 3], expected: [f64; 3]) {
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() < 1e-4, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn vec3_consumes_and_parses_three_tokens() {
        assert_eq!(Vec3Arg.token_count(), 3);
        assert_eq!(Vec3Arg.protocol_parser(), Some(Parser::Vec3));

        let parsed = Vec3Arg.parse("10 64 -3.5").unwrap();
        assert_eq!(*parsed.downcast::<[f64; 3]>().unwrap(), [10.0, 64.0, -3.5]);
        assert!(Vec3Arg.parse("10 64").is_err());
        assert!(Vec3Arg.parse("10 64 NaN").is_err());
    }

    #[test]
    fn vec3_without_context_rejects_relative_forms() {
        assert!(Vec3Arg.parse("~ ~ ~").is_err());
        assert!(BlockPosArg.parse("^ ^ ^1").is_err());
    }

    #[test]
    fn vec3_resolves_relative_against_executor() {
        let mut world = World::new();
        let player = executor(&mut world, 0.0, 0.0);
        assert_eq!(vec3(&world, player, "~ ~ ~").unwrap(), [10.0, 64.0, -5.0]);
        assert_eq!(
            vec3(&world, player, "~1.5 ~-2 3").unwrap(),
            [11.5, 62.0, 3.0]
        );
    }

    #[test]
    fn vec3_relative_without_position_errors() {
        let mut world = World::new();
        let player = world.spawn_empty().id();
        assert!(vec3(&world, player, "~ ~ ~").is_err());
        assert_eq!(vec3(&world, player, "1 2 3").unwrap(), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn local_coordinates_follow_the_executor_rotation() {
        let mut world = World::new();
        let south = executor(&mut world, 0.0, 0.0);
        approx(vec3(&world, south, "^ ^ ^2").unwrap(), [10.0, 64.0, -3.0]);
        approx(vec3(&world, south, "^1 ^ ^").unwrap(), [11.0, 64.0, -5.0]);
        approx(vec3(&world, south, "^ ^3 ^").unwrap(), [10.0, 67.0, -5.0]);

        let west = executor(&mut world, 90.0, 0.0);
        approx(vec3(&world, west, "^ ^ ^2").unwrap(), [8.0, 64.0, -5.0]);

        let down = executor(&mut world, 0.0, 90.0);
        approx(vec3(&world, down, "^ ^ ^2").unwrap(), [10.0, 62.0, -5.0]);
    }

    #[test]
    fn mixing_local_and_world_coordinates_is_rejected() {
        let mut world = World::new();
        let player = executor(&mut world, 0.0, 0.0);
        assert_eq!(
            vec3(&world, player, "^1 ~ 3").unwrap_err(),
            "cannot mix world and local (^) coordinates"
        );
    }

    #[test]
    fn block_pos_requires_integers_and_floors_relative_offsets() {
        let mut world = World::new();
        let player = world
            .spawn(Position {
                x: 10.4,
                y: 64.0,
                z: -5.5,
            })
            .id();
        assert_eq!(block_pos(&world, player, "1 2 3").unwrap(), [1, 2, 3]);
        assert!(block_pos(&world, player, "1.5 2 3").is_err());
        assert_eq!(block_pos(&world, player, "~ ~ ~").unwrap(), [10, 64, -6]);
        assert_eq!(
            block_pos(&world, player, "~-1 ~0.5 ~1.5").unwrap(),
            [9, 64, -4]
        );
        assert_eq!(BlockPosArg.protocol_parser(), Some(Parser::BlockPos));
        assert_eq!(BlockPosArg.token_count(), 3);
    }
}
