//! Scoreboards as entities, like boss bars: spawn an [`Objective`] or a
//! [`Team`], mutate it, despawn it. Each carries an [`Audience`]; a
//! `PostUpdate` system diffs the component against what its viewers last
//! received and sends only the changed scores, parameters or members.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use tracing::warn;
use ussr_nbt::owned::{Nbt, Tag};
use voidmc_protocol::clientbound::{
    ClientboundPacket, NumberFormat, ObjectiveAction, ObjectiveInfo, ResetScore,
    SetDisplayObjective, SetObjective, SetPlayerTeam, SetScore, TeamAction, TeamFlags,
    TeamParameters,
};

pub use voidmc_protocol::clientbound::{
    CollisionRule, DisplaySlot, NameTagVisibility, RenderType, TeamColor,
};

use crate::components::PlayerName;
use crate::messages::{TextColor, plain_text_component, text_component};
use crate::players::{Audience, Players, Recipients};
use crate::schedule::VoidSystems;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreFormat {
    Blank,
    Styled(TextColor),
    Fixed(String),
}

impl ScoreFormat {
    fn wire(&self) -> NumberFormat {
        match self {
            ScoreFormat::Blank => NumberFormat::Blank,
            ScoreFormat::Styled(color) => NumberFormat::Styled(Nbt {
                name: "".into(),
                compound: vec![("color".into(), Tag::String(color.to_string().into()))].into(),
            }),
            ScoreFormat::Fixed(text) => NumberFormat::Fixed(plain_text_component(text)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Score {
    pub value: i32,
    pub display: Option<String>,
    pub format: Option<ScoreFormat>,
}

impl From<i32> for Score {
    fn from(value: i32) -> Self {
        Score {
            value,
            ..Score::default()
        }
    }
}

#[derive(Component, Clone, Debug)]
#[require(ObjectiveState)]
pub struct Objective {
    pub name: String,
    pub slot: DisplaySlot,
    pub title: String,
    pub color: TextColor,
    pub render: RenderType,
    pub format: Option<ScoreFormat>,
    pub scores: BTreeMap<String, Score>,
    pub audience: Audience,
}

impl Objective {
    pub fn new(name: impl Into<String>, slot: DisplaySlot) -> Self {
        let name = name.into();
        Self {
            title: name.clone(),
            name,
            slot,
            color: TextColor::White,
            render: RenderType::Integer,
            format: None,
            scores: BTreeMap::new(),
            audience: Audience::All,
        }
    }

    pub fn sidebar(name: impl Into<String>) -> Self {
        Self::new(name, DisplaySlot::Sidebar)
    }

    pub fn list(name: impl Into<String>) -> Self {
        Self::new(name, DisplaySlot::List)
    }

    pub fn below_name(name: impl Into<String>) -> Self {
        Self::new(name, DisplaySlot::BelowName)
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = color;
        self
    }

    pub fn render(mut self, render: RenderType) -> Self {
        self.render = render;
        self
    }

    pub fn hearts(self) -> Self {
        self.render(RenderType::Hearts)
    }

    pub fn format(mut self, format: ScoreFormat) -> Self {
        self.format = Some(format);
        self
    }

    pub fn score(mut self, owner: impl Into<String>, score: impl Into<Score>) -> Self {
        self.scores.insert(owner.into(), score.into());
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    /// Keeps the owner's display name and format; use [`Objective::score_mut`]
    /// to change those.
    pub fn set(&mut self, owner: impl Into<String>, value: i32) {
        self.score_mut(owner).value = value;
    }

    pub fn score_mut(&mut self, owner: impl Into<String>) -> &mut Score {
        self.scores.entry(owner.into()).or_default()
    }

    pub fn get(&self, owner: &str) -> Option<i32> {
        self.scores.get(owner).map(|score| score.value)
    }

    pub fn remove(&mut self, owner: &str) -> Option<Score> {
        self.scores.remove(owner)
    }

    pub fn clear(&mut self) {
        self.scores.clear();
    }

    fn info(&self) -> ObjectiveInfo {
        ObjectiveInfo {
            display_name: text_component(&self.title, self.color),
            render_type: self.render,
            number_format: self.format.as_ref().map(ScoreFormat::wire),
        }
    }

    fn objective(&self, action: ObjectiveAction) -> SetObjective {
        SetObjective {
            name: self.name.clone(),
            action,
        }
    }

    fn display(&self, slot: DisplaySlot, shown: bool) -> SetDisplayObjective {
        SetDisplayObjective {
            slot,
            name: if shown {
                self.name.clone()
            } else {
                String::new()
            },
        }
    }

    fn set_score(&self, owner: &str, score: &Score) -> SetScore {
        SetScore {
            owner: owner.into(),
            objective: self.name.clone(),
            value: score.value,
            display_name: score.display.as_deref().map(plain_text_component),
            number_format: score.format.as_ref().map(ScoreFormat::wire),
        }
    }

    fn reset_score(&self, owner: &str) -> ResetScore {
        ResetScore {
            owner: owner.into(),
            objective: Some(self.name.clone()),
        }
    }

    fn snapshot(&self) -> ObjectiveSnapshot {
        ObjectiveSnapshot {
            slot: self.slot,
            title: self.title.clone(),
            color: self.color,
            render: self.render,
            format: self.format.clone(),
            scores: self.scores.clone(),
        }
    }

    fn full_state(&self) -> Vec<ClientboundPacket> {
        let mut packets = vec![self.objective(ObjectiveAction::Create(self.info())).into()];
        packets.extend(
            self.scores
                .iter()
                .map(|(owner, score)| self.set_score(owner, score).into()),
        );
        packets.push(self.display(self.slot, true).into());
        packets
    }

    fn updates_since(&self, sent: &ObjectiveSnapshot) -> Vec<ClientboundPacket> {
        let mut packets = Vec::new();
        if self.title != sent.title
            || self.color != sent.color
            || self.render != sent.render
            || self.format != sent.format
        {
            packets.push(self.objective(ObjectiveAction::Update(self.info())).into());
        }
        for (owner, score) in &self.scores {
            if sent.scores.get(owner) != Some(score) {
                packets.push(self.set_score(owner, score).into());
            }
        }
        for owner in sent.scores.keys() {
            if !self.scores.contains_key(owner) {
                packets.push(self.reset_score(owner).into());
            }
        }
        if self.slot != sent.slot {
            packets.push(self.display(sent.slot, false).into());
            packets.push(self.display(self.slot, true).into());
        }
        packets
    }
}

#[derive(Debug, Clone)]
struct ObjectiveSnapshot {
    slot: DisplaySlot,
    title: String,
    color: TextColor,
    render: RenderType,
    format: Option<ScoreFormat>,
    scores: BTreeMap<String, Score>,
}

#[derive(Component, Debug, Default)]
pub struct ObjectiveState {
    sync: SyncState<ObjectiveSnapshot>,
}

impl ObjectiveState {
    pub fn viewers(&self) -> impl Iterator<Item = Entity> + '_ {
        self.sync.viewers.iter().copied()
    }
}

#[derive(Component, Clone, Debug)]
#[require(TeamState)]
pub struct Team {
    pub name: String,
    pub display_name: String,
    pub color: TeamColor,
    pub prefix: String,
    pub suffix: String,
    pub friendly_fire: bool,
    pub see_invisible_friends: bool,
    pub name_tags: NameTagVisibility,
    pub collision: CollisionRule,
    pub players: HashSet<Entity>,
    pub entries: BTreeSet<String>,
    pub audience: Audience,
}

impl Team {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            display_name: name.clone(),
            name,
            color: TeamColor::Reset,
            prefix: String::new(),
            suffix: String::new(),
            friendly_fire: true,
            see_invisible_friends: true,
            name_tags: NameTagVisibility::Always,
            collision: CollisionRule::Always,
            players: HashSet::new(),
            entries: BTreeSet::new(),
            audience: Audience::All,
        }
    }

    pub fn display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = display_name.into();
        self
    }

    pub fn color(mut self, color: impl Into<TeamColor>) -> Self {
        self.color = color.into();
        self
    }

    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    pub fn friendly_fire(mut self, allowed: bool) -> Self {
        self.friendly_fire = allowed;
        self
    }

    pub fn see_invisible_friends(mut self, visible: bool) -> Self {
        self.see_invisible_friends = visible;
        self
    }

    pub fn name_tags(mut self, visibility: NameTagVisibility) -> Self {
        self.name_tags = visibility;
        self
    }

    pub fn collision(mut self, rule: CollisionRule) -> Self {
        self.collision = rule;
        self
    }

    pub fn members(mut self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.players.extend(players);
        self
    }

    pub fn entries(mut self, entries: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.entries.extend(entries.into_iter().map(Into::into));
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    pub fn add(&mut self, player: Entity) -> bool {
        self.players.insert(player)
    }

    pub fn remove(&mut self, player: Entity) -> bool {
        self.players.remove(&player)
    }

    pub fn contains(&self, player: Entity) -> bool {
        self.players.contains(&player)
    }

    /// Raw scoreboard entries: entity UUIDs or names of players not on this server.
    pub fn add_entry(&mut self, entry: impl Into<String>) -> bool {
        self.entries.insert(entry.into())
    }

    pub fn remove_entry(&mut self, entry: &str) -> bool {
        self.entries.remove(entry)
    }

    fn resolved_members(&self, names: &Query<&PlayerName>) -> BTreeSet<String> {
        let mut members = self.entries.clone();
        members.extend(
            self.players
                .iter()
                .filter_map(|player| names.get(*player).ok())
                .map(|name| name.0.clone()),
        );
        members
    }

    fn parameters(&self) -> TeamParameters {
        let mut flags = TeamFlags::empty();
        flags.set(TeamFlags::FRIENDLY_FIRE, self.friendly_fire);
        flags.set(TeamFlags::SEE_INVISIBLE_FRIENDS, self.see_invisible_friends);
        TeamParameters {
            display_name: plain_text_component(&self.display_name),
            flags,
            name_tag_visibility: self.name_tags,
            collision_rule: self.collision,
            color: self.color,
            prefix: plain_text_component(&self.prefix),
            suffix: plain_text_component(&self.suffix),
        }
    }

    fn packet(&self, action: TeamAction) -> SetPlayerTeam {
        SetPlayerTeam {
            name: self.name.clone(),
            action,
        }
    }

    fn snapshot(&self, members: BTreeSet<String>) -> TeamSnapshot {
        TeamSnapshot {
            display_name: self.display_name.clone(),
            color: self.color,
            prefix: self.prefix.clone(),
            suffix: self.suffix.clone(),
            friendly_fire: self.friendly_fire,
            see_invisible_friends: self.see_invisible_friends,
            name_tags: self.name_tags,
            collision: self.collision,
            members,
        }
    }

    fn parameters_changed(&self, sent: &TeamSnapshot) -> bool {
        self.display_name != sent.display_name
            || self.color != sent.color
            || self.prefix != sent.prefix
            || self.suffix != sent.suffix
            || self.friendly_fire != sent.friendly_fire
            || self.see_invisible_friends != sent.see_invisible_friends
            || self.name_tags != sent.name_tags
            || self.collision != sent.collision
    }

    fn full_state(&self, members: &BTreeSet<String>) -> Vec<ClientboundPacket> {
        vec![
            self.packet(TeamAction::Create {
                parameters: self.parameters(),
                entities: members.iter().cloned().collect(),
            })
            .into(),
        ]
    }

    fn updates_since(
        &self,
        sent: &TeamSnapshot,
        members: &BTreeSet<String>,
    ) -> Vec<ClientboundPacket> {
        let mut packets = Vec::new();
        if self.parameters_changed(sent) {
            packets.push(self.packet(TeamAction::Update(self.parameters())).into());
        }
        let joined: Vec<String> = members.difference(&sent.members).cloned().collect();
        if !joined.is_empty() {
            packets.push(self.packet(TeamAction::AddEntities(joined)).into());
        }
        let left: Vec<String> = sent.members.difference(members).cloned().collect();
        if !left.is_empty() {
            packets.push(self.packet(TeamAction::RemoveEntities(left)).into());
        }
        packets
    }
}

impl From<TextColor> for TeamColor {
    fn from(color: TextColor) -> Self {
        match color {
            TextColor::Black => TeamColor::Black,
            TextColor::DarkBlue => TeamColor::DarkBlue,
            TextColor::DarkGreen => TeamColor::DarkGreen,
            TextColor::DarkAqua => TeamColor::DarkAqua,
            TextColor::DarkRed => TeamColor::DarkRed,
            TextColor::DarkPurple => TeamColor::DarkPurple,
            TextColor::Gold => TeamColor::Gold,
            TextColor::Gray => TeamColor::Gray,
            TextColor::DarkGray => TeamColor::DarkGray,
            TextColor::Blue => TeamColor::Blue,
            TextColor::Green => TeamColor::Green,
            TextColor::Aqua => TeamColor::Aqua,
            TextColor::Red => TeamColor::Red,
            TextColor::LightPurple => TeamColor::LightPurple,
            TextColor::Yellow => TeamColor::Yellow,
            TextColor::White => TeamColor::White,
            TextColor::Rgb(_) => {
                warn!(%color, "teams only take the 16 named colours, using none");
                TeamColor::Reset
            }
        }
    }
}

#[derive(Debug, Clone)]
struct TeamSnapshot {
    display_name: String,
    color: TeamColor,
    prefix: String,
    suffix: String,
    friendly_fire: bool,
    see_invisible_friends: bool,
    name_tags: NameTagVisibility,
    collision: CollisionRule,
    members: BTreeSet<String>,
}

#[derive(Component, Debug, Default)]
pub struct TeamState {
    sync: SyncState<TeamSnapshot>,
}

impl TeamState {
    pub fn viewers(&self) -> impl Iterator<Item = Entity> + '_ {
        self.sync.viewers.iter().copied()
    }
}

#[derive(Debug)]
struct SyncState<S> {
    name: String,
    sent: Option<S>,
    viewers: HashSet<Entity>,
    pending: Option<HashSet<Entity>>,
    conflict_logged: bool,
}

impl<S> Default for SyncState<S> {
    fn default() -> Self {
        Self {
            name: String::new(),
            sent: None,
            viewers: HashSet::new(),
            pending: None,
            conflict_logged: false,
        }
    }
}

impl<S> SyncState<S> {
    fn desired(&self, ready: &Recipients, audience: &Audience) -> Option<HashSet<Entity>> {
        let mut count = 0;
        let same = ready.iter().filter(|r| audience.includes(r)).all(|r| {
            count += 1;
            self.viewers.contains(&r.entity())
        }) && count == self.viewers.len();
        (!same).then(|| {
            ready
                .iter()
                .filter(|r| audience.includes(r))
                .map(|r| r.entity())
                .collect()
        })
    }

    fn kept(&self) -> impl Iterator<Item = Entity> + '_ {
        self.viewers
            .iter()
            .copied()
            .filter(|viewer| self.pending.as_ref().is_none_or(|p| p.contains(viewer)))
    }
}

/// Which entity owns each `(viewer, name)` pair, so two objectives (or teams)
/// with the same name never reach one client. Names are per-client on the
/// wire, so disjoint audiences may share one.
#[derive(Resource, Default)]
struct Claims {
    objectives: HashMap<(Entity, String), Entity>,
    teams: HashMap<(Entity, String), Entity>,
}

fn release(
    claims: &mut HashMap<(Entity, String), Entity>,
    viewer: Entity,
    name: &str,
    owner: Entity,
) {
    if let Entry::Occupied(entry) = claims.entry((viewer, name.to_string()))
        && *entry.get() == owner
    {
        entry.remove();
    }
}

fn claim(
    claims: &mut HashMap<(Entity, String), Entity>,
    viewer: Entity,
    name: &str,
    owner: Entity,
) -> Result<(), Entity> {
    match claims.entry((viewer, name.to_string())) {
        Entry::Occupied(entry) if *entry.get() != owner => Err(*entry.get()),
        Entry::Occupied(_) => Ok(()),
        Entry::Vacant(entry) => {
            entry.insert(owner);
            Ok(())
        }
    }
}

pub struct ScoreboardPlugin;

impl Plugin for ScoreboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Claims>()
            .add_observer(remove_objective_from_viewers)
            .add_observer(remove_team_from_viewers)
            .add_observer(forget_player_in_teams)
            .add_systems(
                PostUpdate,
                (sync_objectives, sync_teams)
                    .chain()
                    .in_set(VoidSystems::ScoreboardSync),
            );
    }
}

fn begin_sync<S>(
    players: &Players,
    ready: &Recipients,
    claims: &mut HashMap<(Entity, String), Entity>,
    owner: Entity,
    name: &str,
    audience: &Audience,
    state: &mut SyncState<S>,
    remove: impl Fn(&str) -> ClientboundPacket,
) {
    if state.sent.is_some() && state.name != name {
        players.send_to(state.viewers.iter().copied(), remove(&state.name));
        for viewer in state.viewers.drain() {
            release(claims, viewer, &state.name, owner);
        }
        state.sent = None;
    }
    state.name = name.to_string();
    state.pending = state.desired(ready, audience);
    if let Some(pending) = &state.pending {
        for gone in state.viewers.difference(pending) {
            players.send(*gone, remove(name));
            release(claims, *gone, name, owner);
        }
    }
}

fn finish_sync<S>(
    players: &Players,
    claims: &mut HashMap<(Entity, String), Entity>,
    owner: Entity,
    kind: &str,
    state: &mut SyncState<S>,
    full_state: impl Fn() -> Vec<ClientboundPacket>,
) {
    let Some(pending) = state.pending.take() else {
        return;
    };
    let mut viewers = HashSet::with_capacity(pending.len());
    let mut packets = None;
    for viewer in pending {
        if state.viewers.contains(&viewer) {
            viewers.insert(viewer);
            continue;
        }
        match claim(claims, viewer, &state.name, owner) {
            Ok(()) => {
                for packet in packets.get_or_insert_with(&full_state) {
                    players.send(viewer, packet.clone());
                }
                viewers.insert(viewer);
            }
            Err(other) => {
                if !state.conflict_logged {
                    state.conflict_logged = true;
                    warn!(
                        name = %state.name,
                        entity = ?owner,
                        conflicts_with = ?other,
                        ?viewer,
                        "{kind} name already shown to this player by another entity; not sent"
                    );
                }
            }
        }
    }
    state.viewers = viewers;
}

fn sync_objectives(
    players: Players,
    mut claims: ResMut<Claims>,
    mut objectives: Query<(Entity, Ref<Objective>, &mut ObjectiveState)>,
) {
    let ready = players.ready();
    for (entity, objective, mut state) in objectives.iter_mut() {
        begin_sync(
            &players,
            &ready,
            &mut claims.objectives,
            entity,
            &objective.name,
            &objective.audience,
            &mut state.sync,
            |name| {
                SetObjective {
                    name: name.into(),
                    action: ObjectiveAction::Remove,
                }
                .into()
            },
        );
    }
    for (entity, objective, mut state) in objectives.iter_mut() {
        let state = &mut state.sync;
        match &state.sent {
            Some(sent) if objective.is_changed() => {
                let packets = objective.updates_since(sent);
                if !packets.is_empty() {
                    let kept: Vec<Entity> = state.kept().collect();
                    for packet in packets {
                        players.send_to(kept.iter().copied(), packet);
                    }
                    state.sent = Some(objective.snapshot());
                }
            }
            Some(_) => {}
            None => state.sent = Some(objective.snapshot()),
        }
        finish_sync(
            &players,
            &mut claims.objectives,
            entity,
            "objective",
            state,
            || objective.full_state(),
        );
    }
}

fn sync_teams(
    players: Players,
    mut claims: ResMut<Claims>,
    names: Query<&PlayerName>,
    mut teams: Query<(Entity, Ref<Team>, &mut TeamState)>,
) {
    let ready = players.ready();
    for (entity, team, mut state) in teams.iter_mut() {
        begin_sync(
            &players,
            &ready,
            &mut claims.teams,
            entity,
            &team.name,
            &team.audience,
            &mut state.sync,
            |name| {
                SetPlayerTeam {
                    name: name.into(),
                    action: TeamAction::Remove,
                }
                .into()
            },
        );
    }
    for (entity, team, mut state) in teams.iter_mut() {
        let state = &mut state.sync;
        let mut members = None;
        match &state.sent {
            Some(sent) if team.is_changed() => {
                let resolved = members.insert(team.resolved_members(&names));
                let packets = team.updates_since(sent, resolved);
                if !packets.is_empty() {
                    let kept: Vec<Entity> = state.kept().collect();
                    for packet in packets {
                        players.send_to(kept.iter().copied(), packet);
                    }
                    state.sent = Some(team.snapshot(resolved.clone()));
                }
            }
            Some(_) => {}
            None => {
                let resolved = members.insert(team.resolved_members(&names));
                state.sent = Some(team.snapshot(resolved.clone()));
            }
        }
        if state.pending.is_none() {
            continue;
        }
        let members = members.unwrap_or_else(|| {
            state
                .sent
                .as_ref()
                .map(|sent| sent.members.clone())
                .unwrap_or_default()
        });
        finish_sync(&players, &mut claims.teams, entity, "team", state, || {
            team.full_state(&members)
        });
    }
}

fn remove_objective_from_viewers(
    event: On<Remove, Objective>,
    players: Players,
    mut claims: ResMut<Claims>,
    mut objectives: Query<&mut ObjectiveState>,
) {
    let Ok(mut state) = objectives.get_mut(event.entity) else {
        return;
    };
    let state = &mut state.sync;
    for viewer in state.viewers.drain() {
        players.send(
            viewer,
            SetObjective {
                name: state.name.clone(),
                action: ObjectiveAction::Remove,
            },
        );
        release(&mut claims.objectives, viewer, &state.name, event.entity);
    }
    state.sent = None;
}

fn remove_team_from_viewers(
    event: On<Remove, Team>,
    players: Players,
    mut claims: ResMut<Claims>,
    mut teams: Query<&mut TeamState>,
) {
    let Ok(mut state) = teams.get_mut(event.entity) else {
        return;
    };
    let state = &mut state.sync;
    for viewer in state.viewers.drain() {
        players.send(
            viewer,
            SetPlayerTeam {
                name: state.name.clone(),
                action: TeamAction::Remove,
            },
        );
        release(&mut claims.teams, viewer, &state.name, event.entity);
    }
    state.sent = None;
}

fn forget_player_in_teams(event: On<Remove, PlayerName>, mut teams: Query<&mut Team>) {
    for mut team in teams.iter_mut() {
        if team
            .bypass_change_detection()
            .players
            .contains(&event.entity)
        {
            team.players.remove(&event.entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use flume::Receiver;
    use voidmc_protocol::clientbound::{PlayPacket, TeamColor};

    use super::*;
    use crate::components::{ClientId, PlayerDimension, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::world::DimensionId;

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
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
        .add_plugins(ScoreboardPlugin);
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32, name: &str) -> Entity {
        app.world_mut()
            .spawn((ClientId(id), PlayerReady, PlayerName(name.into())))
            .id()
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Sent {
        Create(u32, String),
        Update(u32, String),
        RemoveObjective(u32, String),
        Display(u32, DisplaySlot, String),
        Score(u32, String, i32),
        Reset(u32, String),
        TeamCreate(u32, String, Vec<String>),
        TeamRemove(u32, String),
        TeamUpdate(u32, String),
        TeamAdd(u32, String, Vec<String>),
        TeamLeave(u32, String, Vec<String>),
    }

    fn drain(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        rx.try_iter()
            .map(|out| {
                let id = out.client_id;
                let ClientboundPacket::Play(packet) = out.packet else {
                    panic!("unexpected packet {:?}", out.packet);
                };
                match packet {
                    PlayPacket::SetObjective(p) => match p.action {
                        ObjectiveAction::Create(_) => Sent::Create(id, p.name),
                        ObjectiveAction::Update(_) => Sent::Update(id, p.name),
                        ObjectiveAction::Remove => Sent::RemoveObjective(id, p.name),
                    },
                    PlayPacket::SetDisplayObjective(p) => Sent::Display(id, p.slot, p.name),
                    PlayPacket::SetScore(p) => Sent::Score(id, p.owner, p.value),
                    PlayPacket::ResetScore(p) => Sent::Reset(id, p.owner),
                    PlayPacket::SetPlayerTeam(p) => match p.action {
                        TeamAction::Create { entities, .. } => {
                            Sent::TeamCreate(id, p.name, entities)
                        }
                        TeamAction::Remove => Sent::TeamRemove(id, p.name),
                        TeamAction::Update(_) => Sent::TeamUpdate(id, p.name),
                        TeamAction::AddEntities(e) => Sent::TeamAdd(id, p.name, e),
                        TeamAction::RemoveEntities(e) => Sent::TeamLeave(id, p.name, e),
                    },
                    other => panic!("unexpected packet {other:?}"),
                }
            })
            .collect()
    }

    fn sorted(rx: &Receiver<OutgoingPacket>) -> Vec<Sent> {
        let mut sent = drain(rx);
        sent.sort();
        sent
    }

    fn create(id: u32) -> Sent {
        Sent::Create(id, "race".into())
    }

    fn display(id: u32) -> Sent {
        Sent::Display(id, DisplaySlot::Sidebar, "race".into())
    }

    fn score(id: u32, owner: &str, value: i32) -> Sent {
        Sent::Score(id, owner.into(), value)
    }

    #[test]
    fn builders_fill_every_field() {
        let objective = Objective::sidebar("race")
            .title("Alpine Rush")
            .color(TextColor::Gold)
            .hearts()
            .format(ScoreFormat::Blank)
            .score("Leo", 3)
            .score(
                "Adam",
                Score {
                    value: 1,
                    display: Some("A".into()),
                    format: Some(ScoreFormat::Fixed("1st".into())),
                },
            );
        assert_eq!(objective.name, "race");
        assert_eq!(objective.title, "Alpine Rush");
        assert_eq!(objective.slot, DisplaySlot::Sidebar);
        assert_eq!(objective.color, TextColor::Gold);
        assert_eq!(objective.render, RenderType::Hearts);
        assert_eq!(objective.format, Some(ScoreFormat::Blank));
        assert_eq!(objective.get("Leo"), Some(3));
        assert_eq!(objective.scores["Adam"].display.as_deref(), Some("A"));
        assert_eq!(Objective::list("x").slot, DisplaySlot::List);
        assert_eq!(Objective::below_name("x").slot, DisplaySlot::BelowName);
        assert_eq!(Objective::below_name("x").title, "x");
        assert!(matches!(
            Objective::sidebar("x")
                .viewers([Entity::PLACEHOLDER])
                .audience,
            Audience::Explicit(_)
        ));

        let team = Team::new("red")
            .display_name("Red Team")
            .color(TextColor::Red)
            .prefix("[R] ")
            .suffix(" ✦")
            .friendly_fire(false)
            .see_invisible_friends(false)
            .name_tags(NameTagVisibility::HideForOtherTeams)
            .collision(CollisionRule::Never)
            .members([Entity::PLACEHOLDER])
            .entries(["Steve"]);
        assert_eq!(team.display_name, "Red Team");
        assert_eq!(team.color, TeamColor::Red);
        assert_eq!(team.prefix, "[R] ");
        assert_eq!(team.suffix, " ✦");
        assert!(!team.friendly_fire && !team.see_invisible_friends);
        assert_eq!(team.name_tags, NameTagVisibility::HideForOtherTeams);
        assert_eq!(team.collision, CollisionRule::Never);
        assert!(team.contains(Entity::PLACEHOLDER));
        assert!(team.entries.contains("Steve"));
        assert_eq!(Team::new("x").color, TeamColor::Reset);
        assert_eq!(
            Team::new("x").color(TextColor::rgb(0x123456)).color,
            TeamColor::Reset
        );
        assert_eq!(Team::new("x").color(TeamColor::Bold).color, TeamColor::Bold);
        assert_eq!(
            Team::new("x").parameters().flags,
            TeamFlags::FRIENDLY_FIRE | TeamFlags::SEE_INVISIBLE_FRIENDS
        );
        assert_eq!(team.parameters().flags, TeamFlags::empty());
    }

    #[test]
    fn score_formats_reach_the_wire_as_number_formats() {
        assert_eq!(ScoreFormat::Blank.wire(), NumberFormat::Blank);
        assert_eq!(
            ScoreFormat::Fixed("★".into()).wire(),
            NumberFormat::Fixed(plain_text_component("★"))
        );
        let NumberFormat::Styled(style) = ScoreFormat::Styled(TextColor::Red).wire() else {
            panic!("styled");
        };
        assert_eq!(style.compound.tags.len(), 1);
        assert_eq!(style.compound.tags[0].0.to_string(), "color");
        assert!(matches!(&style.compound.tags[0].1, Tag::String(s) if s.to_string() == "red"));

        let objective = Objective::sidebar("race").title("😀".repeat(11000));
        crate::messages::assert_guarded(&objective.info().display_name, &objective.title);
        let team = Team::new("t").prefix("😀".repeat(11000));
        crate::messages::assert_guarded(&team.parameters().prefix, &team.prefix);
    }

    #[test]
    fn objective_is_created_in_full_then_stays_quiet() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        player(&mut app, 2, "Adam");
        app.world_mut()
            .spawn(Objective::sidebar("race").score("Leo", 3).score("Adam", 1));

        app.update();
        let sent = drain(&rx);
        let for_one: Vec<&Sent> = sent
            .iter()
            .filter(|s| {
                matches!(
                    s,
                    Sent::Create(1, _) | Sent::Score(1, ..) | Sent::Display(1, ..)
                )
            })
            .collect();
        assert_eq!(
            for_one,
            vec![
                &create(1),
                &score(1, "Adam", 1),
                &score(1, "Leo", 3),
                &display(1)
            ]
        );
        assert_eq!(sent.len(), 8);

        app.update();
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn changing_one_score_sends_one_packet() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let objective = app
            .world_mut()
            .spawn(Objective::sidebar("race").score("Leo", 3).score("Adam", 1))
            .id();
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<Objective>(objective)
            .unwrap()
            .set("Leo", 4);
        app.update();
        assert_eq!(drain(&rx), vec![score(1, "Leo", 4)]);

        app.world_mut()
            .get_mut::<Objective>(objective)
            .unwrap()
            .set("Leo", 4);
        app.update();
        assert!(drain(&rx).is_empty());

        {
            let mut objective = app.world_mut().get_mut::<Objective>(objective).unwrap();
            objective.remove("Adam");
            objective.set("Zoe", 9);
            objective.score_mut("Leo").display = Some("Léo".into());
        }
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                score(1, "Leo", 4),
                score(1, "Zoe", 9),
                Sent::Reset(1, "Adam".into())
            ]
        );

        app.world_mut()
            .get_mut::<Objective>(objective)
            .unwrap()
            .clear();
        app.update();
        assert_eq!(
            sorted(&rx),
            vec![Sent::Reset(1, "Leo".into()), Sent::Reset(1, "Zoe".into())]
        );
    }

    #[test]
    fn header_and_slot_changes_send_update_and_display_packets() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let objective = app.world_mut().spawn(Objective::sidebar("race")).id();
        app.update();
        drain(&rx);

        {
            let mut objective = app.world_mut().get_mut::<Objective>(objective).unwrap();
            objective.title = "Lap 2".into();
            objective.format = Some(ScoreFormat::Blank);
        }
        app.update();
        assert_eq!(drain(&rx), vec![Sent::Update(1, "race".into())]);

        app.world_mut()
            .get_mut::<Objective>(objective)
            .unwrap()
            .slot = DisplaySlot::List;
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Display(1, DisplaySlot::Sidebar, "".into()),
                Sent::Display(1, DisplaySlot::List, "race".into())
            ]
        );
    }

    #[test]
    fn renaming_removes_the_old_objective_and_creates_the_new_one() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let objective = app
            .world_mut()
            .spawn(Objective::sidebar("race").score("Leo", 1))
            .id();
        app.update();
        drain(&rx);

        app.world_mut()
            .get_mut::<Objective>(objective)
            .unwrap()
            .name = "lap".into();
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::RemoveObjective(1, "race".into()),
                Sent::Create(1, "lap".into()),
                score(1, "Leo", 1),
                Sent::Display(1, DisplaySlot::Sidebar, "lap".into())
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn late_joiners_get_the_full_state_and_leavers_a_remove() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1, "Leo");
        let objective = app
            .world_mut()
            .spawn(Objective::sidebar("race").score("Leo", 1))
            .id();
        app.update();
        drain(&rx);

        let second = player(&mut app, 2, "Adam");
        app.world_mut()
            .get_mut::<Objective>(objective)
            .unwrap()
            .set("Leo", 2);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                score(1, "Leo", 2),
                create(2),
                score(2, "Leo", 2),
                display(2)
            ]
        );

        app.world_mut().entity_mut(first).remove::<PlayerReady>();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::RemoveObjective(1, "race".into())]);

        app.world_mut().despawn(second);
        app.update();
        assert!(drain(&rx).is_empty());
        let state = app.world().get::<ObjectiveState>(objective).unwrap();
        assert_eq!(state.viewers().count(), 0);
        assert!(app.world().resource::<Claims>().objectives.is_empty());
    }

    #[test]
    fn despawn_and_component_removal_send_remove() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        player(&mut app, 2, "Adam");
        let despawned = app.world_mut().spawn(Objective::sidebar("race")).id();
        let stripped = app.world_mut().spawn(Team::new("red")).id();
        app.update();
        drain(&rx);

        app.world_mut().despawn(despawned);
        assert_eq!(
            sorted(&rx),
            vec![
                Sent::RemoveObjective(1, "race".into()),
                Sent::RemoveObjective(2, "race".into())
            ]
        );

        app.world_mut().entity_mut(stripped).remove::<Team>();
        assert_eq!(
            sorted(&rx),
            vec![
                Sent::TeamRemove(1, "red".into()),
                Sent::TeamRemove(2, "red".into())
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());
        let claims = app.world().resource::<Claims>();
        assert!(claims.objectives.is_empty() && claims.teams.is_empty());
    }

    #[test]
    fn audience_changes_add_and_remove_viewers() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1, "Leo");
        let second = player(&mut app, 2, "Adam");
        app.world_mut().spawn((
            ClientId(3),
            PlayerReady,
            PlayerDimension(DimensionId::Nether),
        ));
        let objective = app
            .world_mut()
            .spawn(Objective::sidebar("race").viewers([first]))
            .id();
        app.update();
        assert_eq!(drain(&rx), vec![create(1), display(1)]);

        app.world_mut()
            .get_mut::<Objective>(objective)
            .unwrap()
            .audience = Audience::explicit([first, second]);
        app.update();
        assert_eq!(drain(&rx), vec![create(2), display(2)]);

        {
            let mut objective = app.world_mut().get_mut::<Objective>(objective).unwrap();
            objective.audience = Audience::explicit([second]);
            objective.set("Leo", 1);
        }
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::RemoveObjective(1, "race".into()), score(2, "Leo", 1)]
        );

        app.world_mut()
            .get_mut::<Objective>(objective)
            .unwrap()
            .audience = Audience::InDimension(DimensionId::Nether);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::RemoveObjective(2, "race".into()),
                create(3),
                score(3, "Leo", 1),
                display(3)
            ]
        );
    }

    #[test]
    fn same_name_for_disjoint_audiences_is_fine_but_one_client_gets_it_once() {
        let (mut app, rx) = test_app();
        let first = player(&mut app, 1, "Leo");
        let second = player(&mut app, 2, "Adam");
        let owned = app
            .world_mut()
            .spawn(Objective::sidebar("sidebar").viewers([first]))
            .id();
        app.world_mut()
            .spawn(Objective::sidebar("sidebar").viewers([second]));
        app.update();
        assert_eq!(
            sorted(&rx),
            vec![
                Sent::Create(1, "sidebar".into()),
                Sent::Create(2, "sidebar".into()),
                Sent::Display(1, DisplaySlot::Sidebar, "sidebar".into()),
                Sent::Display(2, DisplaySlot::Sidebar, "sidebar".into()),
            ]
        );

        let duplicate = app
            .world_mut()
            .spawn(Objective::sidebar("sidebar").viewers([first, second]))
            .id();
        app.update();
        app.update();
        assert!(drain(&rx).is_empty());
        assert_eq!(
            app.world()
                .get::<ObjectiveState>(duplicate)
                .unwrap()
                .viewers()
                .count(),
            0
        );

        app.world_mut().despawn(owned);
        drain(&rx);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![
                Sent::Create(1, "sidebar".into()),
                Sent::Display(1, DisplaySlot::Sidebar, "sidebar".into())
            ]
        );
        assert_eq!(
            app.world()
                .get::<ObjectiveState>(duplicate)
                .unwrap()
                .viewers()
                .collect::<Vec<_>>(),
            vec![first]
        );
    }

    #[test]
    fn team_is_created_with_resolved_members_then_diffs_membership() {
        let (mut app, rx) = test_app();
        let leo = player(&mut app, 1, "Leo");
        let adam = player(&mut app, 2, "Adam");
        let team = app
            .world_mut()
            .spawn(Team::new("red").members([leo]).entries(["Steve"]))
            .id();
        app.update();
        assert_eq!(
            sorted(&rx),
            vec![
                Sent::TeamCreate(1, "red".into(), vec!["Leo".into(), "Steve".into()]),
                Sent::TeamCreate(2, "red".into(), vec!["Leo".into(), "Steve".into()]),
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());

        app.world_mut().get_mut::<Team>(team).unwrap().add(adam);
        app.update();
        assert_eq!(
            sorted(&rx),
            vec![
                Sent::TeamAdd(1, "red".into(), vec!["Adam".into()]),
                Sent::TeamAdd(2, "red".into(), vec!["Adam".into()]),
            ]
        );

        {
            let mut team = app.world_mut().get_mut::<Team>(team).unwrap();
            team.remove_entry("Steve");
            team.prefix = "[R] ".into();
        }
        app.update();
        assert_eq!(
            sorted(&rx),
            vec![
                Sent::TeamUpdate(1, "red".into()),
                Sent::TeamUpdate(2, "red".into()),
                Sent::TeamLeave(1, "red".into(), vec!["Steve".into()]),
                Sent::TeamLeave(2, "red".into(), vec!["Steve".into()]),
            ]
        );

        assert!(app.world_mut().get_mut::<Team>(team).unwrap().remove(leo));
        app.update();
        assert_eq!(
            sorted(&rx),
            vec![
                Sent::TeamLeave(1, "red".into(), vec!["Leo".into()]),
                Sent::TeamLeave(2, "red".into(), vec!["Leo".into()]),
            ]
        );
    }

    #[test]
    fn despawned_players_leave_their_teams() {
        let (mut app, rx) = test_app();
        let leo = player(&mut app, 1, "Leo");
        let adam = player(&mut app, 2, "Adam");
        let team = app
            .world_mut()
            .spawn(Team::new("red").members([leo, adam]).viewers([leo]))
            .id();
        app.update();
        drain(&rx);

        app.world_mut().despawn(adam);
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::TeamLeave(1, "red".into(), vec!["Adam".into()])]
        );
        assert!(!app.world().get::<Team>(team).unwrap().contains(adam));
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn late_joining_viewer_gets_current_team_members() {
        let (mut app, rx) = test_app();
        let leo = player(&mut app, 1, "Leo");
        let team = app.world_mut().spawn(Team::new("red").members([leo])).id();
        app.update();
        drain(&rx);

        let adam = player(&mut app, 2, "Adam");
        app.update();
        assert_eq!(
            drain(&rx),
            vec![Sent::TeamCreate(2, "red".into(), vec!["Leo".into()])]
        );

        app.world_mut().get_mut::<Team>(team).unwrap().add(adam);
        let zoe = player(&mut app, 3, "Zoe");
        app.update();
        assert_eq!(
            sorted(&rx),
            vec![
                Sent::TeamCreate(3, "red".into(), vec!["Adam".into(), "Leo".into()]),
                Sent::TeamAdd(1, "red".into(), vec!["Adam".into()]),
                Sent::TeamAdd(2, "red".into(), vec!["Adam".into()]),
            ]
        );
        let _ = zoe;
    }

    #[test]
    fn duplicate_team_names_on_one_client_are_skipped() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        app.world_mut().spawn(Team::new("red"));
        let duplicate = app.world_mut().spawn(Team::new("red")).id();
        app.update();
        assert_eq!(drain(&rx), vec![Sent::TeamCreate(1, "red".into(), vec![])]);
        assert_eq!(
            app.world()
                .get::<TeamState>(duplicate)
                .unwrap()
                .viewers()
                .count(),
            0
        );
    }
}
