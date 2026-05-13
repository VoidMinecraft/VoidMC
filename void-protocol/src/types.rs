use voidmc_codec::{Decode, DecodeError, Encode, VarI32};

/// A block position packed into an i64 using Minecraft's bit layout.
///
/// Layout: x (26 bits) | z (26 bits) | y (12 bits)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i16,
    pub z: i32,
}

impl Encode for BlockPosition {
    fn encode(&self, buf: &mut Vec<u8>) {
        let val = ((self.x as i64 & 0x3FFFFFF) << 38)
            | ((self.z as i64 & 0x3FFFFFF) << 12)
            | (self.y as i64 & 0xFFF);
        val.encode(buf);
    }
}

impl Decode for BlockPosition {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let val = i64::decode(buf)?;
        let mut x = (val >> 38) as i32;
        let mut z = ((val >> 12) & 0x3FFFFFF) as i32;
        let mut y = (val & 0xFFF) as i16;

        // Sign-extend from 26 bits
        if x >= 1 << 25 {
            x -= 1 << 26;
        }
        if z >= 1 << 25 {
            z -= 1 << 26;
        }
        // Sign-extend from 12 bits
        if y >= 1 << 11 {
            y -= 1 << 12;
        }

        Ok(BlockPosition { x, y, z })
    }
}

/// Low-precision Vec3 used by 1.21.7+ for entity movement / velocity fields.
///
/// Wire format (see `net.minecraft.network.LpVec3`):
/// - First byte 0x00 means zero vector.
/// - Otherwise: 1 byte | 1 byte | 4 bytes (u32) packed (15 bits per axis + 2 scale + continuation),
///   optionally followed by a varint for the upper bits of the scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LpVec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl LpVec3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    const ABS_MAX: f64 = 1.7179869183E10;
    const ABS_MIN: f64 = 3.051944088384301E-5;

    fn sanitize(v: f64) -> f64 {
        if v.is_nan() {
            0.0
        } else {
            v.clamp(-Self::ABS_MAX, Self::ABS_MAX)
        }
    }

    fn pack(v: f64) -> u64 {
        (((v * 0.5 + 0.5) * 32766.0).round() as i64) as u64 & 0x7FFF
    }

    fn unpack(v: u64) -> f64 {
        let q = (v & 0x7FFF).min(32766) as f64;
        q * 2.0 / 32766.0 - 1.0
    }
}

impl Encode for LpVec3 {
    fn encode(&self, buf: &mut Vec<u8>) {
        let x = Self::sanitize(self.x);
        let y = Self::sanitize(self.y);
        let z = Self::sanitize(self.z);
        let chess = x.abs().max(y.abs()).max(z.abs());
        if chess < Self::ABS_MIN {
            buf.push(0);
            return;
        }
        let scale = chess.ceil() as i64 as u64;
        let is_partial = (scale & 3) != scale;
        let markers: u64 = if is_partial { (scale & 3) | 4 } else { scale };
        let s = scale as f64;
        let xn = Self::pack(x / s) << 3;
        let yn = Self::pack(y / s) << 18;
        let zn = Self::pack(z / s) << 33;
        let buffer: u64 = markers | xn | yn | zn;
        buf.push(buffer as u8);
        buf.push((buffer >> 8) as u8);
        buf.extend_from_slice(&((buffer >> 16) as u32).to_be_bytes());
        if is_partial {
            VarI32((scale >> 2) as i32).encode(buf);
        }
    }
}

impl Decode for LpVec3 {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }
        let lowest = buf[0];
        *buf = &buf[1..];
        if lowest == 0 {
            return Ok(Self::ZERO);
        }
        if buf.len() < 5 {
            return Err(DecodeError::UnexpectedEof);
        }
        let middle = buf[0];
        *buf = &buf[1..];
        let highest = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        *buf = &buf[4..];
        let buffer: u64 = ((highest as u64) << 16) | ((middle as u64) << 8) | (lowest as u64);
        let mut scale: u64 = (lowest as u64) & 3;
        if (lowest & 4) == 4 {
            let ext = VarI32::decode(buf)?.0 as u32 as u64;
            scale |= ext << 2;
        }
        let s = scale as f64;
        Ok(Self {
            x: Self::unpack(buffer >> 3) * s,
            y: Self::unpack(buffer >> 18) * s,
            z: Self::unpack(buffer >> 33) * s,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum Hand {
    MainHand = 0,
    OffHand = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum BlockFace {
    Bottom = 0,
    Top = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum PlayerActionStatus {
    StartedDigging = 0,
    CancelledDigging = 1,
    FinishedDigging = 2,
    DropItemStack = 3,
    DropItem = 4,
    ShootArrowOrFinishEating = 5,
    SwapItemInHand = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum PlayerCommandAction {
    StartSneaking = 0,
    StopSneaking = 1,
    LeaveBed = 2,
    StartSprinting = 3,
    StopSprinting = 4,
    StartJumpWithHorse = 5,
    StopJumpWithHorse = 6,
    OpenVehicleInventory = 7,
    StartFlyingWithElytra = 8,
}
