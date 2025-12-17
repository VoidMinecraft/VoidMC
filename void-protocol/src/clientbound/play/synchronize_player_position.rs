use void_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct SynchronizePlayerPosition {
    #[codec(varint32)]
    pub teleport_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u32,
}
