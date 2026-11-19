use voidmc::components::{PlayerDimension, Position};
use voidmc::{
    BiomeBuilder, BiomeId, BlockPosition, Command, CommandBuilder, CommandContext, IntegerArg,
    RegistryDataStore, StringArg, biome_at, set_biome,
};

pub(super) const CRIMSON_SKY: &str = "void_example:crimson_sky";

pub(super) fn register_crimson_sky(registries: &mut RegistryDataStore) {
    BiomeBuilder::new(CRIMSON_SKY)
        .precipitation(false)
        .temperature(1.2)
        .sky_color(0xff0044)
        .fog_color(0x220011)
        .water_color(0x5f1030)
        .grass_color(0x9a3a4f)
        .register(registries)
        .expect("crimson_sky registers once");
}

pub(super) fn biome_command() -> Command {
    CommandBuilder::new("biome")
        .description(
            "Example command: show the biome you stand in, or set its 4x4x4 cell \
             and the cells within [radius] cells of it",
        )
        .arg_optional("name", StringArg::single_word())
        .arg_optional("radius", IntegerArg::new(0, 4))
        .handler(handle_biome)
        .build()
}

fn handle_biome(ctx: &mut CommandContext) {
    let player = ctx.entity;
    let name = ctx.get::<String>("name").cloned();
    let radius = ctx.get::<i32>("radius").copied().unwrap_or(0);
    let outcome = ctx.with_world_mut(|world| {
        let (Some(position), Some(dimension)) = (
            world.get::<Position>(player).map(|p| BlockPosition {
                x: p.x.floor() as i32,
                y: p.y.floor() as i16,
                z: p.z.floor() as i32,
            }),
            world.get::<PlayerDimension>(player).map(|d| d.0),
        ) else {
            return Err("You have no position.".to_string());
        };
        let registries = world.resource::<RegistryDataStore>();
        let Some(name) = name else {
            let current = biome_at(world, dimension, position).ok_or("Chunk not loaded.")?;
            let label = registries
                .biome_name(current)
                .unwrap_or("<unknown>")
                .to_string();
            return Ok(format!("You are in {label} (id {}).", current.0));
        };
        let biome: BiomeId = registries
            .biome(&name)
            .ok_or_else(|| format!("Unknown biome '{name}'."))?;
        let previous = set_biome(world, dimension, position, biome).ok_or("Chunk not loaded.")?;
        let mut cells = 1;
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                for dz in -radius..=radius {
                    let cell = BlockPosition {
                        x: position.x + dx * 4,
                        y: position.y + (dy * 4) as i16,
                        z: position.z + dz * 4,
                    };
                    if (dx, dy, dz) != (0, 0, 0)
                        && set_biome(world, dimension, cell, biome).is_some()
                    {
                        cells += 1;
                    }
                }
            }
        }
        Ok(format!(
            "Set {cells} cell(s) to {name} (yours was id {}). Try /biome {CRIMSON_SKY} 4.",
            previous.0
        ))
    });
    match outcome {
        Ok(message) => ctx.reply(&message),
        Err(message) => ctx.reply_error(&message),
    }
}
