//! What ties [`crate::goto`] to a session: where the points are on the model, the keys,
//! the list and the tab list.
//!
//! The client sends nothing for the Tab key outside a screen, only for the keys of
//! `PlayerInput`, so jumps go by those. Holding Tab still shows the client's own list, which
//! is filled here: who the figures are, and in its footer who the view is on. In the
//! dialog the key is the client's: it moves down the rows, and Enter presses one.

use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use voidmc::{Players, TextColor, events::PlayerInputEvent};
use voidmc_protocol::clientbound::SetTabListHeaderFooter;
use voidmc_ui::{Dialog, Text, UiClick};

use super::{Atlas, ICON_LIFT, Level, MapMode, MapSession, Navigation};
use crate::goto::{Action, Goto, LIST_SHIFT, Point, Target, YOU, list, stepped, towards};
use crate::players::{Characters, place_on};
use crate::pointer::detail_back;
use crate::regions::RegionState;
use crate::scene;

const TAB_HEADER: &str = "Carte";
const TAB_NO_TARGET: &str = "Personne de suivi";
const TAB_TARGET: &str = "Vue sur";

/// Where a target stands on the model of the session, up to what the view centres on.
fn spot(
    session: &MapSession,
    level: &Level,
    characters: &Characters,
    target: Target,
) -> Option<[f64; 3]> {
    let build = f64::from(session.model().scale().build);
    match target {
        Target::You => {
            let [x, y, z] = session.pins.you.as_ref()?.at;
            Some([x, y + session.you_lift() * build, z])
        }
        Target::Character(id) => {
            let character = characters.0.get(&id)?;
            place_on(&level.scene, &character.position, character.afloat)
        }
        Target::Waypoint([x, z]) => {
            let waypoint = level
                .waypoints
                .iter()
                .find(|w| [w.marker.x, w.marker.z] == [x, z])?;
            let [at_x, at_y, at_z] = waypoint.at;
            Some([at_x, at_y + ICON_LIFT * build, at_z])
        }
    }
}

/// Where a pan has to be for this point of the model to be in the middle of the screen.
/// A pan goes by the sea, and the view comes in from the south: what stands high shows
/// further north. It holds for any height of the view.
pub(super) fn view_on(session: &MapSession, [x, y, z]: [f64; 3]) -> [f64; 2] {
    let ratio = session.model().ratio();
    let lift = y - f64::from(scene::SEA_Y);
    [x / ratio, (z - detail_back(lift)) / ratio]
}

/// Everything the view can go to, in the order of the list: the player, then who is
/// around by name, then the places by name.
fn points(session: &MapSession, level: &Level, characters: &Characters) -> Vec<Point> {
    let point = |target, name: &str, closed| {
        let at = view_on(session, spot(session, level, characters, target)?);
        Some(Point {
            target,
            name: name.to_string(),
            at,
            closed,
        })
    };
    let you = point(Target::You, YOU, false);
    let mut people: Vec<Point> = characters
        .0
        .iter()
        .filter_map(|(id, c)| point(Target::Character(*id), &c.name, false))
        .collect();
    people.sort_by(|a, b| a.name.cmp(&b.name));
    let mut places: Vec<Point> = level
        .waypoints
        .iter()
        .filter_map(|w| {
            let target = Target::Waypoint([w.marker.x, w.marker.z]);
            point(target, &w.marker.name, w.state != RegionState::Discovered)
        })
        .collect();
    places.sort_by(|a, b| a.name.cmp(&b.name));
    you.into_iter().chain(people).chain(places).collect()
}

/// A tick of a view that goes by jumps or is on a target, in place of the mouse. In free
/// navigation a gesture of the mouse lets go of the target: the view stays where it is
/// and slides again, as nothing but the target has changed.
pub(super) fn steer(session: &mut MapSession, atlas: &Atlas, characters: &Characters) {
    let Some(level) = atlas.level(session.model()) else {
        return;
    };
    let flick = session.goto.flick();
    if flick.is_some() && session.looks.navigation == Navigation::Free {
        tracing::info!("map target let go");
        session.goto.clear();
    } else if let Some(way) = flick {
        let points = points(session, level, characters);
        let current = session.goto.target();
        let here = points.iter().find(|point| Some(point.target) == current);
        let from = here.map_or(session.pan.view(), |point| point.at);
        if let Some(point) = towards(&points, from, current, way) {
            tracing::info!(name = point.name, ?way, "map jump");
            session.goto.choose(point);
        }
    }
    let target = session.goto.target();
    let point = target.and_then(|target| spot(session, level, characters, target));
    match point {
        Some(point) => {
            let mut goal = view_on(session, point);
            let listing = session.goto.listing;
            if listing {
                goal[0] -= LIST_SHIFT * session.zoom.height();
            }
            session.goto.held_on = listing.then_some(point);
            let view = session.goto.approach(session.pan.view(), goal);
            session.pan.place(view);
        }
        None => session.goto.clear(),
    }
}

/// Keeps the footer of the tab list on who the view is on.
pub(super) fn tell_tab_list(goto: &mut Goto, player: Entity, players: &Players) {
    let now = goto.name().map(str::to_string);
    if goto.told.as_ref() == Some(&now) {
        return;
    }
    let footer = match &now {
        Some(name) => format!("{TAB_TARGET} {name}"),
        None => TAB_NO_TARGET.to_string(),
    };
    players.send(
        player,
        SetTabListHeaderFooter {
            header: Text::new(TAB_HEADER).color(TextColor::Aqua).to_nbt(),
            footer: Text::new(footer).color(TextColor::Gold).to_nbt(),
        },
    );
    goto.told = Some(now);
}

/// The client keeps a header and a footer until it is told otherwise.
pub(super) fn forget_tab_list(
    event: On<Remove, MapSession>,
    sessions: Query<&MapSession>,
    players: Players,
) {
    let told = sessions
        .get(event.entity)
        .is_ok_and(|session| session.goto.told.is_some());
    if told {
        players.send(
            event.entity,
            SetTabListHeaderFooter {
                header: Text::new("").to_nbt(),
                footer: Text::new("").to_nbt(),
            },
        );
    }
}

/// What the keys of targets are, for the line on screen.
pub(super) fn hint(start: Text, navigation: Navigation, following: bool) -> Text {
    let key = Text::keybind;
    let start = match navigation {
        Navigation::Jumps => start
            .append(key("key.forward"))
            .append(Text::new(" / "))
            .append(key("key.back"))
            .append(Text::new(" : cible   |   ")),
        Navigation::Free if following => start.append(Text::new("Souris : libre   |   ")),
        Navigation::Free => start,
    };
    start
        .append(key("key.jump"))
        .append(Text::new(" : liste   |   "))
}

/// Leaves the view where it is, for a player who changes how it moves.
pub(super) fn start_over(session: &mut MapSession) {
    session.goto.start_over();
    let view = session.pan.view();
    session.pan.place(view);
}

fn open_list(
    session: &mut MapSession,
    player: Entity,
    atlas: &Atlas,
    characters: &Characters,
    players: &Players,
) {
    let Some(level) = atlas.level(session.model()) else {
        return;
    };
    session.goto.listing = true;
    players.send(player, list(&points(session, level, characters)).packet());
}

/// Jump opens the list, however the view moves. By jumps, forward and back go down and up
/// it without opening it. These are what a client reports of the keys, with a spectator's
/// camera on another entity.
pub(super) fn read_keys(
    event: On<PlayerInputEvent>,
    mut sessions: Query<&mut MapSession>,
    atlas: Res<Atlas>,
    characters: Res<Characters>,
    players: Players,
) {
    let Ok(mut session) = sessions.get_mut(event.entity) else {
        return;
    };
    let keys = [event.forward, event.backward, event.jump];
    let pressed: [bool; 3] = std::array::from_fn(|at| keys[at] && !session.goto.keys[at]);
    session.goto.keys = keys;
    if session.mode != MapMode::Pan || !session.at_hand() || session.is_frozen() {
        return;
    }
    if pressed[2] {
        open_list(&mut session, event.entity, &atlas, &characters, &players);
    }
    if session.looks.navigation == Navigation::Jumps
        && (pressed[0] || pressed[1])
        && let Some(level) = atlas.level(session.model())
    {
        let points = points(&session, level, &characters);
        let (current, view) = (session.goto.target(), session.pan.view());
        if let Some(point) = stepped(&points, current, view, pressed[0]) {
            tracing::info!(name = point.name, "map next target");
            session.goto.choose(point);
        }
    }
}

/// A row of the list. The dialog stays as it is, with its scroll and the Tab key's place.
pub(super) fn click(
    event: On<UiClick>,
    mut sessions: Query<&mut MapSession>,
    atlas: Res<Atlas>,
    characters: Res<Characters>,
    players: Players,
) {
    let Some(action) = Action::parse(&event.id) else {
        return;
    };
    let player = event.player;
    let Ok(mut session) = sessions.get_mut(player) else {
        players.send(player, Dialog::close());
        return;
    };
    match action {
        Action::Go(target) => {
            let Some(level) = atlas.level(session.model()) else {
                return;
            };
            let points = points(&session, level, &characters);
            if let Some(point) = points.iter().find(|point| point.target == target) {
                tracing::info!(name = point.name, "map target from the list");
                session.goto.choose(point);
            }
        }
        Action::Close => {
            session.goto.listing = false;
            session.goto.held_on = None;
            players.send(player, Dialog::close());
        }
    }
}
