use bevy_ecs::prelude::*;
use voidmc::components::Operator;
use voidmc::events::PlayerReadyEvent;
use voidmc::{Inventory, On, Query};
use voidmc_worldedit::WorldEditConfig;

/// The environment variable that decides whether every player is an operator.
const OPERATORS_ENV: &str = "VOID_EXAMPLE_OPERATORS";

/// Whether every player is made an operator, from the value of [`OPERATORS_ENV`]. The example is a local
/// test server, so this is the default; `VOID_EXAMPLE_OPERATORS=0` turns it off, and nobody is then an
/// operator.
fn everyone_is_operator(value: Option<&str>) -> bool {
    value != Some("0")
}

/// Every player is an operator unless `VOID_EXAMPLE_OPERATORS=0` is set, so they may run the operator-only
/// default commands. Operators get the wand on join unless they already carry one.
pub(super) fn grant_wand(
    event: On<PlayerReadyEvent>,
    config: Res<WorldEditConfig>,
    mut commands: Commands,
    mut players: Query<(&mut Inventory, Has<Operator>)>,
) {
    let everyone = everyone_is_operator(std::env::var(OPERATORS_ENV).ok().as_deref());
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

#[cfg(test)]
mod tests {
    use super::everyone_is_operator;

    #[test]
    fn every_player_is_an_operator_unless_turned_off() {
        assert!(everyone_is_operator(None));
        assert!(everyone_is_operator(Some("1")));
        assert!(everyone_is_operator(Some("")));
        assert!(!everyone_is_operator(Some("0")));
    }
}
