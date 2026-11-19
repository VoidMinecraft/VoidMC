//! Basic redstone: levers and buttons power redstone dust, and dust or a
//! lever/button triggers note blocks. Opt-in: add [`RedstonePlugin`].
//!
//! Supported, following vanilla 26.1.2:
//!
//! - **Lever** (every face and facing): a main-hand right-click toggles
//!   `powered` (vanilla secondary-use rules: not while sneaking with an item
//!   in either hand), with the lever click sound.
//! - **Buttons** (stone, polished blackstone and every wooden button): a
//!   right-click on an unpowered button powers it; it releases by itself after
//!   20 game ticks (stone, polished blackstone) or 30 (wood), with the
//!   matching click sounds. A click on a pressed button does nothing.
//! - A powered lever or button powers its six neighbours with level 15.
//! - **Redstone dust** (`redstone_wire`), flat only: a dust next to a powered
//!   lever or button has power 15, and power drops by one per dust along
//!   horizontally adjacent dust on the same level, so the 16th dust from the
//!   source is unpowered. Dust powers the block below it and the horizontal
//!   neighbours it points into, computed like vanilla's connection shape (a
//!   straight line points into the block past its end). Only the `power`
//!   property is written; the connection properties stay as placed.
//! - **Note block**: powered when a neighbouring lever/button is powered, a
//!   powered dust above it, or a powered dust beside it pointing into it. On
//!   the rising edge of that input it stores `powered=true` and plays its
//!   note once (sound + block event), unless a non-air block is directly
//!   above it. It plays again only after it was unpowered. The instrument and
//!   note come from its stored block state; the instrument is not
//!   recomputed from the block below.
//!
//! Not supported: dust going up or down a block, power through solid blocks
//! (a lever does not strongly power the block it is attached to), redstone
//! torches, repeaters, comparators, observers, pistons, redstone blocks,
//! pressure plates, tuning a note block by right-clicking it, and changes made
//! without a [`BlockChangeEvent`] (direct `ChunkData` writes).
//!
//! Propagation runs once per tick in [`VoidSystems::Redstone`], after the
//! block interactions of the tick, over the changed positions queued by
//! [`BlockChangeEvent`]. Each change recomputes the dust within 15 steps of
//! it from the sources within 30 steps: bounded work, no recursion, loops of
//! dust terminate, and unloaded chunks are never read or written. At most
//! [`MAX_CHANGES_PER_TICK`] changes are processed per tick; the rest wait for
//! the next one.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use voidmc_data::Version;
use voidmc_data::v26_1_2::props::{East3Up, Instrument, North3Up, South3Up, West3Up};
use voidmc_data::v26_1_2::state::{Lever, NoteBlock, RedstoneWire};
use voidmc_protocol::clientbound;
use voidmc_protocol::types::BlockPosition;

use crate::components::PlayerDimension;
use crate::events::BlockChangeEvent;
use crate::item_behavior::{BlockInteraction, BlockInteractionRegistry, ItemUseContext, UseResult};
use crate::players::{Audience, WorldPlayers};
use crate::schedule::VoidSystems;
use crate::sounds::{Sound, SoundSource, WorldSounds};
use crate::world::{ChunkData, ChunkIndex, ChunkPos, DimensionId, set_block_state};

const VERSION: Version = Version::V26_1_2;

/// Power of a lever or button, and of the first dust next to one.
pub const MAX_POWER: u8 = 15;

/// Changed positions processed per tick; the rest are kept for later ticks.
pub const MAX_CHANGES_PER_TICK: usize = 256;

/// How far (in dust steps) a change can alter dust power.
const UPDATE_RADIUS: u32 = MAX_POWER as u32;

/// How far (in dust steps) sources are looked for: a dust within
/// `UPDATE_RADIUS` of a change gets its power from a source at most
/// `MAX_POWER - 1` further away.
const SEARCH_RADIUS: u32 = 2 * MAX_POWER as u32;

/// Enables levers, buttons, flat redstone dust and note blocks (see the
/// [module docs](self) for exactly what is supported).
///
/// Not part of `DefaultPlugins`: without it, a right-click on a lever uses
/// the held item as before and no block reacts to power.
pub struct RedstonePlugin;

impl Plugin for RedstonePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Redstone>()
            .init_resource::<BlockInteractionRegistry>()
            .add_observer(queue_block_change)
            .configure_sets(
                Update,
                VoidSystems::Redstone
                    .after(VoidSystems::ItemUseDrain)
                    .after(VoidSystems::CommandDrain)
                    .after(VoidSystems::MenuClickDrain),
            )
            .add_systems(Update, redstone_tick.in_set(VoidSystems::Redstone));

        let mut registry = app.world_mut().resource_mut::<BlockInteractionRegistry>();
        registry.register_for("minecraft:lever", LeverInteraction);
        for block in voidmc_data::block_types(VERSION) {
            if button_kind(block.name).is_some() {
                registry.register_for(block.name, ButtonInteraction);
            }
        }
    }
}

/// Queued changes, scheduled button releases and the redstone tick counter.
#[derive(Resource, Default)]
pub struct Redstone {
    tick: u64,
    queue: VecDeque<(DimensionId, BlockPosition)>,
    queued: HashSet<(DimensionId, BlockPosition)>,
    releases: Vec<ButtonRelease>,
    /// Set while the plugin writes its own changes, which it already handles.
    applying: bool,
}

impl Redstone {
    /// Queues a position whose neighbourhood must be re-evaluated, as a
    /// [`BlockChangeEvent`] does for every redstone component.
    pub fn queue_change(&mut self, dimension: DimensionId, position: BlockPosition) {
        if self.queued.insert((dimension, position)) {
            self.queue.push_back((dimension, position));
        }
    }

    /// Changes waiting for a later tick.
    pub fn pending_changes(&self) -> usize {
        self.queue.len()
    }

    /// Redstone ticks run so far.
    pub fn tick(&self) -> u64 {
        self.tick
    }
}

struct ButtonRelease {
    dimension: DimensionId,
    position: BlockPosition,
    due: u64,
}

fn queue_block_change(event: On<BlockChangeEvent>, mut redstone: ResMut<Redstone>) {
    if redstone.applying || !(is_component(event.old_state) || is_component(event.new_state)) {
        return;
    }
    redstone.queue_change(event.dimension, event.position);
}

// --- Block kinds -------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ButtonKind {
    ticks: u64,
    click_on: &'static str,
    click_off: &'static str,
}

const STONE_BUTTON: ButtonKind = ButtonKind {
    ticks: 20,
    click_on: "block.stone_button.click_on",
    click_off: "block.stone_button.click_off",
};

const fn wooden(click_on: &'static str, click_off: &'static str) -> ButtonKind {
    ButtonKind {
        ticks: 30,
        click_on,
        click_off,
    }
}

fn button_kind(name: &str) -> Option<ButtonKind> {
    let material = name.strip_prefix("minecraft:")?.strip_suffix("_button")?;
    Some(match material {
        "stone" | "polished_blackstone" => STONE_BUTTON,
        "cherry" => wooden(
            "block.cherry_wood_button.click_on",
            "block.cherry_wood_button.click_off",
        ),
        "bamboo" => wooden(
            "block.bamboo_wood_button.click_on",
            "block.bamboo_wood_button.click_off",
        ),
        "crimson" | "warped" => wooden(
            "block.nether_wood_button.click_on",
            "block.nether_wood_button.click_off",
        ),
        _ => wooden(
            "block.wooden_button.click_on",
            "block.wooden_button.click_off",
        ),
    })
}

/// A lever or button: `Some(powered)`.
fn source_powered(state: i32) -> Option<bool> {
    if let Some(lever) = Lever::from_state_id(state) {
        return Some(lever.powered);
    }
    let block = voidmc_data::block_type_of_state(VERSION, state)?;
    button_kind(block.name)?;
    Some(block.property(state, "powered")? == "true")
}

fn is_component(state: i32) -> bool {
    source_powered(state).is_some()
        || RedstoneWire::from_state_id(state).is_some()
        || NoteBlock::from_state_id(state).is_some()
}

fn is_air(state: i32) -> bool {
    voidmc_data::block_type_of_state(VERSION, state).is_some_and(|block| {
        matches!(
            block.name,
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    })
}

// --- World access ------------------------------------------------------------

fn block_at(world: &World, dimension: DimensionId, position: BlockPosition) -> Option<i32> {
    let chunk = ChunkPos::new(position.x.div_euclid(16), position.z.div_euclid(16));
    let entity = *world.resource::<ChunkIndex>().0.get(&(dimension, chunk))?;
    world.get::<ChunkData>(entity)?.get_block(
        position.x.rem_euclid(16) as u8,
        i32::from(position.y),
        position.z.rem_euclid(16) as u8,
    )
}

fn offset(position: BlockPosition, (dx, dy, dz): (i32, i16, i32)) -> BlockPosition {
    BlockPosition {
        x: position.x + dx,
        y: position.y + dy,
        z: position.z + dz,
    }
}

const UP: (i32, i16, i32) = (0, 1, 0);
const DOWN: (i32, i16, i32) = (0, -1, 0);
/// North, east, south, west: the order of [`wire_outputs`].
const SIDES: [(i32, i16, i32); 4] = [(0, 0, -1), (1, 0, 0), (0, 0, 1), (-1, 0, 0)];
const NEIGHBOURS: [(i32, i16, i32); 6] = [UP, DOWN, SIDES[0], SIDES[1], SIDES[2], SIDES[3]];

fn neighbours(position: BlockPosition) -> impl Iterator<Item = BlockPosition> {
    NEIGHBOURS
        .into_iter()
        .map(move |delta| offset(position, delta))
}

fn wire_at(world: &World, dimension: DimensionId, position: BlockPosition) -> Option<RedstoneWire> {
    RedstoneWire::from_state_id(block_at(world, dimension, position)?)
}

/// Whether a lever or button next to `position` is powered.
fn next_to_powered_source(world: &World, dimension: DimensionId, position: BlockPosition) -> bool {
    neighbours(position).any(|neighbour| {
        block_at(world, dimension, neighbour).and_then(source_powered) == Some(true)
    })
}

/// The horizontal sides (north, east, south, west) a dust sends power into:
/// vanilla's `getConnectionState`, restricted to the same level. Dust
/// connects to dust, levers and buttons; a dust connected on one axis only
/// also points both ways along it; an unconnected dust placed as a dot points
/// nowhere, and otherwise everywhere.
fn wire_outputs(
    world: &World,
    dimension: DimensionId,
    position: BlockPosition,
    wire: RedstoneWire,
) -> [bool; 4] {
    let mut connected = SIDES.map(|side| {
        block_at(world, dimension, offset(position, side)).is_some_and(|state| {
            RedstoneWire::from_state_id(state).is_some() || source_powered(state).is_some()
        })
    });
    let was_dot = wire.north == North3Up::None
        && wire.east == East3Up::None
        && wire.south == South3Up::None
        && wire.west == West3Up::None;
    if was_dot && !connected.contains(&true) {
        return connected;
    }
    let [north, east, south, west] = connected;
    if !north && !south {
        connected[1] = true;
        connected[3] = true;
    }
    if !east && !west {
        connected[0] = true;
        connected[2] = true;
    }
    connected
}

/// Whether a note block at `position` receives power.
fn note_block_input(world: &World, dimension: DimensionId, position: BlockPosition) -> bool {
    if next_to_powered_source(world, dimension, position) {
        return true;
    }
    if wire_at(world, dimension, offset(position, UP)).is_some_and(|wire| wire.power > 0) {
        return true;
    }
    SIDES.iter().enumerate().any(|(index, side)| {
        let wire_position = offset(position, *side);
        let Some(wire) = wire_at(world, dimension, wire_position) else {
            return false;
        };
        // The dust must point back at the note block: the opposite side.
        wire.power > 0 && wire_outputs(world, dimension, wire_position, wire)[(index + 2) % 4]
    })
}

// --- Interactions ------------------------------------------------------------

struct LeverInteraction;

impl BlockInteraction for LeverInteraction {
    fn on_use(&self, ctx: &mut ItemUseContext, state: i32) -> UseResult {
        let (Some(target), Some(lever)) = (ctx.block, Lever::from_state_id(state)) else {
            return UseResult::Pass;
        };
        let player = ctx.player;
        let position = target.position;
        let pulled = Lever {
            powered: !lever.powered,
            ..lever
        };
        ctx.with_world_mut(|world| {
            let Some(dimension) = world.get::<PlayerDimension>(player).map(|d| d.0) else {
                return;
            };
            set_block_state(
                world,
                dimension,
                position,
                pulled.to_state_id(),
                Some(player),
            );
            tracing::debug!(?position, powered = pulled.powered, "Lever pulled");
            WorldSounds::new(world).play(
                Sound::new("block.lever.click")
                    .category(SoundSource::Blocks)
                    .volume(0.3)
                    .pitch(if pulled.powered { 0.6 } else { 0.5 })
                    .at(dimension, center(position)),
            );
        });
        UseResult::Handled
    }
}

struct ButtonInteraction;

impl BlockInteraction for ButtonInteraction {
    fn on_use(&self, ctx: &mut ItemUseContext, state: i32) -> UseResult {
        let Some(target) = ctx.block else {
            return UseResult::Pass;
        };
        let Some(block) = voidmc_data::block_type_of_state(VERSION, state) else {
            return UseResult::Pass;
        };
        let Some(kind) = button_kind(block.name) else {
            return UseResult::Pass;
        };
        if block.property(state, "powered") != Some("false") {
            // Already pressed: the click is consumed and changes nothing.
            return UseResult::Handled;
        }
        let Some(pressed) = block.with_property(state, "powered", "true") else {
            return UseResult::Pass;
        };
        let player = ctx.player;
        let position = target.position;
        ctx.with_world_mut(|world| {
            let Some(dimension) = world.get::<PlayerDimension>(player).map(|d| d.0) else {
                return;
            };
            if set_block_state(world, dimension, position, pressed, Some(player)).is_none() {
                return;
            }
            let mut redstone = world.resource_mut::<Redstone>();
            // The press runs before this tick's redstone tick, which counts
            // as the first of the button's ticks.
            let due = redstone.tick + 1 + kind.ticks;
            redstone.releases.push(ButtonRelease {
                dimension,
                position,
                due,
            });
            tracing::debug!(?position, due, "Button pressed");
            let chunk = ChunkPos::new(position.x.div_euclid(16), position.z.div_euclid(16));
            WorldSounds::new(world).play(
                Sound::new(kind.click_on)
                    .category(SoundSource::Blocks)
                    .at(dimension, center(position))
                    .audience(
                        Audience::custom(move |r| r.sees_chunk(dimension, chunk)).except(player),
                    ),
            );
        });
        UseResult::Handled
    }
}

fn center(position: BlockPosition) -> (f64, f64, f64) {
    (
        f64::from(position.x) + 0.5,
        f64::from(position.y) + 0.5,
        f64::from(position.z) + 0.5,
    )
}

// --- Tick ----------------------------------------------------------------------

fn redstone_tick(world: &mut World) {
    let tick = {
        let mut redstone = world.resource_mut::<Redstone>();
        redstone.tick += 1;
        redstone.tick
    };
    world.resource_mut::<Redstone>().applying = true;
    release_buttons(world, tick);
    let batch: Vec<(DimensionId, BlockPosition)> = {
        let mut redstone = world.resource_mut::<Redstone>();
        let count = redstone.queue.len().min(MAX_CHANGES_PER_TICK);
        let batch: Vec<_> = redstone.queue.drain(..count).collect();
        for change in &batch {
            redstone.queued.remove(change);
        }
        batch
    };

    // Like vanilla's neighbour updates, a change notifies the note blocks next
    // to it and next to the dust whose power it changed, not the changed block
    // itself: a note block placed next to powered dust waits for the next
    // change around it.
    let mut note_blocks = Vec::new();
    let mut seen = HashSet::new();
    for &(dimension, position) in &batch {
        let mut candidates: Vec<BlockPosition> = neighbours(position).collect();
        for wire in update_wires(world, dimension, position) {
            candidates.extend(neighbours(wire));
        }
        for candidate in candidates {
            if seen.insert((dimension, candidate)) {
                note_blocks.push((dimension, candidate));
            }
        }
    }
    for (dimension, position) in note_blocks {
        update_note_block(world, dimension, position);
    }
    world.resource_mut::<Redstone>().applying = false;
}

fn release_buttons(world: &mut World, tick: u64) {
    let due: Vec<ButtonRelease> = {
        let mut redstone = world.resource_mut::<Redstone>();
        let (due, waiting) = std::mem::take(&mut redstone.releases)
            .into_iter()
            .partition(|release| release.due <= tick);
        redstone.releases = waiting;
        due
    };
    for release in due {
        let Some(state) = block_at(world, release.dimension, release.position) else {
            continue;
        };
        let Some(block) = voidmc_data::block_type_of_state(VERSION, state) else {
            continue;
        };
        let Some(kind) = button_kind(block.name) else {
            continue; // broken or replaced while pressed
        };
        let Some(released) = block.with_property(state, "powered", "false") else {
            continue;
        };
        if set_block_state(world, release.dimension, release.position, released, None).is_none() {
            continue;
        }
        tracing::debug!(position = ?release.position, "Button released");
        world
            .resource_mut::<Redstone>()
            .queue_change(release.dimension, release.position);
        WorldSounds::new(world).play(
            Sound::new(kind.click_off)
                .category(SoundSource::Blocks)
                .at(release.dimension, center(release.position)),
        );
    }
}

/// Recomputes the power of the dust a change at `position` can affect and
/// writes the changed ones. Returns the dust whose power changed.
fn update_wires(
    world: &mut World,
    dimension: DimensionId,
    position: BlockPosition,
) -> Vec<BlockPosition> {
    // Dust reachable from the change, with its distance in dust steps.
    let mut depth: HashMap<BlockPosition, u32> = HashMap::new();
    let mut frontier = VecDeque::new();
    for start in std::iter::once(position).chain(neighbours(position)) {
        if wire_at(world, dimension, start).is_some() && depth.insert(start, 0).is_none() {
            frontier.push_back(start);
        }
    }
    let mut order = Vec::new();
    while let Some(wire) = frontier.pop_front() {
        order.push(wire);
        let next = depth[&wire] + 1;
        if next > SEARCH_RADIUS {
            continue;
        }
        for side in SIDES {
            let neighbour = offset(wire, side);
            if !depth.contains_key(&neighbour) && wire_at(world, dimension, neighbour).is_some() {
                depth.insert(neighbour, next);
                frontier.push_back(neighbour);
            }
        }
    }

    // Power from the dust next to a powered lever or button, one less per step.
    let mut power: HashMap<BlockPosition, u8> = HashMap::new();
    let mut frontier = VecDeque::new();
    for &wire in &order {
        if next_to_powered_source(world, dimension, wire) {
            power.insert(wire, MAX_POWER);
            frontier.push_back(wire);
        }
    }
    while let Some(wire) = frontier.pop_front() {
        let next = power[&wire] - 1;
        if next == 0 {
            continue;
        }
        for side in SIDES {
            let neighbour = offset(wire, side);
            if depth.contains_key(&neighbour) && !power.contains_key(&neighbour) {
                power.insert(neighbour, next);
                frontier.push_back(neighbour);
            }
        }
    }

    let mut changed = Vec::new();
    for wire_position in order {
        if depth[&wire_position] > UPDATE_RADIUS {
            continue;
        }
        let Some(wire) = wire_at(world, dimension, wire_position) else {
            continue;
        };
        let target = power.get(&wire_position).copied().unwrap_or(0);
        if wire.power != target {
            let updated = RedstoneWire {
                power: target,
                ..wire
            };
            set_block_state(world, dimension, wire_position, updated.to_state_id(), None);
            changed.push(wire_position);
        }
    }
    changed
}

/// Stores a note block's new input and plays it on a rising edge.
fn update_note_block(world: &mut World, dimension: DimensionId, position: BlockPosition) {
    let Some(note_block) = block_at(world, dimension, position).and_then(NoteBlock::from_state_id)
    else {
        return;
    };
    let powered = note_block_input(world, dimension, position);
    if powered == note_block.powered {
        return;
    }
    let updated = NoteBlock {
        powered,
        ..note_block
    };
    set_block_state(world, dimension, position, updated.to_state_id(), None);
    if powered {
        play_note(world, dimension, position, updated);
    }
}

/// The sound of an instrument, and whether its pitch follows the note.
fn instrument_sound(instrument: Instrument) -> Option<(&'static str, bool)> {
    use Instrument::*;
    Some(match instrument {
        Harp => ("block.note_block.harp", true),
        Basedrum => ("block.note_block.basedrum", true),
        Snare => ("block.note_block.snare", true),
        Hat => ("block.note_block.hat", true),
        Bass => ("block.note_block.bass", true),
        Flute => ("block.note_block.flute", true),
        Bell => ("block.note_block.bell", true),
        Guitar => ("block.note_block.guitar", true),
        Chime => ("block.note_block.chime", true),
        Xylophone => ("block.note_block.xylophone", true),
        IronXylophone => ("block.note_block.iron_xylophone", true),
        CowBell => ("block.note_block.cow_bell", true),
        Didgeridoo => ("block.note_block.didgeridoo", true),
        Bit => ("block.note_block.bit", true),
        Banjo => ("block.note_block.banjo", true),
        Pling => ("block.note_block.pling", true),
        Trumpet => ("block.note_block.trumpet", true),
        TrumpetExposed => ("block.note_block.trumpet_exposed", true),
        TrumpetOxidized => ("block.note_block.trumpet_oxidized", true),
        TrumpetWeathered => ("block.note_block.trumpet_weathered", true),
        Zombie => ("block.note_block.imitate.zombie", false),
        Skeleton => ("block.note_block.imitate.skeleton", false),
        Creeper => ("block.note_block.imitate.creeper", false),
        Dragon => ("block.note_block.imitate.ender_dragon", false),
        WitherSkeleton => ("block.note_block.imitate.wither_skeleton", false),
        Piglin => ("block.note_block.imitate.piglin", false),
        // The sound comes from the player head's block entity, which is not
        // supported: vanilla plays nothing without one either.
        CustomHead => return None,
    })
}

/// Mob-head instruments come from a head placed on top, so they play even
/// though the block above is not air.
fn plays_when_covered(instrument: Instrument) -> bool {
    use Instrument::*;
    matches!(
        instrument,
        Zombie | Skeleton | Creeper | Dragon | WitherSkeleton | Piglin | CustomHead
    )
}

/// Vanilla `NoteBlock.getPitchFromNote`: two octaves around F#.
fn note_pitch(note: u8) -> f32 {
    2f32.powf((f32::from(note) - 12.0) / 12.0)
}

fn play_note(
    world: &World,
    dimension: DimensionId,
    position: BlockPosition,
    note_block: NoteBlock,
) {
    let covered =
        block_at(world, dimension, offset(position, UP)).is_some_and(|state| !is_air(state));
    if covered && !plays_when_covered(note_block.instrument) {
        tracing::debug!(?position, "Covered note block stays silent");
        return;
    }
    let Some((sound, tuned)) = instrument_sound(note_block.instrument) else {
        return;
    };
    let pitch = if tuned {
        note_pitch(note_block.note)
    } else {
        1.0
    };
    tracing::debug!(
        ?position,
        note = note_block.note,
        sound,
        "Note block played"
    );
    WorldSounds::new(world).play(
        Sound::new(sound)
            .category(SoundSource::Records)
            .volume(3.0)
            .pitch(pitch)
            .at(dimension, center(position)),
    );
    if let Some(block_type) =
        voidmc_data::protocol_registry_index(VERSION, "minecraft:block", "minecraft:note_block")
    {
        let chunk = ChunkPos::new(position.x.div_euclid(16), position.z.div_euclid(16));
        WorldPlayers::new(world).broadcast_chunk(
            dimension,
            chunk,
            clientbound::BlockEvent {
                position,
                action_id: 0,
                action_parameter: 0,
                block_type,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_app::App;
    use voidmc_data::v26_1_2::blocks;
    use voidmc_data::v26_1_2::props::{Face, Facing4};
    use voidmc_protocol::clientbound::chunk::{ChunkHeightmaps, ChunkSection, LightData};
    use voidmc_protocol::clientbound::{ClientboundPacket, PlayPacket, SoundEvent};
    use voidmc_protocol::serverbound::{PlayerInput, UseItemOn};
    use voidmc_protocol::types::{BlockFace, Hand};

    use super::*;
    use crate::ServerConfig;
    use crate::components::{ClientId, LoadedChunks, PlayerReady};
    use crate::config::ServerConfigResource;
    use crate::inventory::Inventory;
    use crate::item::ItemStack;
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket, PacketEvent};
    use crate::plugins::interaction::InteractionPlugin;
    use crate::plugins::item_use::ItemUsePlugin;

    const Y: i16 = 100;

    fn at(x: i32, z: i32) -> BlockPosition {
        BlockPosition { x, y: Y, z }
    }

    fn lever(powered: bool) -> i32 {
        Lever {
            face: Face::Floor,
            facing: Facing4::East,
            powered,
        }
        .to_state_id()
    }

    /// Dust shaped as an east-west line, as a vanilla client places it.
    fn line_dust() -> i32 {
        RedstoneWire {
            east: East3Up::Side,
            west: West3Up::Side,
            ..RedstoneWire::DEFAULT
        }
        .to_state_id()
    }

    fn dot() -> i32 {
        RedstoneWire::DEFAULT.to_state_id()
    }

    fn note_block(note: u8) -> i32 {
        NoteBlock {
            instrument: Instrument::Harp,
            note,
            powered: false,
        }
        .to_state_id()
    }

    fn button(name: &str, powered: bool) -> i32 {
        let block = voidmc_data::block_type(VERSION, name).unwrap();
        let state = block
            .with_property(block.default_state_id, "face", "floor")
            .unwrap();
        block
            .with_property(state, "powered", if powered { "true" } else { "false" })
            .unwrap()
    }

    struct Harness {
        app: App,
        outgoing: flume::Receiver<OutgoingPacket>,
        players: [Entity; 2],
        sequence: i32,
    }

    impl Harness {
        fn new() -> Self {
            let mut app = App::new();
            let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
            let (outgoing_tx, outgoing) = flume::unbounded::<OutgoingPacket>();
            let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
            let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
            app.insert_resource(NetworkChannels {
                incoming: incoming_rx,
                outgoing: outgoing_tx,
                disconnect: disconnect_rx,
                kick: kick_tx,
            })
            .insert_resource(ServerConfigResource::from(&ServerConfig::default()))
            .insert_resource(ChunkIndex::default())
            .add_plugins((InteractionPlugin, ItemUsePlugin, RedstonePlugin));

            // Only chunk (0, 0) is loaded.
            let chunk = ChunkData::new(
                (0..24).map(|_| ChunkSection::empty()).collect(),
                ChunkHeightmaps::empty(),
                LightData::empty(),
            );
            let chunk_entity = app.world_mut().spawn(chunk).id();
            app.world_mut()
                .resource_mut::<ChunkIndex>()
                .0
                .insert((DimensionId::Overworld, ChunkPos::new(0, 0)), chunk_entity);

            let players = [1, 2].map(|id| {
                app.world_mut()
                    .spawn((
                        ClientId(id),
                        PlayerDimension(DimensionId::Overworld),
                        LoadedChunks(HashSet::from([ChunkPos::new(0, 0)])),
                        PlayerReady,
                        Inventory::new(),
                    ))
                    .id()
            });
            Self {
                app,
                outgoing,
                players,
                sequence: 0,
            }
        }

        fn world(&mut self) -> &mut World {
            self.app.world_mut()
        }

        /// Sets blocks like a command would, then runs one tick.
        fn build(&mut self, blocks: &[(BlockPosition, i32)]) {
            for &(position, state) in blocks {
                set_block_state(self.world(), DimensionId::Overworld, position, state, None);
            }
            self.app.update();
        }

        fn block(&self, position: BlockPosition) -> i32 {
            block_at(self.app.world(), DimensionId::Overworld, position).unwrap()
        }

        fn power(&self, position: BlockPosition) -> u8 {
            RedstoneWire::from_state_id(self.block(position))
                .unwrap_or_else(|| panic!("no dust at {position:?}"))
                .power
        }

        fn powered(&self, position: BlockPosition) -> bool {
            let state = self.block(position);
            NoteBlock::from_state_id(state)
                .map(|note| note.powered)
                .or_else(|| source_powered(state))
                .unwrap_or_else(|| panic!("no note block, lever or button at {position:?}"))
        }

        fn use_on(&mut self, player: usize, position: BlockPosition, hand: Hand) {
            self.sequence += 1;
            let packet = UseItemOn {
                hand,
                location: position,
                face: BlockFace::Top,
                cursor_x: 0.5,
                cursor_y: 0.5,
                cursor_z: 0.5,
                inside_block: false,
                world_border_hit: false,
                sequence: self.sequence,
            };
            let entity = self.players[player];
            self.world().trigger(PacketEvent {
                client_id: player as u32 + 1,
                entity,
                packet,
            });
        }

        fn click(&mut self, position: BlockPosition) {
            self.use_on(0, position, Hand::MainHand);
            self.app.update();
        }

        fn packets(&self) -> Vec<(u32, PlayPacket)> {
            self.outgoing
                .try_iter()
                .filter_map(|outgoing| match outgoing.packet {
                    ClientboundPacket::Play(packet) => Some((outgoing.client_id, packet)),
                    _ => None,
                })
                .collect()
        }
    }

    /// Note plays (block events) each client received, with their position.
    fn plays(packets: &[(u32, PlayPacket)]) -> Vec<(u32, BlockPosition)> {
        packets
            .iter()
            .filter_map(|(client, packet)| match packet {
                PlayPacket::BlockEvent(event) => Some((*client, event.position)),
                _ => None,
            })
            .collect()
    }

    fn sounds(packets: &[(u32, PlayPacket)], name: &str) -> Vec<(u32, f32)> {
        let id = crate::sounds::resolve(name).unwrap();
        packets
            .iter()
            .filter_map(|(client, packet)| match packet {
                PlayPacket::SoundEffect(effect) if effect.sound == SoundEvent::Registry(id) => {
                    Some((*client, effect.pitch))
                }
                _ => None,
            })
            .collect()
    }

    /// Lever at x=1, `dust` line dust from x=2, note block after it, on z=1.
    fn circuit(harness: &mut Harness, dust: i32) -> (BlockPosition, BlockPosition) {
        let mut blocks = vec![(at(1, 1), lever(false))];
        for x in 2..2 + dust {
            blocks.push((at(x, 1), line_dust()));
        }
        let note = at(2 + dust, 1);
        blocks.push((note, note_block(12)));
        harness.build(&blocks);
        harness.packets();
        (at(1, 1), note)
    }

    #[test]
    fn lever_line_powers_dust_and_plays_the_note_once_per_rising_edge() {
        let mut harness = Harness::new();
        let (lever_at, note) = circuit(&mut harness, 3);

        harness.click(lever_at);
        assert!(harness.powered(lever_at));
        assert_eq!([2, 3, 4].map(|x| harness.power(at(x, 1))), [15, 14, 13]);
        assert!(harness.powered(note));
        let packets = harness.packets();
        assert_eq!(plays(&packets), [(1, note), (2, note)]);
        let harp = sounds(&packets, "block.note_block.harp");
        assert_eq!(harp, [(1, 1.0), (2, 1.0)], "note 12 plays at pitch 1");
        assert_eq!(sounds(&packets, "block.lever.click"), [(1, 0.6), (2, 0.6)]);
        let block_event = packets
            .iter()
            .find_map(|(_, packet)| match packet {
                PlayPacket::BlockEvent(event) => Some(event.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (block_event.action_id, block_event.action_parameter),
            (0, 0)
        );
        assert_eq!(block_event.block_type, 109, "minecraft:note_block");

        for _ in 0..40 {
            harness.app.update();
        }
        assert!(plays(&harness.packets()).is_empty(), "no replay while on");

        harness.click(lever_at);
        assert!(!harness.powered(lever_at));
        assert_eq!([2, 3, 4].map(|x| harness.power(at(x, 1))), [0, 0, 0]);
        assert!(!harness.powered(note));
        let packets = harness.packets();
        assert!(
            plays(&packets).is_empty(),
            "nothing plays on a falling edge"
        );
        assert_eq!(sounds(&packets, "block.lever.click"), [(1, 0.5), (2, 0.5)]);

        harness.click(lever_at);
        assert!(harness.powered(note));
        assert_eq!(plays(&harness.packets()), [(1, note), (2, note)]);
    }

    #[test]
    fn note_pitch_follows_the_stored_note_and_instrument() {
        let mut harness = Harness::new();
        let note = at(3, 3);
        let tuned = NoteBlock {
            instrument: Instrument::Bell,
            note: 24,
            powered: false,
        };
        harness.build(&[(at(2, 3), lever(false)), (note, tuned.to_state_id())]);
        harness.packets();
        harness.click(at(2, 3));
        let packets = harness.packets();
        assert_eq!(
            sounds(&packets, "block.note_block.bell"),
            [(1, 2.0), (2, 2.0)]
        );
        assert_eq!(note_pitch(0), 0.5);
    }

    #[test]
    fn signal_reaches_fifteen_dust_and_dies_on_the_sixteenth() {
        let mut harness = Harness::new();
        // A lever, 14 dust east, then the line turns south and ends at a note
        // block, which the last dust points into.
        let route = |z: i32, dust: usize| -> Vec<BlockPosition> {
            (1..=14)
                .map(|x| at(x, z))
                .chain((1..).map(|dz| at(14, z + dz)))
                .take(dust)
                .collect()
        };
        let fifteen = route(2, 15);
        let sixteen = route(8, 16);
        let reached = at(14, 4);
        let unreached = at(14, 11);
        let mut blocks = vec![
            (at(0, 2), lever(false)),
            (at(0, 8), lever(false)),
            (reached, note_block(5)),
            (unreached, note_block(5)),
        ];
        blocks.extend(fifteen.iter().chain(&sixteen).map(|&p| (p, dot())));
        harness.build(&blocks);
        harness.packets();

        harness.click(at(0, 2));
        let powers: Vec<u8> = fifteen.iter().map(|&p| harness.power(p)).collect();
        assert_eq!(powers, (1..=15).rev().collect::<Vec<u8>>());
        assert!(harness.powered(reached));
        assert_eq!(plays(&harness.packets()).len(), 2);

        harness.click(at(0, 8));
        let powers: Vec<u8> = sixteen.iter().map(|&p| harness.power(p)).collect();
        let mut expected: Vec<u8> = (1..=15).rev().collect();
        expected.push(0);
        assert_eq!(powers, expected);
        assert!(!harness.powered(unreached));
        assert!(plays(&harness.packets()).is_empty());
    }

    #[test]
    fn dust_loops_terminate_and_take_the_shortest_path() {
        let mut harness = Harness::new();
        // A 5x5 ring of dust, fed at its corner through (0, 1).
        let mut ring = Vec::new();
        for i in 1..=5 {
            ring.extend([at(i, 1), at(i, 5), at(1, i), at(5, i)]);
        }
        ring.sort_by_key(|p| (p.x, p.z));
        ring.dedup();
        let mut blocks = vec![(at(0, 0), lever(false)), (at(0, 1), dot())];
        blocks.extend(ring.iter().map(|&p| (p, dot())));
        harness.build(&blocks);

        harness.click(at(0, 0));
        assert_eq!(harness.power(at(0, 1)), 15);
        assert_eq!(harness.power(at(1, 1)), 14);
        assert_eq!(harness.power(at(5, 1)), 10);
        assert_eq!(harness.power(at(1, 5)), 10);
        assert_eq!(harness.power(at(5, 5)), 6, "8 steps either way round");
        assert_eq!(harness.world().resource::<Redstone>().pending_changes(), 0);

        harness.click(at(0, 0));
        assert!(ring.iter().all(|&p| harness.power(p) == 0));
    }

    #[test]
    fn breaking_a_dust_or_the_lever_depowers_what_is_downstream() {
        let mut harness = Harness::new();
        let (lever_at, note) = circuit(&mut harness, 5);
        harness.click(lever_at);
        assert!(harness.powered(note));
        harness.packets();

        harness.build(&[(at(4, 1), blocks::AIR)]);
        assert_eq!([2, 3].map(|x| harness.power(at(x, 1))), [15, 14]);
        assert_eq!([5, 6].map(|x| harness.power(at(x, 1))), [0, 0]);
        assert!(!harness.powered(note));

        // Repairing the line powers it again, and the note plays again.
        harness.build(&[(at(4, 1), line_dust())]);
        assert_eq!(harness.power(at(6, 1)), 11);
        assert!(harness.powered(note));
        assert_eq!(plays(&harness.packets()).len(), 2);

        harness.build(&[(lever_at, blocks::AIR)]);
        assert!((2..=6).all(|x| harness.power(at(x, 1)) == 0));
        assert!(!harness.powered(note));
    }

    #[test]
    fn a_covered_note_block_is_powered_but_silent() {
        let mut harness = Harness::new();
        let (lever_at, note) = circuit(&mut harness, 2);
        harness.build(&[(offset(note, UP), blocks::STONE)]);
        harness.packets();
        harness.click(lever_at);
        assert!(harness.powered(note));
        let packets = harness.packets();
        assert!(plays(&packets).is_empty());
        assert!(sounds(&packets, "block.note_block.harp").is_empty());
    }

    #[test]
    fn a_lever_next_to_a_note_block_triggers_it() {
        let mut harness = Harness::new();
        let note = at(5, 5);
        harness.build(&[(at(5, 6), lever(false)), (note, note_block(0))]);
        harness.click(at(5, 6));
        assert!(harness.powered(note));
        assert_eq!(plays(&harness.packets()).len(), 2);

        // A second, powered lever keeps it powered: no new edge, no new note.
        harness.build(&[(at(4, 5), lever(true))]);
        harness.click(at(5, 6));
        assert!(harness.powered(note));
        assert!(plays(&harness.packets()).is_empty());
    }

    #[test]
    fn dust_only_powers_the_note_block_it_points_into_or_sits_on() {
        let mut harness = Harness::new();
        // A north-south line passing beside the note block at x=4.
        let north_south = RedstoneWire {
            north: North3Up::Side,
            south: South3Up::Side,
            ..RedstoneWire::DEFAULT
        }
        .to_state_id();
        let beside = at(4, 4);
        harness.build(&[
            (at(3, 2), lever(false)),
            (at(3, 3), north_south),
            (at(3, 4), north_south),
            (at(3, 5), north_south),
            (beside, note_block(0)),
        ]);
        harness.click(at(3, 2));
        assert_eq!(harness.power(at(3, 4)), 14);
        assert!(
            !harness.powered(beside),
            "the dust beside it points north-south"
        );

        // Dust on top of a note block powers it, and covers it.
        let below = at(8, 8);
        let dust = offset(below, UP);
        let lever_at = offset(at(9, 8), UP);
        harness.build(&[
            (below, note_block(0)),
            (dust, dot()),
            (lever_at, lever(false)),
        ]);
        harness.packets();
        harness.click(lever_at);
        assert_eq!(harness.power(dust), 15);
        assert!(harness.powered(below));
        assert!(plays(&harness.packets()).is_empty(), "the dust covers it");
    }

    #[test]
    fn buttons_release_after_twenty_or_thirty_ticks() {
        for (name, ticks) in [
            ("minecraft:stone_button", 20),
            ("minecraft:polished_blackstone_button", 20),
            ("minecraft:oak_button", 30),
            ("minecraft:crimson_button", 30),
        ] {
            let mut harness = Harness::new();
            let (button_at, note) = (at(1, 1), at(2, 1));
            harness.build(&[(button_at, button(name, false)), (note, note_block(3))]);
            harness.packets();

            harness.click(button_at);
            assert_eq!(harness.block(button_at), button(name, true), "{name}");
            assert!(harness.powered(note));
            let packets = harness.packets();
            assert_eq!(plays(&packets).len(), 2);
            let kind = button_kind(name).unwrap();
            assert_eq!(
                sounds(&packets, kind.click_on),
                [(2, 1.0)],
                "{name}: the presser predicts its own click"
            );

            // A click while pressed changes nothing (one more tick).
            harness.click(button_at);
            for _ in 1..ticks - 1 {
                harness.app.update();
            }
            assert_eq!(harness.block(button_at), button(name, true), "{name}");
            harness.app.update();
            assert_eq!(
                harness.block(button_at),
                button(name, false),
                "{name} releases {ticks} ticks after the press"
            );
            assert!(!harness.powered(note));
            let packets = harness.packets();
            assert_eq!(sounds(&packets, kind.click_off).len(), 2);
            assert!(plays(&packets).is_empty());
        }
    }

    #[test]
    fn a_button_broken_while_pressed_is_not_released_into_the_world() {
        let mut harness = Harness::new();
        harness.build(&[(at(1, 1), button("minecraft:stone_button", false))]);
        harness.click(at(1, 1));
        harness.build(&[(at(1, 1), blocks::STONE)]);
        for _ in 0..25 {
            harness.app.update();
        }
        assert_eq!(harness.block(at(1, 1)), blocks::STONE);
    }

    #[test]
    fn off_hand_and_sneaking_with_an_item_do_not_toggle_a_lever() {
        let mut harness = Harness::new();
        let lever_at = at(4, 4);
        harness.build(&[
            (offset(lever_at, DOWN), blocks::STONE),
            (lever_at, lever(false)),
        ]);

        // A click a client did not consume sends the main hand then the off
        // hand: the lever toggles once.
        harness.use_on(0, lever_at, Hand::MainHand);
        harness.use_on(0, lever_at, Hand::OffHand);
        harness.app.update();
        assert!(harness.powered(lever_at));
        let acks = harness
            .packets()
            .iter()
            .filter(|(client, packet)| {
                *client == 1 && matches!(packet, PlayPacket::BlockChangedAck(_))
            })
            .count();
        assert_eq!(acks, 2, "both uses are acknowledged");

        // Holding stone: the lever still toggles and nothing is placed.
        let player = harness.players[0];
        let stone = ItemStack::of("minecraft:stone", 64).unwrap();
        harness
            .world()
            .get_mut::<Inventory>(player)
            .unwrap()
            .set(Inventory::HOTBAR_START, stone);
        harness.click(lever_at);
        assert!(!harness.powered(lever_at));
        assert_eq!(harness.block(offset(lever_at, UP)), blocks::AIR);

        // Sneaking with stone in hand places stone against the lever.
        harness.world().trigger(PacketEvent {
            client_id: 1,
            entity: player,
            packet: PlayerInput {
                flags: PlayerInput::SNEAK,
            },
        });
        harness.click(lever_at);
        assert!(!harness.powered(lever_at));
        assert_eq!(harness.block(offset(lever_at, UP)), blocks::STONE);

        // Sneaking with empty hands still toggles, as in vanilla.
        harness
            .world()
            .get_mut::<Inventory>(player)
            .unwrap()
            .set(Inventory::HOTBAR_START, ItemStack::EMPTY);
        harness.click(lever_at);
        assert!(harness.powered(lever_at));
    }

    #[test]
    fn unloaded_chunks_are_neither_read_nor_written() {
        let mut harness = Harness::new();
        // The line runs east into chunk (1, 0), which is not loaded.
        let mut blocks = vec![(at(10, 1), lever(false))];
        for x in 11..16 {
            blocks.push((at(x, 1), line_dust()));
        }
        harness.build(&blocks);
        harness.click(at(10, 1));
        assert_eq!(harness.power(at(15, 1)), 11);
        assert!(block_at(harness.app.world(), DimensionId::Overworld, at(16, 1)).is_none());
        assert_eq!(harness.world().resource::<Redstone>().pending_changes(), 0);
    }

    #[test]
    fn at_most_max_changes_are_processed_per_tick() {
        let mut harness = Harness::new();
        let extra = 10;
        {
            let mut redstone = harness.world().resource_mut::<Redstone>();
            for i in 0..(MAX_CHANGES_PER_TICK + extra) as i32 {
                redstone.queue_change(DimensionId::Overworld, at(i % 16, i / 16));
            }
        }
        harness.app.update();
        assert_eq!(
            harness.world().resource::<Redstone>().pending_changes(),
            extra
        );
        harness.app.update();
        assert_eq!(harness.world().resource::<Redstone>().pending_changes(), 0);
    }

    #[test]
    fn a_note_block_placed_next_to_powered_dust_waits_for_the_next_change() {
        let mut harness = Harness::new();
        let (lever_at, note) = circuit(&mut harness, 2);
        harness.build(&[(note, blocks::AIR)]);
        harness.click(lever_at);
        harness.packets();
        harness.build(&[(note, note_block(0))]);
        assert!(!harness.powered(note), "vanilla: no neighbour changed");
        assert!(plays(&harness.packets()).is_empty());
    }

    #[test]
    fn without_the_plugin_a_lever_click_places_the_held_block() {
        let mut harness = Harness::new();
        *harness.world().resource_mut::<BlockInteractionRegistry>() =
            BlockInteractionRegistry::default();
        let lever_at = at(4, 4);
        harness.build(&[(lever_at, lever(false))]);
        let player = harness.players[0];
        let stone = ItemStack::of("minecraft:stone", 64).unwrap();
        harness
            .world()
            .get_mut::<Inventory>(player)
            .unwrap()
            .set(Inventory::HOTBAR_START, stone);
        harness.click(lever_at);
        assert!(!harness.powered(lever_at));
        assert_eq!(harness.block(offset(lever_at, UP)), blocks::STONE);
    }

    #[test]
    fn every_supported_sound_exists() {
        for instrument in (0..27).filter_map(Instrument::from_index) {
            if let Some((sound, _)) = instrument_sound(instrument) {
                assert!(crate::sounds::resolve(sound).is_some(), "{sound}");
            }
        }
        let buttons: Vec<_> = voidmc_data::block_types(VERSION)
            .iter()
            .filter_map(|block| button_kind(block.name).map(|kind| (block.name, kind)))
            .collect();
        assert_eq!(buttons.len(), 14);
        for (name, kind) in buttons {
            for sound in [kind.click_on, kind.click_off] {
                assert!(crate::sounds::resolve(sound).is_some(), "{name}: {sound}");
            }
        }
        assert!(crate::sounds::resolve("block.lever.click").is_some());
    }
}
