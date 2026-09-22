use std::collections::HashSet;

use bevy_ecs::prelude::Entity;

use crate::players::{Audience, Recipients};

/// Ready members of every `Audience::Explicit` in `audiences`: players an
/// override has taken over, which broader audiences must leave alone.
pub(crate) fn claimed<'a>(
    ready: &Recipients<'_>,
    audiences: impl Iterator<Item = &'a Audience>,
) -> HashSet<Entity> {
    let mut claimed = HashSet::new();
    for audience in audiences {
        if let Audience::Explicit(players) = audience {
            claimed.extend(
                ready
                    .iter()
                    .map(|r| r.entity())
                    .filter(|entity| players.contains(entity)),
            );
        }
    }
    claimed
}

/// `None` when `current` already equals the audience's ready members. A
/// non-explicit audience yields to `claimed`; an explicit one ignores it.
pub(crate) fn desired_viewers(
    ready: &Recipients<'_>,
    audience: &Audience,
    current: &HashSet<Entity>,
    claimed: &HashSet<Entity>,
) -> Option<HashSet<Entity>> {
    let yields = !matches!(audience, Audience::Explicit(_)) && !claimed.is_empty();
    let members = || {
        ready
            .iter()
            .filter(|r| audience.includes(r))
            .map(|r| r.entity())
            .filter(|entity| !yields || !claimed.contains(entity))
    };
    let mut count = 0;
    let same = members().all(|entity| {
        count += 1;
        current.contains(&entity)
    }) && count == current.len();
    (!same).then(|| members().collect())
}
