use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{
    ArgParser, Audience, BossBar, BossBarColor, ChunkPos, Command, CommandBuilder, CommandRegistry,
    CommandSystems, IntegerArg, VoidSystems,
    events::{PlayerQuitEvent, PlayerReadyEvent},
    plugins::boss_bar::BossBarState,
};
use voidmc_protocol::clientbound::commands::{Parser, StringType};

use crate::arena::Arena;
use crate::audio::{Audio, Cue};
use crate::chat::{Chat, Flash, Tone};
use crate::displays;
use crate::items::{self, Items};
use crate::kart::{Kart, PowerUp};
use crate::sidebar;
use crate::terrain::mix;
use crate::track::{GATES, Track};
use crate::travel::{self, Transfer, Travel};
use crate::vehicle::{self, Karts, Pilot};

pub const LAPS: usize = 3;
pub const MAX_LAPS: usize = 20;
pub const COUNTDOWN: u64 = 100;
pub const TIME_LIMIT: u64 = 20 * 60 * 10;
pub const GRID: usize = 8;
const BUILD_PERIOD: u64 = 2;
const HUD_PERIOD: u64 = 5;
const TIME_WARNINGS: [u64; 3] = [1200, 600, 200];
const ROUND_STRIDE: u64 = 0x9e3779b97f4a7c15;
const BOOST_TITLE: &str = "Boost — Saut pour accelerer";

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Lobby,
    Generating,
    Loading,
    Countdown,
    Racing,
    Destroying,
    Results,
}

impl Phase {
    pub fn idle(self) -> bool {
        matches!(self, Phase::Lobby | Phase::Results)
    }
}

#[derive(Resource)]
pub struct Race {
    pub tick: u64,
    pub start: u64,
    pub phase: Phase,
    pub laps: usize,
    pub round: u64,
    pub results: Vec<(String, u64)>,
    pub best: HashMap<(u64, usize, String), u64>,
    pub record: Option<u64>,
    pub roster: Vec<Entity>,
    pub pending: Vec<ChunkPos>,
    pub total_chunks: usize,
}

impl Default for Race {
    fn default() -> Self {
        Self {
            tick: 0,
            start: 0,
            phase: Phase::Lobby,
            laps: LAPS,
            round: 0,
            results: Vec::new(),
            best: HashMap::new(),
            record: None,
            roster: Vec::new(),
            pending: Vec::new(),
            total_chunks: 0,
        }
    }
}

impl Race {
    pub fn time_limit(&self) -> u64 {
        TIME_LIMIT.max(TIME_LIMIT / LAPS as u64 * self.laps as u64)
    }

    pub fn elapsed(&self) -> u64 {
        self.tick.saturating_sub(self.start)
    }

    pub fn countdown(&self) -> u64 {
        self.start.saturating_sub(self.tick)
    }

    pub fn construction(&self) -> usize {
        100 * self.total_chunks.saturating_sub(self.pending.len()) / self.total_chunks.max(1)
    }

    fn schedule(&mut self, phase: Phase, pending: Vec<ChunkPos>) {
        self.phase = phase;
        self.total_chunks = pending.len();
        self.pending = pending;
    }

    fn status(&self, kart: &Kart, transferring: bool) -> String {
        match self.phase {
            Phase::Generating => format!("Construction du circuit : {}%", self.construction()),
            Phase::Destroying => format!("Destruction du circuit : {}%", self.construction()),
            Phase::Lobby | Phase::Results => "Vol libre | /race pour lancer une course".into(),
            Phase::Loading => "Chargement des chunks / synchronisation des pilotes...".into(),
            _ if transferring => "Chargement des chunks / synchronisation des pilotes...".into(),
            _ if !kart.participant => "Prochaine course : attends l'arrivee des pilotes".into(),
            Phase::Countdown => format!("DEPART DANS {}...", self.countdown().div_ceil(20)),
            Phase::Racing => match kart.finished {
                Some(time) => format!("ARRIVEE — {}s | /scores", seconds(time)),
                None => format!(
                    "Tour {}/{} | CP {}/{} | {:.1}s (+{}s) | {} km/h | Bonus : {} [Sprint]",
                    ((kart.next_gate - 1) / GATES + 1).min(self.laps),
                    self.laps,
                    (kart.next_gate - 1) % GATES,
                    GATES,
                    self.elapsed() as f64 / 20.0,
                    kart.penalty / 20,
                    (kart.speed * 72.0) as i32,
                    kart.item.map_or("—", PowerUp::name)
                ),
            },
        }
    }
}

fn seconds(ticks: u64) -> String {
    format!("{:.2}", ticks as f64 / 20.0)
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Racer {
    pub gate: usize,
    pub kart: Entity,
    pub lap_start: u64,
    pub best_lap: Option<u64>,
}

impl Racer {
    pub fn new(kart: Entity) -> Self {
        Self {
            gate: 1,
            kart,
            lap_start: 0,
            best_lap: None,
        }
    }

    fn lap_done(&mut self, race: &Race) -> u64 {
        let time = race.tick - self.lap_start.max(race.start);
        self.lap_start = race.tick;
        self.best_lap = Some(self.best_lap.map_or(time, |best| best.min(time)));
        time
    }
}

#[derive(Component)]
pub struct RaceBar;

#[derive(Event)]
pub struct Join(pub Entity);

#[derive(Event)]
pub struct Leave(pub Entity);

#[derive(Event)]
pub struct Rescue(pub Entity);

#[derive(Event)]
pub struct Launch {
    pub player: Entity,
    pub laps: usize,
    pub seed: Option<u64>,
}

#[derive(Event)]
pub struct Scores(pub Entity);

#[derive(SystemParam)]
struct Grid<'w, 's> {
    race: ResMut<'w, Race>,
    chat: Chat<'w, 's>,
    racers: Query<'w, 's, &'static Racer>,
    commands: Commands<'w, 's>,
}

impl Grid<'_, '_> {
    fn join(&mut self, player: Entity) {
        if self.race.roster.contains(&player) {
            return;
        }
        let slot = self.race.roster.len();
        if slot >= GRID {
            self.chat.say(
                player,
                Tone::Warn,
                format!("La grille est pleine ({GRID} pilotes)."),
            );
            return;
        }
        if self.commands.get_entity(player).is_err() {
            return;
        }
        let mut kart = Kart {
            fuel: 100.0,
            next_gate: 1,
            ..Default::default()
        };
        kart.wait(slot);
        self.race.roster.push(player);
        let kart = vehicle::spawn(&mut self.commands, player, kart);
        self.commands.entity(player).insert((
            Racer::new(kart),
            Flash::default(),
            BossBar::new(BOOST_TITLE)
                .color(BossBarColor::Blue)
                .viewers([player]),
        ));
        let name = self.chat.name(player);
        self.chat.say_all(
            Tone::Info,
            format!("{name} prend un minecart ({}/{GRID} pilotes).", slot + 1),
        );
        self.chat.say(
            player,
            Tone::Notice,
            match self.race.phase {
                Phase::Generating => {
                    "Le circuit se construit : tu participeras a la prochaine manche."
                }
                Phase::Loading | Phase::Countdown | Phase::Racing => {
                    "Manche en cours : observe en vol, tu participeras a la suivante."
                }
                Phase::Destroying => "Demontage en cours : /race sera disponible juste apres.",
                Phase::Lobby | Phase::Results => {
                    "En vol au-dessus de la vallee. /race lance un circuit avec tous les pilotes presents."
                }
            },
        );
    }

    fn leave(&mut self, player: Entity) -> bool {
        let Some(index) = self.race.roster.iter().position(|e| *e == player) else {
            return false;
        };
        self.race.roster.remove(index);
        if let Ok(racer) = self.racers.get(player) {
            self.commands.entity(racer.kart).despawn();
        }
        self.commands
            .entity(player)
            .remove::<(Racer, Flash, BossBar, BossBarState)>();
        true
    }
}

pub struct RacePlugin(pub Arena);

impl Plugin for RacePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.track())
            .insert_resource(self.0.clone())
            .init_resource::<Race>()
            .init_resource::<Items>()
            .init_resource::<displays::Scene>()
            .init_resource::<sidebar::Standings>()
            .add_observer(ready)
            .add_observer(quit)
            .add_observer(join)
            .add_observer(leave)
            .add_observer(rescue)
            .add_observer(launch)
            .add_observer(scores)
            .add_observer(vehicle::input)
            .add_observer(travel::arrived)
            .add_observer(travel::keep_flying)
            .add_systems(
                Update,
                (
                    clock,
                    construct,
                    load,
                    countdown,
                    items::update,
                    items::fly,
                    items::crystals,
                    vehicle::drive,
                    vehicle::bumps,
                    racing,
                    sidebar::standings,
                    sidebar::sync,
                    sidebar::tab_list,
                    hud,
                    race_bar,
                    boost_bar,
                    vehicle::pose,
                    items::effects,
                    displays::sync,
                )
                    .chain()
                    .after(CommandSystems::DrainQueue)
                    .after(VoidSystems::TeleportBarrier),
            );
        app.world_mut().spawn((
            RaceBar,
            BossBar::new("")
                .color(BossBarColor::White)
                .audience(Audience::explicit([])),
        ));
        let seed = self.0.track().seed;
        app.world_mut().spawn(sidebar::tab(seed));
        let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
        for command in race_commands() {
            registry.register(command);
        }
    }
}

pub fn race_commands() -> Vec<Command> {
    vec![
        CommandBuilder::new("race")
            .description("Start a race: /race [tours] [seed], 1–20 laps (default 3)")
            .arg_optional("tours", IntegerArg::new(1, MAX_LAPS as i32))
            .arg_optional("seed", Arc::new(SeedArg))
            .handler(|ctx| {
                let player = ctx.entity;
                let laps = ctx.get::<i32>("tours").copied().unwrap_or(LAPS as i32) as usize;
                let seed = ctx.get::<u64>("seed").copied();
                ctx.with_world_mut(|world| world.trigger(Launch { player, laps, seed }));
            })
            .build(),
        request("join", "Join Alpine Rush / mount your minecart", Join),
        request("leave", "Leave your minecart and watch the race", Leave),
        request(
            "reset",
            "Return to your last checkpoint (+3 seconds)",
            Rescue,
        ),
        request("scores", "Show race results and session records", Scores),
    ]
}

struct SeedArg;

impl ArgParser for SeedArg {
    fn type_name(&self) -> &str {
        "seed"
    }

    fn parse(&self, input: &str) -> Result<Box<dyn Any + Send + Sync>, String> {
        input
            .parse::<u64>()
            .map(|seed| Box::new(seed) as Box<dyn Any + Send + Sync>)
            .map_err(|_| format!("'{input}' is not a circuit seed (unsigned 64-bit integer)"))
    }

    fn protocol_parser(&self) -> Option<Parser> {
        Some(Parser::String(StringType::SingleWord))
    }
}

fn request<E: Event<Trigger<'static>: Default>>(
    name: &str,
    description: &str,
    event: fn(Entity) -> E,
) -> Command {
    CommandBuilder::new(name)
        .description(description)
        .handler(move |ctx| {
            let player = ctx.entity;
            ctx.with_world_mut(|world| world.trigger(event(player)));
        })
        .build()
}

fn ready(event: On<PlayerReadyEvent>, mut grid: Grid, mut travel: Travel) {
    let player = event.entity;
    travel.fly(player);
    grid.commands.entity(player).insert(sidebar::board(player));
    grid.chat.say_all(
        Tone::Event,
        format!("{} rejoint Alpine Rush !", grid.chat.name(player)),
    );
    grid.join(player);
    grid.chat.say(
        player,
        Tone::Info,
        "/race [tours] [seed] : depart (1–20, defaut 3 ; seed pour rejouer un circuit) | /leave : spectateur | /join : revenir | /reset : secours (+3 s)",
    );
    grid.chat.say(
        player,
        Tone::Info,
        "Avancer : accelerer | Reculer : frein / marche arriere | Saut : boost | Sprint : bonus. F5 conseille.",
    );
}

fn quit(event: On<PlayerQuitEvent>, karts: Karts, mut grid: Grid) {
    let player = event.entity;
    let racing = grid.race.phase == Phase::Racing && karts.get(player).is_some_and(Kart::racing);
    grid.leave(player);
    let name = grid.chat.name(player);
    grid.chat.say_others(
        player,
        Tone::Info,
        format!(
            "{name} quitte Alpine Rush.{}",
            if racing { " Abandon de la manche." } else { "" }
        ),
    );
}

fn join(event: On<Join>, mut grid: Grid) {
    grid.join(event.0);
}

fn leave(event: On<Leave>, mut grid: Grid, mut travel: Travel) {
    let player = event.0;
    if !grid.leave(player) {
        return;
    }
    travel.to_lobby(player);
    let name = grid.chat.name(player);
    grid.chat.say_all(
        Tone::Info,
        format!("{name} quitte la grille et rejoint les spectateurs."),
    );
    grid.chat.say(
        player,
        Tone::Notice,
        "Spectateur en vol. /join pour revenir sur la grille.",
    );
}

fn rescue(event: On<Rescue>, race: Res<Race>, map: Res<Track>, mut karts: Karts, mut chat: Chat) {
    if race.phase != Phase::Racing {
        return;
    }
    let Some(mut kart) = karts.get_mut(event.0) else {
        return;
    };
    if !kart.racing() {
        return;
    }
    kart.reset(&map);
    chat.flash(
        event.0,
        Tone::Warn,
        "Retour au checkpoint : +3 s de penalite, bonus retires",
    );
}

fn launch(
    event: On<Launch>,
    mut race: ResMut<Race>,
    arena: Res<Arena>,
    mut karts: Karts,
    mut travel: Travel,
    chat: Chat,
    mut commands: Commands,
) {
    let (player, laps) = (event.player, event.laps);
    if !(1..=MAX_LAPS).contains(&laps) {
        chat.say(
            player,
            Tone::Warn,
            "Utilisation : /race [tours] [seed], de 1 a 20 tours (defaut 3).",
        );
        return;
    }
    if !race.roster.contains(&player) {
        chat.say(player, Tone::Warn, "Utilise /join avant /race.");
        return;
    }
    if !race.phase.idle() {
        chat.say(player, Tone::Warn, "Une course est deja en cours.");
        return;
    }
    for (slot, pilot) in race.roster.iter().enumerate() {
        if let Some(mut kart) = karts.get_mut(*pilot) {
            kart.wait(slot);
            kart.participant = true;
        }
    }
    travel.everyone_to_lobby();
    let map = Track::new(
        event
            .seed
            .unwrap_or_else(|| circuit_seed(arena.map.seed, race.round)),
    );
    let (seed, length) = (map.seed, map.length);
    arena.prepare(map.clone());
    commands.insert_resource(map);
    race.round += 1;
    race.laps = laps;
    race.record = race
        .best
        .iter()
        .filter(|((s, l, _), _)| *s == seed && *l == laps)
        .map(|(_, best)| *best)
        .min();
    race.results.clear();
    race.schedule(Phase::Generating, arena.chunks());
    let (name, round) = (chat.name(player), race.round);
    tracing::info!(round, laps, seed, replay = event.seed.is_some(), "circuit");
    chat.say_all(
        Tone::Event,
        format!(
            "{name} lance la manche {round} : {laps} tour(s), {length:.0} m par tour (Circuit #{seed})."
        ),
    );
}

pub fn circuit_seed(world_seed: u64, round: u64) -> u64 {
    mix(world_seed.wrapping_add(round.wrapping_mul(ROUND_STRIDE)))
}

fn scores(event: On<Scores>, race: Res<Race>, map: Res<Track>, chat: Chat) {
    let player = event.0;
    if !race.results.is_empty() {
        chat.say(
            player,
            Tone::Event,
            format!("Manche {} — {} tour(s) :", race.round, race.laps),
        );
    }
    for (i, (name, time)) in race.results.iter().enumerate() {
        chat.say(
            player,
            Tone::Info,
            format!("{}. {name} — {}s", i + 1, seconds(*time)),
        );
    }
    let seed = map.seed;
    chat.say(
        player,
        Tone::Record,
        format!(
            "Records du circuit (Circuit #{seed}, {} tour(s)) :",
            race.laps
        ),
    );
    let mut best: Vec<_> = race
        .best
        .iter()
        .filter(|((s, laps, _), _)| *s == seed && *laps == race.laps)
        .map(|((_, _, name), time)| (*time, name))
        .collect();
    best.sort();
    for (time, name) in best.into_iter().take(GRID) {
        chat.say(player, Tone::Info, format!("{name} : {}s", seconds(time)));
    }
}

fn clock(mut race: ResMut<Race>) {
    race.tick += 1;
}

#[derive(SystemParam)]
struct Placement<'w, 's> {
    racers: Query<'w, 's, &'static mut Racer>,
    karts: Query<'w, 's, (&'static Pilot, &'static mut Kart)>,
    travel: Travel<'w, 's>,
}

fn construct(
    mut race: ResMut<Race>,
    mut items: ResMut<Items>,
    arena: Res<Arena>,
    mut placement: Placement,
    chat: Chat,
    mut commands: Commands,
) {
    let build = match race.phase {
        Phase::Generating => true,
        Phase::Destroying => false,
        _ => return,
    };
    if !race.tick.is_multiple_of(BUILD_PERIOD) && !race.pending.is_empty() {
        return;
    }
    if let Some(pos) = race.pending.pop() {
        let arena = arena.clone();
        commands.queue(move |world: &mut World| arena.replace(world, pos, build));
    }
    if !race.pending.is_empty() {
        return;
    }
    if build {
        let track = arena.track();
        let mut slot = 0;
        for pilot in &race.roster {
            let Ok(mut racer) = placement.racers.get_mut(*pilot) else {
                continue;
            };
            let Ok((_, mut kart)) = placement.karts.get_mut(racer.kart) else {
                continue;
            };
            if !kart.participant {
                continue;
            }
            kart.grid(&track, slot);
            racer.gate = kart.next_gate;
            racer.lap_start = 0;
            racer.best_lap = None;
            placement.travel.board(*pilot, racer.kart, &kart);
            slot += 1;
        }
        items.place(&track);
        race.phase = Phase::Loading;
        chat.say_all(
            Tone::Info,
            "Circuit pret. Traversez les End Crystals pour un bonus, Sprint pour l'utiliser, Saut + Avancer pour le boost.",
        );
    } else {
        race.phase = Phase::Results;
        chat.say_all(
            Tone::Info,
            "Piste demontee. /scores : resultats | /race : nouveau circuit.",
        );
    }
}

fn load(
    mut race: ResMut<Race>,
    karts: Query<(&Pilot, &Kart)>,
    transfers: Query<(), With<Transfer>>,
) {
    if race.phase != Phase::Loading {
        return;
    }
    if karts
        .iter()
        .any(|(pilot, kart)| kart.participant && transfers.contains(pilot.0))
    {
        return;
    }
    race.phase = Phase::Countdown;
    race.start = race.tick + COUNTDOWN;
}

fn countdown(mut race: ResMut<Race>, chat: Chat, audio: Audio) {
    if race.phase != Phase::Countdown {
        return;
    }
    let remaining = race.countdown();
    if remaining > 0 && remaining <= 60 && remaining.is_multiple_of(20) {
        audio.everyone(Cue::Beep);
    }
    if remaining == 0 {
        race.phase = Phase::Racing;
        audio.everyone(Cue::Go);
        audio.everyone(Cue::Start);
        chat.say_all(Tone::Event, format!("GO ! {} tour(s) !", race.laps));
    }
}

fn racing(
    mut race: ResMut<Race>,
    arena: Res<Arena>,
    map: Res<Track>,
    mut placement: Placement,
    mut chat: Chat,
    audio: Audio,
) {
    if race.phase != Phase::Racing {
        return;
    }
    let Placement {
        racers,
        karts,
        travel,
    } = &mut placement;
    let (laps, time_limit, elapsed) = (race.laps, race.time_limit(), race.elapsed());
    if TIME_WARNINGS.contains(&time_limit.saturating_sub(elapsed)) {
        chat.say_all(
            Tone::Warn,
            format!(
                "Il reste {} secondes pour terminer !",
                (time_limit - elapsed) / 20
            ),
        );
    }
    let mut unfinished = 0;
    for (pilot, mut kart) in karts.iter_mut() {
        let player = pilot.0;
        if !kart.racing() {
            continue;
        }
        let Ok(mut racer) = racers.get_mut(player) else {
            continue;
        };
        if kart.next_gate != racer.gate {
            racer.gate = kart.next_gate;
            if (kart.next_gate - 1).is_multiple_of(GATES) {
                racer.lap_done(&race);
            }
            if (kart.next_gate - 1).is_multiple_of(GATES) && kart.next_gate <= laps * GATES {
                let lap = (kart.next_gate - 1) / GATES + 1;
                if lap == laps {
                    audio.ui(player, Cue::FinalLap);
                    chat.flash(
                        player,
                        Tone::Alert,
                        format!("Dernier tour ({lap}/{laps}) !"),
                    );
                } else {
                    audio.ui(player, Cue::Lap);
                    chat.flash(player, Tone::Notice, format!("Tour {lap}/{laps}"));
                }
            }
        }
        if kart.next_gate > laps * GATES {
            let time = elapsed + kart.penalty;
            kart.finished = Some(time);
            kart.speed = 0.0;
            let name = chat.name(player);
            race.results.push((name.clone(), time));
            race.results.sort_by_key(|(_, time)| *time);
            let record = race.record.is_some_and(|best| time < best);
            race.best
                .entry((map.seed, laps, name.clone()))
                .and_modify(|best| *best = (*best).min(time))
                .or_insert(time);
            audio.ui(player, Cue::Finish);
            chat.say_all(
                Tone::Event,
                format!("{name} termine en {}s !", seconds(time)),
            );
            if record {
                audio.everyone(Cue::Record);
                chat.say_all(
                    Tone::Record,
                    format!("Record du circuit : {name} en {}s !", seconds(time)),
                );
            }
        } else {
            unfinished += 1;
        }
    }
    if unfinished > 0 && elapsed < time_limit {
        return;
    }
    let mut chunks = arena.chunks();
    chunks.reverse();
    race.schedule(Phase::Destroying, chunks);
    if elapsed >= time_limit {
        chat.say_all(Tone::Warn, "Temps limite atteint !");
    } else {
        chat.say_all(Tone::Event, "Course terminee !");
    }
    if race.results.is_empty() {
        chat.say_all(Tone::Info, "Aucun pilote n'a termine cette manche.");
    }
    for (i, (name, time)) in race.results.iter().take(3).enumerate() {
        chat.podium(i + 1, format!("#{} {name} — {}s", i + 1, seconds(*time)));
    }
    chat.say_all(
        Tone::Info,
        format!(
            "Circuit #{} : /race {laps} {} pour le rejouer. Retour en vol, demontage de la piste. /scores : classement complet.",
            map.seed, map.seed
        ),
    );
    for (slot, pilot) in race.roster.iter().enumerate() {
        let Ok(racer) = racers.get(*pilot) else {
            continue;
        };
        if let Ok((_, mut kart)) = karts.get_mut(racer.kart) {
            kart.wait(slot);
        }
    }
    travel.everyone_to_lobby();
}

fn hud(
    race: Res<Race>,
    karts: Query<(&Pilot, &Kart)>,
    transfers: Query<(), With<Transfer>>,
    mut chat: Chat,
) {
    if !race.tick.is_multiple_of(HUD_PERIOD) {
        return;
    }
    for (pilot, kart) in &karts {
        if chat.hud_free(pilot.0) {
            chat.hud(pilot.0, race.status(kart, transfers.contains(pilot.0)));
        }
    }
}

fn race_bar(
    race: Res<Race>,
    mut bar: Single<&mut BossBar, With<RaceBar>>,
    mut shown: Local<Option<(Phase, u64)>>,
) {
    let value = match race.phase {
        Phase::Countdown => race.countdown().div_ceil(20),
        Phase::Racing => race.time_limit().saturating_sub(race.elapsed()) / 20,
        Phase::Lobby | Phase::Generating | Phase::Loading | Phase::Destroying | Phase::Results => {
            *shown = None;
            if !matches!(&bar.audience, Audience::Explicit(viewers) if viewers.is_empty()) {
                bar.audience = Audience::explicit([]);
            }
            return;
        }
    };
    if *shown == Some((race.phase, value)) {
        return;
    }
    *shown = Some((race.phase, value));
    let (title, progress, color) = match race.phase {
        Phase::Countdown => (
            format!("Depart dans {value}..."),
            value as f32 / COUNTDOWN.div_ceil(20) as f32,
            BossBarColor::Yellow,
        ),
        _ => (
            format!("Temps restant : {}:{:02}", value / 60, value % 60),
            value as f32 / (race.time_limit() / 20) as f32,
            BossBarColor::Red,
        ),
    };
    if bar.title != title {
        bar.title = title;
    }
    if bar.progress != progress {
        bar.progress = progress;
    }
    if bar.color != color {
        bar.color = color;
    }
    if !matches!(bar.audience, Audience::All) {
        bar.audience = Audience::All;
    }
}

fn boost_bar(
    karts: Query<(&Pilot, &Kart), Changed<Kart>>,
    mut bars: Query<&mut BossBar, With<Racer>>,
) {
    for (pilot, kart) in &karts {
        let Ok(mut bar) = bars.get_mut(pilot.0) else {
            continue;
        };
        let progress = (kart.fuel / 100.0) as f32;
        if bar.progress != progress {
            bar.progress = progress;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {

    use bevy_app::{App, PostUpdate};
    use flume::Receiver;
    use ussr_nbt::owned::{Nbt, Tag};
    use voidmc::commands::{dispatch_command, plugin::CommandPlugin};
    use voidmc::components::{
        ClientId, LoadedChunks, MinecraftEntityId, PlayerDimension, PlayerName, PlayerReady,
        Position, Rotation, TeleportState,
    };
    use voidmc::network::{NetworkChannels, OutgoingPacket, PacketEvent};
    use voidmc::plugins::abilities::AbilitiesPlugin;
    use voidmc::plugins::boss_bar::BossBarPlugin;
    use voidmc::plugins::movement::MovementPlugin;
    use voidmc::plugins::scoreboard::ScoreboardPlugin;
    use voidmc::plugins::sidebar::SidebarPlugin;
    use voidmc::plugins::tab_list::TabListPlugin;
    use voidmc::plugins::teleport::TeleportPlugin;
    use voidmc::systems::entities::{broadcast_entity_movement, update_previous_entity_positions};
    use voidmc::world::{ChunkIndex, DimensionId};
    use voidmc::{EntityPlugin, Particle, Teleport, TextColor};
    use voidmc_codec::{Encode, VarI32};
    use voidmc_protocol::clientbound::{
        BossEventAction, ClientboundPacket, ManualPlayPacket, ObjectiveAction, Parser, PlayPacket,
        SoundEffect, SoundEvent, SoundSource,
    };
    use voidmc_protocol::serverbound::{ConfirmTeleportation, Pong};

    use super::*;
    use crate::arena::WAIT_Y;
    use crate::chat::{FLASH_PERIODS, HUD_COLOR, podium};
    use crate::kart::RESET_PENALTY;
    use crate::terrain::Alpine;
    use crate::travel::LOBBY;

    pub(crate) const VIEW_RADIUS: i32 = 20;

    #[derive(Debug, Clone, PartialEq)]
    pub(crate) enum Out {
        Chat {
            client: u32,
            overlay: bool,
            color: String,
            text: String,
        },
        Bar {
            client: u32,
            action: String,
            title: Option<String>,
            progress: Option<f32>,
        },
        Chunk(u32),
        Spawn {
            client: u32,
            id: i32,
            kind: i32,
            x: f64,
            y: f64,
            z: f64,
            yaw: u8,
        },
        Remove(u32, Vec<i32>),
        Passengers(u32, i32, Vec<i32>),
        Move {
            client: u32,
            id: i32,
            delta: (i16, i16, i16),
            yaw: Option<u8>,
        },
        Rotate(u32, i32, u8),
        Teleport {
            client: u32,
            id: i32,
            x: f64,
            y: f64,
            z: f64,
            yaw: f32,
        },
        HeadRotation(u32, i32),
        Metadata(u32, i32, Vec<u8>),
        Ping(u32, i32),
        Sync {
            client: u32,
            id: i32,
            x: f64,
            y: f64,
            z: f64,
            yaw: f32,
            pitch: f32,
        },
        Abilities {
            client: u32,
            flags: u8,
            flying_speed: f32,
            walking_speed: f32,
        },
        Particles {
            client: u32,
            particle: Particle,
            at: (f64, f64, f64),
            count: i32,
            offset: (f32, f32, f32),
            speed: f32,
            long_distance: bool,
        },
        Sound {
            client: u32,
            sound: i32,
            source: SoundSource,
            emitter: Option<i32>,
            at: Option<(i32, i32, i32)>,
            volume: f32,
            pitch: f32,
        },
        Objective {
            client: u32,
            action: String,
            title: Option<String>,
        },
        Display(u32, String),
        Line {
            client: u32,
            owner: String,
            text: String,
        },
        ResetLine(u32, String),
        Tab {
            client: u32,
            header: String,
            footer: String,
        },
    }

    pub(crate) struct Harness {
        pub(crate) app: App,
        pub(crate) rx: Receiver<OutgoingPacket>,
    }

    fn registry(event: &SoundEvent) -> i32 {
        match event {
            SoundEvent::Registry(id) => *id,
            other => panic!("expected a registry sound, got {other:?}"),
        }
    }

    pub(crate) fn sounds(out: &[Out], client: u32, cue: Cue) -> Vec<Out> {
        let id = voidmc::sounds::resolve(cue.name()).unwrap();
        out.iter()
            .filter(|o| matches!(o, Out::Sound { client: c, sound, .. } if *c == client && *sound == id))
            .cloned()
            .collect()
    }

    fn text(nbt: &Nbt, key: &str) -> String {
        match nbt
            .compound
            .tags
            .iter()
            .find(|(name, _)| name.to_string() == key)
        {
            Some((_, Tag::String(value))) => value.to_string(),
            other => panic!("expected string {key}, got {other:?}"),
        }
    }

    impl Harness {
        pub(crate) fn new(seed: u64) -> Self {
            let (incoming_tx, incoming_rx) = flume::unbounded::<voidmc::network::ConnectionEvent>();
            let (outgoing_tx, rx) = flume::unbounded::<OutgoingPacket>();
            let mut app = App::new();
            app.insert_resource(NetworkChannels {
                events: incoming_rx,
                outgoing: outgoing_tx,
            })
            .insert_non_send_resource(incoming_tx)
            .init_resource::<ChunkIndex>()
            .configure_sets(
                PostUpdate,
                (
                    VoidSystems::EntityBroadcast,
                    VoidSystems::EntityMetadataSync,
                    VoidSystems::EntityVisibility,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                (broadcast_entity_movement, update_previous_entity_positions)
                    .chain()
                    .in_set(VoidSystems::EntityBroadcast),
            )
            .add_plugins((
                CommandPlugin,
                BossBarPlugin,
                EntityPlugin,
                TeleportPlugin,
                MovementPlugin,
                AbilitiesPlugin,
                ScoreboardPlugin,
                SidebarPlugin,
                TabListPlugin,
                RacePlugin(Arena::new(Alpine { seed })),
            ));
            Self { app, rx }
        }

        pub(crate) fn world(&mut self) -> &mut World {
            self.app.world_mut()
        }

        pub(crate) fn race(&self) -> &Race {
            self.app.world().resource::<Race>()
        }

        pub(crate) fn race_mut(&mut self) -> Mut<'_, Race> {
            self.world().resource_mut::<Race>()
        }

        pub(crate) fn kart_entity(&self, player: Entity) -> Entity {
            self.app.world().get::<Racer>(player).unwrap().kart
        }

        pub(crate) fn kart(&self, player: Entity) -> &Kart {
            self.app
                .world()
                .get::<Kart>(self.kart_entity(player))
                .unwrap()
        }

        pub(crate) fn kart_mut(&mut self, player: Entity) -> Mut<'_, Kart> {
            let kart = self.kart_entity(player);
            self.world().get_mut::<Kart>(kart).unwrap()
        }

        pub(crate) fn spawn(&mut self, id: u32) -> Entity {
            let loaded = ChunkPos::new(0, 0)
                .chunks_in_radius(VIEW_RADIUS)
                .into_iter()
                .collect();
            self.world()
                .spawn((
                    ClientId(id),
                    PlayerReady,
                    MinecraftEntityId::allocate(),
                    Position::default(),
                    Rotation::default(),
                    TeleportState {
                        next_id: 1,
                        pending_id: None,
                    },
                    PlayerName(format!("Pilot{id}")),
                    PlayerDimension(DimensionId::Overworld),
                    LoadedChunks(loaded),
                ))
                .id()
        }

        pub(crate) fn client_id(&self, player: Entity) -> u32 {
            self.app.world().get::<ClientId>(player).unwrap().0
        }

        pub(crate) fn pong(&mut self, player: Entity, id: i32) {
            let client_id = self.client_id(player);
            self.world().trigger(PacketEvent {
                client_id,
                entity: player,
                packet: Pong { id },
            });
            self.world().flush();
        }

        pub(crate) fn confirm(&mut self, player: Entity, teleport_id: i32) {
            let client_id = self.client_id(player);
            self.world().trigger(PacketEvent {
                client_id,
                entity: player,
                packet: ConfirmTeleportation { teleport_id },
            });
            self.world().flush();
        }

        pub(crate) fn transfers(&mut self) -> Vec<(Entity, Teleport)> {
            self.world()
                .query::<(Entity, &Teleport)>()
                .iter(self.app.world())
                .map(|(e, t)| (e, t.clone()))
                .collect()
        }

        pub(crate) fn load_destination(&mut self, player: Entity, teleport: &Teleport) {
            let around = ChunkPos::from_block(teleport.x, teleport.z)
                .chunks_in_radius(teleport.preload_radius);
            self.world()
                .get_mut::<LoadedChunks>(player)
                .unwrap()
                .0
                .extend(around);
        }

        pub(crate) fn settle_transfers(&mut self) -> Vec<Out> {
            let everyone: Vec<Entity> = self.transfers().into_iter().map(|(e, _)| e).collect();
            self.settle_transfers_of(&everyone)
        }

        pub(crate) fn settle_transfers_of(&mut self, players: &[Entity]) -> Vec<Out> {
            let mut out = Vec::new();
            for _ in 0..8 {
                let transfers: Vec<_> = self
                    .transfers()
                    .into_iter()
                    .filter(|(e, _)| players.contains(e))
                    .collect();
                if transfers.is_empty() {
                    break;
                }
                for (player, teleport) in &transfers {
                    self.load_destination(*player, teleport);
                    self.world().entity_mut(*player).insert(teleport.clone());
                }
                self.tick();
                let sent = self.drain();
                for o in &sent {
                    if let Out::Ping(client, id) = o {
                        let player = self.player(*client);
                        if players.contains(&player) {
                            self.pong(player, *id);
                        }
                    }
                }
                out.extend(sent);
                let sent = self.drain();
                for o in &sent {
                    if let Out::Sync { client, id, .. } = o {
                        let player = self.player(*client);
                        if players.contains(&player) {
                            self.confirm(player, *id);
                        }
                    }
                }
                out.extend(sent);
                self.tick();
                out.extend(self.drain());
            }
            assert!(self.transfers().iter().all(|(e, _)| !players.contains(e)));
            out
        }

        pub(crate) fn player(&mut self, client: u32) -> Entity {
            self.world()
                .query::<(Entity, &ClientId)>()
                .iter(self.app.world())
                .find(|(_, id)| id.0 == client)
                .map(|(e, _)| e)
                .unwrap()
        }

        pub(crate) fn connect(&mut self, id: u32) -> Entity {
            let entity = self.spawn(id);
            self.world().trigger(PlayerReadyEvent {
                client_id: id,
                entity,
            });
            self.world().flush();
            entity
        }

        pub(crate) fn disconnect(&mut self, player: Entity) {
            let client_id = self.app.world().get::<ClientId>(player).unwrap().0;
            self.world().trigger(PlayerQuitEvent {
                client_id,
                entity: player,
            });
            self.world().flush();
            self.world().despawn(player);
        }

        pub(crate) fn command(&mut self, player: Entity, name: &str, args: &[&str]) {
            let client_id = self.app.world().get::<ClientId>(player).unwrap().0;
            dispatch_command(
                self.world(),
                client_id,
                player,
                name,
                args.iter().map(|s| s.to_string()).collect(),
            );
            self.world().flush();
        }

        pub(crate) fn tick(&mut self) {
            self.app.update();
        }

        pub(crate) fn ticks(&mut self, n: usize) {
            for _ in 0..n {
                self.tick();
            }
        }

        pub(crate) fn packets(&self) -> Vec<(u32, ClientboundPacket)> {
            self.rx
                .try_iter()
                .map(|out| (out.client_id, out.packet))
                .collect()
        }

        pub(crate) fn drain(&self) -> Vec<Out> {
            self.rx
                .try_iter()
                .map(|out| match out.packet {
                    ClientboundPacket::Play(PlayPacket::SystemChat(chat)) => Out::Chat {
                        client: out.client_id,
                        overlay: chat.overlay,
                        color: text(&chat.content, "color"),
                        text: text(&chat.content, "text"),
                    },
                    ClientboundPacket::Play(PlayPacket::BossEvent(event)) => {
                        let (action, title, progress) = match event.action {
                            BossEventAction::Add {
                                title, progress, ..
                            } => ("add", Some(text(&title, "text")), Some(progress)),
                            BossEventAction::Remove => ("remove", None, None),
                            BossEventAction::UpdateProgress(p) => ("progress", None, Some(p)),
                            BossEventAction::UpdateTitle(t) => {
                                ("title", Some(text(&t, "text")), None)
                            }
                            BossEventAction::UpdateStyle { .. } => ("style", None, None),
                            BossEventAction::UpdateFlags(_) => ("flags", None, None),
                        };
                        Out::Bar {
                            client: out.client_id,
                            action: action.into(),
                            title,
                            progress,
                        }
                    }
                    ClientboundPacket::ManualPlay(ManualPlayPacket::ChunkDataAndLight(_)) => {
                        Out::Chunk(out.client_id)
                    }
                    ClientboundPacket::Play(PlayPacket::SpawnEntity(p)) => Out::Spawn {
                        client: out.client_id,
                        id: p.entity_id,
                        kind: p.entity_type,
                        x: p.x,
                        y: p.y,
                        z: p.z,
                        yaw: p.yaw,
                    },
                    ClientboundPacket::ManualPlay(ManualPlayPacket::RemoveEntities(p)) => {
                        Out::Remove(out.client_id, p.entity_ids)
                    }
                    ClientboundPacket::ManualPlay(ManualPlayPacket::SetPassengers(p)) => {
                        Out::Passengers(out.client_id, p.entity_id, p.passengers)
                    }
                    ClientboundPacket::Play(PlayPacket::UpdateEntityPosition(p)) => Out::Move {
                        client: out.client_id,
                        id: p.entity_id,
                        delta: (p.delta_x, p.delta_y, p.delta_z),
                        yaw: None,
                    },
                    ClientboundPacket::Play(PlayPacket::UpdateEntityPositionAndRotation(p)) => {
                        Out::Move {
                            client: out.client_id,
                            id: p.entity_id,
                            delta: (p.delta_x, p.delta_y, p.delta_z),
                            yaw: Some(p.yaw),
                        }
                    }
                    ClientboundPacket::Play(PlayPacket::UpdateEntityRotation(p)) => {
                        Out::Rotate(out.client_id, p.entity_id, p.yaw)
                    }
                    ClientboundPacket::Play(PlayPacket::TeleportEntity(p)) => Out::Teleport {
                        client: out.client_id,
                        id: p.entity_id,
                        x: p.x,
                        y: p.y,
                        z: p.z,
                        yaw: p.yaw,
                    },
                    ClientboundPacket::Play(PlayPacket::EntityPositionSync(p)) => Out::Teleport {
                        client: out.client_id,
                        id: p.entity_id,
                        x: p.x,
                        y: p.y,
                        z: p.z,
                        yaw: p.yaw,
                    },
                    ClientboundPacket::Play(PlayPacket::SetHeadRotation(p)) => {
                        Out::HeadRotation(out.client_id, p.entity_id)
                    }
                    ClientboundPacket::Play(PlayPacket::SetEntityData(p)) => Out::Metadata(
                        out.client_id,
                        p.entity_id,
                        p.entries.iter().map(|e| e.index).collect(),
                    ),
                    ClientboundPacket::Play(PlayPacket::Ping(p)) => Out::Ping(out.client_id, p.id),
                    ClientboundPacket::Play(PlayPacket::SynchronizePlayerPosition(p)) => {
                        Out::Sync {
                            client: out.client_id,
                            id: p.teleport_id,
                            x: p.x,
                            y: p.y,
                            z: p.z,
                            yaw: p.yaw,
                            pitch: p.pitch,
                        }
                    }
                    ClientboundPacket::Play(PlayPacket::PlayerAbilities(p)) => Out::Abilities {
                        client: out.client_id,
                        flags: p.flags,
                        flying_speed: p.flying_speed,
                        walking_speed: p.walking_speed,
                    },
                    ClientboundPacket::Play(PlayPacket::LevelParticles(p)) => Out::Particles {
                        client: out.client_id,
                        particle: p.particle,
                        at: (p.x, p.y, p.z),
                        count: p.count,
                        offset: (p.offset_x, p.offset_y, p.offset_z),
                        speed: p.max_speed,
                        long_distance: p.long_distance,
                    },
                    ClientboundPacket::Play(PlayPacket::SoundEffect(p)) => Out::Sound {
                        client: out.client_id,
                        sound: registry(&p.sound),
                        source: p.source,
                        emitter: None,
                        at: Some((p.x, p.y, p.z)),
                        volume: p.volume,
                        pitch: p.pitch,
                    },
                    ClientboundPacket::Play(PlayPacket::EntitySoundEffect(p)) => Out::Sound {
                        client: out.client_id,
                        sound: registry(&p.sound),
                        source: p.source,
                        emitter: Some(p.entity_id),
                        at: None,
                        volume: p.volume,
                        pitch: p.pitch,
                    },
                    ClientboundPacket::Play(PlayPacket::SetObjective(p)) => {
                        let (action, title) = match &p.action {
                            ObjectiveAction::Create(info) => ("create", Some(&info.display_name)),
                            ObjectiveAction::Update(info) => ("update", Some(&info.display_name)),
                            ObjectiveAction::Remove => ("remove", None),
                        };
                        Out::Objective {
                            client: out.client_id,
                            action: action.into(),
                            title: title.map(|nbt| text(nbt, "text")),
                        }
                    }
                    ClientboundPacket::Play(PlayPacket::SetDisplayObjective(p)) => {
                        Out::Display(out.client_id, p.name)
                    }
                    ClientboundPacket::Play(PlayPacket::SetScore(p)) => Out::Line {
                        client: out.client_id,
                        owner: p.owner,
                        text: p
                            .display_name
                            .map(|nbt| text(&nbt, "text"))
                            .unwrap_or_default(),
                    },
                    ClientboundPacket::Play(PlayPacket::ResetScore(p)) => {
                        Out::ResetLine(out.client_id, p.owner)
                    }
                    ClientboundPacket::Play(PlayPacket::SetTabListHeaderFooter(p)) => Out::Tab {
                        client: out.client_id,
                        header: text(&p.header, "text"),
                        footer: text(&p.footer, "text"),
                    },
                    other => panic!("unexpected packet {other:?}"),
                })
                .collect()
        }

        pub(crate) fn chats(&self, client: u32) -> Vec<String> {
            self.drain()
                .into_iter()
                .filter_map(|out| match out {
                    Out::Chat {
                        client: c,
                        overlay: false,
                        text,
                        ..
                    } if c == client => Some(text),
                    _ => None,
                })
                .collect()
        }

        pub(crate) fn shortcut_to_countdown(&mut self, player: Entity, laps: &[&str]) -> Vec<Out> {
            self.command(player, "race", laps);
            assert_eq!(self.race().phase, Phase::Generating);
            let mut out = self.settle_transfers();
            self.race_mut().pending.clear();
            self.tick();
            assert_eq!(self.race().phase, Phase::Loading);
            out.extend(self.drain());
            out.extend(self.settle_transfers());
            assert_eq!(self.race().phase, Phase::Countdown);
            out
        }

        pub(crate) fn shortcut_to_racing(&mut self, player: Entity, laps: &[&str]) {
            self.shortcut_to_countdown(player, laps);
            let tick = self.race().tick;
            self.race_mut().start = tick;
            self.tick();
            assert_eq!(self.race().phase, Phase::Racing);
            self.drain();
        }
    }

    fn chat(text: &str) -> String {
        text.to_string()
    }

    fn colored(out: &[Out], client: u32) -> Vec<(String, String)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: c,
                    overlay: false,
                    text,
                    color,
                } if *c == client => Some((text.clone(), color.clone())),
                _ => None,
            })
            .collect()
    }

    fn flashes(out: &[Out], client: u32) -> Vec<(String, String)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: c,
                    overlay: true,
                    text,
                    color,
                } if *c == client && *color != HUD_COLOR.to_string() => {
                    Some((text.clone(), color.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn texts(out: &[Out], client: u32) -> Vec<String> {
        out.iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: c,
                    overlay: false,
                    text,
                    ..
                } if *c == client => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn overlays(out: &[Out], client: u32) -> Vec<String> {
        out.iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: c,
                    overlay: true,
                    text,
                    ..
                } if *c == client => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn bars(out: &[Out], client: u32) -> Vec<(String, Option<String>, Option<f32>)> {
        out.iter()
            .filter_map(|o| match o {
                Out::Bar {
                    client: c,
                    action,
                    title,
                    progress,
                } if *c == client => Some((action.clone(), title.clone(), *progress)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn ready_enrols_the_pilot_and_greets_in_order() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let kart = h.kart(a);
        assert_eq!(
            (kart.fuel, kart.next_gate, kart.participant),
            (100.0, 1, false)
        );
        assert_eq!(kart.y, WAIT_Y);
        assert_eq!(h.app.world().get::<Racer>(a).unwrap().gate, 1);
        let bar = h.app.world().get::<BossBar>(a).unwrap();
        assert_eq!(bar.title, BOOST_TITLE);
        assert_eq!(bar.color, BossBarColor::Blue);
        assert!(matches!(&bar.audience, Audience::Explicit(v) if v.len() == 1 && v.contains(&a)));
        assert_eq!(h.race().roster, vec![a]);
        let out = h.drain();
        let gold = Tone::Event.color().to_string();
        let gray = Tone::Info.color().to_string();
        let aqua = Tone::Notice.color().to_string();
        assert_eq!(
            colored(&out, 1),
            vec![
                ("Pilot1 rejoint Alpine Rush !".to_string(), gold.clone()),
                ("Pilot1 prend un minecart (1/8 pilotes).".to_string(), gray.clone()),
                (
                    "En vol au-dessus de la vallee. /race lance un circuit avec tous les pilotes presents.".to_string(),
                    aqua
                ),
                (
                    "/race [tours] [seed] : depart (1–20, defaut 3 ; seed pour rejouer un circuit) | /leave : spectateur | /join : revenir | /reset : secours (+3 s)".to_string(),
                    gray.clone()
                ),
                (
                    "Avancer : accelerer | Reculer : frein / marche arriere | Saut : boost | Sprint : bonus. F5 conseille.".to_string(),
                    gray
                ),
            ]
        );
        assert!(sounds(&out, 1, Cue::Portal).is_empty());
        h.tick();
        let out = h.drain();
        assert_eq!(
            bars(&out, 1),
            vec![("add".into(), Some(BOOST_TITLE.into()), Some(1.0))]
        );
        let b = h.connect(2);
        let out = h.drain();
        let to_a: Vec<_> = out
            .iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: 1, text, ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            to_a,
            vec![
                chat("Pilot2 rejoint Alpine Rush !"),
                chat("Pilot2 prend un minecart (2/8 pilotes)."),
            ]
        );
        assert_eq!(h.race().roster, vec![a, b]);
        assert_eq!(h.kart(b).x, h.kart(a).x + 3.0);
    }

    #[test]
    fn grid_is_capped_at_eight_and_join_is_idempotent() {
        let mut h = Harness::new(42);
        let pilots: Vec<_> = (1..=9).map(|id| h.connect(id)).collect();
        assert_eq!(h.race().roster.len(), GRID);
        assert!(h.app.world().get::<Racer>(pilots[8]).is_none());
        assert!(h.app.world().get::<BossBar>(pilots[8]).is_none());
        assert!(colored(&h.drain(), 9).contains(&(
            "La grille est pleine (8 pilotes).".to_string(),
            Tone::Warn.color().to_string()
        )));
        h.command(pilots[0], "join", &[]);
        assert!(h.chats(1).is_empty());
        assert_eq!(h.race().roster.len(), GRID);
    }

    #[test]
    fn join_ignores_a_despawned_entity() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let stale = h.spawn(2);
        h.world().despawn(stale);
        h.drain();
        h.world().trigger(Join(stale));
        h.world().flush();
        h.tick();
        assert_eq!(h.race().roster, vec![a]);
        assert!(h.chats(1).is_empty());
    }

    #[test]
    fn commands_are_registered_with_the_reference_descriptions() {
        let h = Harness::new(42);
        let registry = h.app.world().resource::<CommandRegistry>();
        for (name, description) in [
            (
                "race",
                "Start a race: /race [tours] [seed], 1–20 laps (default 3)",
            ),
            ("join", "Join Alpine Rush / mount your minecart"),
            ("leave", "Leave your minecart and watch the race"),
            ("reset", "Return to your last checkpoint (+3 seconds)"),
            ("scores", "Show race results and session records"),
        ] {
            assert_eq!(registry.description(name), Some(description));
        }
        let tree = registry.build_command_tree();
        assert!(tree.nodes.iter().any(|n| n.name.as_deref() == Some("tours")
            && matches!(
                n.parser,
                Some(Parser::Integer {
                    min: Some(1),
                    max: Some(20)
                })
            )));
    }

    #[test]
    fn race_command_validates_laps_and_refuses_double_starts() {
        let seed_error = |value: &str| {
            format!(
                "Invalid value '{value}' for <seed>: expected seed ('{value}' is not a circuit seed (unsigned 64-bit integer))"
            )
        };
        for (args, expected) in [
            (vec![], Ok(3)),
            (vec!["1"], Ok(1)),
            (vec!["5"], Ok(5)),
            (vec!["20"], Ok(20)),
            (vec!["0"], Err("Invalid value '0' for <tours>: expected integer (0 is below minimum 1)".to_string())),
            (vec!["-1"], Err("Invalid value '-1' for <tours>: expected integer (-1 is below minimum 1)".to_string())),
            (vec!["21"], Err("Invalid value '21' for <tours>: expected integer (21 is above maximum 20)".to_string())),
            (vec!["abc"], Err("Invalid value 'abc' for <tours>: expected integer ('abc' is not a valid integer)".to_string())),
            (vec!["2", "3"], Ok(2)),
            (vec!["2", "0"], Ok(2)),
            (vec!["2", "18446744073709551615"], Ok(2)),
            (vec!["2", "18446744073709551616"], Err(seed_error("18446744073709551616"))),
            (vec!["2", "-1"], Err(seed_error("-1"))),
            (vec!["2", "x"], Err(seed_error("x"))),
            (vec!["2", "3", "4"], Err("Too many arguments: expected 2, got 3".to_string())),
        ] {
            let mut h = Harness::new(42);
            let a = h.connect(1);
            h.drain();
            h.command(a, "race", &args);
            let laps = match expected {
                Ok(laps) => laps,
                Err(error) => {
                    assert_eq!(h.race().phase, Phase::Lobby);
                    assert_eq!(h.race().round, 0);
                    assert_eq!(
                        colored(&h.drain(), 1),
                        vec![
                            (error, TextColor::Red.to_string()),
                            (
                                "Usage: /race [tours:integer] [seed:seed]".to_string(),
                                TextColor::Gray.to_string()
                            ),
                        ]
                    );
                    continue;
                }
            };
            assert_eq!(h.race().phase, Phase::Generating);
            assert_eq!((h.race().laps, h.race().round), (laps, 1));
            let seed = h.app.world().resource::<Track>().seed;
            assert!(h.kart(a).participant);
            match args.get(1) {
                Some(explicit) => assert_eq!(seed.to_string(), *explicit),
                None => assert_eq!(
                    seed,
                    circuit_seed(h.app.world().resource::<Arena>().map.seed, 0)
                ),
            }
            let out = h.drain();
            assert_eq!(
                colored(&out, 1),
                vec![(
                    format!(
                        "Pilot1 lance la manche 1 : {laps} tour(s), {:.0} m par tour (Circuit #{seed}).",
                        h.app.world().resource::<Track>().length
                    ),
                    Tone::Event.color().to_string()
                )]
            );
            assert_eq!(
                sounds(&out, 1, Cue::Portal)
                    .iter()
                    .map(|s| match s {
                        Out::Sound { at, .. } => *at,
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                vec![Some((
                    (LOBBY.0 * 8.0) as i32,
                    (LOBBY.1 * 8.0) as i32,
                    (LOBBY.2 * 8.0) as i32
                ))]
            );
            h.command(a, "race", &["2"]);
            assert_eq!(h.race().laps, laps);
            assert_eq!(
                colored(&h.drain(), 1),
                vec![(
                    "Une course est deja en cours.".to_string(),
                    Tone::Warn.color().to_string()
                )]
            );
            assert!(h.race().time_limit() >= TIME_LIMIT / LAPS as u64 * laps as u64);
        }
    }

    #[test]
    fn race_requires_a_kart_and_launch_events_share_the_validation() {
        let mut h = Harness::new(42);
        let a = h.spawn(1);
        h.command(a, "race", &[]);
        assert_eq!(h.chats(1), vec![chat("Utilise /join avant /race.")]);
        h.world().trigger(Launch {
            player: a,
            laps: 0,
            seed: None,
        });
        assert_eq!(
            h.chats(1),
            vec![chat(
                "Utilisation : /race [tours] [seed], de 1 a 20 tours (defaut 3)."
            )]
        );
        assert_eq!(h.race().phase, Phase::Lobby);
    }

    #[test]
    fn full_cycle_builds_counts_down_races_and_demolishes() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.command(b, "leave", &[]);
        h.drain();
        h.command(a, "race", &[]);
        h.drain();
        let arena = h.app.world().resource::<Arena>().clone();
        let chunk_count = arena.chunks().len();
        let first_seed = h.app.world().resource::<Track>().seed;
        assert_eq!(h.race().pending.len(), chunk_count);
        assert!(chunk_count < 200);
        let mut iterations = 0;
        let mut announced = Vec::new();
        while h.race().phase == Phase::Generating {
            let before = h.race().pending.len();
            h.tick();
            iterations += 1;
            assert_eq!(
                h.race().pending.len(),
                before.saturating_sub(usize::from(iterations % 2 == 0))
            );
            announced.extend(h.chats(2));
            assert!(iterations <= chunk_count * 2);
        }
        assert_eq!(iterations, chunk_count * 2);
        assert_eq!(h.race().phase, Phase::Loading);
        h.ticks(3);
        announced.extend(h.chats(2));
        assert_eq!(h.race().phase, Phase::Loading);
        announced.extend(texts(&h.settle_transfers(), 2));
        assert_eq!(
            announced,
            vec![chat(
                "Circuit pret. Traversez les End Crystals pour un bonus, Sprint pour l'utiliser, Saut + Avancer pour le boost."
            )]
        );
        assert_eq!(h.race().phase, Phase::Countdown);
        assert_eq!(h.race().start, h.race().tick + COUNTDOWN);
        assert_eq!(h.app.world().resource::<ChunkIndex>().0.len(), chunk_count);
        assert!(h.kart(a).y < WAIT_Y);
        assert_eq!(h.kart(a).next_gate, 1);
        assert!(h.app.world().get::<Racer>(b).is_none());

        let mut countdown = Vec::new();
        let mut ticks = 0;
        while h.race().phase == Phase::Countdown {
            h.tick();
            ticks += 1;
            countdown.extend(h.drain());
        }
        assert_eq!(ticks, COUNTDOWN);
        assert_eq!(
            colored(&countdown, 1),
            vec![(
                "GO ! 3 tour(s) !".to_string(),
                Tone::Event.color().to_string()
            )]
        );
        let beeps = sounds(&countdown, 2, Cue::Beep);
        assert_eq!(beeps.len(), 4);
        assert!(beeps[..3].iter().all(
            |b| matches!(b, Out::Sound { pitch, source: SoundSource::Ui, emitter: None, .. } if *pitch == 1.0)
        ));
        assert!(matches!(beeps[3], Out::Sound { pitch, .. } if pitch == 2.0));
        assert_eq!(sounds(&countdown, 2, Cue::Start).len(), 1);
        assert_eq!(sounds(&countdown, 1, Cue::Beep).len(), 4);

        h.kart_mut(a).next_gate = GATES + 1;
        h.tick();
        let out = h.drain();
        assert!(texts(&out, 1).is_empty());
        assert_eq!(
            flashes(&out, 1),
            vec![("Tour 2/3".to_string(), Tone::Notice.color().to_string())]
        );
        assert_eq!(sounds(&out, 1, Cue::Lap).len(), 1);
        assert!(sounds(&out, 2, Cue::Lap).is_empty());
        h.tick();
        let out = h.drain();
        assert!(texts(&out, 1).is_empty() && sounds(&out, 1, Cue::Lap).is_empty());
        h.kart_mut(a).next_gate = 2 * GATES + 1;
        h.tick();
        let out = h.drain();
        assert!(texts(&out, 1).is_empty());
        assert_eq!(
            flashes(&out, 1),
            vec![(
                "Dernier tour (3/3) !".to_string(),
                Tone::Alert.color().to_string()
            )]
        );
        assert_eq!(sounds(&out, 1, Cue::FinalLap).len(), 1);
        assert!(sounds(&out, 1, Cue::Lap).is_empty());
        {
            let mut kart = h.kart_mut(a);
            kart.next_gate = 3 * GATES + 1;
            kart.penalty = RESET_PENALTY;
        }
        h.tick();
        let time = h.race().elapsed() + RESET_PENALTY;
        assert_eq!(h.kart(a).finished, Some(time));
        assert_eq!(h.race().results, vec![("Pilot1".to_string(), time)]);
        assert_eq!(
            h.race().best.get(&(first_seed, LAPS, "Pilot1".into())),
            Some(&time)
        );
        assert_eq!(h.race().phase, Phase::Destroying);
        assert_eq!(h.race().pending.len(), chunk_count);
        assert_eq!(h.kart(a).y, WAIT_Y);
        let stamp = seconds(time);
        let out = h.drain();
        assert_eq!(
            colored(&out, 2),
            vec![
                (
                    format!("Pilot1 termine en {stamp}s !"),
                    Tone::Event.color().to_string()
                ),
                (
                    "Course terminee !".to_string(),
                    Tone::Event.color().to_string()
                ),
                (format!("#1 Pilot1 — {stamp}s"), podium(1).to_string()),
                (
                    format!(
                        "Circuit #{first_seed} : /race 3 {first_seed} pour le rejouer. Retour en vol, demontage de la piste. /scores : classement complet."
                    ),
                    Tone::Info.color().to_string()
                ),
            ]
        );
        assert_eq!(sounds(&out, 1, Cue::Finish).len(), 1);
        assert!(sounds(&out, 2, Cue::Finish).is_empty());
        assert!(sounds(&out, 1, Cue::Record).is_empty());
        assert_eq!(sounds(&out, 1, Cue::Portal).len(), 1);
        assert_eq!(sounds(&out, 2, Cue::Portal).len(), 1);

        let mut demolition = Vec::new();
        while h.race().phase == Phase::Destroying {
            h.tick();
            demolition.extend(h.chats(2));
        }
        assert_eq!(
            demolition,
            vec![chat(
                "Piste demontee. /scores : resultats | /race : nouveau circuit."
            )]
        );
        assert_eq!(h.race().phase, Phase::Results);
        h.command(a, "scores", &[]);
        assert_eq!(
            colored(&h.drain(), 1),
            vec![
                (
                    "Manche 1 — 3 tour(s) :".to_string(),
                    Tone::Event.color().to_string()
                ),
                (
                    format!("1. Pilot1 — {stamp}s"),
                    Tone::Info.color().to_string()
                ),
                (
                    format!("Records du circuit (Circuit #{first_seed}, 3 tour(s)) :"),
                    Tone::Record.color().to_string()
                ),
                (format!("Pilot1 : {stamp}s"), Tone::Info.color().to_string()),
            ]
        );
        h.command(a, "race", &["2"]);
        h.drain();
        assert_eq!(h.race().phase, Phase::Generating);
        assert_eq!(h.race().round, 2);
        assert!(h.race().results.is_empty());
        assert_ne!(h.app.world().resource::<Track>().seed, first_seed);
        h.command(a, "scores", &[]);
        assert_eq!(h.chats(1).len(), 1);
        assert_eq!(h.race().best.len(), 1);
    }

    #[test]
    fn time_limit_warns_then_ends_the_round_without_finishers() {
        let mut h = Harness::new(7);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &["5"]);
        let limit = h.race().time_limit();
        assert_eq!(limit, 200 * 20 * 5);
        for (warning, seconds) in TIME_WARNINGS.into_iter().zip([60, 30, 10]) {
            {
                let mut race = h.race_mut();
                race.tick = limit;
                race.start = warning + 1;
            }
            h.tick();
            assert_eq!(
                colored(&h.drain(), 1),
                vec![(
                    format!("Il reste {seconds} secondes pour terminer !"),
                    Tone::Warn.color().to_string()
                )]
            );
        }
        {
            let mut race = h.race_mut();
            race.tick = limit;
            race.start = 1;
        }
        h.tick();
        assert_eq!(h.race().phase, Phase::Destroying);
        assert!(h.kart(a).finished.is_none());
        assert_eq!(
            colored(&h.drain(), 1),
            vec![
                (
                    "Temps limite atteint !".to_string(),
                    Tone::Warn.color().to_string()
                ),
                (
                    "Aucun pilote n'a termine cette manche.".to_string(),
                    Tone::Info.color().to_string()
                ),
                (
                    format!(
                        "Circuit #{seed} : /race 5 {seed} pour le rejouer. Retour en vol, demontage de la piste. /scores : classement complet.",
                        seed = h.app.world().resource::<Track>().seed
                    ),
                    Tone::Info.color().to_string()
                ),
            ]
        );
    }

    #[test]
    fn leave_and_quit_update_the_grid_and_end_an_abandoned_round() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let c = h.connect(3);
        h.tick();
        h.drain();
        h.command(c, "leave", &[]);
        assert!(h.app.world().get::<Racer>(c).is_none());
        assert!(h.app.world().get::<BossBar>(c).is_none());
        assert!(h.app.world().get::<BossBarState>(c).is_none());
        assert_eq!(h.race().roster, vec![a, b]);
        let out = h.drain();
        assert_eq!(bars(&out, 3), vec![("remove".into(), None, None)]);
        assert_eq!(overlays(&out, 3).len(), 0);
        let to_c: Vec<_> = out
            .iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client: 3, text, ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            to_c,
            vec![
                chat("Pilot3 quitte la grille et rejoint les spectateurs."),
                chat("Spectateur en vol. /join pour revenir sur la grille."),
            ]
        );
        assert_eq!(sounds(&out, 3, Cue::Portal).len(), 1);
        h.command(c, "leave", &[]);
        assert!(h.drain().is_empty());
        h.command(c, "join", &[]);
        assert_eq!(h.race().roster, vec![a, b, c]);
        assert!(
            h.chats(1)
                .contains(&chat("Pilot3 prend un minecart (3/8 pilotes)."))
        );
        h.command(c, "leave", &[]);
        h.drain();

        h.shortcut_to_racing(a, &[]);
        assert!(h.kart(a).racing() && h.kart(b).racing());
        h.disconnect(b);
        assert_eq!(h.race().roster, vec![a]);
        let out = h.drain();
        let mut farewells: Vec<_> = out
            .iter()
            .filter_map(|o| match o {
                Out::Chat {
                    client,
                    text,
                    overlay: false,
                    ..
                } => Some((*client, text.clone())),
                _ => None,
            })
            .collect();
        farewells.sort();
        assert_eq!(
            farewells,
            vec![
                (1, chat("Pilot2 quitte Alpine Rush. Abandon de la manche.")),
                (3, chat("Pilot2 quitte Alpine Rush. Abandon de la manche.")),
            ]
        );
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        h.disconnect(a);
        assert_eq!(
            h.chats(3),
            vec![chat("Pilot1 quitte Alpine Rush. Abandon de la manche.")]
        );
        h.tick();
        assert_eq!(h.race().phase, Phase::Destroying);
        assert!(
            h.chats(3)
                .contains(&chat("Aucun pilote n'a termine cette manche."))
        );
        h.disconnect(c);
        assert!(h.race().roster.is_empty());
        h.tick();
        assert!(h.chats(3).is_empty());
    }

    #[test]
    fn late_joiners_watch_and_only_participants_are_gridded() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.command(a, "race", &[]);
        let b = h.connect(2);
        assert!(h.chats(2).contains(&chat(
            "Le circuit se construit : tu participeras a la prochaine manche."
        )));
        assert!(!h.kart(b).participant);
        h.settle_transfers();
        h.race_mut().pending.clear();
        h.tick();
        assert_eq!(h.race().phase, Phase::Loading);
        h.settle_transfers();
        assert_eq!(h.race().phase, Phase::Countdown);
        assert!(h.kart(a).y < WAIT_Y);
        assert_eq!(h.kart(b).y, WAIT_Y);
        let c = h.connect(3);
        assert!(h.chats(3).contains(&chat(
            "Manche en cours : observe en vol, tu participeras a la suivante."
        )));
        let tick = h.race().tick;
        h.race_mut().start = tick;
        h.tick();
        h.kart_mut(a).next_gate = LAPS * GATES + 1;
        h.tick();
        assert_eq!(h.race().phase, Phase::Destroying);
        let d = h.connect(4);
        assert!(h.chats(4).contains(&chat(
            "Demontage en cours : /race sera disponible juste apres."
        )));
        assert_eq!(h.race().roster, vec![a, b, c, d]);
    }

    #[test]
    fn reset_only_rescues_an_unfinished_participant_during_the_race() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.drain();
        h.command(a, "reset", &[]);
        assert!(h.drain().is_empty());
        assert_eq!(h.kart(a).penalty, 0);
        h.shortcut_to_racing(a, &[]);
        let b = h.connect(2);
        h.drain();
        h.command(b, "reset", &[]);
        assert!(h.drain().is_empty());
        h.kart_mut(a).next_gate = 3;
        h.command(a, "reset", &[]);
        assert_eq!(h.kart(a).penalty, RESET_PENALTY);
        let out = h.drain();
        assert!(texts(&out, 1).is_empty());
        assert_eq!(
            flashes(&out, 1),
            vec![(
                "Retour au checkpoint : +3 s de penalite, bonus retires".to_string(),
                Tone::Warn.color().to_string()
            )]
        );
        let (x, z) = (h.kart(a).x, h.kart(a).z);
        let map = h.app.world().resource::<Track>().clone();
        assert!((map.project(x, z).phase - 2.0 * std::f64::consts::TAU / GATES as f64).abs() < 0.1);
    }

    #[test]
    fn hud_action_bar_follows_the_phase_every_five_ticks() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        h.ticks(5);
        let out = h.drain();
        assert_eq!(
            overlays(&out, 1),
            vec!["Vol libre | /race pour lancer une course".to_string()]
        );
        assert!(flashes(&out, 1).is_empty() && flashes(&out, 2).is_empty());
        h.shortcut_to_countdown(a, &[]);
        let c = h.connect(3);
        let tick = h.race().tick;
        h.ticks((5 - tick % 5) as usize);
        let out = h.drain();
        assert_eq!(overlays(&out, 1), vec!["DEPART DANS 5...".to_string()]);
        assert_eq!(
            overlays(&out, 3),
            vec!["Prochaine course : attends l'arrivee des pilotes".to_string()]
        );
        let tick = h.race().tick;
        h.race_mut().start = tick + 1;
        h.ticks(5);
        let out = h.drain();
        assert_eq!(
            overlays(&out, 1),
            vec!["Tour 1/3 | CP 0/8 | 0.2s (+0s) | 0 km/h | Bonus : — [Sprint]".to_string()]
        );
        {
            let mut kart = h.kart_mut(a);
            kart.next_gate = GATES + 3;
            kart.penalty = RESET_PENALTY;
            kart.speed = 0.5;
            kart.item = Some(PowerUp::Turbo);
        }
        h.ticks(10);
        let out = h.drain();
        assert_eq!(
            overlays(&out, 1),
            vec![
                "Tour 2/3 | CP 2/8 | 0.5s (+3s) | 33 km/h | Bonus : TURBO [Sprint]".to_string(),
                "Tour 2/3 | CP 2/8 | 0.7s (+3s) | 30 km/h | Bonus : TURBO [Sprint]".to_string()
            ]
        );
        h.kart_mut(a).next_gate = LAPS * GATES + 1;
        h.ticks(5);
        let out = h.drain();
        let time = h.kart(a).finished.unwrap();
        assert_eq!(
            overlays(&out, 1),
            vec![format!("ARRIVEE — {}s | /scores", seconds(time))]
        );
        assert_eq!(
            overlays(&out, 2),
            vec!["Tour 1/3 | CP 0/8 | 0.9s (+0s) | 0 km/h | Bonus : — [Sprint]".to_string()]
        );
        h.kart_mut(b).next_gate = LAPS * GATES + 1;
        h.ticks(5);
        let out = h.drain();
        assert_eq!(h.race().phase, Phase::Destroying);
        for client in [1, 2, 3] {
            let shown = overlays(&out, client);
            assert_eq!(shown.len(), 1);
            assert!(shown[0].starts_with("Destruction du circuit : "));
        }
        let _ = c;
    }

    #[test]
    fn a_flash_holds_the_action_bar_for_eight_hud_periods_then_the_hud_resumes() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_racing(a, &[]);
        let tick = h.race().tick;
        h.ticks((5 - tick % 5) as usize);
        h.drain();
        h.kart_mut(a).next_gate = GATES + 1;
        h.tick();
        let out = h.drain();
        assert_eq!(overlays(&out, 1), vec!["Tour 2/3".to_string()]);
        assert_eq!(h.app.world().get::<Flash>(a), Some(&Flash(FLASH_PERIODS)));
        h.ticks(FLASH_PERIODS as usize * HUD_PERIOD as usize);
        assert!(overlays(&h.drain(), 1).is_empty());
        assert_eq!(h.app.world().get::<Flash>(a), Some(&Flash(0)));
        h.ticks(HUD_PERIOD as usize);
        let out = h.drain();
        assert_eq!(overlays(&out, 1).len(), 1);
        assert!(overlays(&out, 1)[0].starts_with("Tour 2/3 | CP 0/8"));
        assert!(flashes(&out, 1).is_empty());
        h.command(a, "leave", &[]);
        assert!(h.app.world().get::<Flash>(a).is_none());
    }

    #[test]
    fn a_record_is_announced_only_when_a_replayed_circuit_is_beaten() {
        fn finish(
            h: &mut Harness,
            player: Entity,
            client: u32,
            delay: usize,
            penalty: u64,
        ) -> (u64, bool) {
            h.ticks(delay);
            h.drain();
            let mut kart = h.kart_mut(player);
            kart.penalty = penalty;
            kart.next_gate = GATES + 1;
            h.tick();
            let out = h.drain();
            let time = h.kart(player).finished.unwrap();
            let record = colored(&out, client).contains(&(
                format!("Record du circuit : Pilot{client} en {}s !", seconds(time)),
                Tone::Record.color().to_string(),
            ));
            assert_eq!(sounds(&out, 1, Cue::Record).len(), usize::from(record));
            assert_eq!(sounds(&out, 2, Cue::Record).len(), usize::from(record));
            assert_eq!(sounds(&out, client, Cue::Finish).len(), 1);
            (time, record)
        }
        let mut h = Harness::new(42);
        let a = h.connect(1);
        let b = h.connect(2);
        let seed = "7";
        let mut rounds = Vec::new();
        for [first, second] in [
            [(a, 1, 10, 60), (b, 2, 2, 0)],
            [(b, 2, 3, 0), (a, 1, 20, 0)],
            [(a, 1, 3, 0), (b, 2, 5, 0)],
        ] {
            h.shortcut_to_racing(a, &["1", seed]);
            assert_eq!(h.app.world().resource::<Track>().seed, 7);
            let previous = h.race().record;
            let round = [first, second].map(|(player, client, delay, penalty)| {
                finish(&mut h, player, client, delay, penalty)
            });
            assert_eq!(h.race().record, previous);
            while h.race().phase != Phase::Results {
                h.tick();
            }
            h.drain();
            rounds.push(round);
        }
        let [[a1, b1], [b2, a2], [a3, b3]] = rounds[..] else {
            unreachable!()
        };
        assert!(b1.0 < a1.0);
        assert!(b2.0 < b1.0);
        assert!(a2.0 > b1.0);
        assert_eq!(a3.0, b2.0);
        assert!(b3.0 > b2.0);
        assert_eq!(
            [a1.1, b1.1, b2.1, a2.1, a3.1, b3.1],
            [false, false, true, false, false, false]
        );
        assert_eq!(h.race().record, Some(b2.0));
        assert_eq!(h.race().best.len(), 2);
        assert_eq!(h.race().best[&(7, 1, "Pilot1".into())], a3.0);
        assert_eq!(h.race().best[&(7, 1, "Pilot2".into())], b2.0);
    }

    #[test]
    fn countdown_beep_is_a_registry_ui_sound_with_the_paper_layout() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.shortcut_to_countdown(a, &[]);
        let tick = h.race().tick;
        h.race_mut().start = tick + 62;
        h.tick();
        h.packets();
        let position = *h.app.world().get::<Position>(a).unwrap();
        h.tick();
        let beeps: Vec<SoundEffect> = h
            .packets()
            .into_iter()
            .filter_map(|(client, packet)| match packet {
                ClientboundPacket::Play(PlayPacket::SoundEffect(p)) if client == 1 => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(beeps.len(), 1);
        let mut bytes = Vec::new();
        PlayPacket::SoundEffect(beeps[0].clone()).encode(&mut bytes);
        let mut expected = vec![0x75];
        VarI32(voidmc::sounds::resolve(Cue::Beep.name()).unwrap() + 1).encode(&mut expected);
        expected.push(SoundSource::Ui as u8);
        for coordinate in [position.x, position.y, position.z] {
            expected.extend(((coordinate * 8.0) as i32).to_be_bytes());
        }
        expected.extend(1.0f32.to_be_bytes());
        expected.extend(1.0f32.to_be_bytes());
        expected.extend(0i64.to_be_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn boss_bars_are_pushed_only_when_their_content_changes() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.tick();
        h.drain();
        h.ticks(40);
        assert!(bars(&h.drain(), 1).is_empty());
        h.command(a, "race", &[]);
        h.tick();
        assert!(bars(&h.drain(), 1).is_empty());
        while h.race().phase == Phase::Generating {
            h.tick();
        }
        assert!(bars(&h.drain(), 1).is_empty());
        h.ticks(4);
        assert!(bars(&h.drain(), 1).is_empty());
        let out = h.settle_transfers();
        assert_eq!(
            bars(&out, 1),
            vec![("add".into(), Some("Depart dans 5...".into()), Some(1.0))]
        );
        h.ticks((COUNTDOWN - 1) as usize);
        let out = h.drain();
        assert_eq!(
            bars(&out, 1)
                .into_iter()
                .filter_map(|(_, title, _)| title)
                .collect::<Vec<_>>(),
            vec![
                "Depart dans 4...",
                "Depart dans 3...",
                "Depart dans 2...",
                "Depart dans 1..."
            ]
        );
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        assert_eq!(
            bars(&h.drain(), 1),
            vec![
                ("progress".into(), None, Some(1.0)),
                ("title".into(), Some("Temps restant : 10:00".into()), None),
                ("style".into(), None, None),
            ]
        );
        h.ticks(40);
        let out = h.drain();
        assert_eq!(
            bars(&out, 1)
                .into_iter()
                .map(|(action, title, _)| (action, title))
                .collect::<Vec<_>>(),
            vec![
                ("progress".into(), None),
                ("title".into(), Some("Temps restant : 9:59".into())),
                ("progress".into(), None),
                ("title".into(), Some("Temps restant : 9:58".into())),
            ]
        );
        h.kart_mut(a).fuel = 50.0;
        h.tick();
        let progress = h.app.world().get::<BossBar>(a).unwrap().progress;
        assert_eq!(progress, (h.kart(a).fuel / 100.0) as f32);
        assert!((progress - 0.5035).abs() < 1e-6);
        assert!(bars(&h.drain(), 1).contains(&("progress".into(), None, Some(progress))));
        h.kart_mut(a).next_gate = LAPS * GATES + 1;
        h.tick();
        assert_eq!(h.race().phase, Phase::Destroying);
        let out = bars(&h.drain(), 1);
        assert_eq!(out[0], ("remove".into(), None, None));
        assert!(
            out[1..].iter().all(|(action, _, _)| action == "progress"),
            "{out:?}"
        );
    }

    #[test]
    fn race_bar_is_left_untouched_between_two_quantised_values() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.command(a, "race", &[]);
        while h.race().phase != Phase::Racing {
            if h.race().phase == Phase::Loading {
                h.settle_transfers();
            } else {
                h.tick();
            }
        }
        h.tick();
        h.drain();
        let bar = h
            .world()
            .query_filtered::<Entity, With<RaceBar>>()
            .single(h.app.world())
            .unwrap();
        let changed = h
            .app
            .world()
            .entity(bar)
            .get_ref::<BossBar>()
            .unwrap()
            .last_changed();
        let title = h.app.world().get::<BossBar>(bar).unwrap().title.clone();
        h.ticks(19);
        assert!(bars(&h.drain(), 1).is_empty());
        let bar_ref = h.app.world().entity(bar).get_ref::<BossBar>().unwrap();
        assert_eq!(bar_ref.last_changed(), changed);
        assert_eq!(bar_ref.title, title);
        h.tick();
        assert_eq!(
            bars(&h.drain(), 1)
                .into_iter()
                .map(|(action, title, _)| (action, title))
                .collect::<Vec<_>>(),
            vec![
                ("progress".into(), None),
                ("title".into(), Some("Temps restant : 9:58".into())),
            ]
        );
    }
}
