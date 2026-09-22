use bevy_ecs::prelude::*;
use voidmc::components::PlayerName;
use voidmc::{Ornament, Sidebar, TabList, TextColor, Widget, WidgetId};

use crate::kart::Kart;
use crate::race::{Phase, Race, Racer};
use crate::track::{GATES, Track};
use crate::vehicle::Pilot;

pub const TITLE: &str = "Alpine Rush";
pub const RANK_LINES: usize = 5;
pub const RANK_HYSTERESIS: f64 = 0.002;
pub const NO_TIME: &str = "--:--.-";
const PROGRESS_WIDTH: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Idle,
    Building,
    Racing,
}

impl Layout {
    pub fn of(phase: Phase) -> Self {
        match phase {
            Phase::Lobby | Phase::Results => Layout::Idle,
            Phase::Generating | Phase::Loading | Phase::Destroying => Layout::Building,
            Phase::Countdown | Phase::Racing => Layout::Racing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Construction,
    Loading,
    Demolition,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Stage::Construction => "Construction",
            Stage::Loading => "Chargement",
            Stage::Demolition => "Demontage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lap {
    Spectator,
    Lap(usize, usize),
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Finished(Entity, u64),
    Racing(Entity, usize),
    Result(u64, usize),
}

impl Row {
    fn player(self) -> Option<Entity> {
        match self {
            Row::Finished(player, _) | Row::Racing(player, _) => Some(player),
            Row::Result(..) => None,
        }
    }
}

#[derive(Resource, Debug, Default)]
pub struct Standings {
    pub rows: Vec<Row>,
    pub version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct View {
    layout: Layout,
    seed: u64,
    round: u64,
    lap: Lap,
    rank: Option<(usize, usize)>,
    seconds: u64,
    stage: Option<(Stage, usize)>,
    best_lap: Option<u64>,
    record: Option<u64>,
    standings: u64,
}

#[derive(Debug, Default)]
struct Handles {
    circuit: Option<WidgetId>,
    round: Option<WidgetId>,
    lap: Option<WidgetId>,
    rank: Option<WidgetId>,
    timer: Option<WidgetId>,
    stage: Option<WidgetId>,
    percent: Option<WidgetId>,
    anchor: Option<WidgetId>,
    rows: Vec<WidgetId>,
    best_lap: Option<WidgetId>,
    record: Option<WidgetId>,
}

#[derive(Component, Debug, Default)]
pub struct Board {
    shown: Option<View>,
    handles: Handles,
}

impl Board {
    pub fn layout(&self) -> Option<Layout> {
        self.shown.map(|view| view.layout)
    }
}

#[derive(Component)]
pub struct RaceTab;

pub fn board(player: Entity) -> impl Bundle {
    (
        Sidebar::new(TITLE)
            .color(TextColor::Gold)
            .ornament(Ornament::brackets("✦"))
            .viewers([player]),
        Board::default(),
    )
}

pub fn tab(seed: u64) -> impl Bundle {
    (
        RaceTab,
        TabList::new()
            .header(TITLE)
            .header_color(TextColor::Gold)
            .footer(footer(seed))
            .footer_color(TextColor::Gray),
    )
}

pub fn footer(seed: u64) -> String {
    format!("Circuit #{seed} · /race [tours] [seed]")
}

pub fn clock_tenths(ticks: u64) -> String {
    format!(
        "{}:{:02}.{}",
        ticks / 1200,
        ticks / 20 % 60,
        ticks % 20 * 10 / 20
    )
}

fn time_or_dash(ticks: Option<u64>) -> String {
    ticks.map_or_else(|| NO_TIME.into(), clock_tenths)
}

pub fn tab_list(map: Res<Track>, mut tabs: Query<&mut TabList, With<RaceTab>>) {
    if !map.is_changed() {
        return;
    }
    let footer = footer(map.seed);
    for mut tab in &mut tabs {
        if tab.footer != footer {
            tab.footer = footer.clone();
        }
    }
}

pub fn standings(
    race: Res<Race>,
    map: Res<Track>,
    karts: Query<(&Pilot, &Kart)>,
    mut standings: ResMut<Standings>,
    mut scratch: Local<Vec<(Row, f64)>>,
) {
    scratch.clear();
    match Layout::of(race.phase) {
        Layout::Racing => {
            for (pilot, kart) in &karts {
                if !kart.participant {
                    continue;
                }
                scratch.push(match kart.finished {
                    Some(time) => (Row::Finished(pilot.0, time), time as f64),
                    None => (
                        Row::Racing(pilot.0, lap_of(kart, race.laps)),
                        match race.phase {
                            Phase::Countdown => slot_of(&race, pilot.0),
                            _ => -kart.progress(&map),
                        },
                    ),
                });
            }
            scratch.sort_by(|a, b| {
                let racing = |row: &Row| matches!(row, Row::Racing(..));
                racing(&a.0)
                    .cmp(&racing(&b.0))
                    .then_with(|| a.1.total_cmp(&b.1))
            });
            settle(&mut scratch, &standings.rows);
        }
        Layout::Idle | Layout::Building => {
            for index in 0..race.results.len() {
                scratch.push((Row::Result(race.round, index), 0.0));
            }
        }
    }
    if scratch.len() == standings.rows.len()
        && scratch.iter().zip(&standings.rows).all(|(a, b)| a.0 == *b)
    {
        return;
    }
    let standings = &mut *standings;
    standings.rows.clear();
    standings.rows.extend(scratch.iter().map(|(row, _)| *row));
    standings.version += 1;
}

fn lap_of(kart: &Kart, laps: usize) -> usize {
    ((kart.next_gate - 1) / GATES + 1).min(laps)
}

fn slot_of(race: &Race, player: Entity) -> f64 {
    race.roster
        .iter()
        .position(|held| *held == player)
        .map_or(f64::MAX, |slot| slot as f64)
}

fn settle(scratch: &mut [(Row, f64)], previous: &[Row]) {
    let rank = |row: Row| {
        previous
            .iter()
            .position(|held| held.player() == row.player())
    };
    for start in 1..scratch.len() {
        let mut at = start;
        while at > 0 {
            let (above, below) = (scratch[at - 1], scratch[at]);
            let close = matches!((above.0, below.0), (Row::Racing(..), Row::Racing(..)))
                && (above.1 - below.1).abs() < RANK_HYSTERESIS;
            let held = matches!((rank(above.0), rank(below.0)), (Some(a), Some(b)) if b < a);
            if !(close && held) {
                break;
            }
            scratch.swap(at - 1, at);
            at -= 1;
        }
    }
}

fn view(
    race: &Race,
    map: &Track,
    standings: &Standings,
    viewer: Entity,
    racers: &Query<&Racer>,
    karts: &Query<(&Pilot, &Kart)>,
) -> View {
    let layout = Layout::of(race.phase);
    let racer = racers.get(viewer).ok();
    let kart = racer.and_then(|racer| karts.get(racer.kart).ok().map(|(_, kart)| kart));
    let lap = match kart {
        Some(kart) if kart.participant && layout == Layout::Racing => match kart.finished {
            Some(_) => Lap::Done,
            None => Lap::Lap(lap_of(kart, race.laps), race.laps),
        },
        _ => Lap::Spectator,
    };
    let rank = standings
        .rows
        .iter()
        .position(|row| row.player() == Some(viewer))
        .map(|index| (index + 1, standings.rows.len()));
    View {
        layout,
        seed: map.seed,
        round: race.round,
        lap,
        rank,
        seconds: match race.phase {
            Phase::Racing => race.elapsed() / 20,
            _ => 0,
        },
        stage: match race.phase {
            Phase::Generating => Some((Stage::Construction, race.construction())),
            Phase::Loading => Some((Stage::Loading, 100)),
            Phase::Destroying => Some((Stage::Demolition, race.construction())),
            _ => None,
        },
        best_lap: racer.and_then(|racer| racer.best_lap),
        record: match (race.record, race.results.first()) {
            (Some(record), Some((_, time))) => Some(record.min(*time)),
            (record, best) => record.or(best.map(|(_, time)| *time)),
        },
        standings: standings.version,
    }
}

pub fn sync(
    race: Res<Race>,
    map: Res<Track>,
    standings: Res<Standings>,
    racers: Query<&Racer>,
    karts: Query<(&Pilot, &Kart)>,
    names: Query<&PlayerName>,
    mut boards: Query<(Entity, &mut Sidebar, &mut Board)>,
) {
    for (viewer, mut sidebar, mut board) in &mut boards {
        let next = view(&race, &map, &standings, viewer, &racers, &karts);
        if board.shown == Some(next) {
            continue;
        }
        let board = &mut *board;
        let previous = match board.shown {
            Some(previous) if previous.layout == next.layout => Some(previous),
            _ => {
                build(&mut sidebar, &mut board.handles, next.layout);
                None
            }
        };
        let lines = Lines {
            race: &race,
            standings: &standings,
            names: &names,
            viewer,
        };
        fill(
            &mut sidebar,
            &mut board.handles,
            &next,
            previous.as_ref(),
            &lines,
        );
        board.shown = Some(next);
    }
}

fn build(sidebar: &mut Sidebar, handles: &mut Handles, layout: Layout) {
    sidebar.clear();
    let mut next = Handles {
        circuit: Some(sidebar.push(Widget::labeled("Circuit", "").color(TextColor::Aqua))),
        ..Default::default()
    };
    match layout {
        Layout::Racing => {
            next.lap = Some(sidebar.push(Widget::labeled("Tour", "")));
            next.rank = Some(sidebar.push(Widget::labeled("Position", "")));
            next.timer = Some(sidebar.push(Widget::timer_ticks(0).label("Temps")));
            next.anchor = Some(sidebar.push(Widget::blank()));
            sidebar.push(Widget::blank());
            next.best_lap = Some(sidebar.push(Widget::labeled("Meilleur tour", NO_TIME)));
        }
        Layout::Building => {
            next.round = Some(sidebar.push(Widget::labeled("Manche", "")));
            sidebar.push(Widget::blank());
            next.stage = Some(sidebar.push(Widget::progress(0, 100).width(PROGRESS_WIDTH)));
            next.percent = Some(sidebar.push(Widget::labeled("Avancement", "")));
            sidebar.push(Widget::blank());
        }
        Layout::Idle => {
            next.round = Some(sidebar.push(Widget::labeled("Manche", "")));
            next.anchor = Some(sidebar.push(Widget::blank()));
            sidebar.push(Widget::blank());
        }
    }
    next.record =
        Some(sidebar.push(Widget::labeled("Record", NO_TIME).color(TextColor::LightPurple)));
    *handles = next;
}

struct Lines<'a, 'w, 's, 'n> {
    race: &'a Race,
    standings: &'a Standings,
    names: &'a Query<'w, 's, &'n PlayerName>,
    viewer: Entity,
}

impl Lines<'_, '_, '_, '_> {
    fn name(&self, player: Entity) -> &str {
        self.names
            .get(player)
            .map_or("Pilote", |name| name.0.as_str())
    }

    fn rows(&self, layout: Layout) -> Vec<(String, TextColor)> {
        let me = self.name(self.viewer);
        let lines: Vec<(String, TextColor)> = self
            .standings
            .rows
            .iter()
            .take(RANK_LINES)
            .enumerate()
            .filter_map(|(index, row)| {
                let rank = index + 1;
                Some(match *row {
                    Row::Finished(player, time) => (
                        format!("{rank}. {} {}", self.name(player), clock_tenths(time)),
                        if player == self.viewer {
                            TextColor::Yellow
                        } else {
                            TextColor::Green
                        },
                    ),
                    Row::Racing(player, lap) => (
                        format!("{rank}. {} T{lap}", self.name(player)),
                        if player == self.viewer {
                            TextColor::Yellow
                        } else {
                            TextColor::White
                        },
                    ),
                    Row::Result(_, index) => {
                        let (name, time) = self.race.results.get(index)?;
                        (
                            format!("{rank}. {name} {}", clock_tenths(*time)),
                            match rank {
                                _ if name == me => TextColor::Yellow,
                                1 => TextColor::Gold,
                                2 => TextColor::Gray,
                                _ => TextColor::White,
                            },
                        )
                    }
                })
            })
            .collect();
        if lines.is_empty() {
            let placeholder = match layout {
                Layout::Racing => "Aucun pilote en course",
                Layout::Idle | Layout::Building => "Aucun resultat : /race",
            };
            return vec![(placeholder.into(), TextColor::DarkGray)];
        }
        lines
    }
}

fn fill(
    sidebar: &mut Mut<Sidebar>,
    handles: &mut Handles,
    next: &View,
    previous: Option<&View>,
    lines: &Lines,
) {
    macro_rules! changed {
        ($field:ident) => {
            previous.is_none_or(|old| old.$field != next.$field)
        };
    }
    if let Some(id) = handles.circuit
        && changed!(seed)
    {
        sidebar.set_value(id, format!("#{}", next.seed));
    }
    if let Some(id) = handles.round
        && changed!(round)
    {
        sidebar.set_value(id, next.round.to_string());
    }
    if let Some(id) = handles.lap
        && changed!(lap)
    {
        sidebar.set_value(
            id,
            match next.lap {
                Lap::Spectator => "Spectateur".into(),
                Lap::Lap(lap, laps) => format!("{lap}/{laps}"),
                Lap::Done => "Arrivee".into(),
            },
        );
    }
    if let Some(id) = handles.rank
        && changed!(rank)
    {
        sidebar.set_value(
            id,
            next.rank
                .map_or_else(|| "-".into(), |(rank, total)| format!("{rank}/{total}")),
        );
    }
    if let Some(id) = handles.timer
        && changed!(seconds)
        && let Some(timer) = sidebar.timer_mut(id)
    {
        timer.set_ticks(next.seconds * 20);
    }
    if let Some((stage, percent)) = next.stage
        && changed!(stage)
    {
        if let Some(id) = handles.stage
            && let Some(progress) = sidebar.progress_mut(id)
        {
            progress.set(percent as u32);
            progress.label = Some(stage.label().into());
        }
        if let Some(id) = handles.percent {
            sidebar.set_value(id, format!("{percent}%"));
        }
    }
    if let Some(id) = handles.best_lap
        && changed!(best_lap)
    {
        sidebar.set_value(id, time_or_dash(next.best_lap));
    }
    if let Some(id) = handles.record
        && changed!(record)
    {
        sidebar.set_value(id, time_or_dash(next.record));
    }
    if let Some(anchor) = handles.anchor
        && changed!(standings)
    {
        let rows = lines.rows(next.layout);
        if handles.rows.len() > rows.len() {
            for extra in handles.rows.drain(rows.len()..) {
                sidebar.remove(extra);
            }
        }
        let mut last = handles.rows.last().copied().unwrap_or(anchor);
        for (index, (text, color)) in rows.into_iter().enumerate() {
            let widget = Widget::from(Widget::text(text).color(color));
            match handles.rows.get(index) {
                Some(id) => {
                    if sidebar.get(*id) != Some(&widget) {
                        sidebar.set(*id, widget);
                    }
                }
                None => {
                    if let Some(id) = sidebar.insert_after(last, widget) {
                        handles.rows.push(id);
                        last = id;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use bevy_ecs::change_detection::Tick;

    use super::*;
    use crate::race::tests::{Harness, Out};
    use crate::race::{COUNTDOWN, LAPS};

    fn plain(line: &str) -> String {
        let mut out = String::new();
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            if c == '§' {
                chars.next();
            } else {
                out.push(c);
            }
        }
        out
    }

    fn rendered(h: &Harness, player: Entity) -> Vec<String> {
        h.app
            .world()
            .get::<Sidebar>(player)
            .unwrap()
            .render()
            .iter()
            .map(|line| plain(line))
            .collect()
    }

    fn layout(h: &Harness, player: Entity) -> Option<Layout> {
        h.app.world().get::<Board>(player).unwrap().layout()
    }

    fn lines(out: &[Out], client: u32) -> Vec<(String, String)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Line {
                    client: c,
                    owner,
                    text,
                } if *c == client => Some((owner.clone(), plain(text))),
                _ => None,
            })
            .collect()
    }

    fn board_packets(out: &[Out], client: u32) -> usize {
        out.iter()
            .filter(|o| match o {
                Out::Line { client: c, .. }
                | Out::Objective { client: c, .. }
                | Out::Display(c, _)
                | Out::ResetLine(c, _) => *c == client,
                _ => false,
            })
            .count()
    }

    fn objectives(out: &[Out], client: u32) -> Vec<(String, Option<String>)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Objective {
                    client: c,
                    action,
                    title,
                } if *c == client => Some((action.clone(), title.as_deref().map(plain))),
                _ => None,
            })
            .collect()
    }

    fn tabs(out: &[Out], client: u32) -> Vec<(String, String)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Tab {
                    client: c,
                    header,
                    footer,
                } if *c == client => Some((header.clone(), footer.clone())),
                _ => None,
            })
            .collect()
    }

    fn boards(h: &mut Harness) -> usize {
        h.world().query::<&Sidebar>().iter(h.app.world()).count()
    }

    fn advance(h: &mut Harness, player: Entity, gate: usize) {
        let map = h.app.world().resource::<Track>().clone();
        let (x, y, z) = map.point((gate - 1) as f64 * TAU / GATES as f64 + 0.02, 0.0);
        let mut kart = h.kart_mut(player);
        kart.next_gate = gate;
        (kart.x, kart.y, kart.z) = (x, y, z);
    }

    #[test]
    fn tenths_clock_formats_minutes_seconds_and_tenths() {
        assert_eq!(clock_tenths(0), "0:00.0");
        assert_eq!(clock_tenths(19), "0:00.9");
        assert_eq!(clock_tenths(20), "0:01.0");
        assert_eq!(clock_tenths(1200 + 20 * 5 + 7), "1:05.3");
        assert_eq!(clock_tenths(1200 * 61), "61:00.0");
        assert_eq!(footer(7), "Circuit #7 · /race [tours] [seed]");
    }

    #[test]
    fn a_board_and_the_tab_list_reach_the_pilot_on_the_first_tick_after_ready() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        assert!(h.app.world().get::<Sidebar>(a).is_some());
        assert_eq!(layout(&h, a), None);
        h.tick();
        let out = h.drain();
        assert_eq!(layout(&h, a), Some(Layout::Idle));
        assert_eq!(
            objectives(&out, 1),
            vec![("create".into(), Some("✦ Alpine Rush ✦".into()))]
        );
        assert!(
            out.iter()
                .any(|o| matches!(o, Out::Display(1, name) if name == "sidebar"))
        );
        let seed = h.app.world().resource::<Track>().seed;
        assert_eq!(
            rendered(&h, a),
            vec![
                format!("Circuit: #{seed}"),
                "Manche: 0".to_string(),
                String::new(),
                "Aucun resultat : /race".to_string(),
                String::new(),
                "Record: --:--.-".to_string(),
            ]
        );
        assert_eq!(
            lines(&out, 1)
                .into_iter()
                .map(|(_, text)| text)
                .collect::<Vec<_>>(),
            rendered(&h, a)
        );
        assert_eq!(
            tabs(&out, 1),
            vec![("Alpine Rush".to_string(), footer(seed))]
        );
        h.ticks(40);
        let out = h.drain();
        assert_eq!(board_packets(&out, 1), 0, "{out:?}");
        assert!(tabs(&out, 1).is_empty());
    }

    #[test]
    fn phases_switch_the_layout_and_the_boss_bar_no_longer_reports_construction() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.tick();
        h.drain();
        h.command(a, "race", &["1", "7"]);
        h.tick();
        assert_eq!(layout(&h, a), Some(Layout::Building));
        let lines_a = rendered(&h, a);
        assert_eq!(lines_a[0], "Circuit: #7");
        assert_eq!(lines_a[1], "Manche: 1");
        assert!(lines_a[3].starts_with("Construction "), "{lines_a:?}");
        assert!(lines_a[4].starts_with("Avancement: "), "{lines_a:?}");
        assert!(!h.drain().iter().any(|o| matches!(o, Out::Bar { .. })));
        let shown_percent = |h: &Harness| rendered(h, a)[4].clone();
        let before = shown_percent(&h);
        while shown_percent(&h) == before {
            h.tick();
        }
        assert!(h.race().construction() > 0);
        assert_eq!(
            shown_percent(&h),
            format!("Avancement: {}%", h.race().construction())
        );
        h.race_mut().pending.clear();
        h.tick();
        assert_eq!(h.race().phase, Phase::Loading);
        assert_eq!(
            rendered(&h, a)[3..5],
            ["Chargement ▮▮▮▮▮▮▮▮▮▮", "Avancement: 100%"]
        );
        h.settle_transfers();
        assert_eq!(h.race().phase, Phase::Countdown);
        assert_eq!(layout(&h, a), Some(Layout::Racing));
        assert_eq!(
            rendered(&h, a),
            vec![
                "Circuit: #7",
                "Tour: 1/1",
                "Position: 1/2",
                "Temps: 00:00",
                "",
                "1. Pilot1 T1",
                "2. Pilot2 T1",
                "",
                "Meilleur tour: --:--.-",
                "Record: --:--.-",
            ]
        );
        assert_eq!(rendered(&h, b)[2], "Position: 2/2");
        h.race_mut().start = h.race().tick;
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        h.drain();
        h.kart_mut(b).next_gate = GATES + 1;
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        let time = h.kart(b).finished.unwrap();
        assert_eq!(
            rendered(&h, a)[1..3],
            ["Tour: 1/1".to_string(), "Position: 2/2".to_string()]
        );
        assert_eq!(
            rendered(&h, a)[5..7],
            [
                format!("1. Pilot2 {}", clock_tenths(time)),
                "2. Pilot1 T1".to_string()
            ]
        );
        assert_eq!(rendered(&h, b)[1], "Tour: Arrivee");
        h.kart_mut(a).next_gate = GATES + 1;
        h.tick();
        assert_eq!(h.race().phase, Phase::Destroying);
        assert_eq!(layout(&h, a), Some(Layout::Building));
        assert!(rendered(&h, a)[3].starts_with("Demontage "));
        h.race_mut().pending.clear();
        h.tick();
        assert_eq!(h.race().phase, Phase::Results);
        assert_eq!(layout(&h, a), Some(Layout::Idle));
        let results = h.race().results.clone();
        assert_eq!(
            rendered(&h, a),
            vec![
                "Circuit: #7".to_string(),
                "Manche: 1".to_string(),
                String::new(),
                format!("1. Pilot2 {}", clock_tenths(results[0].1)),
                format!("2. Pilot1 {}", clock_tenths(results[1].1)),
                String::new(),
                format!("Record: {}", clock_tenths(results[0].1)),
            ]
        );
        let a_line = h.app.world().get::<Sidebar>(a).unwrap().render()[4].clone();
        assert!(
            a_line.starts_with("§e"),
            "the viewer is highlighted: {a_line}"
        );
        let b_line = h.app.world().get::<Sidebar>(b).unwrap().render()[3].clone();
        assert!(b_line.starts_with("§e"), "{b_line}");
        h.drain();
        h.ticks(40);
        assert_eq!(board_packets(&h.drain(), 1), 0);
    }

    #[test]
    fn the_timer_costs_one_line_per_second_and_nothing_between() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.tick();
        h.shortcut_to_racing(a, &[]);
        h.ticks(18);
        let out = h.drain();
        assert_eq!(board_packets(&out, 1), 0, "{out:?}");
        h.tick();
        assert_eq!(h.race().elapsed(), 20);
        assert_eq!(
            lines(&h.drain(), 1),
            vec![("line03".to_string(), "Temps: 00:01".to_string())]
        );
        h.ticks(19);
        assert_eq!(board_packets(&h.drain(), 1), 0);
        h.tick();
        assert_eq!(
            lines(&h.drain(), 1),
            vec![("line03".to_string(), "Temps: 00:02".to_string())]
        );
    }

    #[test]
    fn the_ranking_is_rewritten_only_when_the_order_changes() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.tick();
        h.shortcut_to_racing(a, &[]);
        let version = h.app.world().resource::<Standings>().version;
        assert_eq!(rendered(&h, a)[5..7], ["1. Pilot1 T1", "2. Pilot2 T1"]);
        h.kart_mut(a).speed = 0.3;
        h.ticks(5);
        assert!(
            h.kart(a).progress(h.app.world().resource::<Track>())
                > h.kart(b).progress(h.app.world().resource::<Track>())
        );
        assert_eq!(h.app.world().resource::<Standings>().version, version);
        assert_eq!(board_packets(&h.drain(), 1), 0);
        h.kart_mut(a).speed = 0.0;
        advance(&mut h, b, 3);
        h.tick();
        assert_eq!(h.app.world().resource::<Standings>().version, version + 1);
        let out = h.drain();
        assert_eq!(
            lines(&out, 1),
            vec![
                ("line02".to_string(), "Position: 2/2".to_string()),
                ("line05".to_string(), "1. Pilot2 T1".to_string()),
                ("line06".to_string(), "2. Pilot1 T1".to_string()),
            ]
        );
        assert_eq!(
            lines(&out, 2),
            vec![
                ("line02".to_string(), "Position: 1/2".to_string()),
                ("line05".to_string(), "1. Pilot2 T1".to_string()),
                ("line06".to_string(), "2. Pilot1 T1".to_string()),
            ]
        );
        h.ticks(10);
        assert_eq!(board_packets(&h.drain(), 1), 0);
        advance(&mut h, b, 4);
        h.tick();
        assert_eq!(h.app.world().resource::<Standings>().version, version + 1);
        assert_eq!(board_packets(&h.drain(), 1), 0);
    }

    fn place(h: &mut Harness, player: Entity, lap: f64) {
        let map = h.app.world().resource::<Track>().clone();
        let (x, y, z) = map.point(lap * TAU, 0.0);
        let mut kart = h.kart_mut(player);
        (kart.x, kart.y, kart.z) = (x, y, z);
    }

    fn progress(h: &Harness, player: Entity) -> f64 {
        h.kart(player).progress(h.app.world().resource::<Track>())
    }

    fn rows(h: &Harness) -> Vec<Row> {
        h.app.world().resource::<Standings>().rows.clone()
    }

    fn version(h: &Harness) -> u64 {
        h.app.world().resource::<Standings>().version
    }

    fn board_changed_since(h: &mut Harness, player: Entity, since: Tick) -> bool {
        let now = h.world().change_tick();
        h.world()
            .query::<Ref<Sidebar>>()
            .get(h.app.world(), player)
            .unwrap()
            .last_changed()
            .is_newer_than(since, now)
    }

    #[test]
    fn two_karts_within_the_hysteresis_band_keep_their_previous_order() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.tick();
        h.shortcut_to_racing(a, &[]);
        let start = version(&h);
        assert_eq!(rows(&h), vec![Row::Racing(a, 1), Row::Racing(b, 1)]);
        place(&mut h, a, 0.1);
        place(&mut h, b, 0.1 + RANK_HYSTERESIS / 2.0);
        h.tick();
        let gap = progress(&h, b) - progress(&h, a);
        assert!(gap > 0.0 && gap < RANK_HYSTERESIS, "{gap}");
        assert_eq!(rows(&h), vec![Row::Racing(a, 1), Row::Racing(b, 1)]);
        assert_eq!(version(&h), start);
        assert_eq!(board_packets(&h.drain(), 1), 0);
        place(&mut h, b, 0.1 + RANK_HYSTERESIS * 2.0);
        h.tick();
        assert!(progress(&h, b) - progress(&h, a) > RANK_HYSTERESIS);
        assert_eq!(rows(&h), vec![Row::Racing(b, 1), Row::Racing(a, 1)]);
        assert_eq!(version(&h), start + 1);
        assert_eq!(
            lines(&h.drain(), 1),
            vec![
                ("line02".to_string(), "Position: 2/2".to_string()),
                ("line05".to_string(), "1. Pilot2 T1".to_string()),
                ("line06".to_string(), "2. Pilot1 T1".to_string()),
            ]
        );
        place(&mut h, b, 0.1 - RANK_HYSTERESIS / 2.0);
        h.tick();
        let gap = progress(&h, a) - progress(&h, b);
        assert!(gap > 0.0 && gap < RANK_HYSTERESIS, "{gap}");
        assert_eq!(rows(&h), vec![Row::Racing(b, 1), Row::Racing(a, 1)]);
        assert_eq!(version(&h), start + 1);
        assert_eq!(board_packets(&h.drain(), 1), 0);
        place(&mut h, b, 0.1 - RANK_HYSTERESIS * 2.0);
        h.tick();
        assert_eq!(rows(&h), vec![Row::Racing(a, 1), Row::Racing(b, 1)]);
        assert_eq!(version(&h), start + 2);
        assert_eq!(board_packets(&h.drain(), 1), 3);
    }

    #[test]
    fn a_finished_kart_leaves_the_hysteresis_band() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.tick();
        h.shortcut_to_racing(a, &[]);
        let start = version(&h);
        place(&mut h, a, 0.1);
        place(&mut h, b, 0.1 + RANK_HYSTERESIS / 2.0);
        h.tick();
        assert_eq!(rows(&h), vec![Row::Racing(a, 1), Row::Racing(b, 1)]);
        h.kart_mut(b).finished = Some(7);
        h.tick();
        assert_eq!(rows(&h), vec![Row::Finished(b, 7), Row::Racing(a, 1)]);
        assert_eq!(version(&h), start + 1);
    }

    #[test]
    fn the_countdown_grid_is_ranked_by_roster_slot() {
        let mut h = Harness::new(42);
        let pilots: Vec<Entity> = (1..=4).map(|id| h.connect(id)).collect();
        h.tick();
        h.shortcut_to_countdown(pilots[0], &[]);
        h.tick();
        let ordered: Vec<Row> = pilots.iter().map(|p| Row::Racing(*p, 1)).collect();
        assert_eq!(rows(&h), ordered);
        for (slot, player) in pilots.iter().enumerate() {
            let map = h.app.world().resource::<Track>().clone();
            h.kart_mut(*player).grid(&map, 3 - slot);
        }
        h.tick();
        assert_eq!(rows(&h), ordered);
        let tick = h.race().tick;
        h.race_mut().start = tick;
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        assert_eq!(rows(&h), ordered);
    }

    #[test]
    fn a_swap_below_the_visible_rows_leaves_the_boards_untouched() {
        let mut h = Harness::new(42);
        let pilots: Vec<Entity> = (1..=7).map(|id| h.connect(id)).collect();
        h.tick();
        h.shortcut_to_racing(pilots[0], &[]);
        for (index, player) in pilots.iter().enumerate() {
            place(&mut h, *player, 0.5 - index as f64 * 0.01);
        }
        h.tick();
        h.drain();
        let start = version(&h);
        let since = h.world().change_tick();
        place(&mut h, pilots[6], 0.5 - 0.05 + RANK_HYSTERESIS * 2.0);
        h.tick();
        assert_eq!(version(&h), start + 1);
        assert_eq!(rows(&h)[5], Row::Racing(pilots[6], 1));
        assert!(!board_changed_since(&mut h, pilots[0], since));
        assert!(board_changed_since(&mut h, pilots[5], since));
        assert_eq!(board_packets(&h.drain(), 1), 0);
    }

    #[test]
    fn a_completed_lap_sets_the_best_lap_and_the_lap_counter() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.tick();
        h.shortcut_to_racing(a, &["3"]);
        h.ticks(45);
        h.drain();
        h.kart_mut(a).next_gate = GATES + 1;
        h.tick();
        let racer = *h.app.world().get::<Racer>(a).unwrap();
        assert_eq!(racer.best_lap, Some(47));
        assert_eq!(racer.lap_start, h.race().tick);
        assert_eq!(
            lines(&h.drain(), 1),
            vec![
                ("line01".to_string(), "Tour: 2/3".to_string()),
                ("line05".to_string(), "1. Pilot1 T2".to_string()),
                ("line07".to_string(), "Meilleur tour: 0:02.3".to_string()),
            ]
        );
        h.ticks(100);
        h.drain();
        h.kart_mut(a).next_gate = 2 * GATES + 1;
        h.tick();
        assert_eq!(h.app.world().get::<Racer>(a).unwrap().best_lap, Some(47));
        assert!(
            !lines(&h.drain(), 1)
                .iter()
                .any(|(owner, _)| owner == "line07")
        );
        h.ticks(20);
        h.drain();
        h.kart_mut(a).next_gate = 3 * GATES + 1;
        h.tick();
        assert_eq!(h.app.world().get::<Racer>(a).unwrap().best_lap, Some(21));
        assert!(h.kart(a).finished.is_some());
        assert_eq!(h.race().phase, Phase::Destroying);
        assert_eq!(layout(&h, a), Some(Layout::Building));
    }

    #[test]
    fn a_new_round_forgets_the_best_lap_and_moves_the_footer_to_the_new_seed() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.tick();
        h.shortcut_to_racing(a, &["1", "7"]);
        h.ticks(30);
        h.kart_mut(a).next_gate = GATES + 1;
        h.tick();
        assert_eq!(h.app.world().get::<Racer>(a).unwrap().best_lap, Some(32));
        while h.race().phase != Phase::Results {
            h.tick();
        }
        h.drain();
        h.shortcut_to_countdown(a, &["1", "9"]);
        assert_eq!(h.app.world().get::<Racer>(a).unwrap().best_lap, None);
        assert_eq!(rendered(&h, a)[0], "Circuit: #9");
        assert_eq!(rendered(&h, a)[7], "Meilleur tour: --:--.-");
        let footer_shown = h
            .world()
            .query_filtered::<&TabList, With<RaceTab>>()
            .single(h.app.world())
            .unwrap()
            .footer
            .clone();
        assert_eq!(footer_shown, footer(9));
    }

    #[test]
    fn spectators_keep_a_board_and_quitting_removes_it() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.tick();
        h.command(b, "leave", &[]);
        h.tick();
        h.drain();
        h.shortcut_to_racing(a, &[]);
        assert_eq!(layout(&h, b), Some(Layout::Racing));
        assert_eq!(rendered(&h, b)[1..3], ["Tour: Spectateur", "Position: -"]);
        assert_eq!(rendered(&h, b)[5], "1. Pilot1 T1");
        h.disconnect(b);
        h.tick();
        let out = h.drain();
        assert_eq!(objectives(&out, 2), vec![("remove".into(), None)]);
        assert_eq!(boards(&mut h), 1);
        assert_eq!(rendered(&h, a)[5..6], ["1. Pilot1 T1"]);
        h.disconnect(a);
        h.tick();
        assert_eq!(boards(&mut h), 0);
    }

    #[test]
    fn countdown_boss_bar_still_counts_while_the_sidebar_holds_the_state() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.tick();
        h.shortcut_to_countdown(a, &[]);
        h.drain();
        h.ticks(COUNTDOWN as usize);
        let out = h.drain();
        assert!(
            out.iter()
                .any(|o| matches!(o, Out::Bar { title: Some(t), .. } if t == "Depart dans 1..."))
        );
        assert_eq!(h.race().phase, Phase::Racing);
        assert_eq!(h.race().laps, LAPS);
        assert_eq!(rendered(&h, a)[3], "Temps: 00:00");
    }
}
