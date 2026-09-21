use voidmc::{Command, CommandBuilder, CommandContext, FloatArg, Sound, StringArg, WorldSounds};

pub(super) fn sound_command() -> Command {
    CommandBuilder::new("sound")
        .description("Example command: play a sound event at your position")
        .arg("id", StringArg::single_word())
        .arg_optional("volume", FloatArg::new(0.0, 10.0))
        .arg_optional("pitch", FloatArg::new(0.5, 2.0))
        .handler(handle_sound)
        .build()
}

fn handle_sound(ctx: &mut CommandContext) {
    let id = ctx.get::<String>("id").cloned().unwrap_or_default();
    let volume = ctx.get::<f32>("volume").copied().unwrap_or(1.0);
    let pitch = ctx.get::<f32>("pitch").copied().unwrap_or(1.0);
    if voidmc::sounds::resolve(&id).is_none() {
        ctx.reply_error(&format!("Unknown sound event '{id}'."));
        return;
    }
    let player = ctx.entity;
    let played = ctx.with_world(|world| {
        WorldSounds::new(world).play_to(player, Sound::new(&id).volume(volume).pitch(pitch))
    });
    if played {
        ctx.reply(&format!("Playing {id} (volume {volume}, pitch {pitch})."));
    } else {
        ctx.reply_error("Could not play the sound.");
    }
}
