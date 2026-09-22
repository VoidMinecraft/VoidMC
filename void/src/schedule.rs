//! Framework phases as public `SystemSet`s, so user systems can be ordered
//! with `.before(VoidSystems::X)` / `.after(VoidSystems::X)` in the same schedule.

use bevy_ecs::schedule::SystemSet;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum VoidSystems {
    /// `PreUpdate`: packet decode + dispatch (`On<PacketEvent<T>>` fires here), disconnects.
    NetworkIngest,

    /// `Update`: contains `CommandSystems::DrainQueue`.
    CommandDrain,
    /// `Update`: `ItemBehavior` handlers run here.
    ItemUseDrain,
    /// `Update`: `MenuClickEvent` and menu `on_click` handlers run here.
    MenuClickDrain,
    /// `Update`, after `CommandDrain`.
    KeepAlive,
    /// `Update`, after `KeepAlive`: settle, wander, physics.
    EntitySimulation,
    /// `Update`.
    ItemPickup,

    /// `PostUpdate`, after `ChunkStreaming`: movement/motion of non-player
    /// entities to their current viewers.
    EntityBroadcast,
    /// `PostUpdate`, after `EntityBroadcast`: dirty metadata indices and
    /// changed passenger lists to current viewers.
    EntityMetadataSync,
    /// `PostUpdate`, after `EntityMetadataSync`: viewer diff — `SpawnEntity` /
    /// `RemoveEntities` as players start or stop seeing an entity's chunk.
    EntityVisibility,
    /// `PostUpdate`: other players' movement and head rotation.
    PlayerBroadcast,
    /// `PostUpdate`, before `ChunkStreaming`: Block Entity Data packets for changed block entities.
    BlockEntitySync,
    /// `PostUpdate`: chunk load/unload packets, updates `LoadedChunks`.
    ChunkStreaming,
    /// `PostUpdate`: container packets for changed inventories and open menus.
    InventorySync,
    /// `PostUpdate`: boss bar add/update/remove packets.
    BossBarSync,
    /// `PostUpdate`: objective, score and team packets for changed scoreboards.
    ScoreboardSync,
    /// `PostUpdate`: Player Abilities packets for changed `PlayerAbilities`.
    AbilitiesSync,
    /// `Update`, after `CommandDrain`: advances in-flight [`crate::Teleport`]s.
    TeleportBarrier,
    /// `PostUpdate`: world border initialize/update/reset packets.
    WorldBorderSync,
    /// `PostUpdate`: advances world clocks, Set Time packets on change and every 20 ticks while running.
    WorldTimeSync,
    /// `PostUpdate`: weather Game Event packets on change and per transition tick.
    WeatherSync,
    /// `PostUpdate`: tab list header/footer and changed player entry actions.
    TabListSync,
    /// `PostUpdate`: refreshes the status-response snapshot read by the network thread.
    StatusSnapshot,
    /// `PostUpdate`: TPS tracking (only with `metrics_debug`).
    Metrics,
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
        app.world_mut()
            .spawn((ClientId(1), PlayerReady, KeepAliveState::default()));

        app.update();

        let seen = app.world().resource::<Seen>();
        assert_eq!(seen.before, Some(0));
        assert_eq!(seen.after, Some(1));
    }
}
