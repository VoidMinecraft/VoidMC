use voidmc::{Command, CommandBuilder, CommandContext, ItemStack, Menu, MenuSlot, WorldMenus};

const WARES: [(&str, &str); 4] = [
    ("minecraft:diamond_sword", "a sword"),
    ("minecraft:golden_apple", "an apple"),
    ("minecraft:ender_pearl", "a pearl"),
    ("minecraft:barrier", "the close button"),
];

pub(super) fn menu_command() -> Command {
    CommandBuilder::new("menu")
        .description("Example command: open a read-only chest menu whose clicks talk back")
        .handler(handle_menu)
        .build()
}

fn handle_menu(ctx: &mut CommandContext) {
    let mut menu = Menu::chest(1, "Void Example Shop").on_click(|click| match click.slot {
        MenuSlot::Menu(index) if index < WARES.len() => {
            let (_, label) = WARES[index];
            if index == WARES.len() - 1 {
                click.close();
            } else {
                click.reply(&format!("You clicked {label} (slot {index})."));
            }
        }
        MenuSlot::Menu(_) => click.reply("Nothing here."),
        MenuSlot::Inventory(index) => {
            click.reply(&format!("That is your own inventory slot {index}."))
        }
        MenuSlot::Outside => {}
    });
    for (index, (item, _)) in WARES.iter().enumerate() {
        if let Some(stack) = ItemStack::of(item, 1) {
            menu = menu.slot(index, stack);
        }
    }
    let player = ctx.entity;
    ctx.with_world_mut(|world| WorldMenus::new(world).open(player, menu));
    ctx.reply("Menu opened. Click a slot, or the barrier to close.");
}
