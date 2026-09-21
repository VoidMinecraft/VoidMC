use bevy_ecs::prelude::{Component, World};
use voidmc::{
    Command, CommandBuilder, CommandContext, IntegerArg, StringArg, TICKS_PER_DAY, Weather,
    WeatherKind, WorldTime,
};

#[derive(Component)]
pub(super) struct WorldEnvironment;

pub(super) fn spawn_environment(world: &mut World) {
    world.spawn((WorldEnvironment, WorldTime::new().time_of_day(1000)));
    world.spawn((WorldEnvironment, Weather::clear().transition(100)));
}

pub(super) fn time_command() -> Command {
    CommandBuilder::new("time")
        .description("Example command: set, add, freeze, resume or query the world time")
        .arg("action", StringArg::single_word())
        .arg_optional("value", StringArg::single_word())
        .handler(handle_time)
        .build()
}

fn parse_time(value: &str) -> Option<i64> {
    match value {
        "day" => Some(1000),
        "noon" => Some(6000),
        "night" => Some(13000),
        "midnight" => Some(18000),
        other => other.parse().ok(),
    }
}

fn handle_time(ctx: &mut CommandContext) {
    let action = ctx.get::<String>("action").cloned().unwrap_or_default();
    let value = ctx.get::<String>("value").cloned();
    let outcome = ctx.with_world_mut(|world| {
        let mut query =
            world.query_filtered::<&mut WorldTime, bevy_ecs::prelude::With<WorldEnvironment>>();
        let Ok(mut time) = query.single_mut(world) else {
            return Err("No world clock is running.".to_string());
        };
        match action.as_str() {
            "set" => {
                let ticks = value
                    .as_deref()
                    .and_then(parse_time)
                    .ok_or("Usage: /time set <day|noon|night|midnight|ticks>")?;
                let mut target = time.time_of_day - time.day_time() + ticks;
                if target < time.time_of_day {
                    target += TICKS_PER_DAY;
                }
                time.set(target);
                Ok(format!("Time set to {}.", time.day_time()))
            }
            "add" => {
                let ticks: i64 = value
                    .as_deref()
                    .and_then(|v| v.parse().ok())
                    .ok_or("Usage: /time add <ticks>")?;
                time.add(ticks);
                Ok(format!("Time is now {}.", time.day_time()))
            }
            "freeze" => {
                time.freeze();
                Ok(format!("Time frozen at {}.", time.day_time()))
            }
            "resume" => {
                time.resume();
                Ok("Time resumed.".to_string())
            }
            "query" => Ok(format!(
                "Day {} at {} ({}), age {}.",
                time.time_of_day.div_euclid(TICKS_PER_DAY),
                time.day_time(),
                if time.is_frozen() {
                    "frozen"
                } else {
                    "running"
                },
                time.age
            )),
            _ => Err("Usage: /time <set|add|freeze|resume|query> [value]".to_string()),
        }
    });
    match outcome {
        Ok(message) => ctx.reply(&message),
        Err(message) => ctx.reply_error(&message),
    }
}

pub(super) fn weather_command() -> Command {
    CommandBuilder::new("weather")
        .description("Example command: clear, rain or thunder, with an optional fade in ticks")
        .arg("kind", StringArg::single_word())
        .arg_optional("transition", IntegerArg::new(0, 12_000))
        .handler(handle_weather)
        .build()
}

fn handle_weather(ctx: &mut CommandContext) {
    let kind = match ctx.get::<String>("kind").map(String::as_str) {
        Some("clear") => WeatherKind::Clear,
        Some("rain") => WeatherKind::Rain { level: 1.0 },
        Some("thunder") => WeatherKind::Thunder {
            rain: 1.0,
            thunder: 1.0,
        },
        _ => {
            ctx.reply_error("Usage: /weather <clear|rain|thunder> [transition]");
            return;
        }
    };
    let transition = ctx.get::<i32>("transition").copied();
    let changed = ctx.with_world_mut(|world| {
        let mut query =
            world.query_filtered::<&mut Weather, bevy_ecs::prelude::With<WorldEnvironment>>();
        let Ok(mut weather) = query.single_mut(world) else {
            return false;
        };
        weather.set(kind);
        if let Some(transition) = transition {
            weather.transition = transition as u32;
        }
        true
    });
    if changed {
        ctx.reply(&format!("Weather set to {kind:?}."));
    } else {
        ctx.reply_error("No world weather is running.");
    }
}
