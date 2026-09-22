use std::sync::Arc;

use voidmc::{
    Command, CommandBuilder, CommandContext, GreedyStringArg, ItemArg, ItemId, Sound, StringArg,
    TextColor, ToastFrame, WorldToasts,
};

pub(super) fn toast_command() -> Command {
    CommandBuilder::new("toast")
        .description("Example command: pop an advancement toast (`/toast <task|goal|challenge> <item> <title>`)")
        .arg("frame", StringArg::single_word())
        .arg("item", Arc::new(ItemArg))
        .arg_variadic_required("title", Arc::new(GreedyStringArg))
        .handler(handle_toast)
        .build()
}

fn parse_frame(name: &str) -> Option<ToastFrame> {
    match name {
        "task" => Some(ToastFrame::Task),
        "goal" => Some(ToastFrame::Goal),
        "challenge" => Some(ToastFrame::Challenge),
        _ => None,
    }
}

fn handle_toast(ctx: &mut CommandContext) {
    let frame = ctx.get::<String>("frame").cloned().unwrap_or_default();
    let icon = ctx.get::<ItemId>("item").copied();
    let title = ctx.get::<String>("title").cloned().unwrap_or_default();
    let Some(frame) = parse_frame(&frame) else {
        ctx.reply_error("Frame must be task, goal or challenge.");
        return;
    };
    let Some(icon) = icon else {
        ctx.reply_error("Unknown item.");
        return;
    };
    let player = ctx.entity;
    ctx.with_world(|world| {
        WorldToasts::new(world)
            .toast(player, &title)
            .description("Sent with /toast")
            .icon(icon)
            .frame(frame)
            .color(TextColor::Yellow)
            .sound(Sound::new("entity.player.levelup"))
            .send();
    });
    ctx.reply(&format!("Toast '{title}' sent."));
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::*;
    use flume::Receiver;
    use voidmc::CommandRegistry;
    use voidmc::commands::dispatch_command;
    use voidmc::components::{ClientId, PlayerReady, Position};
    use voidmc::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use voidmc_protocol::clientbound::{
        AdvancementFrame, ClientboundPacket, ManualPlayPacket, PlayPacket,
    };

    use super::toast_command;

    fn command_world() -> (World, Entity, Receiver<OutgoingPacket>) {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();

        let mut world = World::new();
        world.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        });

        let mut registry = CommandRegistry::new();
        registry.register(toast_command());
        world.insert_resource(registry);

        let player = world
            .spawn((
                ClientId(7),
                PlayerReady,
                Position {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
            ))
            .id();
        (world, player, outgoing_rx)
    }

    fn run(args: &[&str]) -> Vec<ClientboundPacket> {
        let (mut world, player, rx) = command_world();
        dispatch_command(
            &mut world,
            7,
            player,
            "toast",
            args.iter().map(|a| a.to_string()).collect(),
        );
        rx.try_iter().map(|out| out.packet).collect()
    }

    #[test]
    fn sends_add_remove_sound_then_reply() {
        let packets = run(&["challenge", "minecraft:gold_ingot", "New", "record!"]);
        let ClientboundPacket::ManualPlay(ManualPlayPacket::UpdateAdvancements(add)) = &packets[0]
        else {
            panic!("expected the add packet, got {:?}", packets[0]);
        };
        let display = add.added[0].advancement.display.as_ref().unwrap();
        assert_eq!(display.frame, AdvancementFrame::Challenge);
        assert_eq!(
            display.icon.item_id,
            voidmc::ItemId::from_name("minecraft:gold_ingot").unwrap().0
        );
        let ClientboundPacket::ManualPlay(ManualPlayPacket::UpdateAdvancements(remove)) =
            &packets[1]
        else {
            panic!("expected the remove packet, got {:?}", packets[1]);
        };
        assert_eq!(remove.removed, vec![add.added[0].id.clone()]);
        assert!(matches!(
            packets[2],
            ClientboundPacket::Play(PlayPacket::SoundEffect(_))
        ));
        assert!(matches!(
            packets[3],
            ClientboundPacket::Play(PlayPacket::SystemChat(_))
        ));
        assert_eq!(packets.len(), 4);
    }

    #[test]
    fn rejects_unknown_frames_and_items() {
        for args in [
            &["spiky", "minecraft:stone", "x"][..],
            &["task", "minecraft:not_an_item", "x"][..],
        ] {
            let packets = run(args);
            assert!(!packets.is_empty());
            assert!(packets.iter().all(|packet| matches!(
                packet,
                ClientboundPacket::Play(PlayPacket::SystemChat(_))
            )));
        }
    }
}
