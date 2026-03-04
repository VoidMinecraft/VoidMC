mod chat_command;
mod chat_command_unsigned;
mod chat_message;
mod client_information;
mod command_suggestions_request;
mod confirm_teleportation;
mod keep_alive;
mod player_loaded;
mod pong;
mod set_player_pos;
mod set_player_pos_and_rot;
mod set_player_rotation;
mod signed_chat_command;
mod tick_end;

pub use chat_command::*;
pub use chat_command_unsigned::*;
pub use chat_message::*;
pub use client_information::*;
pub use command_suggestions_request::*;
pub use confirm_teleportation::*;
pub use keep_alive::*;
pub use player_loaded::*;
pub use pong::*;
pub use set_player_pos::*;
pub use set_player_pos_and_rot::*;
pub use set_player_rotation::*;
pub use signed_chat_command::*;
pub use tick_end::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x00)]
    ConfirmTeleportation(ConfirmTeleportation),
    #[codec(packet_id = 0x04)]
    ChatCommand(ChatCommand),
    #[codec(packet_id = 0x05)]
    ChatCommandUnsigned(ChatCommandUnsigned),
    #[codec(packet_id = 0x06)]
    SignedChatCommand(SignedChatCommand),
    #[codec(packet_id = 0x07)]
    ChatMessage(ChatMessage),
    #[codec(packet_id = 0x0B)]
    TickEnd(TickEnd),
    #[codec(packet_id = 0x0C)]
    ClientInformation(ClientInformation),
    #[codec(packet_id = 0x0D)]
    CommandSuggestionsRequest(CommandSuggestionsRequest),
    #[codec(packet_id = 0x1A)]
    KeepAlive(KeepAlive),
    #[codec(packet_id = 0x1C)]
    SetPlayerPos(SetPlayerPos),
    #[codec(packet_id = 0x1D)]
    SetPlayerPosAndRot(SetPlayerPosAndRot),
    #[codec(packet_id = 0x1E)]
    SetPlayerRotation(SetPlayerRotation),
    #[codec(packet_id = 0x29)]
    Pong(Pong),
    #[codec(packet_id = 0x2A)]
    PlayerLoaded(PlayerLoaded),
}
