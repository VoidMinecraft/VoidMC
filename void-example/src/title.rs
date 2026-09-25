use std::sync::Arc;

use voidmc::{Command, CommandBuilder, CommandContext, GreedyStringArg, TextColor, WorldTitles};

pub(super) fn title_command() -> Command {
    CommandBuilder::new("title")
        .description("Example command: show a title (`clear` / `reset` hide it)")
        .arg_variadic_required("title", Arc::new(GreedyStringArg))
        .handler(handle_title)
        .build()
}

fn handle_title(ctx: &mut CommandContext) {
    let title = ctx.get::<String>("title").cloned().unwrap_or_default();
    let player = ctx.entity;
    let reply = ctx.with_world(|world| {
        let titles = WorldTitles::new(world);
        match title.as_str() {
            "clear" => {
                titles.clear(player).send();
                "Title cleared.".to_string()
            }
            "reset" => {
                titles.reset(player).send();
                "Title cleared and times reset.".to_string()
            }
            _ => {
                titles
                    .title(player, &title)
                    .subtitle("")
                    .color(TextColor::Gold)
                    .times(10, 70, 20)
                    .send();
                format!("Title '{title}' sent.")
            }
        }
    });
    ctx.reply(&reply);
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::*;
    use flume::Receiver;
    use ussr_nbt::owned::{Nbt, Tag};
    use voidmc::CommandRegistry;
    use voidmc::commands::dispatch_command;
    use voidmc::components::{ClientId, PlayerReady};
    use voidmc::network::{NetworkChannels, OutgoingPacket};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket};

    use super::title_command;

    fn command_world() -> (World, Entity, Receiver<OutgoingPacket>) {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<voidmc::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();

        let mut world = World::new();
        world.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        });

        let mut registry = CommandRegistry::new();
        registry.register(title_command());
        world.insert_resource(registry);

        let player = world.spawn((ClientId(7), PlayerReady)).id();
        (world, player, outgoing_rx)
    }

    fn text(nbt: &Nbt) -> String {
        match nbt
            .compound
            .tags
            .iter()
            .find(|(name, _)| name.to_string() == "text")
        {
            Some((_, Tag::String(value))) => value.to_string(),
            other => panic!("expected text field, got {other:?}"),
        }
    }

    fn run(args: &[&str]) -> Vec<PlayPacket> {
        let (mut world, player, rx) = command_world();
        dispatch_command(
            &mut world,
            7,
            player,
            "title",
            args.iter().map(|a| a.to_string()).collect(),
        );
        rx.try_iter()
            .map(|out| match out.packet {
                ClientboundPacket::Play(packet) => packet,
                other => panic!("expected a play packet, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn multi_word_title_is_sent_as_one_greedy_string() {
        let packets = run(&["Hello", "world"]);
        let titles: Vec<String> = packets
            .iter()
            .filter_map(|packet| match packet {
                PlayPacket::SetTitleText(p) => Some(text(&p.text)),
                _ => None,
            })
            .collect();
        assert_eq!(titles, vec!["Hello world".to_string()]);
        assert!(
            packets
                .iter()
                .any(|packet| matches!(packet, PlayPacket::SetTitlesAnimation(_)))
        );
    }

    #[test]
    fn clear_still_clears_without_resetting_times() {
        let packets = run(&["clear"]);
        let clears: Vec<bool> = packets
            .iter()
            .filter_map(|packet| match packet {
                PlayPacket::ClearTitles(p) => Some(p.reset_times),
                _ => None,
            })
            .collect();
        assert_eq!(clears, vec![false]);
        assert!(
            !packets
                .iter()
                .any(|packet| matches!(packet, PlayPacket::SetTitleText(_)))
        );
    }

    #[test]
    fn reset_clears_and_resets_times() {
        let packets = run(&["reset"]);
        let clears: Vec<bool> = packets
            .iter()
            .filter_map(|packet| match packet {
                PlayPacket::ClearTitles(p) => Some(p.reset_times),
                _ => None,
            })
            .collect();
        assert_eq!(clears, vec![true]);
    }
}
