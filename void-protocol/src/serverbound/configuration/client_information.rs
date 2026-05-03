use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum ChatMode {
    Enabled = 0x0,
    CommandOnly = 0x1,
    Hidden = 0x2,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum MainHand {
    Left = 0x0,
    Right = 0x1,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum ParticleStatus {
    All = 0x0,
    Decreased = 0x1,
    Minimal = 0x2,
}

#[derive(Debug, Encode, Decode)]
pub struct ClientInformation {
    pub locale: String,
    pub view_distance: u8,
    pub chat_mode: ChatMode,
    pub chat_colors: bool,
    pub skin_parts: u8,
    pub main_hand: MainHand,
    pub enable_text_filtering: bool,
    pub allow_server_listings: bool,
    pub particle_status: ParticleStatus,
}
