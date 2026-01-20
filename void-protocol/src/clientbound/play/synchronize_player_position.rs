use bitflags::bitflags;
use void_codec::{Decode, DecodeError, Encode};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TeleportFlags: u32 {
        const RelativeX = 0x0001;
        const RelativeY = 0x0002;
        const RelativeZ = 0x0004;
        const RelativeYaw = 0x0008;
        const RelativePitch = 0x0010;
        const RelativeVelocityX = 0x0020;
        const RelativeVelocityY = 0x0040;
        const RelativeVelocityZ = 0x0080;
        const RelativeVelocityAccordingToChangeInRotation = 0x0100;
    }
}

impl Encode for TeleportFlags {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.bits().encode(buf);
    }
}

impl Decode for TeleportFlags {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let bits = u32::decode(buf)?;
        Ok(TeleportFlags::from_bits_truncate(bits))
    }
}

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
    pub flags: TeleportFlags,
}
