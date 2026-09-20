use voidmc::components::{PlayerDimension, Position};
use voidmc::{
    Command, CommandBuilder, CommandContext, IntegerArg, Particle, StringArg, WorldParticles,
};

pub(super) fn particle_command() -> Command {
    CommandBuilder::new("particle")
        .description("Example command: spawn a payload-free particle around you")
        .arg("type", StringArg::single_word())
        .arg_optional("count", IntegerArg::new(1, 1000))
        .handler(handle_particle)
        .build()
}

fn handle_particle(ctx: &mut CommandContext) {
    let name = ctx.get::<String>("type").cloned().unwrap_or_default();
    let count = ctx.get::<i32>("count").copied().unwrap_or(20);

    let Some(particle) = Particle::simple(&name) else {
        if Particle::needs_data(&name) {
            ctx.reply_error(&format!(
                "'{name}' needs a payload; build it in code with Particle::{{..}}"
            ));
        } else {
            ctx.reply_error(&format!("Unknown particle '{name}'"));
        }
        return;
    };

    let entity = ctx.entity;
    let Some((position, dimension)) = ctx.with_world(|world| {
        Some((
            *world.get::<Position>(entity)?,
            world.get::<PlayerDimension>(entity)?.0,
        ))
    }) else {
        ctx.reply_error("You need a position to spawn particles");
        return;
    };

    ctx.with_world(|world| {
        WorldParticles::new(world)
            .spawn(particle)
            .at([position.x, position.y + 1.0, position.z])
            .dimension(dimension)
            .count(count)
            .offset(0.5, 0.5, 0.5)
            .speed(0.05)
            .send();
    });
    ctx.reply(&format!("Spawned {count} x {name}"));
}
