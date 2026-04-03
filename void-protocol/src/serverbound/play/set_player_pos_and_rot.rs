use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct SetPlayerPosAndRot {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
}
