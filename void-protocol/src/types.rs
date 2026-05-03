use voidmc_codec::{Decode, DecodeError, Encode};

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
