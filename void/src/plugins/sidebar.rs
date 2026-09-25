//! A widget-based sidebar on top of [`Objective`]: spawn a [`Sidebar`] with a
//! title and a list of [`Widget`]s, mutate the widgets, despawn it. A
//! `PostUpdate` system (`VoidSystems::SidebarSync`) renders the widgets into
//! lines and writes them into the entity's managed sidebar objective, so the
//! scoreboard sync only sends the lines whose text changed.

use std::collections::BTreeMap;
use std::ops::{Index, IndexMut};
use std::sync::Arc;
use std::time::Duration;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::prelude::*;
use tracing::warn;
use voidmc_protocol::clientbound::DisplaySlot;

use crate::messages::TextColor;
use crate::players::Audience;
use crate::plugins::scoreboard::{Objective, Score, ScoreFormat};
use crate::schedule::VoidSystems;

pub const MAX_SIDEBAR_LINES: usize = 15;
pub const MAX_LINE_CHARS: usize = 40;
pub const SIDEBAR_OBJECTIVE: &str = "sidebar";

const SEPARATOR_WIDTH: usize = 20;
const LEGACY_PREFIX: char = '§';

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Ornament {
    #[default]
    None,
    Brackets(String),
    Line,
}

impl Ornament {
    pub fn brackets(glyph: impl Into<String>) -> Self {
        Ornament::Brackets(glyph.into())
    }

    fn apply(&self, title: &str) -> String {
        match self {
            Ornament::None => title.to_string(),
            Ornament::Brackets(glyph) => format!("{glyph} {title} {glyph}"),
            Ornament::Line => format!("─ {title} ─"),
        }
    }
}

fn named_or_white(color: TextColor) -> TextColor {
    if color.legacy_code().is_some() {
        color
    } else {
        warn!(%color, "sidebar lines take the 16 named colours, using white");
        TextColor::White
    }
}

fn paint(out: &mut String, color: TextColor) {
    if let Some(code) = color.legacy_code() {
        out.push(LEGACY_PREFIX);
        out.push(code);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    pub color: Option<TextColor>,
}

impl Text {
    pub fn color(mut self, color: TextColor) -> Self {
        self.color = Some(named_or_white(color));
        self
    }

    fn render(&self, out: &mut Vec<String>) {
        let mut line = String::new();
        if let Some(color) = self.color {
            paint(&mut line, color);
        }
        line.push_str(&self.text);
        out.push(line);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labeled {
    pub label: String,
    pub value: String,
    pub color: TextColor,
}

impl Labeled {
    pub fn color(mut self, color: TextColor) -> Self {
        self.color = named_or_white(color);
        self
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
    }

    fn render(&self, out: &mut Vec<String>) {
        let mut line = String::new();
        paint(&mut line, TextColor::White);
        line.push_str(&self.label);
        line.push_str(": ");
        paint(&mut line, self.color);
        line.push_str(&self.value);
        out.push(line);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranking {
    pub entries: Vec<(String, i32)>,
    pub limit: usize,
    pub highlight: Option<String>,
    pub color: TextColor,
}

impl Default for Ranking {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            limit: 5,
            highlight: None,
            color: TextColor::Yellow,
        }
    }
}

impl Ranking {
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn highlight(mut self, name: impl Into<String>) -> Self {
        self.highlight = Some(name.into());
        self
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = named_or_white(color);
        self
    }

    pub fn entries(mut self, entries: impl IntoIterator<Item = (impl Into<String>, i32)>) -> Self {
        self.set(entries);
        self
    }

    pub fn set(&mut self, entries: impl IntoIterator<Item = (impl Into<String>, i32)>) {
        self.entries = entries
            .into_iter()
            .map(|(name, score)| (name.into(), score))
            .collect();
    }

    pub fn set_highlight(&mut self, name: Option<impl Into<String>>) {
        self.highlight = name.map(Into::into);
    }

    fn render(&self, out: &mut Vec<String>) {
        let mut ranked: Vec<&(String, i32)> = self.entries.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (index, (name, score)) in ranked.into_iter().take(self.limit).enumerate() {
            let mut line = String::new();
            if self.highlight.as_deref() == Some(name.as_str()) {
                paint(&mut line, self.color);
                line.push_str(&format!("{}. {name} {score}", index + 1));
            } else {
                paint(&mut line, TextColor::Gray);
                line.push_str(&format!("{}. ", index + 1));
                paint(&mut line, TextColor::White);
                line.push_str(name);
                line.push(' ');
                paint(&mut line, TextColor::Gray);
                line.push_str(&score.to_string());
            }
            out.push(line);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timer {
    pub seconds: u64,
    pub label: Option<String>,
    pub color: TextColor,
}

impl Timer {
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = named_or_white(color);
        self
    }

    pub fn set(&mut self, elapsed: Duration) {
        self.seconds = elapsed.as_secs();
    }

    pub fn set_ticks(&mut self, ticks: u64) {
        self.seconds = ticks / 20;
    }

    pub fn clock(&self) -> String {
        let (hours, minutes, seconds) = (
            self.seconds / 3600,
            self.seconds / 60 % 60,
            self.seconds % 60,
        );
        if hours > 0 {
            format!("{hours}:{minutes:02}:{seconds:02}")
        } else {
            format!("{minutes:02}:{seconds:02}")
        }
    }

    fn render(&self, out: &mut Vec<String>) {
        let mut line = String::new();
        if let Some(label) = &self.label {
            paint(&mut line, TextColor::White);
            line.push_str(label);
            line.push_str(": ");
        }
        paint(&mut line, self.color);
        line.push_str(&self.clock());
        out.push(line);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub value: u32,
    pub max: u32,
    pub width: usize,
    pub label: Option<String>,
    pub color: TextColor,
}

impl Progress {
    pub fn width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = named_or_white(color);
        self
    }

    pub fn set(&mut self, value: u32) {
        self.value = value;
    }

    pub fn set_max(&mut self, max: u32) {
        self.max = max;
    }

    fn filled(&self) -> usize {
        if self.max == 0 {
            return 0;
        }
        let value = self.value.min(self.max) as u64;
        (value * self.width as u64 / self.max as u64) as usize
    }

    fn render(&self, out: &mut Vec<String>) {
        let mut line = String::new();
        if let Some(label) = &self.label {
            paint(&mut line, TextColor::White);
            line.push_str(label);
            line.push(' ');
        }
        let filled = self.filled();
        paint(&mut line, self.color);
        line.extend(std::iter::repeat_n('▮', filled));
        paint(&mut line, TextColor::DarkGray);
        line.extend(std::iter::repeat_n('▯', self.width - filled));
        out.push(line);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Widget {
    Text(Text),
    Labeled(Labeled),
    Blank,
    Separator(char),
    Ranking(Ranking),
    Timer(Timer),
    Progress(Progress),
}

impl Widget {
    pub fn text(text: impl Into<String>) -> Text {
        Text {
            text: text.into(),
            color: None,
        }
    }

    pub fn labeled(label: impl Into<String>, value: impl Into<String>) -> Labeled {
        Labeled {
            label: label.into(),
            value: value.into(),
            color: TextColor::White,
        }
    }

    pub fn blank() -> Widget {
        Widget::Blank
    }

    pub fn separator(glyph: char) -> Widget {
        Widget::Separator(glyph)
    }

    pub fn ranking() -> Ranking {
        Ranking::default()
    }

    pub fn timer(elapsed: Duration) -> Timer {
        Timer {
            seconds: elapsed.as_secs(),
            label: None,
            color: TextColor::White,
        }
    }

    pub fn timer_ticks(ticks: u64) -> Timer {
        Self::timer(Duration::from_secs(ticks / 20))
    }

    pub fn progress(value: u32, max: u32) -> Progress {
        Progress {
            value,
            max,
            width: 10,
            label: None,
            color: TextColor::Green,
        }
    }

    pub fn render(&self, out: &mut Vec<String>) {
        match self {
            Widget::Text(text) => text.render(out),
            Widget::Labeled(labeled) => labeled.render(out),
            Widget::Blank => out.push(String::new()),
            Widget::Separator(glyph) => {
                let mut line = String::new();
                paint(&mut line, TextColor::DarkGray);
                line.extend(std::iter::repeat_n(*glyph, SEPARATOR_WIDTH));
                out.push(line);
            }
            Widget::Ranking(ranking) => ranking.render(out),
            Widget::Timer(timer) => timer.render(out),
            Widget::Progress(progress) => progress.render(out),
        }
    }
}

impl From<Text> for Widget {
    fn from(text: Text) -> Self {
        Widget::Text(text)
    }
}

impl From<Labeled> for Widget {
    fn from(labeled: Labeled) -> Self {
        Widget::Labeled(labeled)
    }
}

impl From<Ranking> for Widget {
    fn from(ranking: Ranking) -> Self {
        Widget::Ranking(ranking)
    }
}

impl From<Timer> for Widget {
    fn from(timer: Timer) -> Self {
        Widget::Timer(timer)
    }
}

impl From<Progress> for Widget {
    fn from(progress: Progress) -> Self {
        Widget::Progress(progress)
    }
}

/// Blank lines share one text, so each line's owner is its position; the
/// scoreboard layer then resends exactly the positions whose text changed.
#[derive(Component, Clone, Debug)]
#[require(
    SidebarState,
    Objective = Objective::sidebar(SIDEBAR_OBJECTIVE).format(ScoreFormat::Blank)
)]
pub struct Sidebar {
    pub title: String,
    pub color: TextColor,
    pub ornament: Ornament,
    pub audience: Audience,
    widgets: Vec<(WidgetId, Widget)>,
    next_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WidgetId(u32);

impl Sidebar {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            color: TextColor::White,
            ornament: Ornament::None,
            audience: Audience::All,
            widgets: Vec::new(),
            next_id: 0,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = color;
        self
    }

    pub fn ornament(mut self, ornament: Ornament) -> Self {
        self.ornament = ornament;
        self
    }

    pub fn widget(mut self, widget: impl Into<Widget>) -> Self {
        self.push(widget);
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = audience;
        self
    }

    pub fn viewers(self, players: impl IntoIterator<Item = Entity>) -> Self {
        self.audience(Audience::explicit(players))
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    fn next_id(&mut self) -> WidgetId {
        let id = WidgetId(self.next_id);
        self.next_id += 1;
        id
    }

    fn position(&self, id: WidgetId) -> Option<usize> {
        self.widgets.iter().position(|(held, _)| *held == id)
    }

    pub fn push(&mut self, widget: impl Into<Widget>) -> WidgetId {
        let id = self.next_id();
        self.widgets.push((id, widget.into()));
        id
    }

    pub fn insert_before(&mut self, id: WidgetId, widget: impl Into<Widget>) -> Option<WidgetId> {
        let index = self.position(id)?;
        let new = self.next_id();
        self.widgets.insert(index, (new, widget.into()));
        Some(new)
    }

    pub fn insert_after(&mut self, id: WidgetId, widget: impl Into<Widget>) -> Option<WidgetId> {
        let index = self.position(id)?;
        let new = self.next_id();
        self.widgets.insert(index + 1, (new, widget.into()));
        Some(new)
    }

    pub fn set(&mut self, id: WidgetId, widget: impl Into<Widget>) -> bool {
        match self.get_mut(id) {
            Some(slot) => {
                *slot = widget.into();
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: WidgetId) -> Option<Widget> {
        let index = self.position(id)?;
        Some(self.widgets.remove(index).1)
    }

    pub fn truncate(&mut self, len: usize) {
        self.widgets.truncate(len);
    }

    pub fn clear(&mut self) {
        self.widgets.clear();
    }

    pub fn len(&self) -> usize {
        self.widgets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = WidgetId> + '_ {
        self.widgets.iter().map(|(id, _)| *id)
    }

    pub fn widgets(&self) -> impl Iterator<Item = (WidgetId, &Widget)> + '_ {
        self.widgets.iter().map(|(id, widget)| (*id, widget))
    }

    pub fn get(&self, id: WidgetId) -> Option<&Widget> {
        self.widgets
            .iter()
            .find(|(held, _)| *held == id)
            .map(|(_, widget)| widget)
    }

    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut Widget> {
        self.widgets
            .iter_mut()
            .find(|(held, _)| *held == id)
            .map(|(_, widget)| widget)
    }

    pub fn set_value(&mut self, id: WidgetId, value: impl Into<String>) -> bool {
        match self.get_mut(id) {
            Some(Widget::Labeled(labeled)) => {
                labeled.set_value(value);
                true
            }
            Some(Widget::Text(text)) => {
                text.text = value.into();
                true
            }
            _ => false,
        }
    }

    pub fn labeled_mut(&mut self, id: WidgetId) -> Option<&mut Labeled> {
        match self.get_mut(id) {
            Some(Widget::Labeled(labeled)) => Some(labeled),
            _ => None,
        }
    }

    pub fn ranking_mut(&mut self, id: WidgetId) -> Option<&mut Ranking> {
        match self.get_mut(id) {
            Some(Widget::Ranking(ranking)) => Some(ranking),
            _ => None,
        }
    }

    pub fn timer_mut(&mut self, id: WidgetId) -> Option<&mut Timer> {
        match self.get_mut(id) {
            Some(Widget::Timer(timer)) => Some(timer),
            _ => None,
        }
    }

    pub fn progress_mut(&mut self, id: WidgetId) -> Option<&mut Progress> {
        match self.get_mut(id) {
            Some(Widget::Progress(progress)) => Some(progress),
            _ => None,
        }
    }

    pub fn rendered_title(&self) -> String {
        self.ornament.apply(&self.title)
    }

    pub fn render(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (_, widget) in &self.widgets {
            widget.render(&mut lines);
        }
        for line in &mut lines {
            fit(line);
        }
        lines
    }

    fn scores(&self, lines: &[String]) -> BTreeMap<String, Score> {
        lines
            .iter()
            .take(MAX_SIDEBAR_LINES)
            .enumerate()
            .map(|(index, line)| {
                (
                    line_owner(index),
                    Score {
                        value: (MAX_SIDEBAR_LINES - index) as i32,
                        display: Some(line.clone()),
                        format: None,
                    },
                )
            })
            .collect()
    }
}

impl Index<WidgetId> for Sidebar {
    type Output = Widget;

    fn index(&self, id: WidgetId) -> &Widget {
        self.get(id).expect("unknown sidebar widget id")
    }
}

impl IndexMut<WidgetId> for Sidebar {
    fn index_mut(&mut self, id: WidgetId) -> &mut Widget {
        self.get_mut(id).expect("unknown sidebar widget id")
    }
}

pub fn line_owner(index: usize) -> String {
    format!("line{index:02}")
}

fn fit(line: &mut String) {
    let mut visible = 0;
    let mut skip_code = false;
    let mut trailing_code = None;
    for (index, c) in line.char_indices() {
        if skip_code {
            skip_code = false;
            continue;
        }
        if c == LEGACY_PREFIX {
            skip_code = true;
            trailing_code.get_or_insert(index);
            continue;
        }
        if visible == MAX_LINE_CHARS {
            line.truncate(trailing_code.unwrap_or(index));
            return;
        }
        trailing_code = None;
        visible += 1;
    }
    if let Some(index) = trailing_code {
        line.truncate(index);
    }
}

fn same_audience(a: &Audience, b: &Audience) -> bool {
    match (a, b) {
        (Audience::All, Audience::All) => true,
        (Audience::InDimension(a), Audience::InDimension(b)) => a == b,
        (Audience::Explicit(a), Audience::Explicit(b)) => a == b,
        (Audience::Custom(a), Audience::Custom(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

#[derive(Component, Debug, Default)]
pub struct SidebarState {
    truncated: bool,
}

pub struct SidebarPlugin;

impl Plugin for SidebarPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PostUpdate,
            VoidSystems::SidebarSync.before(VoidSystems::ScoreboardSync),
        )
        .add_systems(PostUpdate, render_sidebars.in_set(VoidSystems::SidebarSync))
        .add_observer(remove_board);
    }
}

fn remove_board(event: On<Remove, Sidebar>, mut commands: Commands) {
    commands
        .entity(event.entity)
        .try_remove::<(Objective, SidebarState)>();
}

fn render_sidebars(mut sidebars: Query<(Entity, Ref<Sidebar>, &mut Objective, &mut SidebarState)>) {
    for (entity, sidebar, mut objective, mut state) in &mut sidebars {
        if !sidebar.is_changed() {
            continue;
        }
        let lines = sidebar.render();
        let truncated = lines.len() > MAX_SIDEBAR_LINES;
        if truncated && !state.truncated {
            warn!(
                ?entity,
                lines = lines.len(),
                "sidebar has more than {MAX_SIDEBAR_LINES} lines; the rest is not shown"
            );
        }
        if state.truncated != truncated {
            state.truncated = truncated;
        }
        let title = sidebar.rendered_title();
        let scores = sidebar.scores(&lines);
        let current = objective.bypass_change_detection();
        let mut dirty = false;
        if current.title != title {
            current.title = title;
            dirty = true;
        }
        if current.color != sidebar.color {
            current.color = sidebar.color;
            dirty = true;
        }
        if current.format != Some(ScoreFormat::Blank) {
            current.format = Some(ScoreFormat::Blank);
            dirty = true;
        }
        if current.slot != DisplaySlot::Sidebar {
            warn!(
                ?entity,
                slot = ?current.slot,
                "sidebar objective was on another display slot; moved to the sidebar slot"
            );
            current.slot = DisplaySlot::Sidebar;
            dirty = true;
        }
        if current.scores != scores {
            current.scores = scores;
            dirty = true;
        }
        if !same_audience(&current.audience, &sidebar.audience) {
            current.audience = sidebar.audience.clone();
            dirty = true;
        }
        if dirty {
            objective.set_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bevy_app::App;
    use flume::Receiver;
    use ussr_nbt::owned::Tag;
    use voidmc_protocol::clientbound::{
        ClientboundPacket, NumberFormat, ObjectiveAction, PlayPacket,
    };

    use super::*;
    use crate::components::{ClientId, PlayerName, PlayerReady};
    use crate::network::{NetworkChannels, OutgoingPacket};
    use crate::plugins::scoreboard::ScoreboardPlugin;

    fn test_app() -> (App, Receiver<OutgoingPacket>) {
        let (incoming_tx, incoming_rx) = flume::unbounded::<crate::network::ConnectionEvent>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            events: incoming_rx,
            outgoing: outgoing_tx,
        })
        .insert_non_send_resource(incoming_tx)
        .add_plugins((ScoreboardPlugin, SidebarPlugin));
        (app, outgoing_rx)
    }

    fn player(app: &mut App, id: u32, name: &str) -> Entity {
        app.world_mut()
            .spawn((ClientId(id), PlayerReady, PlayerName(name.into())))
            .id()
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Sent {
        Create(u32, String, bool),
        Update(u32, String),
        Remove(u32),
        Display(u32, DisplaySlot),
        Score(u32, String, i32, String),
        Reset(u32, String),
    }

    fn text(nbt: &ussr_nbt::owned::Nbt) -> String {
        nbt.compound
            .tags
            .iter()
            .find(|(name, _)| name.to_string() == "text")
            .map(|(_, tag)| match tag {
                Tag::String(value) => value.to_string(),
                other => panic!("unexpected tag {other:?}"),
            })
            .expect("text field")
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
                        ObjectiveAction::Create(info) => Sent::Create(
                            id,
                            text(&info.display_name),
                            info.number_format == Some(NumberFormat::Blank),
                        ),
                        ObjectiveAction::Update(info) => Sent::Update(id, text(&info.display_name)),
                        ObjectiveAction::Remove => Sent::Remove(id),
                    },
                    PlayPacket::SetDisplayObjective(p) => Sent::Display(id, p.slot),
                    PlayPacket::SetScore(p) => {
                        assert_eq!(p.number_format, None);
                        let display = p
                            .display_name
                            .as_ref()
                            .expect("sidebar lines carry a display name");
                        Sent::Score(id, p.owner, p.value, text(display))
                    }
                    PlayPacket::ResetScore(p) => Sent::Reset(id, p.owner),
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

    fn score(id: u32, index: usize, line: &str) -> Sent {
        Sent::Score(
            id,
            line_owner(index),
            (MAX_SIDEBAR_LINES - index) as i32,
            line.into(),
        )
    }

    fn lines(sidebar: &Sidebar) -> Vec<String> {
        sidebar.render()
    }

    fn race() -> Sidebar {
        Sidebar::new("Alpine Rush")
            .color(TextColor::Gold)
            .ornament(Ornament::brackets("✦"))
            .widget(Widget::labeled("Tour", "1/3"))
            .widget(
                Widget::ranking()
                    .limit(5)
                    .entries([("Leo", 1200), ("Adam", 1100)]),
            )
            .widget(Widget::blank())
            .widget(Widget::labeled("Meilleur tour", "--:--"))
    }

    #[test]
    fn each_widget_renders_its_exact_lines() {
        let sidebar = Sidebar::new("t")
            .widget(Widget::text("plain"))
            .widget(Widget::text("aqua").color(TextColor::Aqua))
            .widget(Widget::labeled("Tour", "1/3"))
            .widget(Widget::labeled("Vitesse", "42").color(TextColor::Gold))
            .widget(Widget::blank())
            .widget(Widget::separator('─'))
            .widget(Widget::timer(Duration::from_secs(83)))
            .widget(Widget::timer(Duration::from_secs(3723)).label("Temps"))
            .widget(Widget::timer_ticks(45).color(TextColor::Red))
            .widget(Widget::progress(3, 10).width(5))
            .widget(Widget::progress(7, 10).label("HP").color(TextColor::Red))
            .widget(Widget::progress(50, 0))
            .widget(Widget::progress(99, 10).width(4));
        assert_eq!(
            lines(&sidebar),
            [
                "plain",
                "§baqua",
                "§fTour: §f1/3",
                "§fVitesse: §642",
                "",
                "§8────────────────────",
                "§f01:23",
                "§fTemps: §f1:02:03",
                "§c00:02",
                "§a▮§8▯▯▯▯",
                "§fHP §c▮▮▮▮▮▮▮§8▯▯▯",
                "§a§8▯▯▯▯▯▯▯▯▯▯",
                "§a▮▮▮▮",
            ]
        );
    }

    #[test]
    fn ranking_sorts_limits_and_highlights() {
        let sidebar =
            Sidebar::new("t").widget(Widget::ranking().limit(3).highlight("Adam").entries([
                ("Leo", 1200),
                ("Adam", 1300),
                ("Zoe", 900),
                ("Bob", 1200),
            ]));
        assert_eq!(
            lines(&sidebar),
            ["§e1. Adam 1300", "§72. §fBob §71200", "§73. §fLeo §71200"]
        );
        let sidebar = Sidebar::new("t").widget(
            Widget::ranking()
                .color(TextColor::Aqua)
                .highlight("Leo")
                .entries([("Leo", 1)]),
        );
        assert_eq!(lines(&sidebar), ["§b1. Leo 1"]);
        assert!(lines(&Sidebar::new("t").widget(Widget::ranking())).is_empty());
    }

    #[test]
    fn title_ornaments_wrap_the_title() {
        assert_eq!(Sidebar::new("Race").rendered_title(), "Race");
        assert_eq!(
            Sidebar::new("Race")
                .ornament(Ornament::brackets("◆"))
                .rendered_title(),
            "◆ Race ◆"
        );
        assert_eq!(
            Sidebar::new("Race")
                .ornament(Ornament::Line)
                .rendered_title(),
            "─ Race ─"
        );
    }

    #[test]
    fn long_lines_are_cut_to_the_visible_limit_ignoring_colour_codes() {
        let long = "x".repeat(60);
        let sidebar = Sidebar::new("t")
            .widget(Widget::text(&long).color(TextColor::Gold))
            .widget(Widget::labeled("é".repeat(38), "value"));
        let rendered = lines(&sidebar);
        assert_eq!(rendered[0], format!("§6{}", "x".repeat(MAX_LINE_CHARS)));
        assert_eq!(rendered[1], format!("§f{}: ", "é".repeat(38)));
        let mut trailing = String::from("§fa§b§c");
        fit(&mut trailing);
        assert_eq!(trailing, "§fa");
        let mut cut = format!("{}§6§7x", "x".repeat(MAX_LINE_CHARS));
        fit(&mut cut);
        assert_eq!(cut, "x".repeat(MAX_LINE_CHARS));
    }

    #[test]
    fn rgb_colours_fall_back_to_white() {
        assert_eq!(
            Widget::labeled("a", "b")
                .color(TextColor::rgb(0x123456))
                .color,
            TextColor::White
        );
        assert_eq!(
            Widget::text("a").color(TextColor::rgb(0x123456)).color,
            Some(TextColor::White)
        );
    }

    fn ids(sidebar: &Sidebar) -> Vec<WidgetId> {
        sidebar.ids().collect()
    }

    #[test]
    fn mutators_edit_widgets_in_place() {
        let mut sidebar = race();
        let [tour, ranking, blank, best] = ids(&sidebar)[..] else {
            panic!("four widgets");
        };
        assert!(sidebar.set_value(tour, "2/3"));
        assert!(!sidebar.set_value(ranking, "nope"));
        sidebar.ranking_mut(ranking).unwrap().set([("Zoe", 1)]);
        assert!(sidebar.ranking_mut(tour).is_none());
        assert!(sidebar.labeled_mut(tour).is_some());
        assert!(sidebar.timer_mut(tour).is_none());
        assert!(sidebar.progress_mut(tour).is_none());
        let timer = sidebar.push(Widget::timer_ticks(200));
        sidebar.timer_mut(timer).unwrap().set_ticks(1200);
        let progress = sidebar.push(Widget::progress(0, 4));
        sidebar.progress_mut(progress).unwrap().set(2);
        let separator = sidebar.insert_before(tour, Widget::separator('=')).unwrap();
        assert_eq!(sidebar.remove(blank), Some(Widget::Blank));
        assert_eq!(sidebar.remove(blank), None);
        assert!(sidebar.set_value(best, "01:02"));
        sidebar.set_title("Rush");
        assert_eq!(
            lines(&sidebar),
            [
                "§8====================",
                "§fTour: §f2/3",
                "§71. §fZoe §71",
                "§fMeilleur tour: §f01:02",
                "§f01:00",
                "§a▮▮▮▮▮§8▯▯▯▯▯",
            ]
        );
        assert_eq!(sidebar.rendered_title(), "✦ Rush ✦");
        assert!(matches!(
            sidebar.get(separator),
            Some(Widget::Separator('='))
        ));
        assert!(matches!(sidebar[separator], Widget::Separator('=')));
        sidebar[separator] = Widget::separator('-');
        assert_eq!(lines(&sidebar)[0], "§8--------------------");
        assert!(sidebar.set(separator, Widget::blank()));
        assert_eq!(lines(&sidebar)[0], "");
        assert_eq!(sidebar.len(), 6);
        sidebar.truncate(2);
        assert_eq!(ids(&sidebar), [separator, tour]);
        sidebar.clear();
        assert!(sidebar.is_empty());
        assert!(lines(&sidebar).is_empty());
    }

    #[test]
    fn unknown_ids_are_refused_without_panicking() {
        let mut sidebar = race();
        let mut other = Sidebar::new("other");
        for _ in 0..10 {
            other.push(Widget::blank());
        }
        let foreign = other.push(Widget::blank());
        assert!(sidebar.get(foreign).is_none());
        assert!(sidebar.get_mut(foreign).is_none());
        assert!(!sidebar.set(foreign, Widget::blank()));
        assert!(!sidebar.set_value(foreign, "x"));
        assert!(sidebar.remove(foreign).is_none());
        assert!(sidebar.insert_before(foreign, Widget::blank()).is_none());
        assert!(sidebar.insert_after(foreign, Widget::blank()).is_none());
        assert!(sidebar.labeled_mut(foreign).is_none());
        assert!(sidebar.ranking_mut(foreign).is_none());
        assert!(sidebar.timer_mut(foreign).is_none());
        assert!(sidebar.progress_mut(foreign).is_none());
        assert_eq!(sidebar.len(), 4);
    }

    #[test]
    fn ids_survive_inserts_and_removals() {
        let mut sidebar = Sidebar::new("t");
        let a = sidebar.push(Widget::text("a"));
        let b = sidebar.push(Widget::labeled("b", "1"));
        let c = sidebar.push(Widget::text("c"));
        let before = sidebar.insert_before(a, Widget::text("before")).unwrap();
        let after = sidebar.insert_after(c, Widget::text("after")).unwrap();
        assert_eq!(ids(&sidebar), [before, a, b, c, after]);
        assert_eq!(
            sidebar
                .remove(a)
                .map(|w| lines(&Sidebar::new("").widget(w))),
            Some(vec!["a".into()])
        );
        assert!(sidebar.set_value(b, "2"));
        assert_eq!(sidebar.labeled_mut(b).unwrap().value, "2");
        let again = sidebar.push(Widget::text("a"));
        assert_ne!(again, a);
        assert!(sidebar.get(a).is_none());
        assert_eq!(lines(&sidebar), ["before", "§fb: §f2", "c", "after", "a"]);
        assert_eq!(
            sidebar.widgets().map(|(id, _)| id).collect::<Vec<_>>(),
            [before, b, c, after, again]
        );
    }

    #[test]
    fn first_sync_sends_the_whole_board_with_blank_numbers() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        app.world_mut().spawn(race());
        app.update();
        assert_eq!(
            sorted(&rx),
            [
                Sent::Create(1, "✦ Alpine Rush ✦".into(), true),
                Sent::Display(1, DisplaySlot::Sidebar),
                score(1, 0, "§fTour: §f1/3"),
                score(1, 1, "§71. §fLeo §71200"),
                score(1, 2, "§72. §fAdam §71100"),
                score(1, 3, ""),
                score(1, 4, "§fMeilleur tour: §f--:--"),
            ]
        );
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn changing_one_widget_resends_only_its_line() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let board = app.world_mut().spawn(race()).id();
        app.update();
        drain(&rx);

        let tour = ids(app.world().get::<Sidebar>(board).unwrap())[0];
        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .set_value(tour, "2/3");
        app.update();
        assert_eq!(drain(&rx), [score(1, 0, "§fTour: §f2/3")]);

        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .set_value(tour, "2/3");
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn ranking_reorder_resends_only_moved_lines_and_resets_dropped_ones() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let board = app.world_mut().spawn(race()).id();
        app.update();
        drain(&rx);

        let ranking = ids(app.world().get::<Sidebar>(board).unwrap())[1];
        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .ranking_mut(ranking)
            .unwrap()
            .set([("Leo", 1200), ("Adam", 1300)]);
        app.update();
        assert_eq!(
            sorted(&rx),
            [
                score(1, 1, "§71. §fAdam §71300"),
                score(1, 2, "§72. §fLeo §71200"),
            ]
        );

        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .ranking_mut(ranking)
            .unwrap()
            .set([("Adam", 1300)]);
        app.update();
        assert_eq!(
            sorted(&rx),
            [
                score(1, 2, ""),
                score(1, 3, "§fMeilleur tour: §f--:--"),
                Sent::Reset(1, line_owner(4)),
            ]
        );
    }

    #[test]
    fn title_changes_update_the_objective_header_only() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let board = app.world_mut().spawn(race()).id();
        app.update();
        drain(&rx);

        let mut sidebar = app.world_mut().get_mut::<Sidebar>(board).unwrap();
        sidebar.ornament = Ornament::Line;
        sidebar.color = TextColor::Aqua;
        app.update();
        assert_eq!(drain(&rx), [Sent::Update(1, "─ Alpine Rush ─".into())]);
    }

    #[test]
    fn late_joiners_get_the_current_board() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let board = app.world_mut().spawn(race()).id();
        app.update();
        let tour = ids(app.world().get::<Sidebar>(board).unwrap())[0];
        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .set_value(tour, "3/3");
        app.update();
        drain(&rx);

        player(&mut app, 2, "Adam");
        app.update();
        assert_eq!(
            sorted(&rx),
            [
                Sent::Create(2, "✦ Alpine Rush ✦".into(), true),
                Sent::Display(2, DisplaySlot::Sidebar),
                score(2, 0, "§fTour: §f3/3"),
                score(2, 1, "§71. §fLeo §71200"),
                score(2, 2, "§72. §fAdam §71100"),
                score(2, 3, ""),
                score(2, 4, "§fMeilleur tour: §f--:--"),
            ]
        );
    }

    #[test]
    fn more_than_fifteen_lines_are_truncated() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let mut sidebar = Sidebar::new("t");
        for index in 0..20 {
            sidebar.push(Widget::text(format!("line {index}")));
        }
        let board = app.world_mut().spawn(sidebar).id();
        app.update();
        let sent = drain(&rx);
        let scores: Vec<&Sent> = sent
            .iter()
            .filter(|s| matches!(s, Sent::Score(..)))
            .collect();
        assert_eq!(scores.len(), MAX_SIDEBAR_LINES);
        assert_eq!(*scores[0], score(1, 0, "line 0"));
        assert_eq!(*scores[14], score(1, 14, "line 14"));
        assert!(app.world().get::<SidebarState>(board).unwrap().truncated);

        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .truncate(3);
        app.update();
        let sent = drain(&rx);
        assert_eq!(
            sent.iter().filter(|s| matches!(s, Sent::Reset(..))).count(),
            12
        );
        assert!(!app.world().get::<SidebarState>(board).unwrap().truncated);
    }

    #[test]
    fn audience_changes_reach_the_objective() {
        let (mut app, rx) = test_app();
        let leo = player(&mut app, 1, "Leo");
        let adam = player(&mut app, 2, "Adam");
        let board = app.world_mut().spawn(race().viewers([leo])).id();
        app.update();
        let sent = drain(&rx);
        assert!(sent.iter().all(|s| matches!(
            s,
            Sent::Create(1, ..) | Sent::Display(1, ..) | Sent::Score(1, ..)
        )));

        app.world_mut().get_mut::<Sidebar>(board).unwrap().audience = Audience::explicit([adam]);
        app.update();
        let sent = sorted(&rx);
        assert_eq!(sent[0], Sent::Create(2, "✦ Alpine Rush ✦".into(), true));
        assert!(sent.contains(&Sent::Remove(1)));

        app.world_mut().get_mut::<Sidebar>(board).unwrap().audience = Audience::explicit([adam]);
        app.update();
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn a_custom_audience_hands_a_player_over_to_a_personal_board() {
        let (mut app, rx) = test_app();
        let leo = player(&mut app, 1, "Leo");
        player(&mut app, 2, "Adam");
        let personal = Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()));
        let shared = personal.clone();
        app.world_mut()
            .spawn(race().audience(Audience::custom(move |r| {
                !shared.read().unwrap().contains(&r.entity())
            })));
        app.update();
        let sent = drain(&rx);
        assert_eq!(
            sent.iter()
                .filter(|s| matches!(s, Sent::Create(..)))
                .count(),
            2
        );

        app.world_mut().spawn(race().title("Leo").viewers([leo]));
        app.update();
        assert!(drain(&rx).is_empty());

        personal.write().unwrap().insert(leo);
        app.update();
        app.update();
        let sent = sorted(&rx);
        assert_eq!(sent[0], Sent::Create(1, "✦ Leo ✦".into(), true));
        assert!(sent.contains(&Sent::Remove(1)));
        assert!(!sent.iter().any(|s| matches!(s, Sent::Remove(2))));
    }

    struct WarningCounter;

    static WARNINGS: AtomicUsize = AtomicUsize::new(0);

    impl log::Log for WarningCounter {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) && record.target().starts_with("bevy_ecs") {
                WARNINGS.fetch_add(1, Ordering::SeqCst);
            }
        }

        fn flush(&self) {}
    }

    fn count_warnings() -> usize {
        let _ = log::set_logger(&WarningCounter);
        log::set_max_level(log::LevelFilter::Warn);
        WARNINGS.load(Ordering::SeqCst)
    }

    #[test]
    fn despawning_removes_the_board() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let before = count_warnings();

        let board = app.world_mut().spawn(race()).id();
        app.update();
        drain(&rx);
        app.world_mut().despawn(board);
        app.update();
        assert_eq!(drain(&rx), [Sent::Remove(1)]);

        let board = app.world_mut().spawn(race()).id();
        app.update();
        drain(&rx);
        app.world_mut().commands().entity(board).despawn();
        app.update();
        assert_eq!(drain(&rx), [Sent::Remove(1)]);

        assert_eq!(WARNINGS.load(Ordering::SeqCst), before);
    }

    #[test]
    fn a_reinserted_board_starts_with_fresh_state() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let mut sidebar = Sidebar::new("t");
        for index in 0..20 {
            sidebar.push(Widget::text(format!("line {index}")));
        }
        let board = app.world_mut().spawn(sidebar).id();
        app.update();
        assert!(app.world().get::<SidebarState>(board).unwrap().truncated);

        app.world_mut().entity_mut(board).remove::<Sidebar>();
        app.update();
        assert!(app.world().get::<SidebarState>(board).is_none());

        app.world_mut().entity_mut(board).insert(race());
        app.update();
        drain(&rx);
        assert!(!app.world().get::<SidebarState>(board).unwrap().truncated);
    }

    #[test]
    fn removing_the_component_removes_the_board() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let board = app.world_mut().spawn(race()).id();
        app.update();
        drain(&rx);
        app.world_mut().entity_mut(board).remove::<Sidebar>();
        app.update();
        assert_eq!(drain(&rx), [Sent::Remove(1)]);
        assert!(app.world().get::<Objective>(board).is_none());

        app.world_mut().spawn(race().title("Next"));
        app.update();
        let sent = sorted(&rx);
        assert_eq!(sent[0], Sent::Create(1, "✦ Next ✦".into(), true));
        assert_eq!(sent[1], Sent::Display(1, DisplaySlot::Sidebar));
    }

    #[test]
    fn an_existing_objective_is_moved_to_the_sidebar_slot() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let board = app
            .world_mut()
            .spawn(Objective::list("custom").score(
                "someone",
                Score {
                    value: 3,
                    display: Some("x".into()),
                    format: None,
                },
            ))
            .id();
        app.update();
        drain(&rx);
        app.world_mut().entity_mut(board).insert(race());
        app.update();
        let sent = sorted(&rx);
        assert!(sent.contains(&Sent::Display(1, DisplaySlot::Sidebar)));
        assert!(sent.contains(&Sent::Display(1, DisplaySlot::List)));
        assert!(sent.contains(&Sent::Reset(1, "someone".into())));
        assert!(sent.contains(&score(1, 0, "§fTour: §f1/3")));
        let objective = app.world().get::<Objective>(board).unwrap();
        assert_eq!(objective.slot, DisplaySlot::Sidebar);
        assert_eq!(objective.name, "custom");
    }
}
