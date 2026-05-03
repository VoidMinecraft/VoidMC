mod chat_command;
mod chat_message;
mod client_information;
mod close_container;
mod command_suggestions_request;
mod confirm_teleportation;
mod interact;
mod keep_alive;
mod player_abilities;
mod player_action;
mod player_command;
mod player_loaded;
mod pong;
mod set_held_item;
mod set_player_pos;
mod set_player_pos_and_rot;
mod set_player_rotation;
mod signed_chat_command;
mod swing_arm;
mod tick_end;
mod use_item;
mod use_item_on;

pub use chat_command::*;
pub use chat_message::*;
pub use client_information::*;
pub use close_container::*;
pub use command_suggestions_request::*;
pub use confirm_teleportation::*;
pub use interact::*;
pub use keep_alive::*;
pub use player_abilities::*;
pub use player_action::*;
pub use player_command::*;
pub use player_loaded::*;
pub use pong::*;
pub use set_held_item::*;
pub use set_player_pos::*;
pub use set_player_pos_and_rot::*;
pub use set_player_rotation::*;
pub use signed_chat_command::*;
pub use swing_arm::*;
pub use tick_end::*;
pub use use_item::*;
pub use use_item_on::*;
use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
#[codec(tagged)]
pub enum PlayPacket {
    #[codec(packet_id = 0x00)]
    ConfirmTeleportation(ConfirmTeleportation),
    #[codec(packet_id = 0x07)]
    ChatCommand(ChatCommand),
    #[codec(packet_id = 0x08)]
    SignedChatCommand(SignedChatCommand),
    #[codec(packet_id = 0x09)]
    ChatMessage(ChatMessage),
    #[codec(packet_id = 0x0D)]
    TickEnd(TickEnd),
    #[codec(packet_id = 0x0E)]
    ClientInformation(ClientInformation),
    #[codec(packet_id = 0x0F)]
    CommandSuggestionsRequest(CommandSuggestionsRequest),
    #[codec(packet_id = 0x13)]
    CloseContainer(CloseContainer),
    #[codec(packet_id = 0x1A)]
    Interact(Interact),
    #[codec(packet_id = 0x1C)]
    KeepAlive(KeepAlive),
    #[codec(packet_id = 0x1E)]
    SetPlayerPos(SetPlayerPos),
    #[codec(packet_id = 0x1F)]
    SetPlayerPosAndRot(SetPlayerPosAndRot),
    #[codec(packet_id = 0x20)]
    SetPlayerRotation(SetPlayerRotation),
    #[codec(packet_id = 0x28)]
    PlayerAbilities(PlayerAbilities),
    #[codec(packet_id = 0x29)]
    PlayerAction(PlayerAction),
    #[codec(packet_id = 0x2A)]
    PlayerCommand(PlayerCommand),
    #[codec(packet_id = 0x2C)]
    PlayerLoaded(PlayerLoaded),
    #[codec(packet_id = 0x2D)]
    Pong(Pong),
    #[codec(packet_id = 0x35)]
    SetHeldItem(SetHeldItem),
    #[codec(packet_id = 0x3F)]
    SwingArm(SwingArm),
    #[codec(packet_id = 0x42)]
    UseItemOn(UseItemOn),
    #[codec(packet_id = 0x43)]
    UseItem(UseItem),
}
