use bevy_ecs::prelude::*;
use voidmc::components::Operator;
use voidmc::events::PlayerReadyEvent;
use voidmc::{Inventory, On, Query};
use voidmc_worldedit::WorldEditConfig;

/// Operators get the wand on join unless they already carry one. Setting `VOID_EXAMPLE_OPERATORS=1` makes
/// every player an operator, for local testing only.
pub(super) fn grant_wand(
    event: On<PlayerReadyEvent>,
    config: Res<WorldEditConfig>,
    mut commands: Commands,
    mut players: Query<(&mut Inventory, Has<Operator>)>,
) {
    let everyone = std::env::var("VOID_EXAMPLE_OPERATORS").is_ok_and(|v| v == "1");
    if everyone {
        commands.entity(event.entity).insert(Operator);
    }
    let Ok((mut inventory, operator)) = players.get_mut(event.entity) else {
        return;
    };
    if (operator || everyone)
        && let Some(wand) = config.wand()
        && !(0..Inventory::SIZE)
            .map(|index| inventory.get(index))
            .chain([inventory.cursor()])
            .any(|stack| stack.item == wand.item)
    {
        inventory.give(wand);
    }
}
