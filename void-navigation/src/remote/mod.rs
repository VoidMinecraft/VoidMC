//! Operator remote: select a navigating mob, see its path and destination,
//! and drive it. Everything drawn is sent to the watching operator only, as
//! client-side display entities and locator-bar waypoints; no server entity is
//! created, and the systems do not run at all while nobody holds a
//! [`NavRemote`].

mod control;
mod render;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::common_conditions::any_with_component;
use voidmc::components::{
    EntityDimension, EntityUuid, MinecraftEntityId, PlayerDimension, Position,
};
use voidmc::entity::EntityMetadata;
use voidmc::item::ItemId;
use voidmc::item_behavior::ItemBehaviorRegistry;
use voidmc::world::DimensionId;
use voidmc::{CommandRegistry, Players};
use voidmc_protocol::clientbound::entity_metadata::entity_index;
use voidmc_protocol::clientbound::{ClientboundPacket, EntityMetadataValue};

pub use control::{nav_command, select_looked_at};

use crate::adapter::{NavigationSystems, Navigator};
use crate::pathing::Vec3;
use render::{Frame, Scene};

pub const DEFAULT_WAND: &str = "minecraft:blaze_rod";

/// Who may use the remote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RemoteAccess {
    #[default]
    Operators,
    Everyone,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteSettings {
    pub access: RemoteAccess,
    pub wand: ItemId,
}

impl RemoteSettings {
    pub fn allows(&self, world: &World, player: Entity) -> bool {
        match self.access {
            RemoteAccess::Everyone => true,
            RemoteAccess::Operators => world.get::<voidmc::components::Operator>(player).is_some(),
        }
    }
}

/// Present on a player using the remote; holds the selected mob and what the
/// player's client currently shows for it.
#[derive(Component, Debug, Default)]
pub struct NavRemote {
    selected: Option<Entity>,
    scene: Scene,
    used_this_tick: bool,
}

impl NavRemote {
    pub fn selected(&self) -> Option<Entity> {
        self.selected
    }

    pub(crate) fn claim_use(&mut self) -> bool {
        !std::mem::replace(&mut self.used_this_tick, true)
    }
}

/// Makes `operator` watch and drive `target`, or releases the selection with
/// `None`. Returns false when `target` has no [`Navigator`].
pub fn select(world: &mut World, operator: Entity, target: Option<Entity>) -> bool {
    if let Some(target) = target
        && world.get::<Navigator>(target).is_none()
    {
        return false;
    }
    let Ok(mut player) = world.get_entity_mut(operator) else {
        return false;
    };
    match player.get_mut::<NavRemote>() {
        Some(mut remote) => remote.selected = target,
        None if target.is_some() => {
            player.insert(NavRemote {
                selected: target,
                ..NavRemote::default()
            });
        }
        None => {}
    }
    true
}

pub struct NavRemotePlugin {
    access: RemoteAccess,
    wand: &'static str,
    command: bool,
}

impl Default for NavRemotePlugin {
    fn default() -> Self {
        Self {
            access: RemoteAccess::Operators,
            wand: DEFAULT_WAND,
            command: true,
        }
    }
}

impl NavRemotePlugin {
    pub fn access(mut self, access: RemoteAccess) -> Self {
        self.access = access;
        self
    }

    /// The item that selects and drives mobs, e.g. `"minecraft:blaze_rod"`.
    pub fn wand(mut self, item: &'static str) -> Self {
        self.wand = item;
        self
    }

    pub fn command(mut self, enabled: bool) -> Self {
        self.command = enabled;
        self
    }
}

impl Plugin for NavRemotePlugin {
    fn build(&self, app: &mut App) {
        let wand = ItemId::from_name(self.wand)
            .unwrap_or_else(|| panic!("unknown remote wand item {}", self.wand));
        app.insert_resource(RemoteSettings {
            access: self.access,
            wand,
        })
        .add_observer(control::select_on_interact)
        .add_observer(clear_on_remove)
        .add_systems(
            Update,
            render_scenes
                .after(NavigationSystems)
                .run_if(any_with_component::<NavRemote>),
        );
        let world = app.world_mut();
        world
            .get_resource_or_insert_with(ItemBehaviorRegistry::default)
            .register(wand, control::RemoteWand);
        if self.command
            && let Some(mut registry) = world.get_resource_mut::<CommandRegistry>()
        {
            registry.register(nav_command());
        }
    }
}

type Watched<'w, 's> = Query<
    'w,
    's,
    (
        &'static Navigator,
        &'static Position,
        &'static MinecraftEntityId,
        &'static EntityUuid,
        Option<&'static EntityMetadata>,
        Option<&'static EntityDimension>,
    ),
>;

fn frame_of<'a>(
    watched: &'a Watched,
    target: Entity,
    tick: u64,
) -> Option<(Frame<'a>, DimensionId)> {
    let (navigator, position, network_id, uuid, metadata, dimension) = watched.get(target).ok()?;
    let flags = match metadata.and_then(|m| m.get(entity_index::FLAGS)) {
        Some(EntityMetadataValue::Byte(flags)) => *flags,
        _ => 0,
    };
    let path = navigator.path();
    let dimension = dimension.map_or(DimensionId::Overworld, |d| d.0);
    let frame = Frame {
        target,
        network_id: network_id.0,
        uuid: uuid.0,
        flags,
        position: Vec3::new(position.x, position.y, position.z),
        origin: navigator.path_origin(),
        path: path.points(),
        complete: path.is_complete(),
        waypoint: navigator.waypoint(),
        revision: navigator.path_revision(),
        destination: navigator.destination(),
        tick,
    };
    Some((frame, dimension))
}

fn render_scenes(
    players: Players,
    watched: Watched,
    mut remotes: Query<(Entity, &mut NavRemote, Option<&PlayerDimension>)>,
    mut packets: Local<Vec<ClientboundPacket>>,
    mut tick: Local<u64>,
) {
    *tick += 1;
    for (watcher, mut remote, watcher_dimension) in &mut remotes {
        let remote = remote.bypass_change_detection();
        remote.used_this_tick = false;
        if remote.selected.is_none() && remote.scene.is_empty() {
            continue;
        }
        let watcher_dimension = watcher_dimension.map_or(DimensionId::Overworld, |d| d.0);
        let found = remote
            .selected
            .and_then(|target| frame_of(&watched, target, *tick));
        match &found {
            None => {
                remote.selected = None;
                remote.scene.clear(&mut packets);
            }
            Some((_, dimension)) if *dimension != watcher_dimension => {
                remote.scene.clear(&mut packets)
            }
            Some((frame, _)) => remote.scene.sync(Some(frame), &mut packets),
        }
        for packet in packets.drain(..) {
            players.send(watcher, packet);
        }
    }
}

fn clear_on_remove(
    event: On<Remove, NavRemote>,
    players: Players,
    mut remotes: Query<&mut NavRemote>,
    mut packets: Local<Vec<ClientboundPacket>>,
) {
    if let Ok(mut remote) = remotes.get_mut(event.entity) {
        remote.scene.clear(&mut packets);
        for packet in packets.drain(..) {
            players.send(event.entity, packet);
        }
    }
}
