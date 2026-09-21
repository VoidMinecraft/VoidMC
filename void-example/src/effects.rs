use voidmc::{
    Attributes, Command, CommandBuilder, CommandContext, DoubleArg, Effect, EffectDuration,
    EntityAttribute, IntegerArg, Modifier, StatusEffects, StringArg,
};

const SPEED_MODIFIER: &str = "void_example:speed_command";

pub(super) fn effect_command() -> Command {
    CommandBuilder::new("effect")
        .description("Example command: give yourself a status effect, or clear them")
        .arg("effect", StringArg::single_word())
        .arg_optional("seconds", IntegerArg::new(0, 1_000_000))
        .arg_optional("amplifier", IntegerArg::new(0, 255))
        .flag("hidden", Some('h'), "No icon, no particles")
        .flag("ambient", Some('a'), "Beacon-style faint particles")
        .handler(handle_effect)
        .build()
}

fn handle_effect(ctx: &mut CommandContext) {
    let name = ctx.get::<String>("effect").cloned().unwrap_or_default();
    let player = ctx.entity;

    if name == "clear" {
        ctx.with_world_mut(|world| {
            world.entity_mut(player).remove::<StatusEffects>();
        });
        ctx.reply("Effects cleared.");
        return;
    }

    let qualified = if name.contains(':') {
        name.clone()
    } else {
        format!("minecraft:{name}")
    };
    let Some(effect) = Effect::from_name(&qualified) else {
        ctx.reply_error(&format!("Unknown effect '{name}'."));
        return;
    };
    let duration = match ctx.get::<i32>("seconds").copied() {
        Some(seconds) if seconds > 0 => EffectDuration::seconds(seconds as u32),
        Some(_) => {
            let removed = ctx.with_world_mut(|world| {
                world
                    .get_mut::<StatusEffects>(player)
                    .and_then(|mut effects| effects.remove(effect))
                    .is_some()
            });
            ctx.reply(if removed {
                "Effect removed."
            } else {
                "You did not have that effect."
            });
            return;
        }
        None => EffectDuration::Infinite,
    };
    let amplifier = ctx.get::<i32>("amplifier").copied().unwrap_or(0) as u8;
    let hidden = ctx.flag("hidden");
    let ambient = ctx.flag("ambient");

    ctx.with_world_mut(|world| {
        let mut entity = world.entity_mut(player);
        if !entity.contains::<StatusEffects>() {
            entity.insert(StatusEffects::new());
        }
        let mut effects = entity.get_mut::<StatusEffects>().unwrap();
        let mut slot = effects.add(effect, amplifier, duration);
        if hidden {
            slot = slot.hidden();
        }
        if ambient {
            slot.ambient();
        }
    });
    ctx.reply(&format!(
        "{} {} for {}.",
        effect.name(),
        amplifier + 1,
        match duration {
            EffectDuration::Ticks(ticks) => format!("{}s", ticks / 20),
            EffectDuration::Infinite => "ever".to_string(),
        }
    ));
}

pub(super) fn speed_command() -> Command {
    CommandBuilder::new("speed")
        .description("Example command: multiply your walking speed (1.0 resets)")
        .arg("factor", DoubleArg::new(0.0, 20.0))
        .handler(handle_speed)
        .build()
}

fn handle_speed(ctx: &mut CommandContext) {
    let factor = ctx.get::<f64>("factor").copied().unwrap_or(1.0);
    let player = ctx.entity;
    let value = ctx.with_world_mut(|world| {
        let mut entity = world.entity_mut(player);
        if !entity.contains::<Attributes>() {
            entity.insert(Attributes::new());
        }
        let mut attributes = entity.get_mut::<Attributes>().unwrap();
        if factor == 1.0 {
            attributes.remove_modifier(EntityAttribute::MovementSpeed, SPEED_MODIFIER);
        } else {
            attributes.add_modifier(
                EntityAttribute::MovementSpeed,
                Modifier::multiply_total(SPEED_MODIFIER, factor - 1.0),
            );
        }
        attributes.value(EntityAttribute::MovementSpeed)
    });
    ctx.reply(&format!("Movement speed is now {value:.3} ({factor}x)."));
}
