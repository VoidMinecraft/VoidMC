use std::time::{Duration, Instant};

use bevy_ecs::prelude::{Component, Local, Query, With};
use voidmc::components::{PlayerName, PlayerReady, Position};
use voidmc::{Ornament, Sidebar, TextColor, Widget};

#[derive(Component)]
pub(super) struct AltitudeBoard;

const ONLINE: usize = 0;
const RANKING: usize = 2;
const UPTIME: usize = 4;

pub(super) fn altitude_board() -> impl bevy_ecs::bundle::Bundle {
    (
        Sidebar::new("Altitude")
            .color(TextColor::Gold)
            .ornament(Ornament::brackets("✦"))
            .widget(Widget::labeled("Online", "0").color(TextColor::Aqua))
            .widget(Widget::separator('─'))
            .widget(Widget::ranking().limit(5))
            .widget(Widget::blank())
            .widget(Widget::timer(Duration::ZERO).label("Uptime")),
        AltitudeBoard,
    )
}

pub(super) fn altitude_system(
    players: Query<(&PlayerName, &Position), With<PlayerReady>>,
    mut boards: Query<&mut Sidebar, With<AltitudeBoard>>,
    mut started: Local<Option<Instant>>,
    mut last: Local<(Vec<(String, i32)>, u64)>,
) {
    let started = *started.get_or_insert_with(Instant::now);
    let mut altitudes: Vec<(String, i32)> = players
        .iter()
        .map(|(name, position)| (name.0.clone(), position.y.floor() as i32))
        .collect();
    altitudes.sort();
    let uptime = started.elapsed().as_secs();
    if (altitudes.as_slice(), uptime) == (last.0.as_slice(), last.1) {
        return;
    }
    let online = altitudes.len().to_string();
    for mut board in &mut boards {
        board.set_value(ONLINE, &online);
        if let Some(ranking) = board.ranking_mut(RANKING) {
            ranking.set(altitudes.iter().cloned());
        }
        if let Some(timer) = board.timer_mut(UPTIME) {
            timer.set(Duration::from_secs(uptime));
        }
    }
    *last = (altitudes, uptime);
}
