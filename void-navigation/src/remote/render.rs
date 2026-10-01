use uuid::Uuid;
use voidmc::EntityKind;
use voidmc::components::MinecraftEntityId;
use voidmc::entity::{BlockDisplay, Display, DisplayTransform, EntityMetadata, MetadataSource};
use voidmc_data::v26_1_2::blocks;
use voidmc_protocol::clientbound::entity_metadata::{entity_flag, entity_index};
use voidmc_protocol::clientbound::{
    ClientboundPacket, EntityMetadataEntry, EntityMetadataValue, RemoveEntities, SetEntityData,
    SpawnEntity, TrackedWaypoint, Waypoint, WaypointColor, WaypointIcon, WaypointId,
    WaypointOperation, WaypointPosition,
};
use voidmc_protocol::types::LpVec3;

use crate::pathing::Vec3;

pub const MAX_SEGMENTS: usize = 64;
const THICKNESS: f32 = 0.07;
const LIFT: f64 = 0.12;
const MARKER_SIZE: f32 = 0.35;
const WAYPOINT_REFRESH_TICKS: u64 = 5;
const COMPLETE_BEAM: i32 = blocks::LIME_CONCRETE;
const PARTIAL_BEAM: i32 = blocks::ORANGE_CONCRETE;
const MARKER: i32 = blocks::GOLD_BLOCK;
const MARKER_GLOW: i32 = 0xFFD700;
const MOB_WAYPOINT: WaypointColor = WaypointColor::rgb(0x55FF55);
const GOAL_WAYPOINT: WaypointColor = WaypointColor::rgb(0xFFAA00);

/// What the watched mob looks like this tick.
pub(crate) struct Frame<'a> {
    pub target: bevy_ecs::entity::Entity,
    pub network_id: i32,
    pub uuid: Uuid,
    pub flags: i8,
    pub position: Vec3,
    pub origin: Vec3,
    pub path: &'a [Vec3],
    pub complete: bool,
    pub waypoint: usize,
    pub revision: u32,
    pub destination: Option<Vec3>,
    pub tick: u64,
}

/// The client-side-only entities and locator-bar entries one watcher sees.
/// Nothing here exists on the server: the displays are spawned straight to
/// the watcher's client and removed the same way.
#[derive(Debug)]
pub(crate) struct Scene {
    target: Option<bevy_ecs::entity::Entity>,
    revision: u32,
    segments: Vec<i32>,
    cleared: usize,
    marker: Option<i32>,
    glow: Option<(i32, i8)>,
    mob_waypoint: Option<(Uuid, [i32; 3])>,
    goal_waypoint: Option<[i32; 3]>,
    goal_id: Uuid,
    last_refresh: u64,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            target: None,
            revision: 0,
            segments: Vec::new(),
            cleared: 0,
            marker: None,
            glow: None,
            mob_waypoint: None,
            goal_waypoint: None,
            goal_id: Uuid::new_v4(),
            last_refresh: 0,
        }
    }
}

impl Scene {
    pub fn is_empty(&self) -> bool {
        self.target.is_none()
            && self.segments.len() == self.cleared
            && self.marker.is_none()
            && self.glow.is_none()
            && self.mob_waypoint.is_none()
            && self.goal_waypoint.is_none()
    }

    pub fn sync(&mut self, frame: Option<&Frame>, out: &mut Vec<ClientboundPacket>) {
        let Some(frame) = frame else {
            self.clear(out);
            return;
        };
        if self.target != Some(frame.target) {
            self.clear(out);
            self.target = Some(frame.target);
            self.glow = Some((frame.network_id, frame.flags));
            out.push(glow_packet(
                frame.network_id,
                frame.flags | entity_flag::GLOWING as i8,
            ));
            self.revision = frame.revision.wrapping_sub(1);
        }
        if self.revision != frame.revision {
            self.redraw(frame, out);
        } else {
            self.trim(frame.waypoint, out);
        }
        self.sync_waypoints(frame, out);
    }

    pub fn clear(&mut self, out: &mut Vec<ClientboundPacket>) {
        self.drop_path(out);
        if let Some((network_id, flags)) = self.glow.take() {
            out.push(glow_packet(network_id, flags));
        }
        if let Some((uuid, _)) = self.mob_waypoint.take() {
            out.push(untrack(WaypointId::Uuid(uuid)));
        }
        if self.goal_waypoint.take().is_some() {
            out.push(untrack(WaypointId::Uuid(self.goal_id)));
        }
        self.target = None;
    }

    fn drop_path(&mut self, out: &mut Vec<ClientboundPacket>) {
        let mut ids: Vec<i32> = self.segments.drain(..).skip(self.cleared).collect();
        ids.extend(self.marker.take());
        self.cleared = 0;
        if !ids.is_empty() {
            out.push(RemoveEntities { entity_ids: ids }.into());
        }
    }

    fn redraw(&mut self, frame: &Frame, out: &mut Vec<ClientboundPacket>) {
        self.drop_path(out);
        self.revision = frame.revision;
        let beam = if frame.complete {
            COMPLETE_BEAM
        } else {
            PARTIAL_BEAM
        };
        let mut from = frame.origin;
        for &to in frame.path.iter().take(MAX_SEGMENTS) {
            let id = MinecraftEntityId::allocate().0;
            segment(id, from, to, beam, out);
            self.segments.push(id);
            from = to;
        }
        if let Some(&end) = frame.path.last() {
            let id = MinecraftEntityId::allocate().0;
            marker(id, end, out);
            self.marker = Some(id);
        }
        self.trim(frame.waypoint, out);
    }

    fn trim(&mut self, waypoint: usize, out: &mut Vec<ClientboundPacket>) {
        let done = waypoint.min(self.segments.len());
        if done > self.cleared {
            out.push(
                RemoveEntities {
                    entity_ids: self.segments[self.cleared..done].to_vec(),
                }
                .into(),
            );
            self.cleared = done;
        }
    }

    fn sync_waypoints(&mut self, frame: &Frame, out: &mut Vec<ClientboundPacket>) {
        let due = frame.tick >= self.last_refresh + WAYPOINT_REFRESH_TICKS;
        let here = block_of(frame.position);
        match self.mob_waypoint {
            None => {
                out.push(track(WaypointId::Uuid(frame.uuid), MOB_WAYPOINT, here));
                self.mob_waypoint = Some((frame.uuid, here));
                self.last_refresh = frame.tick;
            }
            Some((uuid, last)) if due && last != here => {
                out.push(update(WaypointId::Uuid(uuid), MOB_WAYPOINT, here));
                self.mob_waypoint = Some((uuid, here));
                self.last_refresh = frame.tick;
            }
            _ => {}
        }
        let goal = frame.destination.map(block_of);
        if goal != self.goal_waypoint {
            match goal {
                Some(goal) => out.push(track(WaypointId::Uuid(self.goal_id), GOAL_WAYPOINT, goal)),
                None => out.push(untrack(WaypointId::Uuid(self.goal_id))),
            }
            self.goal_waypoint = goal;
        }
    }
}

fn block_of(point: Vec3) -> [i32; 3] {
    [
        point.x.floor() as i32,
        point.y.floor() as i32,
        point.z.floor() as i32,
    ]
}

fn track(id: WaypointId, color: WaypointColor, [x, y, z]: [i32; 3]) -> ClientboundPacket {
    waypoint(WaypointOperation::Track, id, color, [x, y, z])
}

fn update(id: WaypointId, color: WaypointColor, at: [i32; 3]) -> ClientboundPacket {
    waypoint(WaypointOperation::Update, id, color, at)
}

fn waypoint(
    operation: WaypointOperation,
    id: WaypointId,
    color: WaypointColor,
    [x, y, z]: [i32; 3],
) -> ClientboundPacket {
    TrackedWaypoint {
        operation,
        waypoint: Waypoint {
            id,
            icon: WaypointIcon {
                color: Some(color),
                ..WaypointIcon::default()
            },
            position: WaypointPosition::Block { x, y, z },
        },
    }
    .into()
}

fn untrack(id: WaypointId) -> ClientboundPacket {
    TrackedWaypoint {
        operation: WaypointOperation::Untrack,
        waypoint: Waypoint::untracked(id),
    }
    .into()
}

fn glow_packet(network_id: i32, flags: i8) -> ClientboundPacket {
    SetEntityData {
        entity_id: network_id,
        entries: vec![EntityMetadataEntry {
            index: entity_index::FLAGS,
            value: EntityMetadataValue::Byte(flags),
        }],
    }
    .into()
}

fn spawn_display(id: i32, at: Vec3, metadata: EntityMetadata, out: &mut Vec<ClientboundPacket>) {
    out.push(
        SpawnEntity {
            entity_id: id,
            entity_uuid: Uuid::new_v4(),
            entity_type: EntityKind::BlockDisplay.id(),
            x: at.x,
            y: at.y + LIFT,
            z: at.z,
            velocity: LpVec3::ZERO,
            pitch: 0,
            yaw: 0,
            head_yaw: 0,
            data: 0,
        }
        .into(),
    );
    out.push(
        SetEntityData {
            entity_id: id,
            entries: metadata.entries(),
        }
        .into(),
    );
}

/// A thin beam from `from` to `to`: a unit block scaled to `t × t × length`,
/// turned so its +Z axis follows the segment and re-centred on it.
pub(crate) fn segment_transform(from: Vec3, to: Vec3) -> Option<DisplayTransform> {
    let delta = to - from;
    let length = delta.length();
    if length < 1.0e-3 {
        return None;
    }
    let direction = delta * (1.0 / length);
    let rotation = rotation_from_z(direction);
    let half = THICKNESS / 2.0;
    let offset = rotate(rotation, [half, half, 0.0]);
    Some(DisplayTransform {
        translation: [-offset[0], -offset[1], -offset[2]],
        scale: [THICKNESS, THICKNESS, length as f32],
        left_rotation: rotation,
        right_rotation: [0.0, 0.0, 0.0, 1.0],
    })
}

fn segment(id: i32, from: Vec3, to: Vec3, block: i32, out: &mut Vec<ClientboundPacket>) {
    let Some(transform) = segment_transform(from, to) else {
        return;
    };
    let mut metadata = EntityMetadata::default();
    Display {
        transform,
        brightness: Some((15, 15)),
        view_range: 2.0,
        ..Display::default()
    }
    .write(&mut metadata);
    BlockDisplay(block).write(&mut metadata);
    spawn_display(id, from, metadata, out);
}

fn marker(id: i32, at: Vec3, out: &mut Vec<ClientboundPacket>) {
    let mut metadata = EntityMetadata::default();
    let half = MARKER_SIZE / 2.0;
    Display {
        transform: DisplayTransform::default()
            .translation(-half, 0.0, -half)
            .uniform_scale(MARKER_SIZE),
        brightness: Some((15, 15)),
        view_range: 4.0,
        glow_color: Some(MARKER_GLOW),
        ..Display::default()
    }
    .write(&mut metadata);
    BlockDisplay(MARKER).write(&mut metadata);
    metadata.set(
        entity_index::FLAGS,
        EntityMetadataValue::Byte(entity_flag::GLOWING as i8),
    );
    spawn_display(id, at, metadata, out);
}

fn rotation_from_z(direction: Vec3) -> [f32; 4] {
    let (x, y, z) = (direction.x, direction.y, direction.z);
    if z < -1.0 + 1.0e-9 {
        return [0.0, 1.0, 0.0, 0.0];
    }
    let (qx, qy, qz, qw) = (-y, x, 0.0, 1.0 + z);
    let norm = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
    [
        (qx / norm) as f32,
        (qy / norm) as f32,
        (qz / norm) as f32,
        (qw / norm) as f32,
    ]
}

pub(crate) fn rotate([x, y, z, w]: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let u = [x, y, z];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let uv = dot(u, v);
    let uu = dot(u, u);
    let c = cross(u, v);
    let mut r = [0.0; 3];
    for i in 0..3 {
        r[i] = 2.0 * uv * u[i] + (w * w - uu) * v[i] + 2.0 * w * c[i];
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0e-4)
    }

    #[test]
    fn beams_point_from_start_to_end() {
        for to in [
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.0, 0.0, 5.0),
        ] {
            let transform = segment_transform(Vec3::ZERO, to).expect("transform");
            let axis = rotate(transform.left_rotation, [0.0, 0.0, transform.scale[2]]);
            assert!(
                close(axis, [to.x as f32, to.y as f32, to.z as f32]),
                "{to:?} -> {axis:?}"
            );
            let centre = rotate(
                transform.left_rotation,
                [THICKNESS / 2.0, THICKNESS / 2.0, 0.0],
            );
            let centre = [
                centre[0] + transform.translation[0],
                centre[1] + transform.translation[1],
                centre[2] + transform.translation[2],
            ];
            assert!(close(centre, [0.0, 0.0, 0.0]), "{to:?} -> {centre:?}");
        }
        assert!(segment_transform(Vec3::ZERO, Vec3::ZERO).is_none());
    }

    fn frame<'a>(path: &'a [Vec3], waypoint: usize, revision: u32, tick: u64) -> Frame<'a> {
        Frame {
            target: bevy_ecs::world::World::new().spawn_empty().id(),
            network_id: 7,
            uuid: Uuid::from_u128(9),
            flags: 0,
            position: Vec3::new(0.5, 64.0, 0.5),
            origin: Vec3::new(0.5, 64.0, 0.5),
            path,
            complete: true,
            waypoint,
            revision,
            destination: path.last().copied(),
            tick,
        }
    }

    fn count(out: &[ClientboundPacket], name: &str) -> usize {
        out.iter()
            .filter(|packet| format!("{packet:?}").contains(name))
            .count()
    }

    fn removed(out: &[ClientboundPacket]) -> usize {
        out.iter()
            .map(|packet| match packet {
                ClientboundPacket::ManualPlay(
                    voidmc_protocol::clientbound::ManualPlayPacket::RemoveEntities(remove),
                ) => remove.entity_ids.len(),
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn draws_trims_and_clears_only_what_changed() {
        let path = [
            Vec3::new(4.5, 64.0, 0.5),
            Vec3::new(4.5, 64.0, 6.5),
            Vec3::new(9.5, 65.0, 6.5),
        ];
        let mut scene = Scene::default();
        let mut out = Vec::new();
        scene.sync(Some(&frame(&path, 0, 1, 0)), &mut out);
        assert_eq!(count(&out, "SpawnEntity"), 4);
        assert_eq!(count(&out, "TrackedWaypoint"), 2);
        assert_eq!(count(&out, "SetEntityData"), 5);

        out.clear();
        scene.sync(Some(&frame(&path, 0, 1, 1)), &mut out);
        assert!(out.is_empty(), "{out:?}");

        scene.sync(Some(&frame(&path, 2, 1, 2)), &mut out);
        assert_eq!(removed(&out), 2);
        assert_eq!(count(&out, "SpawnEntity"), 0);

        out.clear();
        scene.sync(Some(&frame(&path[..1], 0, 2, 3)), &mut out);
        assert_eq!(removed(&out), 2);
        assert_eq!(count(&out, "SpawnEntity"), 2);

        out.clear();
        scene.sync(None, &mut out);
        assert_eq!(removed(&out), 2);
        assert_eq!(count(&out, "Untrack"), 2);
        assert_eq!(count(&out, "SetEntityData"), 1);
        assert!(scene.is_empty());
        out.clear();
        scene.sync(None, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn mob_waypoint_updates_are_throttled() {
        let path = [Vec3::new(30.5, 64.0, 0.5)];
        let mut scene = Scene::default();
        let mut out = Vec::new();
        scene.sync(Some(&frame(&path, 0, 1, 100)), &mut out);
        out.clear();
        let mut moved = frame(&path, 0, 1, 101);
        moved.position = Vec3::new(3.5, 64.0, 0.5);
        scene.sync(Some(&moved), &mut out);
        assert_eq!(count(&out, "Update"), 0);
        moved.tick = 105;
        scene.sync(Some(&moved), &mut out);
        assert_eq!(count(&out, "Update"), 1);
    }
}
