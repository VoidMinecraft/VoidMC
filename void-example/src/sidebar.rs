use std::time::{Duration, Instant};

use bevy_ecs::prelude::{Component, Local, Query, With};
use voidmc::components::{PlayerName, PlayerReady, Position};
use voidmc::{Ornament, Sidebar, TextColor, Widget, WidgetId};

#[derive(Component)]
pub(super) struct AltitudeBoard {
    online: WidgetId,
    ranking: WidgetId,
    uptime: WidgetId,
}

pub(super) fn altitude_board() -> impl bevy_ecs::bundle::Bundle {
    let mut sidebar = Sidebar::new("Altitude")
        .color(TextColor::Gold)
        .ornament(Ornament::brackets("✦"));
    let online = sidebar.push(Widget::labeled("Online", "0").color(TextColor::Aqua));
    sidebar.push(Widget::separator('─'));
    let ranking = sidebar.push(Widget::ranking().limit(5));
    sidebar.push(Widget::blank());
    let uptime = sidebar.push(Widget::timer(Duration::ZERO).label("Uptime"));
    (
        sidebar,
        AltitudeBoard {
            online,
            ranking,
            uptime,
        },
    )
}

pub(super) fn altitude_system(
    players: Query<(&PlayerName, &Position), With<PlayerReady>>,
    mut boards: Query<(&mut Sidebar, &AltitudeBoard)>,
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
    for (mut board, handles) in &mut boards {
        board.set_value(handles.online, &online);
        if let Some(ranking) = board.ranking_mut(handles.ranking) {
            ranking.set(altitudes.iter().cloned());
        }
        if let Some(timer) = board.timer_mut(handles.uptime) {
            timer.set(Duration::from_secs(uptime));
        }
    }
    *last = (altitudes, uptime);
}
