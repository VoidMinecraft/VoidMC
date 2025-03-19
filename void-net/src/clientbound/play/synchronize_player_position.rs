use crate::codec::{PacketDecode, PacketEncode};
use crate::{Packet, PacketId};

#[derive(Debug)]
pub struct SynchronizePlayerPosition {
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

impl Packet for SynchronizePlayerPosition {
    fn encode<E: PacketEncode>(&self, encoder: &mut E) -> std::io::Result<()> {
        encoder.encode_vari32(self.teleport_id)?;
        encoder.encode_f64(self.x)?;
        encoder.encode_f64(self.y)?;
        encoder.encode_f64(self.z)?;
        encoder.encode_f64(self.vx)?;
        encoder.encode_f64(self.vy)?;
        encoder.encode_f64(self.vz)?;
        encoder.encode_f32(self.yaw)?;
        encoder.encode_f32(self.pitch)?;
        encoder.encode_u32(self.flags)
    }

    fn decode<D: PacketDecode>(decoder: &mut D) -> std::io::Result<Self> {
        let teleport_id = decoder.decode_vari32()?;
        let x = decoder.decode_f64()?;
        let y = decoder.decode_f64()?;
        let z = decoder.decode_f64()?;
        let vx = decoder.decode_f64()?;
        let vy = decoder.decode_f64()?;
        let vz = decoder.decode_f64()?;
        let yaw = decoder.decode_f32()?;
        let pitch = decoder.decode_f32()?;
        let flags = decoder.decode_u32()?;

        Ok(Self {
            teleport_id,
            x,
            y,
            z,
            vx,
            vy,
            vz,
            yaw,
            pitch,
            flags,
        })
    }
}

impl PacketId for SynchronizePlayerPosition {
    const ID: i32 = 0x42;
}
