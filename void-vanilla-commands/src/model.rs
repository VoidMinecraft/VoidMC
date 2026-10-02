use std::collections::{BTreeMap, BTreeSet};

use bevy_ecs::prelude::*;
use voidmc::{DisplaySlot, Objective, RenderType, Score, ScoreFormat, TextColor};

/// What feeds an objective's scores. Only criteria the engine can honour
/// exist: both are changed by commands or code, never automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criteria {
    Dummy,
    Trigger,
}

/// A vanilla objective: it exists whether or not a display slot shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreObjective {
    pub criteria: Criteria,
    pub title: String,
    pub color: TextColor,
    pub render: RenderType,
    pub format: Option<ScoreFormat>,
    pub display_auto_update: bool,
    pub scores: BTreeMap<String, Score>,
    enabled: BTreeSet<String>,
}

impl ScoreObjective {
    fn new(name: &str, criteria: Criteria) -> Self {
        Self {
            criteria,
            title: name.to_string(),
            color: TextColor::White,
            render: RenderType::Integer,
            format: None,
            display_auto_update: false,
            scores: BTreeMap::new(),
            enabled: BTreeSet::new(),
        }
    }

    pub fn get(&self, holder: &str) -> Option<i32> {
        self.scores.get(holder).map(|score| score.value)
    }

    /// Keeps the holder's display name and number format.
    pub fn set(&mut self, holder: impl Into<String>, value: i32) {
        self.scores.entry(holder.into()).or_default().value = value;
    }

    /// Also disables the holder's trigger, as in vanilla.
    pub fn reset(&mut self, holder: &str) -> bool {
        self.enabled.remove(holder);
        self.scores.remove(holder).is_some()
    }

    /// Lets `holder` use the trigger once; `false` if it already could.
    pub fn enable(&mut self, holder: &str) -> bool {
        if !self.enabled.insert(holder.to_string()) {
            return false;
        }
        self.scores.entry(holder.to_string()).or_default();
        true
    }

    pub fn is_enabled(&self, holder: &str) -> bool {
        self.enabled.contains(holder)
    }
}

/// The server's vanilla scoreboard. Objectives in a display slot are mirrored
/// into engine [`Objective`] entities every tick the scoreboard changes.
#[derive(Resource, Debug, Default)]
pub struct Scoreboard {
    objectives: BTreeMap<String, ScoreObjective>,
    displays: BTreeMap<DisplaySlot, String>,
}

impl Scoreboard {
    /// `None` when an objective already has that name.
    pub fn add_objective(&mut self, name: &str, criteria: Criteria) -> Option<&mut ScoreObjective> {
        if self.objectives.contains_key(name) {
            return None;
        }
        Some(
            self.objectives
                .entry(name.to_string())
                .or_insert(ScoreObjective::new(name, criteria)),
        )
    }

    /// Also clears every display slot that showed it.
    pub fn remove_objective(&mut self, name: &str) -> Option<ScoreObjective> {
        self.displays.retain(|_, shown| shown != name);
        self.objectives.remove(name)
    }

    pub fn objective(&self, name: &str) -> Option<&ScoreObjective> {
        self.objectives.get(name)
    }

    pub fn objective_mut(&mut self, name: &str) -> Option<&mut ScoreObjective> {
        self.objectives.get_mut(name)
    }

    pub fn objectives(&self) -> impl Iterator<Item = (&str, &ScoreObjective)> {
        self.objectives
            .iter()
            .map(|(name, objective)| (name.as_str(), objective))
    }

    pub fn get(&self, holder: &str, objective: &str) -> Option<i32> {
        self.objective(objective)?.get(holder)
    }

    /// `false` when the objective does not exist.
    pub fn set(&mut self, holder: &str, objective: &str, value: i32) -> bool {
        self.objective_mut(objective)
            .map(|objective| objective.set(holder, value))
            .is_some()
    }

    /// Resets one score, or every score of `holder` when `objective` is `None`.
    pub fn reset(&mut self, holder: &str, objective: Option<&str>) -> bool {
        match objective {
            Some(name) => self
                .objective_mut(name)
                .is_some_and(|objective| objective.reset(holder)),
            None => self
                .objectives
                .values_mut()
                .fold(false, |any, objective| objective.reset(holder) | any),
        }
    }

    /// Every score holder with at least one score.
    pub fn holders(&self) -> BTreeSet<&str> {
        self.objectives
            .values()
            .flat_map(|objective| objective.scores.keys().map(String::as_str))
            .collect()
    }

    pub fn display(&self, slot: DisplaySlot) -> Option<&str> {
        self.displays.get(&slot).map(String::as_str)
    }

    /// Shows `objective` in `slot`, or clears it with `None`; `false` when
    /// the objective does not exist.
    pub fn set_display(&mut self, slot: DisplaySlot, objective: Option<&str>) -> bool {
        match objective {
            Some(name) if self.objectives.contains_key(name) => {
                self.displays.insert(slot, name.to_string());
                true
            }
            Some(_) => false,
            None => {
                self.displays.remove(&slot);
                true
            }
        }
    }

    fn wire_objective(&self, slot: DisplaySlot, name: &str) -> Option<Objective> {
        let objective = self.objectives.get(name)?;
        let shared = self.displays.range(..slot).any(|(_, shown)| shown == name);
        let wire_name = if shared {
            format!("{name}#{}", slot as i32)
        } else {
            name.to_string()
        };
        let mut wire = Objective::new(wire_name, slot)
            .title(objective.title.clone())
            .color(objective.color)
            .render(objective.render);
        wire.format = objective.format.clone();
        wire.scores = objective.scores.clone();
        Some(wire)
    }
}

/// Marks the engine objective that mirrors one display slot.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayedObjective(pub DisplaySlot);

fn same(a: &Objective, b: &Objective) -> bool {
    a.name == b.name
        && a.slot == b.slot
        && a.title == b.title
        && a.color == b.color
        && a.render == b.render
        && a.format == b.format
        && a.scores == b.scores
}

pub(crate) fn sync_displays(
    mut commands: Commands,
    scoreboard: Res<Scoreboard>,
    mut displayed: Query<(Entity, &DisplayedObjective, &mut Objective)>,
) {
    let mut missing: BTreeSet<DisplaySlot> = scoreboard.displays.keys().copied().collect();
    for (entity, &DisplayedObjective(slot), mut objective) in &mut displayed {
        let wanted = scoreboard
            .displays
            .get(&slot)
            .and_then(|name| scoreboard.wire_objective(slot, name));
        missing.remove(&slot);
        match wanted {
            Some(wanted) if !same(&objective, &wanted) => {
                let audience = std::mem::take(&mut objective.audience);
                *objective = wanted.audience(audience);
            }
            Some(_) => {}
            None => commands.entity(entity).despawn(),
        }
    }
    for slot in missing {
        if let Some(wanted) = scoreboard.wire_objective(slot, &scoreboard.displays[&slot]) {
            commands.spawn((wanted, DisplayedObjective(slot)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objectives_hold_scores_until_removed() {
        let mut board = Scoreboard::default();
        board.add_objective("kills", Criteria::Dummy).unwrap().title = "Kills".into();
        assert!(board.add_objective("kills", Criteria::Trigger).is_none());
        assert_eq!(board.objective("kills").unwrap().title, "Kills");

        assert!(board.set("Alice", "kills", 3));
        assert!(!board.set("Alice", "deaths", 1));
        board.add_objective("deaths", Criteria::Dummy);
        board.set("Bob", "deaths", 2);
        assert_eq!(board.get("Alice", "kills"), Some(3));
        assert_eq!(board.holders(), BTreeSet::from(["Alice", "Bob"]));

        assert!(board.set_display(DisplaySlot::Sidebar, Some("kills")));
        assert!(!board.set_display(DisplaySlot::List, Some("nope")));
        board.remove_objective("kills");
        assert_eq!(board.display(DisplaySlot::Sidebar), None);
        assert_eq!(board.holders(), BTreeSet::from(["Bob"]));
    }

    #[test]
    fn reset_disables_triggers_and_can_span_objectives() {
        let mut board = Scoreboard::default();
        board.add_objective("vote", Criteria::Trigger);
        board.add_objective("kills", Criteria::Dummy);
        let vote = board.objective_mut("vote").unwrap();
        assert!(vote.enable("Alice"));
        assert_eq!(vote.get("Alice"), Some(0));
        vote.scores.remove("Alice");
        assert!(!vote.enable("Alice"));
        assert_eq!(vote.get("Alice"), None);
        vote.set("Alice", 0);
        board.set("Alice", "kills", 1);

        assert!(board.reset("Alice", None));
        assert!(!board.objective("vote").unwrap().is_enabled("Alice"));
        assert!(board.holders().is_empty());
        assert!(!board.reset("Alice", Some("kills")));
    }

    #[test]
    fn an_objective_shown_twice_gets_a_second_wire_name() {
        let mut board = Scoreboard::default();
        board.add_objective("hp", Criteria::Dummy);
        board.set_display(DisplaySlot::Sidebar, Some("hp"));
        assert_eq!(
            board
                .wire_objective(DisplaySlot::Sidebar, "hp")
                .unwrap()
                .name,
            "hp"
        );
        board.set_display(DisplaySlot::List, Some("hp"));
        assert_eq!(
            board.wire_objective(DisplaySlot::List, "hp").unwrap().name,
            "hp"
        );
        assert_eq!(
            board
                .wire_objective(DisplaySlot::Sidebar, "hp")
                .unwrap()
                .name,
            "hp#1"
        );
    }
}
