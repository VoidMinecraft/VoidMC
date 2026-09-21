//! A widget-based sidebar on top of [`Objective`]: spawn a [`Sidebar`] with a
//! title and a list of [`Widget`]s, mutate the widgets, despawn it. A
//! `PostUpdate` system (`VoidSystems::SidebarSync`) renders the widgets into
//! lines and writes them into the entity's managed sidebar objective, so the
//! scoreboard sync only sends the lines whose text changed.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use bevy_app::{App, Plugin, PostUpdate};
use bevy_ecs::prelude::*;
use tracing::warn;

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
    pub widgets: Vec<Widget>,
    pub audience: Audience,
}

impl Sidebar {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            color: TextColor::White,
            ornament: Ornament::None,
            widgets: Vec::new(),
            audience: Audience::All,
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
        self.widgets.push(widget.into());
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

    pub fn push(&mut self, widget: impl Into<Widget>) -> usize {
        self.widgets.push(widget.into());
        self.widgets.len() - 1
    }

    pub fn insert(&mut self, index: usize, widget: impl Into<Widget>) {
        self.widgets.insert(index, widget.into());
    }

    pub fn set(&mut self, index: usize, widget: impl Into<Widget>) {
        self.widgets[index] = widget.into();
    }

    pub fn remove(&mut self, index: usize) -> Widget {
        self.widgets.remove(index)
    }

    pub fn clear(&mut self) {
        self.widgets.clear();
    }

    pub fn get(&self, index: usize) -> Option<&Widget> {
        self.widgets.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Widget> {
        self.widgets.get_mut(index)
    }

    pub fn set_value(&mut self, index: usize, value: impl Into<String>) -> bool {
        match self.widgets.get_mut(index) {
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

    pub fn labeled_mut(&mut self, index: usize) -> Option<&mut Labeled> {
        match self.widgets.get_mut(index) {
            Some(Widget::Labeled(labeled)) => Some(labeled),
            _ => None,
        }
    }

    pub fn ranking_mut(&mut self, index: usize) -> Option<&mut Ranking> {
        match self.widgets.get_mut(index) {
            Some(Widget::Ranking(ranking)) => Some(ranking),
            _ => None,
        }
    }

    pub fn timer_mut(&mut self, index: usize) -> Option<&mut Timer> {
        match self.widgets.get_mut(index) {
            Some(Widget::Timer(timer)) => Some(timer),
            _ => None,
        }
    }

    pub fn progress_mut(&mut self, index: usize) -> Option<&mut Progress> {
        match self.widgets.get_mut(index) {
            Some(Widget::Progress(progress)) => Some(progress),
            _ => None,
        }
    }

    pub fn rendered_title(&self) -> String {
        self.ornament.apply(&self.title)
    }

    pub fn render(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for widget in &self.widgets {
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

pub fn line_owner(index: usize) -> String {
    format!("line{index:02}")
}

fn fit(line: &mut String) {
    let mut visible = 0;
    let mut skip_code = false;
    for (index, c) in line.char_indices() {
        if skip_code {
            skip_code = false;
            continue;
        }
        if c == LEGACY_PREFIX {
            skip_code = true;
            continue;
        }
        if visible == MAX_LINE_CHARS {
            line.truncate(index);
            return;
        }
        visible += 1;
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
        .add_systems(PostUpdate, render_sidebars.in_set(VoidSystems::SidebarSync));
    }
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
    use bevy_app::App;
    use flume::Receiver;
    use ussr_nbt::owned::Tag;
    use voidmc_protocol::clientbound::{
        ClientboundPacket, DisplaySlot, NumberFormat, ObjectiveAction, PlayPacket,
    };

    use super::*;
    use crate::components::{ClientId, PlayerName, PlayerReady};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::plugins::scoreboard::ScoreboardPlugin;

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
                        Sent::Score(
                            id,
                            p.owner,
                            p.value,
                            p.display_name.as_ref().map(text).unwrap_or_default(),
                        )
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
                "§a▮▮▮▮§8",
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
        assert_eq!(rendered[1], format!("§f{}: §f", "é".repeat(38)));
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

    #[test]
    fn mutators_edit_widgets_in_place() {
        let mut sidebar = race();
        assert!(sidebar.set_value(0, "2/3"));
        assert!(!sidebar.set_value(1, "nope"));
        assert!(!sidebar.set_value(9, "nope"));
        sidebar.ranking_mut(1).unwrap().set([("Zoe", 1)]);
        assert!(sidebar.ranking_mut(0).is_none());
        assert!(sidebar.labeled_mut(0).is_some());
        assert!(sidebar.timer_mut(0).is_none());
        assert!(sidebar.progress_mut(0).is_none());
        let index = sidebar.push(Widget::timer_ticks(200));
        sidebar.timer_mut(index).unwrap().set_ticks(1200);
        let index = sidebar.push(Widget::progress(0, 4));
        sidebar.progress_mut(index).unwrap().set(2);
        sidebar.insert(0, Widget::separator('='));
        assert_eq!(sidebar.remove(3), Widget::Blank);
        sidebar.set_title("Rush");
        assert_eq!(
            lines(&sidebar),
            [
                "§8====================",
                "§fTour: §f2/3",
                "§71. §fZoe §71",
                "§fMeilleur tour: §f--:--",
                "§f01:00",
                "§a▮▮▮▮▮§8▯▯▯▯▯",
            ]
        );
        assert_eq!(sidebar.rendered_title(), "✦ Rush ✦");
        assert!(matches!(sidebar.get(0), Some(Widget::Separator('='))));
        assert!(sidebar.get_mut(99).is_none());
        sidebar.clear();
        assert!(lines(&sidebar).is_empty());
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

        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .set_value(0, "2/3");
        app.update();
        assert_eq!(drain(&rx), [score(1, 0, "§fTour: §f2/3")]);

        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .set_value(0, "2/3");
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

        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .ranking_mut(1)
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
            .ranking_mut(1)
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
        app.world_mut()
            .get_mut::<Sidebar>(board)
            .unwrap()
            .set_value(0, "3/3");
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
            .widgets
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
    fn despawning_removes_the_board() {
        let (mut app, rx) = test_app();
        player(&mut app, 1, "Leo");
        let board = app.world_mut().spawn(race()).id();
        app.update();
        drain(&rx);
        app.world_mut().despawn(board);
        app.update();
        assert_eq!(drain(&rx), [Sent::Remove(1)]);
    }
}
