mod configuration;
mod login;
mod play;
mod status;

pub use configuration::*;
pub use login::*;
pub use play::*;
pub use status::*;

#[derive(Debug, Clone)]
pub enum ClientboundPacket {
    Status(StatusPacket),
    Login(LoginPacket),
    Configuration(ConfigurationPacket),
    Play(PlayPacket),
    ManualPlay(ManualPlayPacket),
}
