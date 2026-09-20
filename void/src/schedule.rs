//! Public system sets for ordering user systems against framework phases.
//!
//! Every built-in system runs inside one of these sets, so a plugin can say
//! "after chunk streaming" without naming private functions:
//!
//! ```ignore
//! app.add_systems(PostUpdate, my_system.after(VoidSystems::ChunkStreaming));
//! ```
//!
//! Sets are named after what the systems in them do today. The schedule each
//! set lives in is fixed (see each variant); ordering constraints only apply
//! within a schedule.

use bevy_ecs::schedule::SystemSet;

/// Framework phases, grouped by Bevy schedule.
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum VoidSystems {
    /// `PreUpdate` — drains the network thread's incoming channel, spawns
    /// client entities, decodes and dispatches packets (`On<PacketEvent<T>>`
    /// observers fire here), and despawns disconnected clients.
    NetworkIngest,

    /// `Update` — executes queued chat commands with `&mut World`
    /// (`CommandSystems::DrainQueue` is contained in this set).
    CommandDrain,
    /// `Update` — executes queued item uses / block breaks through
    /// `ItemBehavior` handlers with `&mut World`.
    ItemUseDrain,
    /// `Update` — sends `KeepAlive` to ready players on its interval.
    /// Runs after [`CommandDrain`](Self::CommandDrain).
    KeepAlive,
    /// `Update` — server-side entity simulation: settling fresh spawns,
    /// wandering, gravity/physics. Runs after [`KeepAlive`](Self::KeepAlive).
    EntitySimulation,
    /// `Update` — dropped-item pickup and pickup-delay ticking.
    ItemPickup,

    /// `PostUpdate` — sends spawn, movement, motion and metadata packets for
    /// non-player `SpawnedEntity`s, then records their previous positions.
    EntityBroadcast,
    /// `PostUpdate` — sends other players' movement and head rotation, then
    /// records previous positions.
    PlayerBroadcast,
    /// `PostUpdate` — loads/generates and sends chunks as players move,
    /// unloads out-of-range chunks, updates `LoadedChunks`.
    ChunkStreaming,
    /// `PostUpdate` — re-sends the inventory window of players flagged
    /// `InventoryDirty`.
    InventorySync,
}

#[cfg(test)]
mod tests {
    use bevy_app::{App, Update};
    use bevy_ecs::prelude::*;
    use bevy_ecs::schedule::IntoScheduleConfigs;
    use flume::Receiver;

    use super::*;
    use crate::components::{ClientId, KeepAliveState, PlayerReady};
    use crate::config::{ServerConfig, ServerConfigResource};
    use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};
    use crate::systems::{GameSystemsPlugin, KeepAliveTicker};
    use crate::world::ChunkIndex;
    use crate::world::generation::{DefaultWorldGenerator, WorldGen};

    #[derive(Resource, Default)]
    struct Seen {
        before: Option<usize>,
        after: Option<usize>,
    }

    #[derive(Resource)]
    struct Outgoing(Receiver<OutgoingPacket>);

    #[test]
    fn user_systems_order_against_keep_alive_set() {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();

        let mut app = App::new();
        app.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        })
        .insert_resource(Outgoing(outgoing_rx))
        .insert_resource(ServerConfigResource::from(&ServerConfig::default()))
        .insert_resource(WorldGen(Box::new(DefaultWorldGenerator::default())))
        .init_resource::<ChunkIndex>()
        .init_resource::<Seen>()
        .add_plugins(GameSystemsPlugin)
        .insert_resource(KeepAliveTicker {
            ticks_since_last: 0,
            interval_ticks: 1,
        })
        .add_systems(
            Update,
            (
                (|rx: Res<Outgoing>, mut seen: ResMut<Seen>| seen.before = Some(rx.0.len()))
                    .before(VoidSystems::KeepAlive),
                (|rx: Res<Outgoing>, mut seen: ResMut<Seen>| seen.after = Some(rx.0.len()))
                    .after(VoidSystems::KeepAlive),
            ),
        );
        app.world_mut().spawn((
            ClientId(1),
            PlayerReady,
            KeepAliveState {
                last_sent_id: 0,
                awaiting_response: false,
            },
        ));

        app.update();

        let seen = app.world().resource::<Seen>();
        assert_eq!(seen.before, Some(0));
        assert_eq!(seen.after, Some(1));
    }
}
