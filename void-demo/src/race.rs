use std::collections::HashMap;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use voidmc::{
    Audience, BossBar, BossBarColor, ChunkPos, Command, CommandBuilder, CommandRegistry,
    CommandSystems, IntegerArg, Messages, VoidSystems,
    components::PlayerName,
    events::{PlayerQuitEvent, PlayerReadyEvent},
    plugins::boss_bar::BossBarState,
};

use crate::arena::Arena;
use crate::displays;
use crate::items::{self, Items};
use crate::kart::{Kart, PowerUp};
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
const MILESTONE: usize = 25;
const HUD_PERIOD: u64 = 5;
const TIME_WARNINGS: [u64; 3] = [1200, 600, 200];
const ROUND_STRIDE: u64 = 0x9e3779b97f4a7c15;
const PREFIX: &str = "[Alpine Rush] ";
const COLOR: &str = "gold";
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
    pub roster: Vec<Entity>,
    pub pending: Vec<ChunkPos>,
    pub total_chunks: usize,
    pub milestone: usize,
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
            roster: Vec::new(),
            pending: Vec::new(),
            total_chunks: 0,
            milestone: 0,
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
        self.milestone = MILESTONE;
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
}

#[derive(Event)]
pub struct Scores(pub Entity);

#[derive(SystemParam)]
pub struct Chat<'w, 's> {
    messages: Messages<'w, 's>,
    names: Query<'w, 's, &'static PlayerName>,
}

impl Chat<'_, '_> {
    pub fn name(&self, player: Entity) -> String {
        self.names
            .get(player)
            .map_or_else(|_| "Pilote".into(), |n| n.0.clone())
    }

    pub fn tell(&self, player: Entity, text: impl AsRef<str>) {
        self.messages
            .message(player, format!("{PREFIX}{}", text.as_ref()))
            .color(COLOR)
            .send();
    }

    pub fn all(&self, text: impl AsRef<str>) {
        self.messages
            .broadcast(format!("{PREFIX}{}", text.as_ref()))
            .color(COLOR)
            .send();
    }

    pub fn others(&self, player: Entity, text: impl AsRef<str>) {
        self.messages
            .broadcast(format!("{PREFIX}{}", text.as_ref()))
            .color(COLOR)
            .audience(Audience::custom(move |r| r.entity() != player))
            .send();
    }

    pub fn bar(&self, player: Entity, text: impl Into<String>) {
        self.messages.action_bar(player, text).color(COLOR).send();
    }
}

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
            self.chat
                .tell(player, format!("La grille est pleine ({GRID} pilotes)."));
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
            Racer { gate: 1, kart },
            BossBar::new(BOOST_TITLE)
                .color(BossBarColor::Blue)
                .viewers([player]),
        ));
        let name = self.chat.name(player);
        self.chat.all(format!(
            "{name} prend un minecart ({}/{GRID} pilotes).",
            slot + 1
        ));
        self.chat.tell(
            player,
            match self.race.phase {
                Phase::Generating => {
                    "Le nouveau circuit se construit. Tu participeras a la prochaine manche."
                }
                Phase::Loading | Phase::Countdown | Phase::Racing => {
                    "Manche en cours : observe en vol, tu participeras a la suivante."
                }
                Phase::Destroying => {
                    "Demontage du circuit en cours. /race sera disponible juste apres."
                }
                Phase::Lobby | Phase::Results => {
                    "En vol au-dessus de la vallee ! /race genere un circuit et inscrit tous les pilotes presents."
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
            .remove::<(Racer, BossBar, BossBarState)>();
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
                    items::crystals,
                    vehicle::drive,
                    racing,
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
        let mut registry = app.world_mut().resource_mut::<CommandRegistry>();
        for command in race_commands() {
            registry.register(command);
        }
    }
}

pub fn race_commands() -> Vec<Command> {
    vec![
        CommandBuilder::new("race")
            .description("Start a race: /race [tours], 1–20 laps (default 3)")
            .arg_optional("tours", IntegerArg::new(1, MAX_LAPS as i32))
            .handler(|ctx| {
                let player = ctx.entity;
                let laps = ctx.get::<i32>("tours").copied().unwrap_or(LAPS as i32) as usize;
                ctx.with_world_mut(|world| world.trigger(Launch { player, laps }));
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
    let name = grid.chat.name(player);
    grid.chat
        .all(format!("[+] {name} rejoint Alpine Rush. Bienvenue !"));
    grid.join(player);
    grid.chat.tell(
        player,
        "Alpine Rush | /race [tours] : depart (1–20, defaut 3) | /leave : spectateur | /join : revenir",
    );
    grid.chat.tell(
        player,
        "Avancer : accelerer | Reculer : frein / marche arriere | Direction : gauche/droite | Saut : boost | Sprint : bonus | /reset : secours (+3s). F5 conseille.",
    );
}

fn quit(event: On<PlayerQuitEvent>, karts: Karts, mut grid: Grid) {
    let player = event.entity;
    let racing = grid.race.phase == Phase::Racing && karts.get(player).is_some_and(Kart::racing);
    grid.leave(player);
    let name = grid.chat.name(player);
    grid.chat.others(
        player,
        format!(
            "[-] {name} quitte Alpine Rush.{}",
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
    grid.chat.all(format!(
        "{name} quitte la grille et rejoint les spectateurs."
    ));
    grid.chat.tell(
        player,
        "Mode spectateur en vol. /join pour revenir sur la grille.",
    );
}

fn rescue(event: On<Rescue>, race: Res<Race>, map: Res<Track>, mut karts: Karts, chat: Chat) {
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
    chat.tell(
        event.0,
        "Retour au dernier checkpoint : +3 secondes de penalite. Bonus retires.",
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
        chat.tell(player, "Utilisation : /race [tours], de 1 a 20 (defaut 3).");
        return;
    }
    if !race.roster.contains(&player) {
        chat.tell(player, "Utilise /join avant /race.");
        return;
    }
    if !race.phase.idle() {
        chat.tell(player, "Une course est deja en cours.");
        return;
    }
    for (slot, pilot) in race.roster.iter().enumerate() {
        if let Some(mut kart) = karts.get_mut(*pilot) {
            kart.wait(slot);
            kart.participant = true;
        }
    }
    travel.everyone_to_lobby();
    let map = Track::new(mix(arena
        .map
        .seed
        .wrapping_add(race.round.wrapping_mul(ROUND_STRIDE))));
    let (seed, length) = (map.seed, map.length);
    arena.prepare(map.clone());
    commands.insert_resource(map);
    race.round += 1;
    race.laps = laps;
    race.results.clear();
    race.schedule(Phase::Generating, arena.chunks());
    let (name, round) = (chat.name(player), race.round);
    chat.all(format!(
        "{name} lance la manche {round} : {laps} tour(s) ! Nouveau trace : {length:.0} m par tour | seed {seed}."
    ));
    chat.all(format!(
        "Tous en vol ! La piste se construit dans le paysage. Depart apres l'assemblage ; {laps} tour(s), bonus et boost rechargeable."
    ));
}

fn scores(event: On<Scores>, race: Res<Race>, map: Res<Track>, chat: Chat) {
    let player = event.0;
    for (i, (name, time)) in race.results.iter().enumerate() {
        chat.tell(player, format!("{}. {name} — {}s", i + 1, seconds(*time)));
    }
    let seed = map.seed;
    chat.tell(
        player,
        format!(
            "Records du circuit actuel (seed {seed}, {} tour(s)) :",
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
        chat.tell(player, format!("{name} : {}s", seconds(time)));
    }
}

fn clock(mut race: ResMut<Race>) {
    race.tick += 1;
}

fn construct(
    mut race: ResMut<Race>,
    mut items: ResMut<Items>,
    arena: Res<Arena>,
    mut racers: Query<&mut Racer>,
    mut karts: Query<&mut Kart>,
    mut travel: Travel,
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
    let progress = race.construction();
    if progress >= race.milestone && progress < 100 {
        race.milestone = (progress / MILESTONE + 1) * MILESTONE;
        chat.all(format!(
            "{} du circuit : {progress}%.",
            if build { "Construction" } else { "Demontage" }
        ));
    }
    if !race.pending.is_empty() {
        return;
    }
    if build {
        let track = arena.track();
        let mut slot = 0;
        for pilot in &race.roster {
            let Ok(mut racer) = racers.get_mut(*pilot) else {
                continue;
            };
            let Ok(mut kart) = karts.get_mut(racer.kart) else {
                continue;
            };
            if !kart.participant {
                continue;
            }
            kart.grid(&track, slot);
            racer.gate = kart.next_gate;
            travel.board(*pilot, &kart);
            slot += 1;
        }
        items.place(&track);
        race.phase = Phase::Loading;
        chat.all("Circuit pret ! Chargement de la grille chez chaque pilote avant le compte a rebours...");
        chat.all("Traversez les End Crystals pour un bonus, puis Sprint pour l'utiliser. Saut + Avancer = boost.");
    } else {
        race.phase = Phase::Results;
        chat.all(
            "Piste demontee, vallee restauree ! /scores : resultats | /race : nouveau circuit.",
        );
    }
}

fn load(
    mut race: ResMut<Race>,
    karts: Query<(&Pilot, &Kart)>,
    transfers: Query<(), With<Transfer>>,
    chat: Chat,
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
    chat.all("Tous les pilotes sont charges ! Depart dans 5 secondes !");
}

fn countdown(mut race: ResMut<Race>, chat: Chat) {
    if race.phase != Phase::Countdown {
        return;
    }
    let remaining = race.countdown();
    if remaining > 0 && remaining <= 60 && remaining.is_multiple_of(20) {
        chat.all(format!("Depart dans {}...", remaining / 20));
    }
    if remaining == 0 {
        race.phase = Phase::Racing;
        chat.all(format!(
            "GO ! {} tour(s) — bonne course a tous !",
            race.laps
        ));
    }
}

fn racing(
    mut race: ResMut<Race>,
    arena: Res<Arena>,
    map: Res<Track>,
    mut racers: Query<&mut Racer>,
    mut karts: Query<(&Pilot, &mut Kart)>,
    mut travel: Travel,
    chat: Chat,
) {
    if race.phase != Phase::Racing {
        return;
    }
    let (laps, time_limit, elapsed) = (race.laps, race.time_limit(), race.elapsed());
    if TIME_WARNINGS.contains(&time_limit.saturating_sub(elapsed)) {
        chat.all(format!(
            "Il reste {} secondes pour terminer !",
            (time_limit - elapsed) / 20
        ));
    }
    let mut unfinished = 0;
    for (pilot, mut kart) in &mut karts {
        let player = pilot.0;
        if !kart.racing() {
            continue;
        }
        let Ok(mut racer) = racers.get_mut(player) else {
            continue;
        };
        if kart.next_gate != racer.gate {
            racer.gate = kart.next_gate;
            if (kart.next_gate - 1).is_multiple_of(GATES) && kart.next_gate <= laps * GATES {
                let lap = (kart.next_gate - 1) / GATES + 1;
                chat.tell(
                    player,
                    format!(
                        "Tour {lap}/{laps} ! {}",
                        if lap == laps {
                            "Dernier tour, donne tout !"
                        } else {
                            "Garde le rythme !"
                        }
                    ),
                );
            }
        }
        if kart.next_gate > laps * GATES {
            let time = elapsed + kart.penalty;
            kart.finished = Some(time);
            kart.speed = 0.0;
            let name = chat.name(player);
            race.results.push((name.clone(), time));
            race.results.sort_by_key(|(_, time)| *time);
            race.best
                .entry((map.seed, laps, name.clone()))
                .and_modify(|best| *best = (*best).min(time))
                .or_insert(time);
            chat.all(format!("{name} termine en {}s ! /scores", seconds(time)));
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
    chat.all(if elapsed >= time_limit {
        "Temps limite atteint !"
    } else {
        "Course terminee !"
    });
    if race.results.is_empty() {
        chat.all("Aucun pilote n'a termine cette manche.");
    }
    for (i, (name, time)) in race.results.iter().take(3).enumerate() {
        chat.all(format!("Podium #{} : {name} — {}s", i + 1, seconds(*time)));
    }
    chat.all("Retour en vol : demontage de la piste. /scores pour le classement complet.");
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
    chat: Chat,
) {
    if !race.tick.is_multiple_of(HUD_PERIOD) {
        return;
    }
    for (pilot, kart) in &karts {
        chat.bar(pilot.0, race.status(kart, transfers.contains(pilot.0)));
    }
}

fn race_bar(
    race: Res<Race>,
    mut bar: Single<&mut BossBar, With<RaceBar>>,
    mut shown: Local<Option<(Phase, u64)>>,
) {
    let value = match race.phase {
        Phase::Generating | Phase::Destroying => race.construction() as u64,
        Phase::Countdown => race.countdown().div_ceil(20),
        Phase::Racing => race.time_limit().saturating_sub(race.elapsed()) / 20,
        Phase::Lobby | Phase::Loading | Phase::Results => {
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
        Phase::Generating => (
            format!("Construction du circuit : {value}%"),
            value as f32 / 100.0,
            BossBarColor::White,
        ),
        Phase::Destroying => (
            format!("Destruction du circuit : {value}%"),
            value as f32 / 100.0,
            BossBarColor::White,
        ),
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
        ClientId, LoadedChunks, MinecraftEntityId, PlayerDimension, PlayerReady, Position,
        Rotation, TeleportState,
    };
    use voidmc::network::{IncomingPacket, NetworkChannels, OutgoingPacket, PacketEvent};
    use voidmc::plugins::abilities::AbilitiesPlugin;
    use voidmc::plugins::boss_bar::BossBarPlugin;
    use voidmc::plugins::movement::MovementPlugin;
    use voidmc::plugins::teleport::TeleportPlugin;
    use voidmc::systems::entities::{broadcast_entity_movement, update_previous_entity_positions};
    use voidmc::world::{ChunkIndex, DimensionId};
    use voidmc::{EntityPlugin, Particle, Teleport};
    use voidmc_protocol::clientbound::{
        BossEventAction, ClientboundPacket, ManualPlayPacket, Parser, PlayPacket,
    };
    use voidmc_protocol::serverbound::{ConfirmTeleportation, Pong};

    use super::*;
    use crate::arena::WAIT_Y;
    use crate::kart::RESET_PENALTY;
    use crate::terrain::Alpine;

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
    }

    pub(crate) struct Harness {
        pub(crate) app: App,
        pub(crate) rx: Receiver<OutgoingPacket>,
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
            let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
            let (outgoing_tx, rx) = flume::unbounded::<OutgoingPacket>();
            let (disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
            let (kick_tx, kick_rx) = flume::unbounded::<u32>();
            let mut app = App::new();
            app.insert_resource(NetworkChannels {
                incoming: incoming_rx,
                outgoing: outgoing_tx,
                disconnect: disconnect_rx,
                kick: kick_tx,
            })
            .insert_non_send_resource((incoming_tx, disconnect_tx, kick_rx))
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
            let mut out = Vec::new();
            for _ in 0..8 {
                let transfers = self.transfers();
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
                        self.pong(player, *id);
                    }
                }
                out.extend(sent);
                let sent = self.drain();
                for o in &sent {
                    if let Out::Sync { client, id, .. } = o {
                        let player = self.player(*client);
                        self.confirm(player, *id);
                    }
                }
                out.extend(sent);
                self.tick();
                out.extend(self.drain());
            }
            assert!(self.transfers().is_empty());
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
        format!("{PREFIX}{text}")
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
        assert!(
            out.iter()
                .all(|o| matches!(o, Out::Chat { color, .. } if color == COLOR))
        );
        assert_eq!(
            texts(&out, 1),
            vec![
                chat("[+] Pilot1 rejoint Alpine Rush. Bienvenue !"),
                chat("Pilot1 prend un minecart (1/8 pilotes)."),
                chat(
                    "En vol au-dessus de la vallee ! /race genere un circuit et inscrit tous les pilotes presents."
                ),
                chat(
                    "Alpine Rush | /race [tours] : depart (1–20, defaut 3) | /leave : spectateur | /join : revenir"
                ),
                chat(
                    "Avancer : accelerer | Reculer : frein / marche arriere | Direction : gauche/droite | Saut : boost | Sprint : bonus | /reset : secours (+3s). F5 conseille."
                ),
            ]
        );
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
                chat("[+] Pilot2 rejoint Alpine Rush. Bienvenue !"),
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
        assert!(
            h.chats(9)
                .contains(&chat("La grille est pleine (8 pilotes)."))
        );
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
            ("race", "Start a race: /race [tours], 1–20 laps (default 3)"),
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
        for (args, expected) in [
            (vec![], Some(3)),
            (vec!["1"], Some(1)),
            (vec!["5"], Some(5)),
            (vec!["20"], Some(20)),
            (vec!["0"], None),
            (vec!["-1"], None),
            (vec!["21"], None),
            (vec!["abc"], None),
            (vec!["2", "3"], None),
        ] {
            let mut h = Harness::new(42);
            let a = h.connect(1);
            h.drain();
            h.command(a, "race", &args);
            let Some(laps) = expected else {
                assert_eq!(h.race().phase, Phase::Lobby);
                assert_eq!(h.race().round, 0);
                assert!(!h.chats(1).is_empty());
                continue;
            };
            assert_eq!(h.race().phase, Phase::Generating);
            assert_eq!((h.race().laps, h.race().round), (laps, 1));
            assert!(h.kart(a).participant);
            let seed = h.app.world().resource::<Track>().seed;
            assert_eq!(
                h.chats(1),
                vec![
                    chat(&format!(
                        "Pilot1 lance la manche 1 : {laps} tour(s) ! Nouveau trace : {:.0} m par tour | seed {seed}.",
                        h.app.world().resource::<Track>().length
                    )),
                    chat(&format!(
                        "Tous en vol ! La piste se construit dans le paysage. Depart apres l'assemblage ; {laps} tour(s), bonus et boost rechargeable."
                    )),
                ]
            );
            h.command(a, "race", &["2"]);
            assert_eq!(h.race().laps, laps);
            assert_eq!(h.chats(1), vec![chat("Une course est deja en cours.")]);
            assert!(h.race().time_limit() >= TIME_LIMIT / LAPS as u64 * laps as u64);
        }
    }

    #[test]
    fn race_requires_a_kart_and_launch_events_share_the_validation() {
        let mut h = Harness::new(42);
        let a = h.spawn(1);
        h.command(a, "race", &[]);
        assert_eq!(h.chats(1), vec![chat("Utilise /join avant /race.")]);
        h.world().trigger(Launch { player: a, laps: 0 });
        assert_eq!(
            h.chats(1),
            vec![chat("Utilisation : /race [tours], de 1 a 20 (defaut 3).")]
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
            vec![
                chat("Construction du circuit : 25%."),
                chat("Construction du circuit : 50%."),
                chat("Construction du circuit : 75%."),
                chat(
                    "Circuit pret ! Chargement de la grille chez chaque pilote avant le compte a rebours..."
                ),
                chat(
                    "Traversez les End Crystals pour un bonus, puis Sprint pour l'utiliser. Saut + Avancer = boost."
                ),
                chat("Tous les pilotes sont charges ! Depart dans 5 secondes !"),
            ]
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
            countdown.extend(h.chats(1));
        }
        assert_eq!(ticks, COUNTDOWN);
        assert_eq!(
            countdown,
            vec![
                chat("Depart dans 3..."),
                chat("Depart dans 2..."),
                chat("Depart dans 1..."),
                chat("GO ! 3 tour(s) — bonne course a tous !"),
            ]
        );

        h.kart_mut(a).next_gate = GATES + 1;
        h.tick();
        assert_eq!(h.chats(1), vec![chat("Tour 2/3 ! Garde le rythme !")]);
        h.tick();
        assert!(h.chats(1).is_empty());
        h.kart_mut(a).next_gate = 2 * GATES + 1;
        h.tick();
        assert_eq!(
            h.chats(1),
            vec![chat("Tour 3/3 ! Dernier tour, donne tout !")]
        );
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
        assert_eq!(
            h.chats(2),
            vec![
                chat(&format!("Pilot1 termine en {stamp}s ! /scores")),
                chat("Course terminee !"),
                chat(&format!("Podium #1 : Pilot1 — {stamp}s")),
                chat("Retour en vol : demontage de la piste. /scores pour le classement complet."),
            ]
        );

        let mut demolition = Vec::new();
        while h.race().phase == Phase::Destroying {
            h.tick();
            demolition.extend(h.chats(2));
        }
        assert_eq!(
            demolition,
            vec![
                chat("Demontage du circuit : 25%."),
                chat("Demontage du circuit : 50%."),
                chat("Demontage du circuit : 75%."),
                chat(
                    "Piste demontee, vallee restauree ! /scores : resultats | /race : nouveau circuit."
                ),
            ]
        );
        assert_eq!(h.race().phase, Phase::Results);
        h.command(a, "scores", &[]);
        assert_eq!(
            h.chats(1),
            vec![
                chat(&format!("1. Pilot1 — {stamp}s")),
                chat(&format!(
                    "Records du circuit actuel (seed {first_seed}, 3 tour(s)) :"
                )),
                chat(&format!("Pilot1 : {stamp}s")),
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
                h.chats(1),
                vec![chat(&format!(
                    "Il reste {seconds} secondes pour terminer !"
                ))]
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
            h.chats(1),
            vec![
                chat("Temps limite atteint !"),
                chat("Aucun pilote n'a termine cette manche."),
                chat("Retour en vol : demontage de la piste. /scores pour le classement complet."),
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
                chat("Mode spectateur en vol. /join pour revenir sur la grille."),
            ]
        );
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
                (
                    1,
                    chat("[-] Pilot2 quitte Alpine Rush. Abandon de la manche.")
                ),
                (
                    3,
                    chat("[-] Pilot2 quitte Alpine Rush. Abandon de la manche.")
                ),
            ]
        );
        h.tick();
        assert_eq!(h.race().phase, Phase::Racing);
        h.disconnect(a);
        assert_eq!(
            h.chats(3),
            vec![chat("[-] Pilot1 quitte Alpine Rush. Abandon de la manche.")]
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
            "Le nouveau circuit se construit. Tu participeras a la prochaine manche."
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
            "Demontage du circuit en cours. /race sera disponible juste apres."
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
        assert_eq!(
            h.chats(1),
            vec![chat(
                "Retour au dernier checkpoint : +3 secondes de penalite. Bonus retires."
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
        assert!(
            out.iter()
                .all(|o| !matches!(o, Out::Chat { overlay: true, color, .. } if color != COLOR))
        );
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
    fn boss_bars_are_pushed_only_when_their_content_changes() {
        let mut h = Harness::new(42);
        let a = h.connect(1);
        h.tick();
        h.drain();
        h.ticks(40);
        assert!(bars(&h.drain(), 1).is_empty());
        h.command(a, "race", &[]);
        h.tick();
        assert_eq!(
            bars(&h.drain(), 1),
            vec![(
                "add".into(),
                Some("Construction du circuit : 0%".into()),
                Some(0.0)
            )]
        );
        while h.race().phase == Phase::Generating {
            h.tick();
        }
        let out = h.drain();
        let updates = bars(&out, 1);
        assert!(
            updates[..updates.len() - 1]
                .iter()
                .all(|(action, _, _)| ["title", "progress", "style"].contains(&action.as_str())),
            "{updates:?}"
        );
        let titles = updates
            .iter()
            .filter(|(action, _, _)| action == "title")
            .count();
        assert!(titles <= h.race().tick as usize / 2);
        assert_eq!(updates[updates.len() - 1], ("remove".into(), None, None));
        h.ticks(4);
        assert!(bars(&h.drain(), 1).is_empty());
        let out = h.settle_transfers();
        assert_eq!(
            bars(&out, 1),
            vec![(
                "add".into(),
                Some("Depart dans 5...".into()),
                Some(1.0)
            )]
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
        assert_eq!(
            bars(&h.drain(), 1)
                .into_iter()
                .filter_map(|(_, title, _)| title)
                .collect::<Vec<_>>(),
            vec!["Destruction du circuit : 0%"]
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
