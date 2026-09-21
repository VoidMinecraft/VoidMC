use bevy_app::Plugin;

pub mod block_entity;
pub mod boss_bar;
pub mod chat;
pub mod configuration;
pub mod handshake;
pub mod interaction;
pub mod inventory;
pub mod item_drops;
pub mod item_use;
pub mod login;
pub mod movement;
pub mod play;
pub mod status;
pub mod world_border;

pub struct DefaultPlugins;

impl Plugin for DefaultPlugins {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_plugins((
            handshake::HandshakePlugin,
            status::StatusPlugin,
            login::LoginPlugin,
            configuration::ConfigurationPlugin,
            play::PlayPlugin,
            movement::MovementPlugin,
            chat::ChatPlugin,
            interaction::InteractionPlugin,
            item_use::ItemUsePlugin,
            inventory::InventoryPlugin,
            item_drops::ItemDropsPlugin,
            boss_bar::BossBarPlugin,
            world_border::WorldBorderPlugin,
            crate::entity::EntityPlugin,
            block_entity::BlockEntityPlugin,
        ));
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;

    use super::DefaultPlugins;
    use crate::commands::plugin::CommandPlugin;
    use crate::config::{ServerConfig, ServerConfigResource};
    use crate::network::{ClientConnected, IncomingPacket, NetworkPlugin, OutgoingPacket};
    use crate::registry::RegistryDataStore;
    use crate::systems::GameSystemsPlugin;
    use crate::world::ChunkIndex;
    use crate::world::generation::{DefaultWorldGenerator, WorldGen};

    #[test]
    fn full_plugin_stack_ticks_without_param_conflicts() {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, _outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let (_connected_tx, connected_rx) = flume::unbounded::<ClientConnected>();

        let mut app = App::new();
        app.add_plugins(NetworkPlugin::new(
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
            kick_tx,
            connected_rx,
        ))
        .add_plugins(DefaultPlugins)
        .add_plugins(CommandPlugin)
        .add_plugins(GameSystemsPlugin)
        .insert_resource(RegistryDataStore::default())
        .insert_resource(ServerConfigResource::from(&ServerConfig::default()))
        .insert_resource(WorldGen(Box::new(DefaultWorldGenerator::default())))
        .init_resource::<ChunkIndex>();

        app.update();
        app.update();
    }
}
