use std::collections::VecDeque;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use bevy_ecs::prelude::*;
use voidmc::world::{CHUNK_MAX_Y, CHUNK_MIN_Y};
use voidmc::{DimensionId, TextColor, WorldMessages};

use super::WorldEditConfig;
use super::extent::ChunkExtent;
use super::session::session_mut;
use crate::clipboard::Clipboard;
use crate::history::ChangeSet;
use crate::job::{Budget, CopyJob, EditJob};
use crate::math::BlockPos;
use crate::operation::{Fill, Operation, Paste, Sequence};
use crate::region::Region;

const MAX_BACKGROUND_TASKS: usize = 4;

pub(crate) const HEIGHT: (i32, i32) = (CHUNK_MIN_Y, CHUNK_MAX_Y);

/// An edit waiting for the queue: an operation in a dimension, optionally
/// owned by a player whose history records it and who hears when it is done.
pub struct Edit {
    dimension: DimensionId,
    owner: Option<Entity>,
    label: String,
    task: Task,
}

impl Edit {
    pub fn new(dimension: DimensionId, operation: impl Operation + 'static) -> Self {
        Self::from_arc(dimension, Arc::new(operation))
    }

    pub fn from_arc(dimension: DimensionId, operation: Arc<dyn Operation>) -> Self {
        Self {
            dimension,
            owner: None,
            label: "Edit".to_string(),
            task: Task::Edit {
                job: EditJob::from_arc(operation, HEIGHT),
                record: Record::History,
            },
        }
    }

    pub fn owner(mut self, player: Entity) -> Self {
        self.owner = Some(player);
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub(crate) fn recording(mut self, record: Record) -> Self {
        if let Task::Edit { record: slot, .. } = &mut self.task {
            *slot = record;
        }
        self
    }

    pub(crate) fn copy(
        dimension: DimensionId,
        region: Arc<dyn Region>,
        origin: BlockPos,
        then: AfterCopy,
    ) -> Self {
        Self {
            dimension,
            owner: None,
            label: "Copy".to_string(),
            task: Task::Copy {
                job: CopyJob::new(region.clone(), origin, HEIGHT),
                region,
                origin,
                then,
            },
        }
    }
}

pub(crate) enum Record {
    History,
    Undo(Arc<ChangeSet>),
    Redo(Arc<ChangeSet>),
}

pub(crate) enum AfterCopy {
    Store,
    Cut,
    Move {
        offset: BlockPos,
        skip_air: bool,
        select: bool,
    },
    Stack {
        offsets: Vec<BlockPos>,
        skip_air: bool,
        select: bool,
    },
}

enum Task {
    Edit {
        job: EditJob,
        record: Record,
    },
    Copy {
        job: CopyJob,
        region: Arc<dyn Region>,
        origin: BlockPos,
        then: AfterCopy,
    },
}

struct Pending {
    edit: Edit,
    started: Instant,
    reported: Instant,
}

pub(crate) struct Finished {
    pub clipboard: Option<(Clipboard, BlockPos)>,
    pub message: String,
}

struct Background {
    owner: Option<Entity>,
    handle: JoinHandle<Result<Finished, String>>,
}

#[derive(Resource, Default)]
pub struct EditQueue {
    pending: VecDeque<Pending>,
    background: Vec<Background>,
}

impl EditQueue {
    pub fn submit(&mut self, edit: Edit) {
        let now = Instant::now();
        self.pending.push_back(Pending {
            edit,
            started: now,
            reported: now,
        });
    }

    pub(crate) fn spawn(
        &mut self,
        owner: Entity,
        work: impl FnOnce() -> Result<Finished, String> + Send + 'static,
    ) -> Result<(), String> {
        if self.background.len() >= MAX_BACKGROUND_TASKS {
            return Err(
                "The server is busy with other schematic work; try again shortly.".to_string(),
            );
        }
        self.background.push(Background {
            owner: Some(owner),
            handle: std::thread::spawn(work),
        });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.pending.len() + self.background.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.background.is_empty()
    }

    pub fn is_busy(&self, player: Entity) -> bool {
        self.pending.iter().any(|p| p.edit.owner == Some(player))
            || self.background.iter().any(|b| b.owner == Some(player))
    }

    /// Lets a leaving player's edits finish without recording them anywhere.
    pub fn detach(&mut self, player: Entity) {
        for pending in &mut self.pending {
            if pending.edit.owner == Some(player) {
                pending.edit.owner = None;
            }
        }
        for task in &mut self.background {
            if task.owner == Some(player) {
                task.owner = None;
            }
        }
    }
}

fn finish_background(world: &mut World) {
    let mut queue = world.resource_mut::<EditQueue>();
    if queue.background.is_empty() {
        return;
    }
    let (done, running) = std::mem::take(&mut queue.background)
        .into_iter()
        .partition::<Vec<_>, _>(|task| task.handle.is_finished());
    queue.background = running;
    for task in done {
        let result = task
            .handle
            .join()
            .unwrap_or_else(|_| Err("The task failed.".to_string()));
        let Some(owner) = task.owner else {
            continue;
        };
        match result {
            Ok(finished) => {
                if let Some((clipboard, origin)) = finished.clipboard
                    && let Some(mut session) = session_mut(world, owner)
                {
                    session.set_clipboard(clipboard, origin);
                }
                reply(world, owner, &finished.message, TextColor::LightPurple);
            }
            Err(error) => reply(world, owner, &error, TextColor::Red),
        }
    }
}

pub(crate) fn run_edit_queue(world: &mut World) {
    finish_background(world);
    if world.resource::<EditQueue>().pending.is_empty() {
        return;
    }
    let config = world.resource::<WorldEditConfig>().clone();
    let mut budget = Budget::blocks(config.blocks_per_tick).with_time(config.time_per_tick);
    let mut local = std::mem::take(&mut world.resource_mut::<EditQueue>().pending);
    while !budget.exhausted() {
        let Some(mut pending) = local.pop_front() else {
            break;
        };
        let done = {
            let mut extent = ChunkExtent::new(world, pending.edit.dimension);
            match &mut pending.edit.task {
                Task::Edit { job, .. } => job.step(&mut extent, &mut budget),
                Task::Copy { job, .. } => job.step(&mut extent, &mut budget),
            }
        };
        if done {
            if let Some(next) = complete(world, pending) {
                local.push_front(next);
            }
        } else {
            report_progress(world, &mut pending);
            local.push_front(pending);
        }
    }
    let mut queue = world.resource_mut::<EditQueue>();
    local.append(&mut queue.pending);
    queue.pending = local;
}

fn report_progress(world: &World, pending: &mut Pending) {
    let Some(owner) = pending.edit.owner else {
        return;
    };
    if pending.reported.elapsed().as_millis() < 1000 {
        return;
    }
    pending.reported = Instant::now();
    let (done, total) = match &pending.edit.task {
        Task::Edit { job, .. } => job.progress(),
        Task::Copy { job, .. } => job.progress(),
    };
    let percent = done * 100 / total.max(1);
    WorldMessages::new(world)
        .action_bar(owner, format!("{}: {percent}%", pending.edit.label))
        .color(TextColor::Gray)
        .send();
}

fn complete(world: &mut World, pending: Pending) -> Option<Pending> {
    let Pending { edit, started, .. } = pending;
    let Edit {
        dimension,
        owner,
        label,
        task,
    } = edit;
    let elapsed = started.elapsed().as_secs_f64();
    match task {
        Task::Edit { job, record } => {
            let (changes, stats) = job.finish();
            let owner = owner?;
            let changed = changes.changed_blocks();
            let mut recorded = true;
            if let Some(mut session) = session_mut(world, owner) {
                recorded = match record {
                    Record::History => session.history.push(changes, dimension),
                    Record::Undo(original) => {
                        session.history.push_redo(original, dimension);
                        true
                    }
                    Record::Redo(original) => {
                        session.history.push_undo(original, dimension);
                        true
                    }
                };
            }
            let mut message = format!("{label}: {changed} blocks changed in {elapsed:.2}s.");
            if !recorded {
                message.push_str(" It was too large for your history and cannot be undone.");
            }
            if stats.unloaded_sections > 0 {
                message.push_str(&format!(
                    " {} unloaded sections were skipped.",
                    stats.unloaded_sections
                ));
            }
            reply(world, owner, &message, TextColor::LightPurple);
            None
        }
        Task::Copy {
            job,
            region,
            origin,
            then,
        } => {
            let skipped = job.unloaded_sections();
            let clipboard = job.finish();
            let owner = owner?;
            if skipped > 0 {
                reply(
                    world,
                    owner,
                    &format!("{skipped} unloaded sections were not copied."),
                    TextColor::Yellow,
                );
            }
            let count = clipboard.block_count();
            let follow = match then {
                AfterCopy::Store => {
                    if let Some(mut session) = session_mut(world, owner) {
                        session.set_clipboard(clipboard, origin);
                    }
                    reply(
                        world,
                        owner,
                        &format!("{count} blocks copied in {elapsed:.2}s."),
                        TextColor::LightPurple,
                    );
                    return None;
                }
                AfterCopy::Cut => {
                    if let Some(mut session) = session_mut(world, owner) {
                        session.set_clipboard(clipboard, origin);
                    }
                    Edit::from_arc(
                        dimension,
                        Arc::new(Fill {
                            region,
                            pattern: crate::block::BlockState::AIR.into(),
                            mask: crate::mask::Mask::Any,
                        }),
                    )
                    .label("Cut")
                }
                AfterCopy::Move {
                    offset,
                    skip_air,
                    select,
                } => {
                    let clipboard = Arc::new(clipboard);
                    if select {
                        shift_selection(world, owner, offset);
                    }
                    Edit::new(
                        dimension,
                        Sequence(vec![
                            Box::new(Fill {
                                region,
                                pattern: crate::block::BlockState::AIR.into(),
                                mask: crate::mask::Mask::Any,
                            }),
                            Box::new(Paste::new(clipboard, offset).skip_air(skip_air)),
                        ]),
                    )
                    .label("Move")
                }
                AfterCopy::Stack {
                    offsets,
                    skip_air,
                    select,
                } => {
                    if select && let Some(last) = offsets.last() {
                        shift_selection(world, owner, *last);
                    }
                    let mut paste =
                        Paste::new(Arc::new(clipboard), BlockPos::ZERO).skip_air(skip_air);
                    paste.origins = offsets;
                    Edit::new(dimension, paste).label("Stack")
                }
            };
            Some(Pending {
                edit: follow.owner(owner),
                started,
                reported: Instant::now(),
            })
        }
    }
}

fn shift_selection(world: &mut World, owner: Entity, offset: BlockPos) {
    if let Some(mut session) = session_mut(world, owner) {
        let _ = session.selection.shift(offset);
    }
}

pub(crate) fn reply(world: &World, player: Entity, message: &str, color: TextColor) {
    WorldMessages::new(world)
        .message(player, message)
        .color(color)
        .send();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_tasks_are_capped_across_players() {
        let mut world = World::new();
        let mut queue = EditQueue::default();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let wait = Arc::new(std::sync::Mutex::new(wait));
        for _ in 0..MAX_BACKGROUND_TASKS {
            let wait = wait.clone();
            let player = world.spawn_empty().id();
            queue
                .spawn(player, move || {
                    let _ = wait.lock().unwrap().recv();
                    Err(String::new())
                })
                .unwrap();
        }
        let late = world.spawn_empty().id();
        assert!(queue.spawn(late, || Err(String::new())).is_err());
        assert!(!queue.is_busy(late));
        drop(release);
        for task in queue.background.drain(..) {
            let _ = task.handle.join();
        }
    }
}
