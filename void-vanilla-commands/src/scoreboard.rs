use std::sync::Arc;

use bevy_ecs::prelude::{DetectChangesMut, Without};
use voidmc::{
    BoolArg, Command, CommandBuilder, CommandContext, DisplaySlot, IntegerArg, Objective,
    RenderType, ScoreFormat,
};

use crate::args::{
    ComponentArg, CriteriaArg, NameArg, ObjectiveArg, Operation, OperationArg, ScoreHolderArg,
    SlotArg, StyleArg, slot_name,
};
use crate::model::{Criteria, DisplayedObjective, ScoreObjective, Scoreboard};
use crate::text::StyledText;
use crate::{Access, respond};

type Outcome = Result<String, String>;

/// `numberformat` with its `blank`, `fixed <component>` and `styled <style>`
/// branches; `apply` receives `None` to clear the format.
fn number_format(
    base: CommandBuilder,
    apply: fn(&mut CommandContext, Option<ScoreFormat>),
) -> CommandBuilder {
    base.handler(move |ctx| apply(ctx, None))
        .subcommand(
            CommandBuilder::new("blank").handler(move |ctx| apply(ctx, Some(ScoreFormat::Blank))),
        )
        .subcommand(
            CommandBuilder::new("fixed")
                .arg_variadic_required("contents", Arc::new(ComponentArg))
                .handler(move |ctx| {
                    let text = ctx.get::<StyledText>("contents").unwrap().text.clone();
                    apply(ctx, Some(ScoreFormat::Fixed(text)))
                }),
        )
        .subcommand(
            CommandBuilder::new("styled")
                .arg_variadic_required("style", Arc::new(StyleArg))
                .handler(move |ctx| {
                    let format = ctx.get::<ScoreFormat>("style").unwrap().clone();
                    apply(ctx, Some(format))
                }),
        )
}

pub fn scoreboard_command(access: Access) -> Command {
    let render = |name: &str, render: RenderType| {
        CommandBuilder::new(name).handler(move |ctx| set_render(ctx, render))
    };
    let objectives = CommandBuilder::new("objectives")
        .subcommand(CommandBuilder::new("list").handler(list_objectives))
        .subcommand(
            CommandBuilder::new("add")
                .arg("objective", Arc::new(NameArg))
                .arg("criteria", Arc::new(CriteriaArg))
                .arg_variadic("displayName", Arc::new(ComponentArg))
                .handler(add_objective),
        )
        .subcommand(
            CommandBuilder::new("remove")
                .arg("objective", Arc::new(ObjectiveArg))
                .handler(remove_objective),
        )
        .subcommand(
            CommandBuilder::new("setdisplay")
                .arg("slot", Arc::new(SlotArg))
                .arg_optional("objective", Arc::new(ObjectiveArg))
                .handler(set_display),
        )
        .subcommand(
            CommandBuilder::new("modify")
                .arg("objective", Arc::new(ObjectiveArg))
                .subcommand(
                    CommandBuilder::new("displayname")
                        .arg_variadic_required("displayName", Arc::new(ComponentArg))
                        .handler(set_display_name),
                )
                .subcommand(
                    CommandBuilder::new("rendertype")
                        .subcommand(render("hearts", RenderType::Hearts))
                        .subcommand(render("integer", RenderType::Integer)),
                )
                .subcommand(
                    CommandBuilder::new("displayautoupdate")
                        .arg("value", Arc::new(BoolArg))
                        .handler(set_auto_update),
                )
                .subcommand(number_format(
                    CommandBuilder::new("numberformat"),
                    set_objective_format,
                )),
        );

    let players = CommandBuilder::new("players")
        .subcommand(
            CommandBuilder::new("list")
                .arg_optional("target", ScoreHolderArg::single())
                .handler(list_players),
        )
        .subcommand(
            CommandBuilder::new("get")
                .arg("target", ScoreHolderArg::single())
                .arg("objective", Arc::new(ObjectiveArg))
                .handler(get_score),
        )
        .subcommand(
            CommandBuilder::new("set")
                .arg("targets", ScoreHolderArg::multiple())
                .arg("objective", Arc::new(ObjectiveArg))
                .arg("score", IntegerArg::unbounded())
                .handler(set_score),
        )
        .subcommand(
            CommandBuilder::new("add")
                .arg("targets", ScoreHolderArg::multiple())
                .arg("objective", Arc::new(ObjectiveArg))
                .arg("score", IntegerArg::min(0))
                .handler(add_score),
        )
        .subcommand(
            CommandBuilder::new("remove")
                .arg("targets", ScoreHolderArg::multiple())
                .arg("objective", Arc::new(ObjectiveArg))
                .arg("score", IntegerArg::min(0))
                .handler(remove_score),
        )
        .subcommand(
            CommandBuilder::new("reset")
                .arg("targets", ScoreHolderArg::multiple())
                .arg_optional("objective", Arc::new(ObjectiveArg))
                .handler(reset_scores),
        )
        .subcommand(
            CommandBuilder::new("enable")
                .arg("targets", ScoreHolderArg::multiple())
                .arg("objective", Arc::new(ObjectiveArg))
                .handler(enable_trigger),
        )
        .subcommand(
            CommandBuilder::new("operation")
                .arg("targets", ScoreHolderArg::multiple())
                .arg("targetObjective", Arc::new(ObjectiveArg))
                .arg("operation", Arc::new(OperationArg))
                .arg("source", ScoreHolderArg::multiple())
                .arg("sourceObjective", Arc::new(ObjectiveArg))
                .handler(operation),
        )
        .subcommand(
            CommandBuilder::new("display")
                .subcommand(
                    CommandBuilder::new("name")
                        .arg("targets", ScoreHolderArg::multiple())
                        .arg("objective", Arc::new(ObjectiveArg))
                        .arg_variadic("text", Arc::new(ComponentArg))
                        .handler(set_score_name),
                )
                .subcommand(number_format(
                    CommandBuilder::new("numberformat")
                        .arg("targets", ScoreHolderArg::multiple())
                        .arg("objective", Arc::new(ObjectiveArg)),
                    set_score_format,
                )),
        );

    CommandBuilder::new("scoreboard")
        .description("Manage scoreboard objectives and scores")
        .requires(move |world, player| access.allows(world, player))
        .subcommand(objectives)
        .subcommand(players)
        .build()
}

fn edit(ctx: &mut CommandContext, f: impl FnOnce(&mut Scoreboard) -> Outcome) -> bool {
    let outcome = ctx.with_world_mut(|world| {
        let mut board = world.get_resource_or_init::<Scoreboard>();
        let outcome = f(board.bypass_change_detection());
        if outcome.is_ok() {
            board.set_changed();
        }
        outcome
    });
    let done = outcome.is_ok();
    respond(ctx, outcome);
    done
}

fn read<R>(ctx: &CommandContext, f: impl FnOnce(&Scoreboard) -> R) -> R {
    ctx.with_world(|world| match world.get_resource::<Scoreboard>() {
        Some(board) => f(board),
        None => f(&Scoreboard::default()),
    })
}

fn edit_objective(ctx: &mut CommandContext, f: impl FnOnce(&str, &mut ScoreObjective) -> Outcome) {
    let name = string(ctx, "objective");
    edit(ctx, |board| {
        let objective = board
            .objective_mut(&name)
            .ok_or_else(|| format!("Unknown scoreboard objective '{name}'"))?;
        f(&name, objective)
    });
}

/// Engine objectives spawned by other code win their slot and name for the
/// players they already show to, so warn instead of failing silently.
fn warn_if_claimed(ctx: &CommandContext, slot: DisplaySlot, name: &str) {
    let claimed = ctx.with_world(|world| {
        world
            .try_query_filtered::<&Objective, Without<DisplayedObjective>>()
            .is_some_and(|mut query| {
                query
                    .iter(world)
                    .any(|other| other.slot == slot || other.name == name)
            })
    });
    if claimed {
        ctx.reply_error(
            "Another objective on this server already uses that display slot or name; \
             players who see it will not see this one",
        );
    }
}

fn string(ctx: &CommandContext, name: &str) -> String {
    ctx.get::<String>(name).cloned().unwrap_or_default()
}

fn holders(ctx: &CommandContext, name: &str) -> Vec<String> {
    ctx.get::<Vec<String>>(name).cloned().unwrap_or_default()
}

fn label(objective: &ScoreObjective) -> String {
    format!("[{}]", objective.title)
}

fn objective_label(board: &Scoreboard, name: &str) -> String {
    board.objective(name).map(label).unwrap_or_default()
}

fn list_objectives(ctx: &mut CommandContext) {
    let outcome = read(ctx, |board| {
        let labels: Vec<String> = board.objectives().map(|(_, o)| label(o)).collect();
        if labels.is_empty() {
            return Ok("There are no objectives".into());
        }
        Ok(format!(
            "There are {} objective(s): {}",
            labels.len(),
            labels.join(", ")
        ))
    });
    respond(ctx, outcome);
}

fn add_objective(ctx: &mut CommandContext) {
    let name = string(ctx, "objective");
    let criteria = *ctx.get::<Criteria>("criteria").unwrap();
    let display = ctx.get::<StyledText>("displayName").cloned();
    edit(ctx, |board| {
        let objective = board
            .add_objective(&name, criteria)
            .ok_or("An objective already exists by that name")?;
        if let Some(display) = display {
            objective.title = display.text;
            objective.color = display.color.unwrap_or(objective.color);
        }
        Ok(format!("Created new objective {}", label(objective)))
    });
}

fn remove_objective(ctx: &mut CommandContext) {
    let name = string(ctx, "objective");
    edit(ctx, |board| {
        let removed = board.remove_objective(&name).unwrap();
        Ok(format!("Removed objective {}", label(&removed)))
    });
}

fn set_display(ctx: &mut CommandContext) {
    let slot = *ctx.get::<DisplaySlot>("slot").unwrap();
    let objective = ctx.get::<String>("objective").cloned();
    let shown = objective.clone();
    let done = edit(ctx, |board| match objective {
        None if board.display(slot).is_none() => {
            Err("Nothing changed. That display slot is already empty".into())
        }
        None => {
            board.set_display(slot, None);
            Ok(format!(
                "Cleared any objectives in display slot {}",
                slot_name(slot)
            ))
        }
        Some(name) if board.display(slot) == Some(&name) => {
            Err("Nothing changed. That display slot is already showing that objective".into())
        }
        Some(name) => {
            board.set_display(slot, Some(&name));
            Ok(format!(
                "Set display slot {} to show objective {}",
                slot_name(slot),
                objective_label(board, &name)
            ))
        }
    });
    if let (true, Some(name)) = (done, shown) {
        warn_if_claimed(ctx, slot, &name);
    }
}

fn set_display_name(ctx: &mut CommandContext) {
    let display = ctx.get::<StyledText>("displayName").cloned().unwrap();
    edit_objective(ctx, |name, objective| {
        objective.title = display.text;
        objective.color = display.color.unwrap_or_default();
        Ok(format!(
            "Changed the display name of {name} to {}",
            label(objective)
        ))
    });
}

fn set_render(ctx: &mut CommandContext, render: RenderType) {
    edit_objective(ctx, |_, objective| {
        objective.render = render;
        Ok(format!(
            "Changed the render type of objective {}",
            label(objective)
        ))
    });
}

fn set_auto_update(ctx: &mut CommandContext) {
    let enabled = *ctx.get::<bool>("value").unwrap();
    edit_objective(ctx, |_, objective| {
        objective.display_auto_update = enabled;
        let state = if enabled { "enabled" } else { "disabled" };
        Ok(format!(
            "Objective {} now has display auto update {state}",
            label(objective)
        ))
    });
}

fn set_objective_format(ctx: &mut CommandContext, format: Option<ScoreFormat>) {
    edit_objective(ctx, |_, objective| {
        let verb = if format.is_some() {
            "Changed"
        } else {
            "Cleared"
        };
        objective.format = format;
        Ok(format!(
            "{verb} objective {} number format",
            label(objective)
        ))
    });
}

fn list_players(ctx: &mut CommandContext) {
    let target = ctx.get::<String>("target").cloned();
    let lines = read(ctx, |board| match target {
        None => {
            let holders: Vec<&str> = board.holders().into_iter().collect();
            if holders.is_empty() {
                return vec!["There are no tracked entities".to_string()];
            }
            vec![format!(
                "There are {} tracked entity/entities: {}",
                holders.len(),
                holders.join(", ")
            )]
        }
        Some(target) => {
            let scores: Vec<String> = board
                .objectives()
                .filter_map(|(_, objective)| {
                    objective
                        .get(&target)
                        .map(|value| format!("{}: {value}", label(objective)))
                })
                .collect();
            if scores.is_empty() {
                return vec![format!("{target} has no scores to show")];
            }
            let mut lines = vec![format!("{target} has {} score(s):", scores.len())];
            lines.extend(scores);
            lines
        }
    });
    for line in lines {
        ctx.reply(&line);
    }
}

fn get_score(ctx: &mut CommandContext) {
    let target = string(ctx, "target");
    let name = string(ctx, "objective");
    let outcome = read(ctx, |board| {
        let objective = board
            .objective(&name)
            .ok_or_else(|| format!("Unknown scoreboard objective '{name}'"))?;
        match objective.get(&target) {
            Some(value) => Ok(format!("{target} has {value} {}", label(objective))),
            None => Err(format!(
                "Can't get value of {name} for {target}; none is set"
            )),
        }
    });
    respond(ctx, outcome);
}

/// Runs `change` on each target's score (missing scores start at 0) and
/// reports it the vanilla way.
fn update_scores(
    ctx: &mut CommandContext,
    single: impl Fn(&str, &str, i32) -> String,
    multiple: impl Fn(&str, usize) -> String,
    change: impl Fn(i32) -> i32,
) {
    let targets = holders(ctx, "targets");
    edit_objective(ctx, |_, objective| {
        let mut last = 0;
        for target in &targets {
            last = change(objective.get(target).unwrap_or(0));
            objective.set(target.clone(), last);
        }
        let label = label(objective);
        Ok(match targets.as_slice() {
            [target] => single(&label, target, last),
            many => multiple(&label, many.len()),
        })
    });
}

fn set_score(ctx: &mut CommandContext) {
    let score = *ctx.get::<i32>("score").unwrap();
    update_scores(
        ctx,
        |objective, target, value| format!("Set {objective} for {target} to {value}"),
        |objective, count| format!("Set {objective} for {count} entities to {score}"),
        |_| score,
    );
}

fn add_score(ctx: &mut CommandContext) {
    let amount = *ctx.get::<i32>("score").unwrap();
    update_scores(
        ctx,
        |objective, target, value| {
            format!("Added {amount} to {objective} for {target} (now {value})")
        },
        |objective, count| format!("Added {amount} to {objective} for {count} entities"),
        |value| value.wrapping_add(amount),
    );
}

fn remove_score(ctx: &mut CommandContext) {
    let amount = *ctx.get::<i32>("score").unwrap();
    update_scores(
        ctx,
        |objective, target, value| {
            format!("Removed {amount} from {objective} for {target} (now {value})")
        },
        |objective, count| format!("Removed {amount} from {objective} for {count} entities"),
        |value| value.wrapping_sub(amount),
    );
}

fn reset_scores(ctx: &mut CommandContext) {
    let targets = holders(ctx, "targets");
    let objective = ctx.get::<String>("objective").cloned();
    edit(ctx, |board| {
        for target in &targets {
            board.reset(target, objective.as_deref());
        }
        let who = match targets.as_slice() {
            [target] => target.clone(),
            many => format!("{} entities", many.len()),
        };
        Ok(match &objective {
            Some(name) => format!("Reset {} for {who}", objective_label(board, name)),
            None => format!("Reset all scores for {who}"),
        })
    });
}

fn enable_trigger(ctx: &mut CommandContext) {
    let targets = holders(ctx, "targets");
    edit_objective(ctx, |_, objective| {
        if objective.criteria != Criteria::Trigger {
            return Err("Can only enable trigger-objectives".into());
        }
        let enabled = targets.iter().filter(|t| objective.enable(t)).count();
        if enabled == 0 {
            return Err("Nothing changed. That trigger is already enabled".into());
        }
        Ok(match targets.as_slice() {
            [target] => format!("Enabled trigger {} for {target}", label(objective)),
            many => format!(
                "Enabled trigger {} for {} entities",
                label(objective),
                many.len()
            ),
        })
    });
}

fn operation(ctx: &mut CommandContext) {
    let targets = holders(ctx, "targets");
    let sources = holders(ctx, "source");
    let target_objective = string(ctx, "targetObjective");
    let source_objective = string(ctx, "sourceObjective");
    let operation = *ctx.get::<Operation>("operation").unwrap();
    edit(ctx, |board| {
        let mut values = std::collections::BTreeMap::new();
        let read = |values: &mut std::collections::BTreeMap<(String, String), i32>,
                    holder: &str,
                    objective: &str| {
            *values
                .entry((holder.to_string(), objective.to_string()))
                .or_insert_with(|| board.get(holder, objective).unwrap_or(0))
        };
        for target in &targets {
            for source in &sources {
                let current = read(&mut values, target, &target_objective);
                let other = read(&mut values, source, &source_objective);
                let (target_value, source_value) = operation.apply(current, other)?;
                values.insert((source.clone(), source_objective.clone()), source_value);
                values.insert((target.clone(), target_objective.clone()), target_value);
            }
        }
        for ((holder, objective), value) in values {
            board.set(&holder, &objective, value);
        }
        let label = objective_label(board, &target_objective);
        Ok(match targets.as_slice() {
            [target] => format!(
                "Set {label} for {target} to {}",
                board.get(target, &target_objective).unwrap_or(0)
            ),
            many => format!("Updated {label} for {} entities", many.len()),
        })
    });
}

fn set_score_name(ctx: &mut CommandContext) {
    let targets = holders(ctx, "targets");
    let text = ctx.get::<StyledText>("text").map(|text| text.text.clone());
    edit_objective(ctx, |_, objective| {
        for target in &targets {
            objective.scores.entry(target.clone()).or_default().display = text.clone();
        }
        let label = label(objective);
        let who = match targets.as_slice() {
            [target] => target.clone(),
            many => format!("{} entities", many.len()),
        };
        Ok(match &text {
            Some(text) => format!("Changed the display name of {who} in {label} to {text}"),
            None => format!("Cleared the display name of {who} in {label}"),
        })
    });
}

fn set_score_format(ctx: &mut CommandContext, format: Option<ScoreFormat>) {
    let targets = holders(ctx, "targets");
    edit_objective(ctx, |_, objective| {
        let verb = if format.is_some() {
            "Changed"
        } else {
            "Cleared"
        };
        for target in &targets {
            objective.scores.entry(target.clone()).or_default().format = format.clone();
        }
        let who = match targets.as_slice() {
            [target] => target.clone(),
            many => format!("{} entities", many.len()),
        };
        Ok(format!(
            "{verb} the number format of {who} in {}",
            label(objective)
        ))
    });
}

#[cfg(test)]
mod tests {
    use voidmc::TextColor;
    use voidmc_protocol::clientbound::{ObjectiveAction, PlayPacket};

    use super::*;
    use crate::testing::{Harness, Run};

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Wire {
        Create(String),
        Update(String),
        Remove(String),
        Display(DisplaySlot, String),
        Score(String, String, i32),
        Reset(String, String),
    }

    fn wire(run: &Run, client: u32) -> Vec<Wire> {
        let mut packets: Vec<Wire> = run
            .packets
            .iter()
            .filter(|(id, _)| *id == client)
            .filter_map(|(_, packet)| {
                Some(match packet {
                    PlayPacket::SetObjective(p) => match p.action {
                        ObjectiveAction::Create(_) => Wire::Create(p.name.clone()),
                        ObjectiveAction::Update(_) => Wire::Update(p.name.clone()),
                        ObjectiveAction::Remove => Wire::Remove(p.name.clone()),
                    },
                    PlayPacket::SetDisplayObjective(p) => Wire::Display(p.slot, p.name.clone()),
                    PlayPacket::SetScore(p) => {
                        Wire::Score(p.objective.clone(), p.owner.clone(), p.value)
                    }
                    PlayPacket::ResetScore(p) => {
                        Wire::Reset(p.objective.clone().unwrap_or_default(), p.owner.clone())
                    }
                    _ => return None,
                })
            })
            .collect();
        packets.sort();
        packets
    }

    fn in_wire_order(run: &Run, client: u32) -> Vec<Wire> {
        run.packets
            .iter()
            .filter(|(id, _)| *id == client)
            .filter_map(|(_, packet)| match packet {
                PlayPacket::SetObjective(p) => Some(match p.action {
                    ObjectiveAction::Create(_) => Wire::Create(p.name.clone()),
                    ObjectiveAction::Update(_) => Wire::Update(p.name.clone()),
                    ObjectiveAction::Remove => Wire::Remove(p.name.clone()),
                }),
                _ => None,
            })
            .collect()
    }

    fn score(objective: &str, owner: &str, value: i32) -> Wire {
        Wire::Score(objective.into(), owner.into(), value)
    }

    #[test]
    fn objectives_are_only_sent_once_displayed() {
        let mut h = Harness::new();
        let run = h.ok(r#"scoreboard objectives add kills dummy {"text":"Kills","color":"gold"}"#);
        assert_eq!(run.reply(), "Created new objective [Kills]");
        assert!(run.packets.is_empty());
        let kills = h.board().objective("kills").unwrap();
        assert_eq!(
            (kills.title.as_str(), kills.color),
            ("Kills", TextColor::Gold)
        );

        h.ok("scoreboard players set Alice kills 3");
        let run = h.ok("scoreboard objectives setdisplay sidebar kills");
        assert_eq!(
            run.reply(),
            "Set display slot sidebar to show objective [Kills]"
        );
        assert_eq!(
            wire(&run, 2),
            vec![
                Wire::Create("kills".into()),
                Wire::Display(DisplaySlot::Sidebar, "kills".into()),
                score("kills", "Alice", 3),
            ]
        );

        let run = h.ok("scoreboard players add @a kills 2");
        assert_eq!(run.reply(), "Added 2 to [Kills] for 2 entities");
        assert_eq!(
            wire(&run, 1),
            vec![score("kills", "Alice", 5), score("kills", "Bob", 2)]
        );

        let run = h.ok("scoreboard players reset Bob kills");
        assert_eq!(run.reply(), "Reset [Kills] for Bob");
        assert_eq!(
            wire(&run, 1),
            vec![Wire::Reset("kills".into(), "Bob".into())]
        );

        let run = h.ok("scoreboard objectives setdisplay sidebar");
        assert_eq!(
            run.reply(),
            "Cleared any objectives in display slot sidebar"
        );
        assert_eq!(wire(&run, 1), vec![Wire::Remove("kills".into())]);
        assert_eq!(
            h.run("scoreboard objectives setdisplay sidebar").reply(),
            "Nothing changed. That display slot is already empty"
        );
        assert_eq!(h.board().get("Alice", "kills"), Some(5));
    }

    #[test]
    fn one_objective_in_two_slots_and_removal() {
        let mut h = Harness::new();
        h.ok("scoreboard objectives add hp dummy");
        h.ok("scoreboard players set Alice hp 20");
        h.ok("scoreboard objectives setdisplay sidebar hp");
        let run = h.ok("scoreboard objectives setdisplay list hp");
        assert_eq!(
            wire(&run, 1),
            vec![
                Wire::Create("hp".into()),
                Wire::Create("hp#1".into()),
                Wire::Remove("hp".into()),
                Wire::Display(DisplaySlot::List, "hp".into()),
                Wire::Display(DisplaySlot::Sidebar, "hp#1".into()),
                score("hp", "Alice", 20),
                score("hp#1", "Alice", 20),
            ]
        );
        let order = in_wire_order(&run, 1);
        let removed = order.iter().position(|w| *w == Wire::Remove("hp".into()));
        let created = order.iter().position(|w| *w == Wire::Create("hp".into()));
        assert!(removed < created, "{order:?}");
        assert_eq!(
            h.run("scoreboard objectives setdisplay list hp").reply(),
            "Nothing changed. That display slot is already showing that objective"
        );

        let run = h.ok("scoreboard objectives remove hp");
        assert_eq!(run.reply(), "Removed objective [hp]");
        assert_eq!(
            wire(&run, 1),
            vec![Wire::Remove("hp".into()), Wire::Remove("hp#1".into())]
        );
        assert_eq!(
            h.ok("scoreboard objectives list").reply(),
            "There are no objectives"
        );
    }

    #[test]
    fn modify_changes_the_header_once() {
        let mut h = Harness::new();
        h.ok("scoreboard objectives add kills dummy");
        h.ok("scoreboard objectives setdisplay below_name kills");

        let run = h.ok("scoreboard objectives modify kills displayname {text:Frags,color:red}");
        assert_eq!(run.reply(), "Changed the display name of kills to [Frags]");
        assert_eq!(wire(&run, 1), vec![Wire::Update("kills".into())]);

        h.ok("scoreboard objectives modify kills rendertype hearts");
        h.ok("scoreboard objectives modify kills numberformat styled {color:aqua}");
        h.ok("scoreboard objectives modify kills displayautoupdate true");
        let kills = h.board().objective("kills").unwrap();
        assert_eq!(kills.render, RenderType::Hearts);
        assert_eq!(kills.format, Some(ScoreFormat::Styled(TextColor::Aqua)));
        assert!(kills.display_auto_update);

        assert_eq!(
            h.ok("scoreboard objectives modify kills numberformat")
                .reply(),
            "Cleared objective [Frags] number format"
        );
        assert_eq!(h.board().objective("kills").unwrap().format, None);
        h.ok("scoreboard objectives modify kills numberformat fixed \"-\"");
        assert_eq!(
            h.board().objective("kills").unwrap().format,
            Some(ScoreFormat::Fixed("-".into()))
        );
        assert_eq!(
            h.ok("scoreboard objectives list").reply(),
            "There are 1 objective(s): [Frags]"
        );
    }

    #[test]
    fn player_scores_set_get_list_and_reset() {
        let mut h = Harness::new();
        h.ok("scoreboard objectives add kills dummy");
        h.ok("scoreboard objectives add deaths dummy");
        assert_eq!(
            h.ok("scoreboard players set @s kills -4").reply(),
            "Set [kills] for Alice to -4"
        );
        h.ok("scoreboard players remove #total kills 6");
        h.ok("scoreboard players set Alice deaths 1");
        assert_eq!(
            h.ok("scoreboard players get Alice kills").reply(),
            "Alice has -4 [kills]"
        );
        assert_eq!(
            h.run("scoreboard players get Bob kills").reply(),
            "Can't get value of kills for Bob; none is set"
        );
        assert_eq!(
            h.ok("scoreboard players list").reply(),
            "There are 2 tracked entity/entities: #total, Alice"
        );
        assert_eq!(
            h.run("scoreboard players list Alice").replies,
            vec!["Alice has 2 score(s):", "[deaths]: 1", "[kills]: -4"]
        );
        assert_eq!(
            h.run("scoreboard players add Alice kills -1").replies.len(),
            2
        );

        assert_eq!(
            h.ok("scoreboard players reset * ").reply(),
            "Reset all scores for 2 entities"
        );
        assert_eq!(
            h.ok("scoreboard players list").reply(),
            "There are no tracked entities"
        );
        assert_eq!(h.run("scoreboard players reset *").replies.len(), 2);
    }

    #[test]
    fn operations_combine_scores_across_objectives() {
        let mut h = Harness::new();
        h.ok("scoreboard objectives add a dummy");
        h.ok("scoreboard objectives add b dummy");
        h.ok("scoreboard players set Alice a 7");
        h.ok("scoreboard players set Bob b 2");

        assert_eq!(
            h.ok("scoreboard players operation Alice a /= Bob b")
                .reply(),
            "Set [a] for Alice to 3"
        );
        h.ok("scoreboard players operation Alice a >< Bob b");
        assert_eq!(h.board().get("Alice", "a"), Some(2));
        assert_eq!(h.board().get("Bob", "b"), Some(3));

        h.ok("scoreboard players operation @a a += Bob b");
        assert_eq!(h.board().get("Bob", "a"), Some(3));
        assert_eq!(h.board().get("Alice", "a"), Some(5));

        assert_eq!(
            h.run("scoreboard players operation Alice a %= Nobody b")
                .reply(),
            "Can't divide by zero"
        );
        assert_eq!(h.board().get("Alice", "a"), Some(5));
        assert_eq!(h.board().get("Nobody", "b"), None);
        assert_eq!(
            h.run("scoreboard players operation Alice a ^= Bob b")
                .replies
                .len(),
            2
        );
    }

    #[test]
    fn triggers_enable_once() {
        let mut h = Harness::new();
        h.ok("scoreboard objectives add vote trigger");
        h.ok("scoreboard objectives add kills dummy");
        assert_eq!(
            h.ok("scoreboard players enable @a vote").reply(),
            "Enabled trigger [vote] for 2 entities"
        );
        assert!(h.board().objective("vote").unwrap().is_enabled("Bob"));
        assert_eq!(
            h.run("scoreboard players enable Bob vote").reply(),
            "Nothing changed. That trigger is already enabled"
        );
        assert_eq!(
            h.run("scoreboard players enable Bob kills").reply(),
            "Can only enable trigger-objectives"
        );
        assert_eq!(
            h.run("scoreboard objectives add hp health").replies.len(),
            2
        );
    }

    #[test]
    fn per_score_display_name_and_format() {
        let mut h = Harness::new();
        h.ok("scoreboard objectives add race dummy");
        h.ok("scoreboard objectives setdisplay sidebar race");
        h.ok("scoreboard players set Alice race 1");

        let run = h.ok("scoreboard players display name Alice race {text:'1st place'}");
        assert_eq!(
            run.reply(),
            "Changed the display name of Alice in [race] to 1st place"
        );
        assert_eq!(wire(&run, 2), vec![score("race", "Alice", 1)]);
        let run = h.ok("scoreboard players display numberformat Alice race blank");
        assert_eq!(run.reply(), "Changed the number format of Alice in [race]");
        let alice = &h.board().objective("race").unwrap().scores["Alice"];
        assert_eq!(alice.display.as_deref(), Some("1st place"));
        assert_eq!(alice.format, Some(ScoreFormat::Blank));

        h.ok("scoreboard players display name Alice race");
        h.ok("scoreboard players display numberformat Alice race");
        let alice = &h.board().objective("race").unwrap().scores["Alice"];
        assert_eq!(
            (alice.display.as_ref(), alice.format.as_ref()),
            (None, None)
        );
    }

    #[test]
    fn setdisplay_warns_when_another_objective_holds_the_slot() {
        let mut h = Harness::new();
        h.app
            .world_mut()
            .spawn(voidmc::Objective::sidebar("altitude"));
        h.app.update();
        h.ok("scoreboard objectives add kills dummy");
        let run = h.run("scoreboard objectives setdisplay sidebar kills");
        assert_eq!(run.replies.len(), 2, "{:?}", run.replies);
        assert!(run.replies[1].starts_with("Another objective"));
        assert_eq!(
            h.ok("scoreboard objectives setdisplay list kills")
                .replies
                .len(),
            1
        );
    }

    #[test]
    fn failed_commands_leave_the_scoreboard_unchanged() {
        use bevy_ecs::prelude::DetectChanges;
        let mut h = Harness::new();
        h.ok("scoreboard objectives add kills dummy");
        let tick = h.app.world().read_change_tick();
        h.run("scoreboard objectives setdisplay sidebar");
        h.run("scoreboard players operation Alice kills /= Bob kills");
        let changed = h.app.world().resource_ref::<Scoreboard>().last_changed();
        assert!(changed.get() < tick.get());
    }

    #[test]
    fn the_bare_command_works_without_the_plugin_resource() {
        use voidmc::components::{ClientId, PlayerName, PlayerReady};
        let mut world = bevy_ecs::world::World::new();
        let (outgoing_tx, _rx) = flume::unbounded();
        let (_i, incoming) = flume::unbounded();
        let (_d, disconnect) = flume::unbounded();
        let (kick, _k) = flume::unbounded();
        world.insert_resource(voidmc::network::NetworkChannels {
            incoming,
            outgoing: outgoing_tx,
            disconnect,
            kick,
        });
        let mut registry = voidmc::CommandRegistry::new();
        registry.register(scoreboard_command(crate::Access::Everyone));
        world.insert_resource(registry);
        let player = world
            .spawn((ClientId(1), PlayerName("Alice".into()), PlayerReady))
            .id();
        for line in ["objectives list", "players list", "objectives add k dummy"] {
            let args = line.split_whitespace().map(String::from).collect();
            voidmc::commands::dispatch_command(&mut world, 1, player, "scoreboard", args);
        }
        assert!(world.resource::<Scoreboard>().objective("k").is_some());
    }

    #[test]
    fn code_can_drive_the_scoreboard_resource() {
        let mut h = Harness::new();
        {
            let mut board = h.app.world_mut().resource_mut::<Scoreboard>();
            board.add_objective("laps", Criteria::Dummy).unwrap().title = "Laps".into();
            board.set("Bob", "laps", 2);
            board.set_display(DisplaySlot::List, Some("laps"));
        }
        h.app.update();
        assert_eq!(
            h.ok("scoreboard players get Bob laps").reply(),
            "Bob has 2 [Laps]"
        );
        let world = h.app.world_mut();
        let displayed = world
            .query::<(&crate::DisplayedObjective, &voidmc::Objective)>()
            .single(world)
            .unwrap();
        assert_eq!(displayed.0.0, DisplaySlot::List);
        assert_eq!(displayed.1.get("Bob"), Some(2));
    }
}
