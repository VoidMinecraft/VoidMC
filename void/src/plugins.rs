use bevy_app::Plugin;

pub mod abilities;
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
pub mod scoreboard;
pub mod sidebar;
pub mod status;
pub mod tab_list;
pub mod teleport;
pub(crate) mod viewers;
pub mod weather;
pub mod world_border;
pub mod world_time;

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
            (
                scoreboard::ScoreboardPlugin,
                sidebar::SidebarPlugin,
                world_border::WorldBorderPlugin,
                world_time::WorldTimePlugin,
                weather::WeatherPlugin,
                tab_list::TabListPlugin,
            ),
            crate::entity::EntityPlugin,
            block_entity::BlockEntityPlugin,
        ));
        app.add_plugins((abilities::AbilitiesPlugin, teleport::TeleportPlugin));
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;

    use super::DefaultPlugins;
    use crate::commands::plugin::CommandPlugin;
    use crate::config::{ServerConfig, ServerConfigResource};
    use crate::network::{ConnectionEvent, NetworkPlugin, OutgoingPacket};
    use crate::registry::RegistryDataStore;
    use crate::systems::GameSystemsPlugin;
    use crate::world::ChunkIndex;
    use crate::world::generation::{DefaultWorldGenerator, WorldGen};

    #[test]
    fn full_plugin_stack_ticks_without_param_conflicts() {
        let (_events_tx, events_rx) = flume::unbounded::<ConnectionEvent>();
        let (outgoing_tx, _outgoing_rx) = flume::unbounded::<OutgoingPacket>();

        let mut app = App::new();
        app.add_plugins(NetworkPlugin::new(events_rx, outgoing_tx))
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
