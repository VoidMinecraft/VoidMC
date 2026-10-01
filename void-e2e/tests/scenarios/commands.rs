use std::collections::{HashMap, HashSet};

use voidmc::{Command, CommandBuilder, EnumArg};
use voidmc_e2e::TestServer;
use voidmc_e2e::brigadier::suggestion::Suggestions;
use voidmc_e2e::core::position::Vec3;
use voidmc_e2e::inventory::ItemStack;
use voidmc_e2e::protocol::packets::game::c_commands::{BrigadierNodeStub, NodeType};
use voidmc_e2e::protocol::packets::game::{
    ClientboundGamePacket, s_command_suggestion::ServerboundCommandSuggestion,
};
use voidmc_e2e::registry::builtin::ItemKind;

fn assert_valid_tree(nodes: &[BrigadierNodeStub], root: usize) {
    assert!(matches!(nodes[root].node_type, NodeType::Root));
    let mut reachable = HashSet::from([root]);
    let mut stack = vec![root];
    while let Some(index) = stack.pop() {
        let node = &nodes[index];
        for child in node.children.iter().chain(&node.redirect_node) {
            let child = *child as usize;
            assert!(
                child < nodes.len(),
                "node {index} points at missing node {child}"
            );
            if reachable.insert(child) {
                stack.push(child);
            }
        }
        if index != root {
            assert!(
                node.name()
                    .is_some_and(|name| !name.is_empty() && !name.contains(' '))
            );
        }
    }
    assert_eq!(
        reachable.len(),
        nodes.len(),
        "every node hangs off the root"
    );
}

#[tokio::test]
async fn command_tree_is_well_formed_and_commands_run() {
    let server = TestServer::start().await;
    let alice = server.join("Alice").await;
    let bob = server.join("Bob").await;

    let (nodes, root) = alice
        .expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    assert_valid_tree(&nodes, root);
    let top_level: HashSet<&str> = nodes[root]
        .children
        .iter()
        .filter_map(|child| nodes[*child as usize].name())
        .collect();
    for command in ["tp", "give", "help", "say"] {
        assert!(
            top_level.contains(command),
            "/{command} missing from {top_level:?}"
        );
    }

    let destination = Vec3::new(10.5, 120.0, -3.5);
    alice.command("tp 10.5 120 -3.5").await;
    alice
        .expect("the teleport", |packet| match packet {
            ClientboundGamePacket::PlayerPosition(teleport) => {
                (teleport.change.pos == destination).then_some(())
            }
            _ => None,
        })
        .await;
    assert_eq!(alice.position(), destination);
    bob.wait_until("Alice teleported", |view| {
        view.entity_by_uuid(alice.uuid())
            .is_some_and(|(_, entity)| entity.position.distance_to(destination) < 1e-3)
    })
    .await;

    alice.command("give diamond 5").await;
    alice
        .wait_until("five diamonds", |view| {
            view.inventory.iter().any(|item| {
                matches!(item, ItemStack::Present(data) if data.kind == ItemKind::Diamond && data.count == 5)
            })
        })
        .await;

    alice.chat("hello from e2e").await;
    bob.expect("Alice's chat line", |packet| match packet {
        ClientboundGamePacket::SystemChat(chat) => chat
            .content
            .to_string()
            .contains("hello from e2e")
            .then_some(()),
        _ => None,
    })
    .await;

    alice.assert_healthy();
    bob.assert_healthy();
    server.stop().await;
}

fn pick_command() -> Command {
    CommandBuilder::new("pick")
        .arg(
            "flavour",
            EnumArg::new([("crème", 1), ("café", 2), ("💎", 3), ("plain", 4)]),
        )
        .handler(|_| {})
        .build()
}

#[tokio::test]
async fn server_suggestions_use_utf16_ranges() {
    let server = TestServer::builder().command(pick_command).start().await;
    let alice = server.join("Alice").await;

    let (nodes, root) = alice
        .expect("the command tree", |packet| match packet {
            ClientboundGamePacket::Commands(tree) => {
                Some((tree.entries.clone(), tree.root_index as usize))
            }
            _ => None,
        })
        .await;
    let pick = nodes[root]
        .children
        .iter()
        .map(|child| &nodes[*child as usize])
        .find(|node| node.name() == Some("pick"))
        .expect("/pick in the tree");
    let flavour = &nodes[pick.children[0] as usize];
    assert!(
        matches!(&flavour.node_type, NodeType::Argument { suggestions_type: Some(kind), .. } if kind.to_string() == "minecraft:ask_server"),
        "the client only asks the server when told to: {flavour:?}"
    );

    let inputs = ["/pick cr", "/pick ca", "/pick 💎", "/pick  é", "/unknown x"];
    for (id, text) in inputs.into_iter().enumerate() {
        alice
            .send(ServerboundCommandSuggestion {
                id: id as u32,
                command: text.to_string(),
            })
            .await;
    }
    alice.sync().await;

    let responses: HashMap<u32, Suggestions> = alice
        .queued(|packet| match packet {
            ClientboundGamePacket::CommandSuggestions(response) => {
                Some((response.id, response.suggestions.clone()))
            }
            _ => None,
        })
        .into_iter()
        .collect();
    let range = |id: u32| {
        let range = responses[&id].range();
        (range.start(), range.length())
    };

    let texts = |id: u32| -> Vec<String> {
        responses[&id]
            .list()
            .iter()
            .map(|suggestion| suggestion.text())
            .collect()
    };

    assert_eq!(texts(0), ["crème"]);
    assert_eq!(range(0), (6, 2));
    assert_eq!(texts(1), ["café"]);
    assert_eq!(range(1), (6, 2));
    assert_eq!(texts(2), ["💎"]);
    assert_eq!(range(2), (6, 2), "an emoji is two UTF-16 units");
    assert!(texts(3).is_empty());
    assert_eq!(range(3), (7, 1));
    assert!(!responses.contains_key(&4));

    alice.assert_healthy();
    server.stop().await;
}
