use std::collections::HashSet;

use bevy_ecs::prelude::Entity;

use crate::players::{Audience, Recipients};

/// `None` when `current` already equals the audience's ready members.
pub(crate) fn desired_viewers(
    ready: &Recipients<'_>,
    audience: &Audience,
    current: &HashSet<Entity>,
) -> Option<HashSet<Entity>> {
    let members = || {
        ready
            .iter()
            .filter(|r| audience.includes(r))
            .map(|r| r.entity())
    };
    let mut count = 0;
    let same = members().all(|entity| {
        count += 1;
        current.contains(&entity)
    }) && count == current.len();
    (!same).then(|| members().collect())
}
